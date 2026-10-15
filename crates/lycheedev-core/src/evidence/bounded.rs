//! Caller-admitted evidence bodies. Hashing, existing-body verification and
//! publication use fixed blocks and the original caller control.
use super::{BlobRef, CaptureRef, blob_path, capture_identity};
use crate::{
    Result, ToolkitError,
    control::OperationControl,
    metadata::Metadata,
    storage::{self, ExclusiveLock},
    workspace,
};
use lycheedev_windows::filesystem::{Destination, Directory};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const BLOCK: usize = 65536;
pub const DEFAULT_CAPTURE_BYTES: usize = 128 << 20;
/// The producer chooses a smaller budget. Frozen Go
/// `internal/records/video_demux.go::normalizeAssetDemux` bounds MaxInputBytes,
/// MaxFrameBytes and MaxOutputBytes at 1<<40 (public CLI admission is smaller).
/// This is an upper bound, never an allocation reservation.
pub const MAX_CAPTURE_BYTES: u64 = 1 << 40;

pub struct CaptureInput<'a> {
    pub bytes: &'a [u8],
    pub maximum_bytes: usize,
    pub media_type: &'a str,
    pub provenance: Value,
    pub complete: bool,
    pub truncated: bool,
}
fn check(control: Option<&OperationControl>) -> Result<()> {
    control.map_or(Ok(()), OperationControl::check)
}
fn limit(message: &str) -> ToolkitError {
    ToolkitError::new("vault.blob_limit", message, 4)
}
fn integrity(message: &str) -> ToolkitError {
    ToolkitError::new("vault.blob_integrity", message, 4)
}
fn io_error(error: io::Error) -> ToolkitError {
    if let Some(control) = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<ToolkitError>())
    {
        control.clone()
    } else {
        ToolkitError::io(error)
    }
}
pub(super) fn digest_bytes(bytes: &[u8], control: Option<&OperationControl>) -> Result<String> {
    let mut digest = Sha256::new();
    for block in bytes.chunks(BLOCK) {
        check(control)?;
        digest.update(block);
    }
    check(control)?;
    Ok(hex::encode(digest.finalize()))
}
fn admitted_ref(blob: &Value, maximum: u64) -> Result<BlobRef> {
    let digest = blob["sha256"]
        .as_str()
        .ok_or_else(|| ToolkitError::invalid("blob digest missing"))?;
    let length = blob["bytes"]
        .as_u64()
        .ok_or_else(|| ToolkitError::invalid("blob length missing"))?;
    if length > maximum || length > MAX_CAPTURE_BYTES {
        return Err(limit("blob exceeds read budget"));
    }
    // Validate the digest before any file operation or allocation.
    blob_path(Path::new(""), digest)?;
    Ok(BlobRef {
        sha256: digest.into(),
        bytes: length,
    })
}
struct OpenBlob {
    file: File,
    _directory: Directory,
    reference: BlobRef,
}
/// A fully scanned immutable file. Keep this handle through its consumer;
/// capture/export need not reopen a path or rehash already verified bytes.
pub struct VerifiedBlob {
    opened: OpenBlob,
    pub(super) home: PathBuf,
    control: OperationControl,
}
impl VerifiedBlob {
    pub fn reference(&self) -> &BlobRef {
        &self.opened.reference
    }
    pub fn rewind(&mut self) -> Result<()> {
        self.control.check()?;
        self.opened.file.seek(SeekFrom::Start(0))?;
        Ok(())
    }
}
impl Read for VerifiedBlob {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.control.check().map_err(io::Error::other)?;
        let n = out.len().min(BLOCK);
        self.opened.file.read(&mut out[..n])
    }
}
fn validate_metadata(info: &fs::Metadata, length: u64) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if info.file_attributes() & 0x400 != 0 {
            return Err(integrity("original blob is a reparse point"));
        }
    }
    if !info.is_file() || info.len() != length {
        return Err(integrity("original blob length or kind differs"));
    }
    Ok(())
}
fn open_blob(
    home: &Path,
    reference: BlobRef,
    control: Option<&OperationControl>,
) -> Result<OpenBlob> {
    check(control)?;
    let path = blob_path(home, &reference.sha256)?;
    storage::guard_path(home, &path)?;
    let directory = Directory::open(path.parent().expect("blob shard"))?;
    // Preserve size/kind admission even when an existing writer would prevent
    // taking the subsequent immutable data-read handle. No body is allocated.
    validate_metadata(
        &directory.metadata(path.file_name().expect("blob digest name"))?,
        reference.bytes,
    )?;
    let file = directory.open_file(path.file_name().expect("blob digest name"))?;
    let info = file.metadata()?;
    validate_metadata(&info, reference.bytes)?;
    check(control)?;
    Ok(OpenBlob {
        file,
        _directory: directory,
        reference,
    })
}
fn scan_blob(
    mut opened: OpenBlob,
    control: Option<&OperationControl>,
    accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    scan_open_blob(&mut opened, control, accept)
}
fn scan_open_blob(
    opened: &mut OpenBlob,
    control: Option<&OperationControl>,
    mut accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut digest = Sha256::new();
    let mut remaining = opened.reference.bytes;
    let mut buffer = [0u8; BLOCK];
    while remaining != 0 {
        check(control)?;
        let admitted = buffer.len().min(remaining as usize);
        let n = opened.file.read(&mut buffer[..admitted])?;
        if n == 0 {
            return Err(integrity("original blob ended before its declared length"));
        }
        check(control)?;
        digest.update(&buffer[..n]);
        accept(&buffer[..n])?;
        remaining -= n as u64;
    }
    check(control)?;
    if opened.file.read(&mut buffer[..1])? != 0
        || hex::encode(digest.finalize()) != opened.reference.sha256
    {
        return Err(integrity("original blob digest or length differs"));
    }
    check(control)
}
/// One SHA256 scan, with an optional format-identity observer (for example the
/// selected CASC CKey). The returned file retains deny-write/delete ownership.
pub fn open_verified_blob_bounded(
    home: &Path,
    blob: &Value,
    maximum: u64,
    control: &OperationControl,
    accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<VerifiedBlob> {
    workspace::open(home)?;
    let mut opened = open_blob(home, admitted_ref(blob, maximum)?, Some(control))?;
    scan_open_blob(&mut opened, Some(control), accept)?;
    opened.file.seek(SeekFrom::Start(0))?;
    Ok(VerifiedBlob { opened, home: home.to_owned(), control: control.clone() })
}

struct DigestReader<'a, R: Read> {
    source: &'a mut R,
    expected: &'a str,
    hash: Sha256,
    terminal: bool,
}
impl<R: Read> Read for DigestReader<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.terminal { return Ok(0); }
        let n = self.source.read(out)?;
        if n == 0 {
            if hex::encode(self.hash.clone().finalize()) != self.expected {
                return Err(io::Error::other(integrity("published stream digest differs")));
            }
            self.terminal = true;
        } else {
            self.hash.update(&out[..n]);
        }
        Ok(n)
    }
}

