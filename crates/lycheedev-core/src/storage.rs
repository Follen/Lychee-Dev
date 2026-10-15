use crate::{Result, ToolkitError};
// Business providers use the shared mechanism layer, rather than adding their
// own Win32 publication path or a direct platform dependency.
pub use lycheedev_windows::filesystem::destination::{FileControl, FileCost};
pub use lycheedev_windows::filesystem::{Destination, Directory, durable_move_noreplace};
mod owned_cleanup;
mod slot_preflight;
pub use owned_cleanup::{
    AuthenticatedCleanup, authenticate_cleanup_directory, authenticate_cleanup_file,
    hold_cleanup_ancestor,
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
pub use slot_preflight::slot_atomic_write_controlled;
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub fn timestamp(value: chrono::DateTime<chrono::Utc>) -> String {
    let text = value.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
    trim_time(text)
}
pub fn timestamp_offset(value: chrono::DateTime<chrono::FixedOffset>) -> String {
    trim_time(value.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true))
}
fn trim_time(text: String) -> String {
    let split = if text.ends_with('Z') {
        text.len() - 1
    } else {
        text.len().saturating_sub(6)
    };
    let (body, zone) = text.split_at(split);
    if let Some((head, fraction)) = body.split_once('.') {
        let fraction = fraction.trim_end_matches('0');
        if fraction.is_empty() {
            format!("{head}{zone}")
        } else {
            format!("{head}.{fraction}{zone}")
        }
    } else {
        text
    }
}
pub fn utc_now() -> String {
    timestamp(chrono::Utc::now())
}
pub fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
pub fn bounded_json(value: &impl Serialize, maximum: usize) -> Result<Vec<u8>> {
    struct Buffer {
        bytes: Vec<u8>,
        maximum: usize,
    }
    impl Write for Buffer {
        fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
            if raw.len() > self.maximum.saturating_sub(self.bytes.len()) {
                return Err(std::io::Error::other("serialized JSON exceeds admission"));
            }
            self.bytes.extend_from_slice(raw);
            Ok(raw.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: Vec::new(),
        maximum,
    };
    serde_json::to_writer(&mut buffer, value)?;
    Ok(buffer.bytes)
}
/// Authenticate a bounded regular file while holding Windows deny-write/delete ownership.
/// The caller keeps this handle through the operation that relies on its identity.
pub fn locked_sha256(
    path: &Path,
    maximum: u64,
    control: &crate::control::OperationControl,
) -> Result<(File, String)> {
    locked_sha256_with(
        path,
        maximum,
        || control.check(),
        lycheedev_windows::filesystem::close_file,
    )
}
fn locked_sha256_with(
    path: &Path,
    maximum: u64,
    mut check: impl FnMut() -> Result<()>,
    close: impl FnOnce(File) -> std::io::Result<()>,
) -> Result<(File, String)> {
    check()?;
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(ToolkitError::invalid("identity file is a link"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    let file = options.open(path)?;
    authenticate_locked_file(file, maximum, check, close)
}
fn authenticate_locked_file(
    mut file: File,
    maximum: u64,
    mut check: impl FnMut() -> Result<()>,
    close: impl FnOnce(File) -> std::io::Result<()>,
) -> Result<(File, String)> {
    match hash_locked_file(&mut file, maximum, &mut check) {
        // Identity consumers retain the original deny-write/delete handle
        // through their dependent operation. Do not shorten that lifetime.
        Ok(digest) => Ok((file, digest)),
        Err(error) => finish_file_with(file, Err(error), check, close),
    }
}
fn hash_locked_file(
    file: &mut File,
    maximum: u64,
    mut check: impl FnMut() -> Result<()>,
) -> Result<String> {
    use std::io::Read;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(ToolkitError::invalid("identity file exceeds admission"));
    }
    let mut digest = Sha256::new();
    let mut read_bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        check()?;
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        read_bytes = read_bytes
            .checked_add(n as u64)
            .ok_or_else(|| ToolkitError::invalid("identity size overflow"))?;
        if read_bytes > metadata.len() {
            return Err(ToolkitError::invalid("identity file grew during read"));
        }
        digest.update(&buffer[..n]);
    }
    if read_bytes != metadata.len() {
        return Err(ToolkitError::invalid("identity length changed during read"));
    }
    check()?;
    Ok(hex::encode(digest.finalize()))
}

/// Stable details discriminator for a file operation with multiple error causes.
pub const FILE_CLEANUP_SCHEMA: &str = "lycheedev.file-cleanup.v1";

/// Finish work that has retained a File, checking its close exactly once even
/// after cancellation or failure. The caller must retain the handle until its
/// last identity-dependent operation, then pass that operation's result here.
/// A final control check cannot suppress cleanup or publish a late success.
///
/// A lone error is unchanged. For multiple errors, the first error's public
/// code/message/exit are retained and details use `lycheedev.file-cleanup.v1`:
/// `primary`, `cleanup`, `control` are each the full ToolkitError or null. The
/// primary's original details are retained inside it, including nested joins.
/// Consumers must preserve these details even for cancellation/deadline codes.
pub fn finish_file<T>(
    file: File,
    result: Result<T>,
    control: &crate::control::OperationControl,
) -> Result<T> {
    finish_file_with(
        file,
        result,
        || control.check(),
        lycheedev_windows::filesystem::close_file,
    )
}
fn finish_file_with<T>(
    file: File,
    result: Result<T>,
    check: impl FnOnce() -> Result<()>,
    close: impl FnOnce(File) -> std::io::Result<()>,
) -> Result<T> {
    // Consume before consulting control: cleanup never needs fresh admission.
    // No File or numeric handle remains here to retry an uncertain close.
    let cleanup = close(file).map_err(ToolkitError::io).err();
    let mut stopped = check().err();
    let (value, primary) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error)),
    };
    // The same sticky stop observed both during work and after close is one
    // cause. Keep the original error byte-for-byte when cleanup succeeded.
    if let (Some(primary), Some(control)) = (&primary, &stopped)
        && primary.code == control.code
        && primary.message == control.message
        && primary.exit_code == control.exit_code
        && primary.details == control.details
    {
        stopped = None;
    }
    let count = usize::from(primary.is_some())
        + usize::from(cleanup.is_some())
        + usize::from(stopped.is_some());
    if count == 0 {
        return Ok(value.expect("successful file operation retained its value"));
    }
    let mut first = primary
        .as_ref()
        .or(cleanup.as_ref())
        .or(stopped.as_ref())
        .expect("failed file operation has a cause")
        .clone();
    if count > 1 {
        first.details = serde_json::json!({
            "schema": FILE_CLEANUP_SCHEMA,
            "primary": primary,
            "cleanup": cleanup,
            "control": stopped,
        });
    }
    Err(first)
}
pub fn sha512_integrity(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha512};
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let digest = Sha512::digest(bytes);
    let mut output = String::from("sha512-");
    for chunk in digest.chunks(3) {
        let value = (chunk[0] as u32) << 16
            | (chunk.get(1).copied().unwrap_or(0) as u32) << 8
            | chunk.get(2).copied().unwrap_or(0) as u32;
        output.push(ALPHABET[(value >> 18) as usize] as char);
        output.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        output.push(if chunk.len() > 1 {
            ALPHABET[((value >> 6) & 63) as usize] as char
        } else {
            '='
        });
        output.push(if chunk.len() > 2 {
            ALPHABET[(value & 63) as usize] as char
        } else {
            '='
        });
    }
    output
}
/// Go's public content identities use encoding/json escaping; preserve those stable identities.
pub fn identity_json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_string(value)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes())
}
pub fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
pub fn atomic_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    atomic_write(path, &serde_json::to_vec_pretty(value)?)
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_write_with_check(path, bytes, || Ok(()))
}
/// Publish with the caller's original cancellation/deadline, including after
/// staging and sync but before the durable atomic replacement.
pub fn atomic_write_controlled(
    path: &Path,
    bytes: &[u8],
    control: &crate::control::OperationControl,
) -> Result<()> {
    atomic_write_with_check(path, bytes, || control.check())
}
fn atomic_write_with_check(
    path: &Path,
    bytes: &[u8],
    check: impl FnMut() -> Result<()>,
) -> Result<()> {
    atomic_write_with_check_and_close(
        path,
        bytes,
        check,
        lycheedev_windows::filesystem::close_file,
    )
}
fn atomic_write_with_check_and_close(
    path: &Path,
    bytes: &[u8],
    check: impl FnMut() -> Result<()>,
    close: impl FnOnce(File) -> std::io::Result<()>,
) -> Result<()> {
    // Ordinary atomic publication has no sharing preflight. Keep its existing
    // checks, discard/close sequence and uncertain-publication retention.
    atomic_write_with_hooks(
        path,
        bytes,
        check,
        close,
        |_| Ok(()),
        owned_cleanup::OwnedTemporary::discard,
        lycheedev_windows::durable_replace,
    )
}
fn atomic_write_with_hooks(
    path: &Path,
    bytes: &[u8],
    mut check: impl FnMut() -> Result<()>,
    close: impl FnOnce(File) -> std::io::Result<()>,
    preflight: impl FnOnce(&Path) -> std::result::Result<(), slot_preflight::Failure>,
    discard: impl FnOnce(owned_cleanup::OwnedTemporary) -> (File, std::io::Result<()>),
    replace: impl FnOnce(&Path, &Path) -> lycheedev_windows::Result<()>,
) -> Result<()> {
    check()?;
    let parent = path
        .parent()
        .ok_or_else(|| ToolkitError::invalid("destination has no parent"))?;
    fs::create_dir_all(parent)?;
    // Bound uncertain residues per exact target, not across unrelated files in
    // this directory. Active and foreign entries are never adopted or removed.
    const TEMPORARY_SLOTS: u64 = 64;
    let target_key = sha256(
        path.file_name()
            .ok_or_else(|| ToolkitError::invalid("destination has no filename"))?
            .to_string_lossy()
            .to_lowercase()
            .as_bytes(),
    );
    let start = NEXT.fetch_add(1, Ordering::Relaxed) % TEMPORARY_SLOTS;
    let mut selected = None;
    for offset in 0..TEMPORARY_SLOTS {
        check()?;
        let temporary = parent.join(format!(
            ".lycheedev-{target_key}-{}.tmp",
            (start + offset) % TEMPORARY_SLOTS
        ));
        match owned_cleanup::OwnedTemporary::create(&temporary) {
            Ok(file) => {
                selected = Some((temporary, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let (temporary, mut owned) = selected.ok_or_else(|| {
        ToolkitError::new("vault.temporary_collision", "all bounded temporary slots for this target are occupied; preserve and inspect uncertain residues", 6)
            .with_details(serde_json::json!({"target":path,"parent":parent,"maximumSlots":TEMPORARY_SLOTS,"namespace":format!(".lycheedev-{target_key}-") }))
    })?;
    let mut written = (|| {
        for chunk in bytes.chunks(64 << 10) {
            check()?;
            owned.file_mut().write_all(chunk)?;
        }
        owned.file_mut().sync_all()?;
        check()
    })();
    if written.is_ok() {
        // The original CREATE_NEW handle is still owned here, after write/sync
        // and before either its close or any rename. Only the slot writer uses
        // this hook. A successful probe does not reserve a future replacement.
        match preflight(path) {
            Ok(()) => {}
            Err(slot_preflight::Failure::Sharing(error)) => written = Err(error),
            Err(slot_preflight::Failure::Close(error)) => {
                // An uncertain probe close must never be retried. Stop here,
                // checked-close the original temp once, and retain its name.
                let error =
                    finish_file_with::<()>(owned.into_file(), Err(error), &mut check, close)
                        .expect_err("probe close failure cannot publish");
                return Err(retained_temporary(
                    error,
                    &temporary,
                    "sharing probe close failed before publication was attempted",
                ));
            }
        }
    }
    if let Err(primary) = written {
        // CREATE_NEW supplied this token's exact identity. Discard it before
        // close, by handle only; a substituted pathname is never deleted.
        let (file, deleted) = discard(owned);
        let delete_succeeded = deleted.is_ok();
        let mut combined = primary.clone();
        if let Err(error) = deleted {
            combined.details = serde_json::json!({"schema":FILE_CLEANUP_SCHEMA,"primary":primary.clone(),"cleanup":ToolkitError::io(error),"control":null});
        }
        let result = finish_file_with::<()>(file, Err(combined), &mut check, close);
        let error = result.expect_err("failed temporary write cannot succeed");
        if delete_succeeded && serde_json::to_value(&error)? == serde_json::to_value(&primary)? {
            return Err(error);
        }
        return Err(retained_temporary(
            error,
            &temporary,
            "identity-bound deletion or close is unconfirmed",
        ));
    }
    // Once this original handle is consumed, no cleanup is allowed by path.
    // A failed/uncertain close or late stop retains bounded, named evidence.
    finish_file_with(owned.into_file(), Ok(()), &mut check, close)
        .map_err(|e| retained_temporary(e, &temporary, "close or post-close control failed"))?;
    check()
        .map_err(|e| retained_temporary(e, &temporary, "stopped after original handle closed"))?;
    replace(&temporary, path).map_err(|e| {
        retained_temporary(
            ToolkitError::new("vault.publish", e, 5),
            &temporary,
            "publication failed after original handle closed",
        )
    })?;
    Ok(())
}
fn retained_temporary(mut error: ToolkitError, path: &Path, detail: &str) -> ToolkitError {
    error.details = serde_json::json!({"schema":"lycheedev.atomic-temporary-retained.v1","temporary":path,"state":"unconfirmed","detail":detail,"cause":error.clone()});
    error
}

pub fn safe_relative(root: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    if path.is_absolute()
        || path.components().any(|p| {
            matches!(
                p,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(ToolkitError::invalid("unsafe relative path"));
    }
    Ok(root.join(path))
}
/// Reject all symlink/reparse components and keep the resolved path inside the selected root.
pub fn guard_path(root: &Path, path: &Path) -> Result<()> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ToolkitError::invalid("path escapes selected root"))?;
    let root_metadata = fs::symlink_metadata(root)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if root_metadata.file_attributes() & 0x400 != 0 {
            return Err(ToolkitError::invalid("reparse point at managed root"));
        }
    }
    if root_metadata.file_type().is_symlink() {
        return Err(ToolkitError::invalid("symlink at managed root"));
    }
    let canonical_root = fs::canonicalize(root)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        if let Ok(metadata) = fs::symlink_metadata(&current) {
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(ToolkitError::invalid("reparse point in managed path"));
                }
            }
            if metadata.file_type().is_symlink() {
                return Err(ToolkitError::invalid("symlink in managed path"));
            }
            if !fs::canonicalize(&current)?.starts_with(&canonical_root) {
                return Err(ToolkitError::invalid("resolved path escapes root"));
            }
        }
    }
    Ok(())
}
/// Kernel lock lifetime follows the handle, not a stale timestamp or owner TTL.
pub struct ExclusiveLock {
    file: Option<File>,
}
impl ExclusiveLock {
    pub fn resource(scope: &Path, resource: &str) -> Result<Self> {
        Self::acquire(&scope.join(format!("{}.lock", sha256(resource.as_bytes()))))
    }
    pub fn acquire(path: &Path) -> Result<Self> {
        // Historical callers impose no new clock/deadline. They still receive
        // checked close if acquiring this handle fails.
        Self::acquire_with(
            path,
            || Ok(()),
            Self::lock_file,
            lycheedev_windows::filesystem::close_file,
        )
    }
    pub fn resource_controlled(
        scope: &Path,
        resource: &str,
        control: &crate::control::OperationControl,
    ) -> Result<Self> {
        Self::acquire_controlled(
            &scope.join(format!("{}.lock", sha256(resource.as_bytes()))),
            control,
        )
    }
    /// The caller composes independent checks serially; acquiring and failed
    /// acquisition cleanup retain the same one-owner checked-close boundary.
    pub(crate) fn resource_checked(
        scope: &Path,
        resource: &str,
        check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        Self::acquire_with(
            &scope.join(format!("{}.lock", sha256(resource.as_bytes()))),
            check,
            Self::lock_file,
            lycheedev_windows::filesystem::close_file,
        )
    }
    pub fn acquire_controlled(
        path: &Path,
        control: &crate::control::OperationControl,
    ) -> Result<Self> {
        Self::acquire_with(
            path,
            || control.check(),
            Self::lock_file,
            lycheedev_windows::filesystem::close_file,
        )
    }
    fn lock_file(file: &File) -> Result<()> {
        fs2::FileExt::try_lock_exclusive(file).map_err(Self::lock_error)
    }
    fn lock_error(error: std::io::Error) -> ToolkitError {
        // fs2 0.4.3 Windows uses ERROR_LOCK_VIOLATION for contention.
        // Access/handle/filesystem errors must not become retryable contention.
        if error.raw_os_error().is_some()
            && error.raw_os_error() == fs2::lock_contended_error().raw_os_error()
        {
            ToolkitError::new("workspace.busy", error.to_string(), 6)
        } else {
            ToolkitError::io(error)
        }
    }
    fn acquire_with(
        path: &Path,
        check: impl FnMut() -> Result<()>,
        lock: impl FnOnce(&File) -> Result<()>,
        close: impl FnOnce(File) -> std::io::Result<()>,
    ) -> Result<Self> {
        Self::acquire_checked_with(path, check, |file, _| lock(file), close)
    }
    // Keep one open/close boundary for both immediate and explicitly waiting
    // callers. The lock operation never owns or reopens this original File.
    fn acquire_checked_with(
        path: &Path,
        mut check: impl FnMut() -> Result<()>,
        lock: impl FnOnce(&File, &mut dyn FnMut() -> Result<()>) -> Result<()>,
        close: impl FnOnce(File) -> std::io::Result<()>,
    ) -> Result<Self> {
        check()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        check()?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        let result = lock(&file, &mut check).and_then(|_| check());
        if let Err(error) = result {
            return finish_file_with(file, Err(error), check, close);
        }
        Ok(Self { file: Some(file) })
    }
    /// Consume the handle and check its close result. Closing the last owning
    /// handle releases the kernel lock, as Go Lease.Close does; do not unlock
    /// first or retry a consumed handle after an uncertain close failure.
    pub fn close(self) -> Result<()> {
        self.close_with(lycheedev_windows::filesystem::close_file)
    }
    fn close_with(mut self, close: impl FnOnce(File) -> std::io::Result<()>) -> Result<()> {
        let file = self.file.take().expect("exclusive lock owns one handle");
        close(file).map_err(ToolkitError::io)
    }
}
impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        // Fallback only. Explicit close has already consumed the ownership and
        // must never unlock/close the old numeric handle a second time.
        if let Some(file) = &self.file {
            let _ = fs2::FileExt::unlock(file);
        }
    }
}

