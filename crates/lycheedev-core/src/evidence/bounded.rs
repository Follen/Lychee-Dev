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
fn check_with(
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    check(control)?;
    supplemental()
}
fn same_error(left: &ToolkitError, right: &ToolkitError) -> bool {
    left.code == right.code
        && left.exit_code == right.exit_code
        && left.message == right.message
        && left.details == right.details
}
// Preserve a primary failure and a distinct later stop. Re-observing the same
// sticky stop is one cause; cleanup failures are never deduplicated here.
fn finish_check<T>(result: Result<T>, stopped: Result<()>) -> Result<T> {
    if let (Err(primary), Err(stop)) = (&result, &stopped)
        && same_error(primary, stop)
    {
        return result;
    }
    crate::metadata::historical_write::finish_historical(result, stopped)
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
    digest_bytes_with_check(bytes, control, &mut || Ok(()))
}
fn digest_bytes_with_check(
    bytes: &[u8],
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<String> {
    let mut digest = Sha256::new();
    for block in bytes.chunks(BLOCK) {
        check_with(control, supplemental)?;
        digest.update(block);
    }
    check_with(control, supplemental)?;
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
    open_blob_with_check(home, reference, control, &mut || Ok(()))
}
fn open_blob_with_check(
    home: &Path,
    reference: BlobRef,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<OpenBlob> {
    check_with(control, supplemental)?;
    let path = blob_path(home, &reference.sha256)?;
    storage::guard_path(home, &path)?;
    let directory = Directory::open(path.parent().expect("blob shard"))?;
    check_with(control, supplemental)?;
    // Preserve size/kind admission even when an existing writer would prevent
    // taking the subsequent immutable data-read handle. No body is allocated.
    validate_metadata(
        &directory.metadata(path.file_name().expect("blob digest name"))?,
        reference.bytes,
    )?;
    check_with(control, supplemental)?;
    let file = directory.open_file(path.file_name().expect("blob digest name"))?;
    let result = (|| {
        let info = file.metadata()?;
        validate_metadata(&info, reference.bytes)?;
        check_with(control, supplemental)
    })();
    if let Err(error) = result {
        return crate::metadata::historical_write::finish_historical(
            Err(error),
            lycheedev_windows::filesystem::close_file(file).map_err(Into::into),
        );
    }
    Ok(OpenBlob {
        file,
        _directory: directory,
        reference,
    })
}
fn scan_blob(
    opened: OpenBlob,
    control: Option<&OperationControl>,
    accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    scan_blob_with_check(opened, control, &mut || Ok(()), accept)
}
fn scan_blob_with_check(
    mut opened: OpenBlob,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
    accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let result = scan_open_blob_with_check(&mut opened, control, supplemental, accept);
    let result = crate::metadata::historical_write::finish_historical(
        result,
        lycheedev_windows::filesystem::close_file(opened.file).map_err(Into::into),
    );
    finish_check(result, check_with(control, supplemental))
}
fn scan_open_blob(
    opened: &mut OpenBlob,
    control: Option<&OperationControl>,
    accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    scan_open_blob_with_check(opened, control, &mut || Ok(()), accept)
}
fn scan_open_blob_with_check(
    opened: &mut OpenBlob,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
    mut accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let mut digest = Sha256::new();
    let mut remaining = opened.reference.bytes;
    let mut buffer = [0u8; BLOCK];
    while remaining != 0 {
        check_with(control, supplemental)?;
        let admitted = buffer.len().min(remaining as usize);
        let read = opened
            .file
            .read(&mut buffer[..admitted])
            .map_err(ToolkitError::from)
            .and_then(|n| {
                if n == 0 {
                    Err(integrity("original blob ended before its declared length"))
                } else {
                    Ok(n)
                }
            });
        let n = finish_check(read, check_with(control, supplemental))?;
        digest.update(&buffer[..n]);
        accept(&buffer[..n])?;
        remaining -= n as u64;
    }
    check_with(control, supplemental)?;
    let read = opened
        .file
        .read(&mut buffer[..1])
        .map_err(ToolkitError::from)
        .and_then(|n| {
            if n != 0 || hex::encode(digest.finalize()) != opened.reference.sha256 {
                Err(integrity("original blob digest or length differs"))
            } else {
                Ok(())
            }
        });
    finish_check(read, check_with(control, supplemental))
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
    read_blob_bounded_with_check(home, blob, maximum, control, || Ok(()))
}
/// An independent caller stop is observed inside actual open and block reads.
/// This adds no usage charge: cached disk bytes are not network bytes.
pub fn read_blob_bounded_with_check(
    home: &Path,
    blob: &Value,
    maximum: usize,
    control: &OperationControl,
    mut supplemental: impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    check_with(Some(control), &mut supplemental)?;
    workspace::open(home)?;
    read_internal_with_check(home, blob, maximum, Some(control), &mut supplemental)
}
pub(super) fn read_internal(
    home: &Path,
    blob: &Value,
    maximum: usize,
    control: Option<&OperationControl>,
) -> Result<Vec<u8>> {
    read_internal_with_check(home, blob, maximum, control, &mut || Ok(()))
}
fn read_internal_with_check(
    home: &Path,
    blob: &Value,
    maximum: usize,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<Vec<u8>> {
    check_with(control, supplemental)?;
    let reference = admitted_ref(blob, maximum as u64)?;
    let opened = open_blob_with_check(home, reference, control, supplemental)?;
    let mut bytes = Vec::new();
    if bytes
        .try_reserve_exact(opened.reference.bytes as usize)
        .is_err()
    {
        return crate::metadata::historical_write::finish_historical(
            Err(limit("body allocation was not admitted")),
            lycheedev_windows::filesystem::close_file(opened.file).map_err(Into::into),
        );
    }
    scan_blob_with_check(opened, control, supplemental, |block| {
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
    publish_blob_bounded_with_check(home, bytes, maximum, control, || Ok(()))
}
pub fn publish_blob_bounded_with_check(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: &OperationControl,
    mut supplemental: impl FnMut() -> Result<()>,
) -> Result<Value> {
    publish_internal_with_check(home, bytes, maximum, Some(control), &mut supplemental)
}
pub(super) fn publish_internal(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: Option<&OperationControl>,
) -> Result<Value> {
    publish_internal_with_check(home, bytes, maximum, control, &mut || Ok(()))
}
fn publish_internal_with_check(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<Value> {
    check_with(control, supplemental)?;
    workspace::open(home)?;
    if bytes.len() > maximum || bytes.len() as u64 > MAX_CAPTURE_BYTES {
        return Err(limit("blob exceeds budget"));
    }
    let _home = Directory::open_shared(home)?;
    workspace::open(home)?;
    let digest = digest_bytes_with_check(bytes, control, supplemental)?;
    let guard =
        ExclusiveLock::resource_checked(&home.join("locks"), &format!("blob:{digest}"), || {
            check_with(control, supplemental)
        })?;
    let result = publish_locked_with_check(home, bytes, maximum, digest, control, supplemental)
        .and_then(|reference| serde_json::to_value(reference).map_err(Into::into));
    let result = crate::metadata::historical_write::finish_historical(result, guard.close());
    finish_check(result, check_with(control, supplemental))
}
pub(super) fn publish_locked(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    digest: String,
    control: Option<&OperationControl>,
) -> Result<BlobRef> {
    publish_locked_with_check(home, bytes, maximum, digest, control, &mut || Ok(()))
}
fn publish_locked_with_check(
    home: &Path,
    bytes: &[u8],
    maximum: usize,
    digest: String,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
) -> Result<BlobRef> {
    check_with(control, supplemental)?;
    let path = blob_path(home, &digest)?;
    let reference = BlobRef {
        sha256: digest,
        bytes: bytes.len() as u64,
    };
    storage::guard_path(home, &path)?;
    if path.exists() {
        // This reference was just computed from the admitted body. Verify it
        // directly instead of allocating and re-parsing a temporary JSON Value.
        let opened = open_blob_with_check(home, reference.clone(), control, supplemental)?;
        scan_blob_with_check(opened, control, supplemental, |_| Ok(()))?;
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
        let mut first_stop = None;
        let result = destination
            .publish(bytes, maximum as u64, || {
                check_with(control, supplemental).map_err(|error| {
                    first_stop.get_or_insert_with(|| error.clone());
                    io::Error::new(io::ErrorKind::Interrupted, error)
                })
            });
        let returned_callback = result.as_ref().err().is_some_and(|error| {
            error.get_ref().and_then(|inner| inner.downcast_ref::<ToolkitError>()).is_some()
        });
        let result = result.map_err(io_error);
        // Destination may report its stage-discard failure in place of the
        // callback error. Retain the original typed stop and the later failure.
        let result = finish_publication(first_stop, result, returned_callback);
        finish_check(result, check_with(control, supplemental))?;
    }
    check_with(control, supplemental)?;
    Ok(reference)
}
fn finish_publication(
    first_stop: Option<ToolkitError>,
    result: Result<()>,
    returned_callback: bool,
) -> Result<()> {
    match (first_stop, result) {
        (Some(stop), Err(error)) if !returned_callback || !same_error(&stop, &error) => {
            crate::metadata::historical_write::finish_historical(Err(stop), Err(error))
        }
        (Some(stop), Ok(())) => Err(stop),
        (_, result) => result,
    }
}
pub fn commit_capture_bounded(
    home: &Path,
    input: CaptureInput<'_>,
    control: &OperationControl,
) -> Result<Value> {
    commit_capture_bounded_with_check(home, input, control, || Ok(()))
}
pub fn commit_capture_bounded_with_check(
    home: &Path,
    input: CaptureInput<'_>,
    control: &OperationControl,
    supplemental: impl FnMut() -> Result<()>,
) -> Result<Value> {
    commit_internal_observed_with_check(home, input, Some(control), supplemental, |_| {})
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
    publication: impl FnMut(CapturePublication),
) -> Result<Value> {
    commit_internal_observed_with_check(home, input, control, || Ok(()), publication)
}
fn commit_internal_observed_with_check(
    home: &Path,
    input: CaptureInput<'_>,
    control: Option<&OperationControl>,
    mut supplemental: impl FnMut() -> Result<()>,
    mut publication: impl FnMut(CapturePublication),
) -> Result<Value> {
    check_with(control, &mut supplemental)?;
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
    let digest = digest_bytes_with_check(input.bytes, control, &mut supplemental)?;
    // Publication and manifest insertion hold the same content guard as remove.
    let guard =
        ExclusiveLock::resource_checked(&home.join("locks"), &format!("blob:{digest}"), || {
            check_with(control, &mut supplemental)
        })?;
    let result = (|| {
        publication(CapturePublication::BeforeBlob);
        let blob = publish_locked_with_check(
            home,
            input.bytes,
            input.maximum_bytes,
            digest,
            control,
            &mut supplemental,
        )?;
        publication(CapturePublication::BlobPublished);
        check_with(control, &mut supplemental)?;
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
        check_with(control, &mut supplemental)?;
        let key = format!("capture/{}", capture.id);
        if let Some(control) = control {
            let mut metadata =
                Metadata::open_controlled_with_check(home, true, control, &mut supplemental)?;
            let written = metadata.put_controlled_with_check(
                &key,
                &capture,
                None,
                control,
                &mut supplemental,
            );
            if written.is_ok() {
                publication(CapturePublication::ManifestPublished);
            }
            crate::metadata::historical_write::finish_historical(written, metadata.close())?;
        } else {
            Metadata::open(home, true)?.put(&key, &capture, None)?;
            publication(CapturePublication::ManifestPublished);
        }
        check_with(control, &mut supplemental)?;
        Ok(serde_json::to_value(capture)?)
    })();
    let result = crate::metadata::historical_write::finish_historical(result, guard.close());
    finish_check(result, check_with(control, &mut supplemental))
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

#[cfg(test)]
mod supplemental_tests;
