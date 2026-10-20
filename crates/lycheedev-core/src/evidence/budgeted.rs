//! Capture headers share the producer's ledger. Body allocation is already
//! admitted by the producer; the existing bounded body path keeps its control.
use super::{BlobRef, CaptureInput, Provenance, VerifiedBlob, bounded};
use crate::{
    Result, ToolkitError,
    control::OperationControl,
    metadata::{CaptureWriteState, Metadata},
    storage, workspace,
};
use bounded::lifecycle::{PathControl, capture_result, close_directory, finish_controls};
use lycheedev_windows::filesystem::Directory;
use serde::Serialize;
use serde_json::{
    Value,
    ser::{CompactFormatter, Formatter},
};
use sha2::{Digest, Sha256};
use std::{
    cell::Cell,
    io::{self, Write},
    path::Path,
};

const METADATA_BYTES: usize = 1_048_576;
const CAPTURE_ID_BYTES: usize = 68;
// A capture has three JSON maps with at most eleven entries each. Include
// their map nodes, Value slots and fixed field names in addition to the
// serialized length, which bounds the decoded string payload.
const VALUE_STRUCTURE_BYTES: usize = 4096;

/// Fixed diagnostic scopes of the existing private stream publisher. These
/// include nested producer/check work and are not additional accounting costs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BlobStreamPhase {
    SourceSpool,
    ShardCopy,
    FinalVerify,
    StageCreate,
    PrivateSpoolReadReady,
    FinalStageSeal,
    DigestLock,
    ShardOpen,
    Rename,
    WinnerOpen,
    CheckedClose,
}

struct NoopBlobStreamSpan;
impl Drop for NoopBlobStreamSpan {
    fn drop(&mut self) {}
}

/// Metadata allocations and work join the caller's existing ledger. JSON work
/// uses emitted/input bytes; each binary body pass uses one unit per 64 KiB.
/// Errors are returned unchanged, including cancellation and limits.
pub trait CaptureAccounting {
    /// Explicit producer capability. User-controlled provenance never selects a
    /// disk domain; Data adapters must preserve this method when wrapping Budget.
    fn evidence_domain(&self) -> crate::metadata::EvidenceDomain {
        crate::metadata::EvidenceDomain::Historical
    }
    fn check(&self) -> Result<()>;
    fn admit_allocation(&self, bytes: usize) -> Result<()>;
    fn admit_work(&self, units: usize) -> Result<()>;
    /// Borrow an optional diagnostic recorder. The default is a zero-sized
    /// no-op: no clock, allocation, checks or changed business result. Observers
    /// must not perform accounting or turn observation into a fallible action.
    fn blob_stream_phase(&self, _phase: BlobStreamPhase) -> impl Drop + '_ {
        NoopBlobStreamSpan
    }
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
    commit_capture_referencing(
        home,
        None,
        input,
        dependencies,
        dependencies,
        control,
        meter,
    )
}

/// Verify immutable inputs at publication under the same writer boundary as
/// removal, without making them permanent identity dependencies. Fixed Go SQL
/// holds WriteMetadata during its query but does not emit a dependencies field.
/// Like the existing path, a failure after blob publication can retain a whole
/// immutable blob; no partial capture manifest is committed.
pub fn commit_capture_budgeted_referencing(
    home: &Path,
    input: CaptureInput<'_>,
    references: &[&str],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    commit_capture_referencing(home, None, input, &[], references, control, meter)
}

/// Authenticate all typed manifest references in the same writer transaction,
/// while emitting only the caller's exact ordered identity dependencies.
/// Demux keeps frame references in its bounded payload and only the source in
/// Dependencies, so a valid 16 MiB manifest does not overflow a 1 MiB document.
pub fn commit_capture_budgeted_with_references(
    home: &Path,
    input: CaptureInput<'_>,
    dependencies: &[&str],
    references: &[&str],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    // Declared dependencies are always authenticated, even if an additional
    // references caller omitted one. Preserve additional-reference order first.
    if dependencies.iter().all(|id| references.contains(id)) {
        return commit_capture_referencing(
            home,
            None,
            input,
            dependencies,
            references,
            control,
            meter,
        );
    }
    check(control, meter)?;
    let capacity = references
        .len()
        .checked_add(dependencies.len())
        .ok_or_else(|| ToolkitError::invalid("capture reference capacity overflow"))?;
    meter.admit_allocation(capacity.saturating_mul(std::mem::size_of::<&str>()))?;
    let mut required = Vec::with_capacity(capacity);
    required.extend_from_slice(references);
    for id in dependencies {
        check(control, meter)?;
        if !required.contains(id) {
            required.push(id);
        }
    }
    commit_capture_referencing(home, None, input, dependencies, &required, control, meter)
}

/// Reuse a request-owned writer without widening the lifetime of the transaction.
/// Dependencies are authenticated afresh under this capture's BEGIN IMMEDIATE.
pub fn commit_capture_budgeted_with_dependencies_and_metadata(
    home: &Path,
    metadata: &mut Metadata,
    input: CaptureInput<'_>,
    dependencies: &[&str],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    commit_capture_referencing(
        home,
        Some(metadata),
        input,
        dependencies,
        dependencies,
        control,
        meter,
    )
}

pub fn commit_capture_budgeted_referencing_with_metadata(
    home: &Path,
    metadata: &mut Metadata,
    input: CaptureInput<'_>,
    references: &[&str],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    commit_capture_referencing(home, Some(metadata), input, &[], references, control, meter)
}

