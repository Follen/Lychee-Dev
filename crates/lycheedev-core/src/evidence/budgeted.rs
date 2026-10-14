//! Capture headers share the producer's ledger. Body allocation is already
//! admitted by the producer; the existing bounded body path keeps its control.
use super::{BlobRef, CaptureInput, Provenance, VerifiedBlob, bounded};
use crate::{
    Result, ToolkitError, control::OperationControl, metadata::Metadata, storage, workspace,
};
use lycheedev_windows::filesystem::Directory;
use serde::Serialize;
use serde_json::{
    Value,
    ser::{CompactFormatter, Formatter},
};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Write},
    path::Path,
};

const METADATA_BYTES: usize = 1_048_576;
const CAPTURE_ID_BYTES: usize = 68;
// A capture has three JSON maps with at most eleven entries each. Include
// their map nodes, Value slots and fixed field names in addition to the
// serialized length, which bounds the decoded string payload.
const VALUE_STRUCTURE_BYTES: usize = 4096;

/// Metadata allocations and work join the caller's existing ledger. JSON work
/// uses emitted/input bytes; each binary body pass uses one unit per 64 KiB.
/// Errors are returned unchanged, including cancellation and limits.
pub trait CaptureAccounting {
    fn check(&self) -> Result<()>;
    fn admit_allocation(&self, bytes: usize) -> Result<()>;
    fn admit_work(&self, units: usize) -> Result<()>;
    /// Admit one dependency's raw/typed/identity scratch before its read.
    /// Calls are sequential: the prior dependency's raw bytes and parsed value
    /// have been dropped before the next admission. An implementation may keep
    /// one reservation until that next call or the accounting scope's drop.
    /// The default retains the existing conservative cumulative accounting.
    fn admit_dependency(&self, bytes: usize) -> Result<()> {
        self.admit_allocation(
            bytes
                .saturating_mul(24)
                .saturating_add(VALUE_STRUCTURE_BYTES),
        )?;
        self.admit_work(bytes.saturating_mul(8))
    }
}

pub struct CaptureHeader<'a> {
    pub media_type: &'a str,
    pub provenance: Value,
    pub complete: bool,
    pub truncated: bool,
}

fn check(control: &OperationControl, meter: &impl CaptureAccounting) -> Result<()> {
    control.check()?;
    meter.check()
}

fn validate(header: &CaptureHeader<'_>) -> Result<()> {
    if header.media_type.is_empty()
        || header.provenance["kind"].as_str().unwrap_or("").is_empty()
        || header.provenance["locator"]
            .as_str()
            .unwrap_or("")
            .is_empty()
        || header.complete && header.truncated
    {
        return Err(ToolkitError::new(
            "evidence.invalid_capture",
            "invalid media/provenance/completeness",
            4,
        ));
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureDocument<'a> {
    schema: &'static str,
    id: &'a str,
    blob: &'a BlobRef,
    media_type: &'a str,
    provenance: Provenance,
    complete: bool,
    truncated: bool,
    captured_at: String,
    #[serde(skip_serializing_if = "<[&str]>::is_empty")]
    dependencies: &'a [&'a str],
}

struct MeteredWriter<'a, W, M> {
    sink: W,
    control: &'a OperationControl,
    meter: &'a M,
    length: usize,
    maximum: usize,
    failure: Option<ToolkitError>,
}
impl<W: Write, M: CaptureAccounting> Write for MeteredWriter<'_, W, M> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let result = (|| {
            check(self.control, self.meter)?;
            let length = self
                .length
                .checked_add(bytes.len())
                .filter(|length| *length <= self.maximum)
                .ok_or_else(|| ToolkitError::invalid("metadata document exceeds 1 MiB"))?;
            self.meter.admit_work(bytes.len())?;
            self.sink.write_all(bytes)?;
            self.length = length;
            Ok(())
        })();
        if let Err(error) = result {
            self.failure = Some(error);
            return Err(io::Error::other("capture serialization stopped"));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.sink.flush()
    }
}