#[cfg(test)]
mod exclusive_lock_close_tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lychee-lock-close-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn consuming_lock_close_releases_without_deleting_namespace() {
        let fixture = Fixture::new();
        let path = fixture.0.join("held.lock");
        let lock = ExclusiveLock::acquire(&path).unwrap();
        assert!(ExclusiveLock::acquire(&path).is_err());
        lock.close().unwrap();
        assert!(path.is_file());
        ExclusiveLock::acquire(&path).unwrap().close().unwrap();
    }
    #[test]
    fn close_failure_consumes_once_and_drop_does_not_unlock_transferred_handle() {
        let fixture = Fixture::new();
        let path = fixture.0.join("held.lock");
        let lock = ExclusiveLock::acquire(&path).unwrap();
        let mut calls = 0;
        let mut transferred = None;
        let error = lock
            .close_with(|file| {
                calls += 1;
                // Inject an uncertain close at the ownership boundary, retaining
                // the handle to detect an illicit Drop unlock. No kernel failure
                // is claimed. The fixture later closes its transferred ownership.
                transferred = Some(file);
                Err(std::io::Error::other("injected close failure"))
            })
            .unwrap_err();
        assert_eq!(error.code, "external.io");
        assert_eq!(calls, 1);
        assert!(ExclusiveLock::acquire(&path).is_err());
        lycheedev_windows::filesystem::close_file(transferred.take().unwrap()).unwrap();
        ExclusiveLock::acquire(&path).unwrap().close().unwrap();
    }
    #[test]
    fn error_after_real_close_is_reported_without_second_close() {
        let fixture = Fixture::new();
        let path = fixture.0.join("held.lock");
        let lock = ExclusiveLock::acquire(&path).unwrap();
        let mut calls = 0;
        let error = lock
            .close_with(|file| {
                calls += 1;
                lycheedev_windows::filesystem::close_file(file)?;
                Err(std::io::Error::other("injected post-close failure"))
            })
            .unwrap_err();
        assert_eq!(error.code, "external.io");
        assert_eq!(calls, 1);
        ExclusiveLock::acquire(&path).unwrap().close().unwrap();
    }
}

