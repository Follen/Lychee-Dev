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
use lycheedev_windows::filesystem::Directory;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

pub(super) mod lifecycle;
use lifecycle::{PathControl, finish_controls, open_workspace};
mod staged;
pub use staged::publish_blob_stream_bounded_with_check;

mod optional;
pub(super) use optional::verify_historical_blob_optional;
pub use optional::{
    open_verified_blob_bounded_optional_with_check, read_blob_bounded_optional_with_check,
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
pub(super) fn digest_bytes_with_check(
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
/// The existing query owner is retained, never a borrowed Store or new ledger.
/// Send preserves transfer to the consuming thread; no Sync/mutex is required.
type BlobCheck = Box<dyn FnMut() -> Result<()> + Send + 'static>;
fn optional_check(supplemental: &mut Option<BlobCheck>) -> Result<()> {
    match supplemental {
        Some(check) => check(),
        None => Ok(()),
    }
}
fn stream_check(control: &OperationControl, supplemental: &mut Option<BlobCheck>) -> Result<()> {
    control.check()?;
    optional_check(supplemental)
}
/// A fully scanned immutable file. Keep this handle through its consumer;
/// capture/export need not reopen a path or rehash already verified bytes.
pub struct VerifiedBlob {
    opened: OpenBlob,
    pub(super) home: PathBuf,
    control: OperationControl,
    supplemental: Option<BlobCheck>,
}
impl VerifiedBlob {
    pub fn reference(&self) -> &BlobRef {
        &self.opened.reference
    }
    pub fn rewind(&mut self) -> Result<()> {
        stream_check(&self.control, &mut self.supplemental)?;
        let result = self
            .opened
            .file
            .seek(SeekFrom::Start(0))
            .map(|_| ())
            .map_err(ToolkitError::from);
        optional::finish_checks(result, &self.control, &mut || {
            optional_check(&mut self.supplemental)
        })
    }
    /// Preserve complete typed IO/stop details. This reads at most one 64 KiB
    /// block; it neither records network usage nor implies complete consumption.
    pub fn read_checked(&mut self, out: &mut [u8]) -> Result<usize> {
        self.read(out).map_err(io_error)
    }
    /// Consume the held file once and check close, including after cancellation.
    /// Closing does not assert EOF or reverify a consumer's interpretation.
    pub fn close(self) -> Result<()> {
        self.close_after(Ok(()))
    }
    /// Close after dependent work, preserving its failure plus distinct close
    /// and stop causes. This is handle cleanup, not a stream-completion claim.
    pub fn close_after<T>(self, result: Result<T>) -> Result<T> {
        self.close_after_with(result, lycheedev_windows::filesystem::close_file)
    }
    fn close_after_with<T>(
        mut self,
        result: Result<T>,
        close: impl FnOnce(File) -> io::Result<()>,
    ) -> Result<T> {
        let closed = close(self.opened.file).map_err(ToolkitError::from);
        let result = crate::metadata::historical_write::finish_historical(result, closed);
        let result = optional::finish_directory(result, self.opened._directory);
        optional::finish_checks(result, &self.control, &mut || {
            optional_check(&mut self.supplemental)
        })
    }
}
impl Read for VerifiedBlob {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        stream_check(&self.control, &mut self.supplemental).map_err(io::Error::other)?;
        let n = out.len().min(BLOCK);
        let result = self.opened.file.read(&mut out[..n]);
        // Cached disk bytes are not network bytes; accounting remains with
        // the caller. A delivered read still observes both original scopes.
        optional::finish_checks(result.map_err(io_error), &self.control, &mut || {
            optional_check(&mut self.supplemental)
        })
        .map_err(io::Error::other)
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
    storage::guard_path_with_check(home, &path, || check_with(control, supplemental))?;
    let directory = Directory::open_controlled(
        path.parent().expect("blob shard"),
        &mut PathControl {
            control,
            supplemental,
        },
    )?;
    let result = (|| {
        let name = path.file_name().expect("blob name");
        validate_metadata(
            &directory.metadata_controlled(
                name,
                &mut PathControl {
                    control,
                    supplemental,
                },
            )?,
            reference.bytes,
        )?;
        let file = directory.open_file_controlled(
            name,
            &mut PathControl {
                control,
                supplemental,
            },
        )?;
        let result = (|| {
            validate_metadata(&file.metadata()?, reference.bytes)?;
            check_with(control, supplemental)
        })();
        match finish_controls(result, control, supplemental) {
            Ok(()) => Ok(file),
            Err(error) => crate::metadata::historical_write::finish_historical(
                Err(error),
                storage::close_file(file).map_err(Into::into),
            ),
        }
    })();
    match result {
        Ok(file) => Ok(OpenBlob {
            file,
            _directory: directory,
            reference,
        }),
        Err(error) => finish_controls(
            optional::finish_directory(Err(error), directory),
            control,
            supplemental,
        ),
    }
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
    lifecycle::close_blob(opened, result, control, supplemental)
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
    accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    scan_open_blob_accounted_with_check(opened, control, supplemental, accept, || Ok(()))
}
fn scan_open_blob_accounted_with_check(
    opened: &mut OpenBlob,
    control: Option<&OperationControl>,
    supplemental: &mut impl FnMut() -> Result<()>,
    mut accept: impl FnMut(&[u8]) -> Result<()>,
    mut admit_read: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut digest = Sha256::new();
    let mut remaining = opened.reference.bytes;
    let mut buffer = [0u8; BLOCK];
    while remaining != 0 {
        check_with(control, supplemental)?;
        admit_read()?;
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
    admit_read()?;
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
    open_verified_blob_internal(home, blob, maximum, control, accept, None)
}
/// Retain the caller's existing budget owner through verification and every
/// subsequent Read/rewind. Only the supplemental check is retained; the format
/// observer remains borrowed and runs during the initial verification scan.
pub fn open_verified_blob_bounded_with_check(
    home: &Path,
    blob: &Value,
    maximum: u64,
    control: &OperationControl,
    accept: impl FnMut(&[u8]) -> Result<()>,
    supplemental: impl FnMut() -> Result<()> + Send + 'static,
) -> Result<VerifiedBlob> {
    open_verified_blob_internal(
        home,
        blob,
        maximum,
        control,
        accept,
        Some(Box::new(supplemental)),
    )
}
fn open_verified_blob_internal(
    home: &Path,
    blob: &Value,
    maximum: u64,
    control: &OperationControl,
    accept: impl FnMut(&[u8]) -> Result<()>,
    mut supplemental: Option<BlobCheck>,
) -> Result<VerifiedBlob> {
    let opened = prepare_verified_blob(home, blob, maximum, control, accept, &mut supplemental)?;
    Ok(VerifiedBlob {
        opened,
        home: home.to_owned(),
        control: control.clone(),
        supplemental,
    })
}
fn prepare_verified_blob(
    home: &Path,
    blob: &Value,
    maximum: u64,
    control: &OperationControl,
    accept: impl FnMut(&[u8]) -> Result<()>,
    supplemental: &mut Option<BlobCheck>,
) -> Result<OpenBlob> {
    stream_check(control, supplemental)?;
    open_workspace(home, Some(control), &mut || optional_check(supplemental))?;
    let opened = open_blob_with_check(
        home,
        admitted_ref(blob, maximum)?,
        Some(control),
        &mut || optional_check(supplemental),
    )?;
    prepare_opened_verified_blob(opened, control, accept, supplemental)
}
fn prepare_opened_verified_blob(
    mut opened: OpenBlob,
    control: &OperationControl,
    accept: impl FnMut(&[u8]) -> Result<()>,
    supplemental: &mut Option<BlobCheck>,
) -> Result<OpenBlob> {
    let result = (|| {
        // Preserve the existing observer order: pre-read stop, actual read,
        // post-read stop, SHA256 then format/work observer. This adds no claim
        // that the observer accounts a disk read interrupted before its call.
        if supplemental.is_some() {
            scan_open_blob_with_check(
                &mut opened,
                Some(control),
                &mut || optional_check(supplemental),
                accept,
            )?;
        } else {
            scan_open_blob(&mut opened, Some(control), accept)?;
        }
        stream_check(control, supplemental)?;
        let result = opened
            .file
            .seek(SeekFrom::Start(0))
            .map(|_| ())
            .map_err(ToolkitError::from);
        optional::finish_checks(result, control, &mut || optional_check(supplemental))
    })();
    match result {
        Ok(()) => Ok(opened),
        Err(error) => close_opened_blob(opened, Err(error), control, supplemental),
    }
}
fn close_opened_blob<T>(
    opened: OpenBlob,
    result: Result<T>,
    control: &OperationControl,
    supplemental: &mut Option<BlobCheck>,
) -> Result<T> {
    let result = crate::metadata::historical_write::finish_historical(
        result,
        lycheedev_windows::filesystem::close_file(opened.file).map_err(ToolkitError::from),
    );
    let result = optional::finish_directory(result, opened._directory);
    optional::finish_checks(result, control, &mut || optional_check(supplemental))
}

struct DigestReader<'a, R: Read> {
    source: &'a mut R,
    expected: &'a str,
    hash: Sha256,
    terminal: bool,
}
impl<R: Read> Read for DigestReader<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.terminal {
            return Ok(0);
        }
        // The underlying reader owns actual-byte accounting and must record
        // consumed bytes before returning, including a concurrent stop/error.
        let n = match self.source.read(out) {
            Ok(n) => n,
            Err(error) => return Err(io::Error::other(io_error(error))),
        };
        if n == 0 {
            if hex::encode(self.hash.clone().finalize()) != self.expected {
                return Err(io::Error::other(integrity(
                    "published stream digest differs",
                )));
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
    publish_blob_reader_internal(home, reader, reference, maximum, control, None)
}
/// Publish with an owned independent stop check and retain that exact owner in
/// the returned immutable blob. The input reader is borrowed only during this
/// call; its network/work ledger is neither replaced nor charged a second time.
pub fn publish_blob_reader_bounded_with_check(
    home: &Path,
    reader: &mut impl Read,
    reference: &BlobRef,
    maximum: u64,
    control: &OperationControl,
    supplemental: impl FnMut() -> Result<()> + Send + 'static,
) -> Result<VerifiedBlob> {
    publish_blob_reader_internal(
        home,
        reader,
        reference,
        maximum,
        control,
        Some(Box::new(supplemental)),
    )
}
fn publish_blob_reader_internal(
    home: &Path,
    reader: &mut impl Read,
    reference: &BlobRef,
    maximum: u64,
    control: &OperationControl,
    mut supplemental: Option<BlobCheck>,
) -> Result<VerifiedBlob> {
    stream_check(control, &mut supplemental)?;
    open_workspace(home, Some(control), &mut || {
        optional_check(&mut supplemental)
    })?;
    let value = serde_json::to_value(reference)?;
    let admitted = admitted_ref(&value, maximum)?;
    let home_owner = Directory::open_shared_controlled(
        home,
        &mut PathControl {
            control: Some(control),
            supplemental: &mut || optional_check(&mut supplemental),
        },
    )?;
    let result = (|| {
        let guard = ExclusiveLock::resource_wait_checked(
            &home.join("locks"),
            &format!("blob:{}", admitted.sha256),
            Some(control.deadline()),
            || stream_check(control, &mut supplemental),
        )?;
        let result = (|| {
            let path = blob_path(home, &admitted.sha256)?;
            storage::guard_path_with_check(home, &path, || {
                stream_check(control, &mut supplemental)
            })?;
            if !path.try_exists()? {
                let shard = path.parent().expect("blob shard");
                match fs::create_dir(shard) {
                    Ok(()) => {}
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e.into()),
                }
                storage::guard_path_with_check(home, &path, || {
                    stream_check(control, &mut supplemental)
                })?;
                let mut checked = DigestReader {
                    source: reader,
                    expected: &admitted.sha256,
                    hash: Sha256::new(),
                    terminal: false,
                };
                lifecycle::publish_known(
                    home,
                    &path,
                    &mut checked,
                    &admitted,
                    maximum,
                    Some(control),
                    &mut || optional_check(&mut supplemental),
                )?;
            }
            prepare_verified_blob(
                home,
                &value,
                maximum,
                control,
                |_| Ok(()),
                &mut supplemental,
            )
        })();
        staged::finish_opened(result, guard.close(), control, &mut supplemental)
    })();
    let result = staged::finish_opened(
        result,
        optional::finish_directory(Ok(()), home_owner),
        control,
        &mut supplemental,
    );
    let stopped =
        optional::finish_checks(Ok(()), control, &mut || optional_check(&mut supplemental));
    let opened = staged::finish_opened(result, stopped, control, &mut supplemental)?;
    Ok(VerifiedBlob {
        opened,
        home: home.to_owned(),
        control: control.clone(),
        supplemental,
    })
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
        return Err(ToolkitError::new(
            "evidence.invalid_capture",
            "invalid media/provenance/completeness",
            4,
        ));
    }
    let guard = ExclusiveLock::resource_wait_checked(
        &blob.home.join("locks"),
        &format!("blob:{}", blob.reference().sha256),
        Some(control.deadline()),
        || control.check(),
    )?;
    let state = std::cell::Cell::new(crate::metadata::CaptureWriteState::NotCommitted);
    let mut value = None;
    let result = (|| {
        let mut capture = CaptureRef {
            schema: "lycheedev.capture.v1".into(),
            id: String::new(),
            blob: blob.reference().clone(),
            media_type: media_type.into(),
            provenance: serde_json::from_value(provenance)?,
            complete,
            truncated,
            captured_at: storage::utc_now(),
            dependencies: Vec::new(),
        };
        capture.id = capture_identity(&capture)?;
        control.check()?;
        let raw = serde_json::to_vec(&capture)?;
        value = Some(serde_json::to_value(&capture)?);
        lifecycle::persist_capture(
            &blob.home,
            &format!("capture/{}", capture.id),
            &raw,
            Some(control),
            &mut || Ok(()),
            &state,
        )
    })();
    let result = crate::metadata::historical_write::finish_historical(result, guard.close());
    let result = finish_controls(result, Some(control), &mut || Ok(()));
    match value {
        Some(value) => lifecycle::capture_result(result, value, state.get()),
        None => result.map(|()| unreachable!("capture preparation did not complete")),
    }
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
    open_workspace(home, Some(control), &mut || Ok(()))?;
    verify_internal(home, blob, maximum, Some(control))
}
// Go VerifyBlob has no producer/capture size ceiling. It is a streaming
// verification of an already authenticated signed historical reference.
pub(super) fn verify_historical_blob(
    home: &Path,
    digest: &str,
    bytes: i64,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    if bytes < 0 || !crate::selection::valid_digest(digest, 64) {
        return Err(integrity("vault.blob_integrity"));
    }
    let reference = BlobRef {
        sha256: digest.into(),
        bytes: bytes as u64,
    };
    scan_blob(
        open_blob(home, reference, Some(control))?,
        Some(control),
        |_| Ok(()),
    )
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
    open_workspace(home, Some(control), &mut supplemental)?;
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
        return lifecycle::close_blob(
            opened,
            Err(limit("body allocation was not admitted")),
            control,
            supplemental,
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
    open_workspace(home, control, supplemental)?;
    if bytes.len() > maximum || bytes.len() as u64 > MAX_CAPTURE_BYTES {
        return Err(limit("blob exceeds budget"));
    }
    let home_owner = Directory::open_shared_controlled(
        home,
        &mut PathControl {
            control,
            supplemental,
        },
    )?;
    let result = (|| {
        open_workspace(home, control, supplemental)?;
        let digest = digest_bytes_with_check(bytes, control, supplemental)?;
        let guard = ExclusiveLock::resource_wait_checked(
            &home.join("locks"),
            &format!("blob:{digest}"),
            control.map(OperationControl::deadline),
            || check_with(control, supplemental),
        )?;
        let result = publish_locked_with_check(home, bytes, maximum, digest, control, supplemental)
            .and_then(|reference| serde_json::to_value(reference).map_err(Into::into));
        crate::metadata::historical_write::finish_historical(result, guard.close())
    })();
    let result = optional::finish_directory(result, home_owner);
    finish_controls(result, control, supplemental)
}
pub(super) fn publish_locked_with_check(
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
    storage::guard_path_with_check(home, &path, || check_with(control, supplemental))?;
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
        storage::guard_path_with_check(home, &path, || check_with(control, supplemental))?;
        lifecycle::publish_bytes(
            home,
            &path,
            bytes,
            &reference,
            maximum as u64,
            control,
            supplemental,
        )?;
    }
    check_with(control, supplemental)?;
    Ok(reference)
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
    open_workspace(home, control, &mut supplemental)?;
    if input.bytes.len() > input.maximum_bytes || input.bytes.len() as u64 > MAX_CAPTURE_BYTES {
        return Err(limit("blob exceeds budget"));
    }
    let provenance = serde_json::from_value(input.provenance)?;
    let home_owner = Directory::open_shared_controlled(
        home,
        &mut PathControl {
            control,
            supplemental: &mut supplemental,
        },
    )?;
    let state = std::cell::Cell::new(crate::metadata::CaptureWriteState::NotCommitted);
    let mut value = None;
    let result = (|| {
        open_workspace(home, control, &mut supplemental)?;
        let digest = digest_bytes_with_check(input.bytes, control, &mut supplemental)?;
        let guard = ExclusiveLock::resource_wait_checked(
            &home.join("locks"),
            &format!("blob:{digest}"),
            control.map(OperationControl::deadline),
            || check_with(control, &mut supplemental),
        )?;
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
            let raw = serde_json::to_vec(&capture)?;
            value = Some(serde_json::to_value(&capture)?);
            let written = lifecycle::persist_capture(
                home,
                &format!("capture/{}", capture.id),
                &raw,
                control,
                &mut supplemental,
                &state,
            );
            if state.get() == crate::metadata::CaptureWriteState::Committed {
                publication(CapturePublication::ManifestPublished);
            }
            written
        })();
        crate::metadata::historical_write::finish_historical(result, guard.close())
    })();
    let result = optional::finish_directory(result, home_owner);
    let result = finish_controls(result, control, &mut supplemental);
    match value {
        Some(value) => lifecycle::capture_result(result, value, state.get()),
        None => result.map(|()| unreachable!("capture preparation did not complete")),
    }
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

#[cfg(test)]
mod stream_supplemental_tests;

#[cfg(test)]
mod optional_tests;

#[cfg(test)]
mod blob_wait_tests;