fn serialize<W: Write, M: CaptureAccounting>(
    value: &CaptureDocument<'_>,
    writer: &mut MeteredWriter<'_, W, M>,
    formatter: impl Formatter,
) -> Result<()> {
    // Serde scans input strings before emitting fragments. Admit that pass
    // before invoking it; the writer separately admits emitted-byte work.
    for text in [
        value.schema,
        value.id,
        &value.blob.sha256,
        value.media_type,
        &value.provenance.kind,
        &value.provenance.locator,
        &value.provenance.snapshot,
        &value.provenance.operation_id,
        &value.provenance.source_commit,
        &value.provenance.base_snapshot,
        &value.provenance.base_source_commit,
        &value.provenance.data_build,
        &value.provenance.session,
        &value.captured_at,
    ] {
        check(writer.control, writer.meter)?;
        writer.meter.admit_work(text.len())?;
    }
    for dependency in value.dependencies {
        check(writer.control, writer.meter)?;
        writer.meter.admit_work(dependency.len())?;
    }
    let result = value.serialize(&mut serde_json::Serializer::with_formatter(
        &mut *writer,
        formatter,
    ));
    if let Some(error) = writer.failure.take() {
        return Err(error);
    }
    result.map_err(Into::into)
}

struct IdentityFormatter;
impl Formatter for IdentityFormatter {
    fn write_string_fragment<W: ?Sized + Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        let mut start = 0;
        for (at, ch) in fragment.char_indices() {
            let escaped: &[u8] = match ch {
                '&' => b"\\u0026",
                '<' => b"\\u003c",
                '>' => b"\\u003e",
                '\u{2028}' => b"\\u2028",
                '\u{2029}' => b"\\u2029",
                _ => continue,
            };
            writer.write_all(&fragment.as_bytes()[start..at])?;
            writer.write_all(escaped)?;
            start = at + ch.len_utf8();
        }
        writer.write_all(&fragment.as_bytes()[start..])
    }
}
struct IdentityHash(Sha256);
impl Write for IdentityHash {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PreparedCapture {
    key: String,
    document: Vec<u8>,
    value: Value,
}

fn prepare(
    blob: &BlobRef,
    header: CaptureHeader<'_>,
    dependencies: &[&str],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<PreparedCapture> {
    check(control, meter)?;
    validate(&header)?;
    // Deserializing this owned Value moves its strings into Provenance; it does
    // not duplicate their payload. Charge the observed header traversal first.
    if let Some(fields) = header.provenance.as_object() {
        for (key, value) in fields {
            check(control, meter)?;
            meter.admit_work(key.len())?;
            meter.admit_work(value.as_str().map_or(1, str::len))?;
        }
    }
    let provenance = serde_json::from_value(header.provenance)?;
    // utc_now formats and then trims a bounded RFC3339 timestamp.
    meter.admit_allocation(128)?;
    let mut capture = CaptureDocument {
        schema: "lycheedev.capture.v1",
        id: "",
        blob,
        media_type: header.media_type,
        provenance,
        complete: header.complete,
        truncated: header.truncated,
        captured_at: storage::utc_now(),
        dependencies,
    };
    let mut counter = MeteredWriter {
        sink: io::sink(),
        control,
        meter,
        length: 0,
        maximum: METADATA_BYTES - CAPTURE_ID_BYTES,
        failure: None,
    };
    serialize(&capture, &mut counter, CompactFormatter)?;
    let length = counter.length + CAPTURE_ID_BYTES;

    // Hash the empty-id document with the same Go escaping as capture_identity,
    // without cloning the header or allocating escaped intermediate strings.
    let mut identity = MeteredWriter {
        sink: IdentityHash(Sha256::new()),
        control,
        meter,
        length: 0,
        maximum: usize::MAX,
        failure: None,
    };
    serialize(&capture, &mut identity, IdentityFormatter)?;
    meter.admit_allocation(CAPTURE_ID_BYTES)?;
    let digest = identity.sink.0.finalize();
    let mut hex_digest = [0; 64];
    hex::encode_to_slice(digest, &mut hex_digest).expect("SHA256 hex width");
    let mut id = String::with_capacity(CAPTURE_ID_BYTES);
    id.push_str("CAP-");
    id.push_str(std::str::from_utf8(&hex_digest).expect("hex is UTF8"));
    capture.id = &id;

    meter.admit_allocation(length)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(length)
        .map_err(|_| ToolkitError::invalid("capture metadata allocation failed"))?;
    let mut writer = MeteredWriter {
        sink: raw,
        control,
        meter,
        length: 0,
        maximum: length,
        failure: None,
    };
    serialize(&capture, &mut writer, CompactFormatter)?;
    let document = writer.sink;

    // Build the public Value before inserting any capture document. No budget
    // admission, serialization or fallible Value construction follows commit.
    meter.admit_allocation(
        length
            + VALUE_STRUCTURE_BYTES
            + dependencies
                .len()
                .saturating_mul(std::mem::size_of::<Value>()),
    )?;
    meter.admit_work(length)?;
    check(control, meter)?;
    let value = serde_json::to_value(capture)?;
    meter.admit_allocation("capture/".len() + id.len())?;
    let key = format!("capture/{id}");
    check(control, meter)?;
    Ok(PreparedCapture {
        key,
        document,
        value,
    })
}

fn persist(
    home: &Path,
    prepared: PreparedCapture,
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    check(control, meter)?;
    Metadata::open(home, true)?.put_serialized(&prepared.key, &prepared.document, None, || {
        check(control, meter)
    })?;
    Ok(prepared.value)
}

/// The body belongs to the producer's existing allocation budget. All capture
/// metadata admission completes before body publication; later cancellation
/// can leave a published blob, just as on the bounded capture path.
pub fn commit_capture_budgeted(
    home: &Path,
    input: CaptureInput<'_>,
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    commit_capture_budgeted_with_dependencies(home, input, &[], control, meter)
}

/// The dependency slice is borrowed only for this call. Its ordered values are
/// part of the immutable identity; empty dependencies preserve the old encoding.
/// References are revalidated under the removal transaction before any body
/// publication, so a producer cannot publish a dangling owner after a gap.
pub fn commit_capture_budgeted_with_dependencies(
    home: &Path,
    input: CaptureInput<'_>,
    dependencies: &[&str],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    check(control, meter)?;
    let header = CaptureHeader {
        media_type: input.media_type,
        provenance: input.provenance,
        complete: input.complete,
        truncated: input.truncated,
    };
    validate(&header)?;
    workspace::open(home)?;
    if input.bytes.len() > input.maximum_bytes
        || input.bytes.len() as u64 > bounded::MAX_CAPTURE_BYTES
    {
        return Err(ToolkitError::new(
            "vault.blob_limit",
            "blob exceeds budget",
            4,
        ));
    }
    meter.admit_allocation(64)?;
    let body_work = input.bytes.len().div_ceil(65536);
    meter.admit_work(body_work)?;
    let reference = BlobRef {
        sha256: bounded::digest_bytes(input.bytes, Some(control))?,
        bytes: input.bytes.len() as u64,
    };
    let prepared = prepare(&reference, header, dependencies, control, meter)?;
    let _home = Directory::open_shared(home)?;
    workspace::open(home)?;
    let _guard = storage::ExclusiveLock::resource(
        &home.join("locks"),
        &format!("blob:{}", reference.sha256),
    )?;
    let publish = || {
        check(control, meter)?;
        meter.admit_work(body_work)?;
        bounded::publish_locked(
            home,
            input.bytes,
            input.maximum_bytes,
            reference.sha256,
            Some(control),
        )?;
        Ok(())
    };
    if dependencies.is_empty() {
        publish()?;
        return persist(home, prepared, control, meter);
    }
    meter.admit_allocation(
        dependencies
            .len()
            .saturating_mul(std::mem::size_of::<String>()),
    )?;
    let mut references = Vec::with_capacity(dependencies.len());
    for id in dependencies {
        check(control, meter)?;
        meter.admit_allocation("capture/".len() + id.len())?;
        references.push(format!("capture/{id}"));
    }
    Metadata::open(home, true)?.put_serialized_referencing(
        &prepared.key,
        &prepared.document,
        &references,
        control,
        || check(control, meter),
        |length| {
            check(control, meter)?;
            // Includes raw bytes, decoded strings/arrays, the identity clone
            // and Go-escaped identity buffers. The document is at most 1 MiB.
            meter.admit_dependency(length)
        },
        |key, raw| {
            check(control, meter)?;
            let capture: super::CaptureRef = serde_json::from_slice(raw)?;
            super::validate_capture(
                &capture,
                key.strip_prefix("capture/").expect("capture reference"),
            )
        },
        publish,
    )?;
    Ok(prepared.value)
}

/// Uses the held verified blob; no body copy, reopen or SHA256 scan is added.
pub fn commit_verified_capture_budgeted(
    blob: &VerifiedBlob,
    header: CaptureHeader<'_>,
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    let prepared = prepare(blob.reference(), header, &[], control, meter)?;
    let _guard = storage::ExclusiveLock::resource(
        &blob.home.join("locks"),
        &format!("blob:{}", blob.reference().sha256),
    )?;
    persist(&blob.home, prepared, control, meter)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Meter;
    impl CaptureAccounting for Meter {
        fn check(&self) -> Result<()> {
            Ok(())
        }
        fn admit_allocation(&self, _: usize) -> Result<()> {
            Ok(())
        }
        fn admit_work(&self, _: usize) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn streamed_identity_matches_fixed_go_html_unicode_golden_and_legacy_identity() {
        let capture = super::super::CaptureRef {
            schema: "lycheedev.capture.v1".into(),
            id: String::new(),
            blob: BlobRef {
                sha256: "a".repeat(64),
                bytes: 3,
            },
            media_type: "application/<json>&".into(),
            provenance: serde_json::from_value(
                serde_json::json!({"kind":"fixture","locator":"<tag>&界🙂\u{2028}\u{2029}"}),
            )
            .unwrap(),
            complete: true,
            truncated: false,
            captured_at: "2026-10-13T00:00:00Z".into(),
            dependencies: Vec::new(),
        };
        let expected = "CAP-0a6dcaebb636641e87100a48d4aaf5047a8dd14eb11e6e2b6de8ee5be8d4c9f5";
        assert_eq!(super::super::capture_identity(&capture).unwrap(), expected);
        let document = CaptureDocument {
            schema: "lycheedev.capture.v1",
            id: "",
            blob: &capture.blob,
            media_type: &capture.media_type,
            provenance: capture.provenance.clone(),
            complete: capture.complete,
            truncated: capture.truncated,
            captured_at: capture.captured_at.clone(),
            dependencies: &[],
        };
        let control = OperationControl::default();
        let mut writer = MeteredWriter {
            sink: Vec::new(),
            control: &control,
            meter: &Meter,
            length: 0,
            maximum: usize::MAX,
            failure: None,
        };
        serialize(&document, &mut writer, IdentityFormatter).unwrap();
        assert_eq!(writer.sink, storage::identity_json(&capture).unwrap());
        assert_eq!(format!("CAP-{}", storage::sha256(&writer.sink)), expected);
        assert!(
            std::str::from_utf8(&writer.sink)
                .unwrap()
                .contains("\\u003ctag\\u003e\\u0026界🙂\\u2028\\u2029")
        );
    }

    #[test]
    fn dependency_identity_preserves_fixed_go_field_and_slice_order() {
        let dependencies = vec![
            format!("CAP-{}", "b".repeat(64)),
            format!("CAP-{}", "c".repeat(64)),
            format!("CAP-{}", "b".repeat(64)),
        ];
        let capture = super::super::CaptureRef {
            schema: "lycheedev.capture.v1".into(),
            id: String::new(),
            blob: BlobRef {
                sha256: "a".repeat(64),
                bytes: 3,
            },
            media_type: "text/plain".into(),
            provenance: serde_json::from_value(
                serde_json::json!({"kind":"fixture","locator":"owner"}),
            )
            .unwrap(),
            complete: true,
            truncated: false,
            captured_at: "2026-10-13T00:00:00Z".into(),
            dependencies,
        };
        // SHA256 of the explicit Go archive.go CaptureRef field order, with
        // dependencies last; ordered duplicates are retained, never sorted.
        let expected = "CAP-656e3ba1eecd10de6981b8a01d668b1c4f4fb361aa9a8f4b9bb8c842dc64cc5a";
        assert_eq!(super::super::capture_identity(&capture).unwrap(), expected);
        let dependencies: Vec<_> = capture.dependencies.iter().map(String::as_str).collect();
        let document = CaptureDocument {
            schema: "lycheedev.capture.v1",
            id: "",
            blob: &capture.blob,
            media_type: &capture.media_type,
            provenance: capture.provenance.clone(),
            complete: capture.complete,
            truncated: capture.truncated,
            captured_at: capture.captured_at.clone(),
            dependencies: &dependencies,
        };
        let control = OperationControl::default();
        let mut writer = MeteredWriter {
            sink: Vec::new(),
            control: &control,
            meter: &Meter,
            length: 0,
            maximum: usize::MAX,
            failure: None,
        };
        serialize(&document, &mut writer, IdentityFormatter).unwrap();
        assert_eq!(writer.sink, storage::identity_json(&capture).unwrap());
        assert_eq!(format!("CAP-{}", storage::sha256(&writer.sink)), expected);
    }
}