fn commit_capture_referencing(
    home: &Path,
    metadata: Option<&mut Metadata>,
    input: CaptureInput<'_>,
    dependencies: &[&str],
    required: &[&str],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    check(control, meter)?;
    if let Some(metadata) = metadata.as_deref() {
        metadata.check_capture_binding(home)?;
    }
    let header = CaptureHeader {
        media_type: input.media_type,
        provenance: input.provenance,
        complete: input.complete,
        truncated: input.truncated,
    };
    validate(&header)?;
    workspace::open_controlled_with_check(home, control, || meter.check())?;
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
        sha256: bounded::digest_bytes_with_check(input.bytes, Some(control), &mut || {
            meter.check()
        })?,
        bytes: input.bytes.len() as u64,
    };
    let prepared = prepare(&reference, header, dependencies, control, meter)?;
    let state = Cell::new(CaptureWriteState::NotCommitted);
    let home_owner = Directory::open_shared_controlled(
        home,
        &mut PathControl {
            control: Some(control),
            supplemental: &mut || meter.check(),
        },
    )?;
    let result = (|| {
        meter.admit_allocation(required.len().saturating_mul(std::mem::size_of::<String>()))?;
        let mut references = Vec::with_capacity(required.len());
        for id in required {
            check(control, meter)?;
            meter.admit_allocation("capture/".len() + id.len())?;
            references.push(format!("capture/{id}"));
        }
        crate::metadata::evidence_catalog::put_capture(
            home,
            meter.evidence_domain(),
            &prepared.key,
            &prepared.document,
            &references,
            control,
            &|| meter.check(),
            |length| meter.admit_dependency(length),
            |key, raw| {
                let capture: super::CaptureRef = serde_json::from_slice(raw)?;
                super::validate_capture(
                    &capture,
                    key.strip_prefix("capture/").expect("capture reference"),
                )
            },
            |owner| {
                check(control, meter)?;
                meter.admit_work(body_work)?;
                if meter.evidence_domain() == crate::metadata::EvidenceDomain::Data {
                    owner.publish_memory(input.bytes, input.maximum_bytes, &reference)?;
                    Ok(())
                } else {
                    let guard = storage::ExclusiveLock::resource_wait_checked(
                        &home.join("locks"),
                        &format!("blob:{}", reference.sha256),
                        Some(control.deadline()),
                        || check(control, meter),
                    )?;
                    let published = bounded::publish_locked_with_check(
                        home,
                        input.bytes,
                        input.maximum_bytes,
                        reference.sha256.clone(),
                        Some(control),
                        &mut || meter.check(),
                    )
                    .map(|_| ());
                    crate::metadata::historical_write::finish_historical(published, guard.close())
                }
            },
            &state,
        )
    })();
    let result = close_directory(result, home_owner);
    let result = finish_controls(result, Some(control), &mut || meter.check());
    capture_result(result, prepared.value, state.get())
}

/// Uses the held verified blob; no body copy, reopen or SHA256 scan is added.
/// The operation's supplied meter is checked inside the real writer/lock wait.
pub fn commit_verified_capture_budgeted(
    blob: &VerifiedBlob,
    header: CaptureHeader<'_>,
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    commit_verified_capture_in(blob, None, header, control, meter)
}

/// Borrow the request's existing writer; the caller must check its final close
/// before returning success. Capture commit and blob ownership stay synchronous.
pub fn commit_verified_capture_budgeted_with_metadata(
    blob: &VerifiedBlob,
    metadata: &mut Metadata,
    header: CaptureHeader<'_>,
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    commit_verified_capture_in(blob, Some(metadata), header, control, meter)
}

fn commit_verified_capture_in(
    blob: &VerifiedBlob,
    metadata: Option<&mut Metadata>,
    header: CaptureHeader<'_>,
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    let prepared = prepare(blob.reference(), header, &[], control, meter)?;
    let state = Cell::new(CaptureWriteState::NotCommitted);
    if let Some(metadata) = metadata.as_deref() {
        metadata.check_capture_binding(&blob.home)?;
    }
    let result = crate::metadata::evidence_catalog::put_capture(
        &blob.home,
        meter.evidence_domain(),
        &prepared.key,
        &prepared.document,
        &[],
        control,
        &|| meter.check(),
        |_| Ok(()),
        |_, _| Ok(()),
        |owner| {
            if meter.evidence_domain() == crate::metadata::EvidenceDomain::Data {
                owner.enroll_verified(blob)?;
            }
            Ok(())
        },
        &state,
    );
    let result = finish_controls(result, Some(control), &mut || meter.check());
    capture_result(result, prepared.value, state.get())
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

#[cfg(test)]
mod lifecycle_tests;

#[cfg(test)]
#[path = "budgeted/manifest_reference_tests.rs"]
mod manifest_reference_tests;

#[cfg(test)]
#[path = "budgeted/borrowed_tests.rs"]
mod borrowed_tests;

/// Data's legacy bounded producers retain their original supplemental check,
/// while explicitly selecting the measured authoritative evidence store.
pub fn commit_data_capture_bounded_with_check(
    home: &Path,
    input: CaptureInput<'_>,
    control: &OperationControl,
    supplemental: impl FnMut() -> Result<()>,
) -> Result<Value> {
    struct DataBounded<C>(std::cell::RefCell<C>);
    impl<C: FnMut() -> Result<()>> CaptureAccounting for DataBounded<C> {
        fn evidence_domain(&self) -> crate::metadata::EvidenceDomain {
            crate::metadata::EvidenceDomain::Data
        }
        fn check(&self) -> Result<()> {
            (*self.0.borrow_mut())()
        }
        fn admit_allocation(&self, _: usize) -> Result<()> {
            self.check()
        }
        fn admit_work(&self, _: usize) -> Result<()> {
            self.check()
        }
    }
    commit_capture_budgeted(
        home,
        input,
        control,
        &DataBounded(std::cell::RefCell::new(supplemental)),
    )
}
