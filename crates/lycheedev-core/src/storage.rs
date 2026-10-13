use crate::{Result, ToolkitError};
// Business providers use the shared mechanism layer, rather than adding their
// own Win32 publication path or a direct platform dependency.
pub use lycheedev_windows::filesystem::{Destination, Directory};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
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
    use std::io::Read;
    control.check()?;
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
    let mut file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > maximum {
        return Err(ToolkitError::invalid("identity file exceeds admission"));
    }
    let mut digest = Sha256::new();
    let mut read_bytes = 0u64;
    let mut buffer = [0u8; 65536];
    loop {
        control.check()?;
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
    control.check()?;
    Ok((file, hex::encode(digest.finalize())))
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
    let parent = path
        .parent()
        .ok_or_else(|| ToolkitError::invalid("destination has no parent"))?;
    fs::create_dir_all(parent)?;
    let mut selected = None;
    for _ in 0..64 {
        let temporary = parent.join(format!(
            ".lycheedev-{}-{}-{}.tmp",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                selected = Some((temporary, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    let (temporary, mut file) = selected.ok_or_else(|| {
        ToolkitError::new(
            "vault.temporary_collision",
            "temporary name collision budget exhausted",
            5,
        )
    })?;
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        lycheedev_windows::durable_replace(&temporary, path)
            .map_err(|e| ToolkitError::new("vault.publish", e, 5))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
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
    file: File,
}
impl ExclusiveLock {
    pub fn resource(scope: &Path, resource: &str) -> Result<Self> {
        Self::acquire(&scope.join(format!("{}.lock", sha256(resource.as_bytes()))))
    }
    pub fn acquire(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)?;
        fs2::FileExt::try_lock_exclusive(&file)
            .map_err(|e| ToolkitError::new("workspace.busy", e.to_string(), 6))?;
        Ok(Self { file })
    }
}
impl Drop for ExclusiveLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}