/// The producer establishes the expected SHA256 before this call. Both the
/// source's terminal error and SHA256 are observed before the staged rename.
pub fn publish_blob_reader_bounded(
    home: &Path,
    reader: &mut impl Read,
    reference: &BlobRef,
    maximum: u64,
    control: &OperationControl,
) -> Result<VerifiedBlob> {
    control.check()?;
    workspace::open(home)?;
    let value = serde_json::to_value(reference)?;
    let admitted = admitted_ref(&value, maximum)?;
    let _home = Directory::open_shared(home)?;
    let _guard = ExclusiveLock::resource(&home.join("locks"), &format!("blob:{}", admitted.sha256))?;
    let path = blob_path(home, &admitted.sha256)?;
    storage::guard_path(home, &path)?;
    if !path.try_exists()? {
        let shard = path.parent().expect("blob shard");
        match fs::create_dir(shard) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        storage::guard_path(home, &path)?;
        let destination = Destination::open(&path, false)?;
        let mut checked = DigestReader { source: reader, expected: &admitted.sha256, hash: Sha256::new(), terminal: false };
        destination.publish_reader(&mut checked, admitted.bytes, maximum, || {
            control.check().map_err(io::Error::other)
        }).map_err(io_error)?;
    }
    // Open once at this boundary; callers keep the returned immutable handle
    // through capture/export instead of validating and reopening the path.
    open_verified_blob_bounded(home, &value, maximum, control, |_| Ok(()))
}