#[cfg(test)]
mod controlled_publication_tests {
    use super::*;
    #[test]
    fn cancellation_after_stage_sync_before_publish_preserves_destination() {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-controlled-publish-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("marker.json");
        fs::write(&path, b"original").unwrap();
        let control = crate::control::OperationControl::default();
        let mut checks = 0;
        let error = atomic_write_with_check(&path, b"replacement", || {
            checks += 1;
            // Entry, staging admission, one block, then after sync. Cancel at
            // that final boundary; cancelling only at entry misses the race.
            if checks == 4 {
                control.cancel_handle().store(true, Ordering::Release);
            }
            control.check()
        })
        .unwrap_err();
        // The failed write path also observes control after consuming close.
        assert_eq!(checks, 5);
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn checked_close_failure_never_publishes_the_staged_bytes() {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-close-failure-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("marker.json");
        fs::write(&path, b"original").unwrap();
        let mut closes = 0;
        let error = atomic_write_with_check_and_close(
            &path,
            b"replacement",
            || Ok(()),
            |file| {
                closes += 1;
                // Close the real fixture handle, then inject the error outcome. No
                // invalid handle, leak or unsafe fabricated File is needed.
                lycheedev_windows::filesystem::close_file(file)?;
                Err(std::io::Error::from_raw_os_error(5))
            },
        )
        .unwrap_err();
        assert_eq!(closes, 1);
        assert_eq!(error.code, "external.io");
        assert_eq!(error.exit_code, 5);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        // The original handle has closed; its pathname is retained rather than
        // risking deletion of a replacement object.
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn cancellation_while_closing_is_rechecked_before_publication() {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-close-cancel-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let path = root.join("marker.json");
        fs::write(&path, b"original").unwrap();
        let control = crate::control::OperationControl::default();
        let error = atomic_write_with_check_and_close(
            &path,
            b"replacement",
            || control.check(),
            |file| {
                lycheedev_windows::filesystem::close_file(file)?;
                control.cancel_handle().store(true, Ordering::Release);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(error.exit_code, 7);
        assert_eq!(fs::read(&path).unwrap(), b"original");
        // The original handle has closed; its pathname is retained rather than
        // risking deletion of a replacement object.
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
#[path = "storage/checked_close_tests.rs"]
mod checked_close_tests;

#[cfg(test)]
#[path = "storage/controlled_lock_tests.rs"]
mod controlled_lock_tests;

#[path = "storage/blob_wait.rs"]
mod blob_wait;