/// Capture an already verified held blob without allocating or rehashing it.
pub fn commit_verified_capture(
    blob: &VerifiedBlob,
    media_type: &str,
    provenance: Value,
    complete: bool,
    truncated: bool,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    if media_type.is_empty()
        || provenance["kind"].as_str().unwrap_or("").is_empty()
        || provenance["locator"].as_str().unwrap_or("").is_empty()
        || complete && truncated
    {
        return Err(ToolkitError::new("evidence.invalid_capture", "invalid media/provenance/completeness", 4));
    }
    let _guard = ExclusiveLock::resource(&blob.home.join("locks"), &format!("blob:{}", blob.reference().sha256))?;
    let mut capture = CaptureRef {
        schema: "lycheedev.capture.v1".into(), id: String::new(),
        blob: blob.reference().clone(), media_type: media_type.into(),
        provenance: serde_json::from_value(provenance)?, complete, truncated,
        captured_at: storage::utc_now(),
        dependencies: Vec::new(),
    };
    capture.id = capture_identity(&capture)?;
    Metadata::open(&blob.home, true)?.put(&format!("capture/{}", capture.id), &capture, None)?;
    control.check()?;
    Ok(serde_json::to_value(capture)?)
}
/// Verify without allocating a second body. The fixed maximum and exact open
/// file length are checked before the first read.
pub fn verify_blob_bounded(
    home: &Path,
    blob: &Value,
    maximum: u64,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    workspace::open(home)?;
    verify_internal(home, blob, maximum, Some(control))
}
pub(super) fn verify_internal(
    home: &Path,
    blob: &Value,
    maximum: u64,
    control: Option<&OperationControl>,
) -> Result<()> {
    check(control)?;
    let reference = admitted_ref(blob, maximum)?;
    scan_blob(open_blob(home, reference, control)?, control, |_| Ok(()))
}
pub fn read_blob_bounded(
    home: &Path,
    blob: &Value,
    maximum: usize,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    control.check()?;
    workspace::open(home)?;
    read_internal(home, blob, maximum, Some(control))
}
pub(super) fn read_internal(
    home: &Path,
    blob: &Value,
    maximum: usize,
    control: Option<&OperationControl>,
) -> Result<Vec<u8>> {
    check(control)?;
    let reference = admitted_ref(blob, maximum as u64)?;
    let opened = open_blob(home, reference, control)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(opened.reference.bytes as usize)
        .map_err(|_| limit("body allocation was not admitted"))?;
    scan_blob(opened, control, |block| {
        bytes.extend_from_slice(block);
        Ok(())
    })?;
    Ok(bytes)
}
pub fn publish_blob_bounded(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: &OperationControl,
) -> Result<Value> {
    publish_internal(home, bytes, maximum, Some(control))
}
pub(super) fn publish_internal(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: Option<&OperationControl>,
) -> Result<Value> {
    check(control)?;
    workspace::open(home)?;
    if bytes.len() > maximum || bytes.len() as u64 > MAX_CAPTURE_BYTES {
        return Err(limit("blob exceeds budget"));
    }
    let _home = Directory::open_shared(home)?;
    workspace::open(home)?;
    let digest = digest_bytes(bytes, control)?;
    let _lock = ExclusiveLock::resource(&home.join("locks"), &format!("blob:{digest}"))?;
    let reference = publish_locked(home, bytes, maximum, digest, control)?;
    Ok(serde_json::to_value(reference)?)
}
pub(super) fn publish_locked(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    digest: String,
    control: Option<&OperationControl>,
) -> Result<BlobRef> {
    check(control)?;
    let path = blob_path(home, &digest)?;
    let mut reference = BlobRef {
        sha256: digest,
        bytes: bytes.len() as u64,
    };
    storage::guard_path(home, &path)?;
    if path.exists() {
        // This reference was just computed from the admitted body. Verify it
        // directly instead of allocating and re-parsing a temporary JSON Value.
        let mut opened = open_blob(home, reference, control)?;
        scan_open_blob(&mut opened, control, |_| Ok(()))?;
        reference = opened.reference;
    } else {
        let shard = path.parent().expect("blob shard");
        // Only create the single known shard; do not recursively follow an
        // attacker-controlled path. The selected home ancestors remain held.
        match fs::create_dir(shard) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
        storage::guard_path(home, &path)?;
        let destination = Destination::open(&path, false)?;
        destination
            .publish(bytes, maximum as u64, || {
                check(control).map_err(|error| io::Error::new(io::ErrorKind::Interrupted, error))
            })
            .map_err(io_error)?;
    }
    check(control)?;
    Ok(reference)
}
pub fn commit_capture_bounded(
    home: &Path,
    input: CaptureInput<'_>,
    control: &OperationControl,
) -> Result<Value> {
    commit_internal(home, input, Some(control))
}
pub(super) fn commit_internal(
    home: &Path,
    input: CaptureInput<'_>,
    control: Option<&OperationControl>,
) -> Result<Value> {
    commit_internal_observed(home, input, control, |_| {})
}
#[derive(Clone, Copy)]
enum CapturePublication {
    BeforeBlob,
    BlobPublished,
    ManifestPublished,
}
fn commit_internal_observed(
    home: &Path,
    input: CaptureInput<'_>,
    control: Option<&OperationControl>,
    mut publication: impl FnMut(CapturePublication),
) -> Result<Value> {
    check(control)?;
    if input.media_type.is_empty()
        || input.provenance["kind"].as_str().unwrap_or("").is_empty()
        || input.provenance["locator"]
            .as_str()
            .unwrap_or("")
            .is_empty()
        || input.complete && input.truncated
    {
        return Err(ToolkitError::new(
            "evidence.invalid_capture",
            "invalid media/provenance/completeness",
            4,
        ));
    }
    workspace::open(home)?;
    if input.bytes.len() > input.maximum_bytes || input.bytes.len() as u64 > MAX_CAPTURE_BYTES {
        return Err(limit("blob exceeds budget"));
    }
    let provenance = serde_json::from_value(input.provenance)?;
    let _home = Directory::open_shared(home)?;
    workspace::open(home)?;
    let digest = digest_bytes(input.bytes, control)?;
    // Publication and manifest insertion hold the same content guard as remove.
    let guard = match control {
        Some(control) => ExclusiveLock::resource_controlled(
            &home.join("locks"),
            &format!("blob:{digest}"),
            control,
        )?,
        None => ExclusiveLock::resource(&home.join("locks"), &format!("blob:{digest}"))?,
    };
    let result = (|| {
        publication(CapturePublication::BeforeBlob);
        let blob = publish_locked(home, input.bytes, input.maximum_bytes, digest, control)?;
        publication(CapturePublication::BlobPublished);
        check(control)?;
        let mut capture = CaptureRef {
            schema: "lycheedev.capture.v1".into(),
            id: String::new(),
            blob,
            media_type: input.media_type.into(),
            provenance,
            complete: input.complete,
            truncated: input.truncated,
            captured_at: storage::utc_now(),
            dependencies: Vec::new(),
        };
        capture.id = capture_identity(&capture)?;
        check(control)?;
        let key = format!("capture/{}", capture.id);
        if let Some(control) = control {
            let mut metadata = Metadata::open_controlled(home, true, control)?;
            let written = metadata.put_controlled(&key, &capture, None, control);
            if written.is_ok() {
                publication(CapturePublication::ManifestPublished);
            }
            crate::metadata::historical_write::finish_historical(written, metadata.close())?;
        } else {
            Metadata::open(home, true)?.put(&key, &capture, None)?;
            publication(CapturePublication::ManifestPublished);
        }
        check(control)?;
        Ok(serde_json::to_value(capture)?)
    })();
    let value = crate::metadata::historical_write::finish_historical(result, guard.close())?;
    check(control)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    #[test]
    fn real_body_scan_stops_after_caller_cancel_without_reading_next_block() {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-bounded-scan-{}-{}",
            std::process::id(),
            crate::storage::utc_now().replace([':', '.'], "")
        ));
        workspace::initialize(&path).unwrap();
        let bytes = vec![0x69u8; BLOCK * 3];
        let blob =
            publish_blob_bounded(&path, &bytes, bytes.len(), &OperationControl::default()).unwrap();
        let token = Arc::new(AtomicBool::new(false));
        let control = OperationControl::new(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            token.clone(),
        );
        let opened = open_blob(
            &path,
            admitted_ref(&blob, bytes.len() as u64).unwrap(),
            Some(&control),
        )
        .unwrap();
        let mut observed = 0;
        let error = scan_blob(opened, Some(&control), |block| {
            observed += block.len();
            token.store(true, Ordering::Release);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
        assert_eq!(observed, BLOCK);
        fs::remove_dir_all(&path).unwrap();
    }
}

#[cfg(test)]
mod controlled_tests;
