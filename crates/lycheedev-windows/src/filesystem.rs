use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};
pub mod destination;
pub mod owned_cleanup;
pub use destination::{Destination, Directory};
pub fn durable_replace(source: &Path, destination: &Path) -> crate::Result<()> {
    let src = native_move_path(source).map_err(|error| error.to_string())?;
    let dst = native_move_path(destination).map_err(|error| error.to_string())?;
    // SAFETY: Both buffers contain one final NUL and no interior NUL, remain
    // live and immutable throughout this synchronous call, and are not retained.
    unsafe {
        MoveFileExW(
            PCWSTR(src.as_ptr()),
            PCWSTR(dst.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|e| e.to_string())
    }
}

/// Match ordinary Win32 lexical path resolution before using the extended-length
/// namespace. Do not canonicalize: resolving filesystem links here could change
/// the exact directory entry selected and validated by the caller. Verbatim
/// inputs already select that namespace and must retain their literal semantics.
fn native_move_path(path: &Path) -> std::io::Result<Vec<u16>> {
    use std::path::{Component, Prefix};
    let invalid = || {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "move path is empty, contains NUL, or exceeds Windows path bound",
        )
    };
    let mut input: Vec<u16> = path.as_os_str().encode_wide().collect();
    if input.is_empty() || input.contains(&0) || input.len() >= 32767 {
        return Err(invalid());
    }
    // Existing callers and the fixed Go boundary pass explicit namespaces
    // through to MoveFileExW. Preserve all of them, including volume GUIDs,
    // device namespaces and literal dot components, without interpretation.
    // Rust's Prefix parser does not recognize the native NT \??\ spelling.
    // Preserve it before GetFullPathNameW can reinterpret it as drive-relative.
    if input.starts_with(&[b'\\' as u16, b'?' as u16, b'?' as u16, b'\\' as u16])
        || matches!(path.components().next(), Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Verbatim(_) | Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _) | Prefix::DeviceNS(_)))
    {
        input.push(0);
        return Ok(input);
    }
    // On Windows absolute uses GetFullPathNameW, without accessing the file
    // system. This resolves relative paths, dot components and ordinary slashes
    // before adding a prefix (adding one first would change those semantics).
    let absolute = std::path::absolute(path)?;
    let prefix = match absolute.components().next() {
        Some(Component::Prefix(prefix)) => prefix.kind(),
        _ => return Err(invalid()),
    };
    // Prefixing an ordinary DOS device name would turn its Win32 device
    // semantics into a literal filesystem entry. Keep the original native
    // spelling instead, leaving the operation and error decision to Windows.
    if matches!(prefix, Prefix::Disk(_) | Prefix::UNC(_, _))
        && absolute.components().any(|component| {
            let Component::Normal(name) = component else {
                return false;
            };
            let name = name.to_string_lossy().to_ascii_uppercase();
            let base = name.split('.').next().unwrap_or("").trim_end_matches(' ');
            matches!(base, "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$")
                || ["COM", "LPT"].iter().any(|prefix| {
                    base.strip_prefix(prefix).is_some_and(|suffix| {
                        matches!(
                            suffix,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
                })
        })
    {
        input.push(0);
        return Ok(input);
    }
    let units: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let mut result: Vec<u16> = match prefix {
        Prefix::VerbatimDisk(_) | Prefix::VerbatimUNC(_, _) => units,
        Prefix::Disk(_) => "\\\\?\\".encode_utf16().chain(units).collect(),
        Prefix::UNC(_, _) => "\\\\?\\UNC\\"
            .encode_utf16()
            .chain(units.into_iter().skip(2))
            .collect(),
        _ => input,
    };
    if result.len() >= 32767 || result.contains(&0) {
        return Err(invalid());
    }
    result.push(0);
    Ok(result)
}

#[cfg(test)]
mod native_move_path_tests {
    use super::*;
    use std::{ffi::OsString, fs, os::windows::ffi::OsStringExt, path::PathBuf};

    fn decoded(path: &Path) -> OsString {
        let units = native_move_path(path).unwrap();
        assert_eq!(units.last(), Some(&0));
        assert!(!units[..units.len() - 1].contains(&0));
        OsString::from_wide(&units[..units.len() - 1])
    }

    #[test]
    fn drive_unc_and_relative_paths_resolve_lexically_before_prefixing() {
        assert_eq!(
            decoded(Path::new(r"C:\fixture/missing/../target")),
            OsString::from(r"\\?\C:\fixture\target")
        );
        assert_eq!(
            decoded(Path::new(r"\\server\share\fixture/../target")),
            OsString::from(r"\\?\UNC\server\share\target")
        );
        assert_eq!(
            decoded(Path::new(r"C:\fixture\target. ")),
            OsString::from(r"\\?\C:\fixture\target")
        );
        for path in [r"C:relative-target", r"\root-relative-target"] {
            assert_eq!(
                decoded(Path::new(path)),
                decoded(&std::path::absolute(path).unwrap())
            );
        }
        assert_eq!(
            decoded(Path::new("missing-directory/../relative-target")),
            decoded(&std::env::current_dir().unwrap().join("relative-target"))
        );
    }

    #[test]
    fn verbatim_drive_unc_and_non_utf8_units_are_preserved() {
        for path in [
            r"\\?\C:\fixture\literal. ",
            r"\\?\UNC\server\share\literal. ",
            r"\\?\C:\fixture\..\literal",
            r"\\?\Volume{12345678-1234-1234-1234-123456789abc}\file",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\file",
            r"\\.\C:\file",
            r"\??\C:\fixture\literal. ",
            r"\??\UNC\server\share\literal",
            r"C:\fixture\NUL.txt",
            r"C:\fixture\COM1",
        ] {
            assert_eq!(decoded(Path::new(path)), OsString::from(path));
        }
        let mut units: Vec<u16> = r"\\?\C:\fixture\".encode_utf16().collect();
        units.push(0xd800);
        let path = OsString::from_wide(&units);
        assert_eq!(decoded(Path::new(&path)), path);
    }

    #[test]
    fn empty_nul_and_path_bound_are_rejected() {
        for path in ["", "C:\\bad\0name"] {
            assert_eq!(
                native_move_path(Path::new(path)).unwrap_err().kind(),
                std::io::ErrorKind::InvalidInput
            );
        }
        // The extended prefix counts against the native bound too.
        let path = format!("C:\\{}", "a".repeat(32762));
        assert_eq!(
            native_move_path(Path::new(&path)).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn long_mixed_separator_paths_publish_and_move_without_overwrite() {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        struct Temp(PathBuf);
        impl Drop for Temp {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let root = std::env::temp_dir().join(format!(
            "lycheedev-long-move-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let temp = Temp(root);
        let long = temp
            .0
            .join("a".repeat(100))
            .join("b".repeat(100))
            .join("mixed/child");
        fs::create_dir_all(&long).unwrap();
        let source = long.join("source");
        let destination = long.join("destination");
        assert!(source.as_os_str().encode_wide().count() > 260);
        fs::write(&source, b"new").unwrap();
        fs::write(&destination, b"old").unwrap();
        durable_replace(&source, &destination).unwrap();
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        assert!(!source.exists());
        fs::write(&source, b"next").unwrap();
        assert!(durable_move_noreplace(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"next");
        assert_eq!(fs::read(&destination).unwrap(), b"new");
        let missing = long.join("missing");
        durable_move_noreplace(&source, &missing).unwrap();
        assert_eq!(fs::read(&missing).unwrap(), b"next");
        // Native NT inputs retain their original identity rather than becoming
        // a current-drive path containing literal question-mark components.
        fn native(path: &Path) -> PathBuf {
            let canonical = fs::canonicalize(path).unwrap();
            let units: Vec<u16> = canonical.as_os_str().encode_wide().collect();
            assert!(units.starts_with(&[92, 92, 63, 92]));
            PathBuf::from(OsString::from_wide(
                &r"\??\"
                    .encode_utf16()
                    .chain(units.into_iter().skip(4))
                    .collect::<Vec<_>>(),
            ))
        }
        fs::write(&source, b"native").unwrap();
        durable_replace(&native(&source), &native(&destination)).unwrap();
        assert!(!source.exists());
        assert_eq!(fs::read(&destination).unwrap(), b"native");
    }
}

/// Consume an owned File and report the kernel close result. Unlike File::drop,
/// this boundary must be checked before treating a staged write as publishable.
/// The caller must not retain or retry the raw handle after this consumes it.
pub fn close_file(file: std::fs::File) -> std::io::Result<()> {
    use std::os::windows::io::IntoRawHandle;
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    let handle = HANDLE(file.into_raw_handle());
    // SAFETY: IntoRawHandle transfers this File's sole ownership of this handle
    // and suppresses its destructor. CloseHandle receives the same valid Win32
    // file handle exactly once. No pointer/buffer or borrow escapes, and this
    // function neither shares, reconstructs nor retries the handle on error:
    // its post-close state must never be guessed from a failed close result.
    unsafe { CloseHandle(handle) }
        .map_err(|error| std::io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32))
}

#[cfg(test)]
mod close_file_tests {
    use super::*;
    use std::{fs, os::windows::fs::OpenOptionsExt, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lycheedev-close-file-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn file(&self) -> std::fs::File {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .share_mode(0)
                .open(self.0.join("owned"))
                .unwrap()
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn checked_close_releases_the_owned_exclusive_file() {
        let temp = Temp::new();
        let file = temp.file();
        let path = temp.0.join("owned");
        assert!(fs::remove_file(&path).is_err());
        close_file(file).unwrap();
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn checked_close_does_not_close_an_independently_owned_duplicate() {
        let temp = Temp::new();
        let file = temp.file();
        let duplicate = file.try_clone().unwrap();
        let path = temp.0.join("owned");
        close_file(file).unwrap();
        assert!(fs::remove_file(&path).is_err());
        close_file(duplicate).unwrap();
        fs::remove_file(path).unwrap();
    }
}

/// Move one file or directory without replacing a racing destination. This is
/// the fixed Go publication boundary: same-volume MoveFileExW with write-through
/// and no COPY_ALLOWED or REPLACE_EXISTING flags.
pub fn durable_move_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    if !source.is_absolute() || !destination.is_absolute() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "durable move requires absolute paths",
        ));
    }
    let source = native_move_path(source)?;
    let destination = native_move_path(destination)?;
    // SAFETY: Both independently owned UTF-16 vectors contain exactly one final
    // NUL and no interior NUL. Their pointers remain live and immutable for this
    // synchronous system-ABI call; Windows retains neither pointer. No handle
    // or borrowed buffer crosses threads. Omitting REPLACE_EXISTING makes the
    // kernel's destination check atomic with the move, including file targets.
    unsafe {
        MoveFileExW(
            PCWSTR(source.as_ptr()),
            PCWSTR(destination.as_ptr()),
            MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|error| std::io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32))
    }
}

#[cfg(test)]
mod durable_move_tests {
    use super::*;
    use std::{fs, path::PathBuf};
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lycheedev-durable-move-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn moves_file_and_directory_without_replacing_existing_destination() {
        let temp = Temp::new();
        for directory in [false, true] {
            let source = temp.0.join(if directory { "directory" } else { "file" });
            let destination = temp.0.join(if directory {
                "directory-out"
            } else {
                "file-out"
            });
            if directory {
                fs::create_dir(&source).unwrap();
                fs::write(source.join("data"), b"owned").unwrap();
            } else {
                fs::write(&source, b"owned").unwrap();
            }
            durable_move_noreplace(&source, &destination).unwrap();
            assert!(!source.exists());
            assert_eq!(
                fs::read(if directory {
                    destination.join("data")
                } else {
                    destination.clone()
                })
                .unwrap(),
                b"owned"
            );
            if directory {
                fs::create_dir(&source).unwrap();
            } else {
                fs::write(&source, b"new owner").unwrap();
            }
            assert!(durable_move_noreplace(&source, &destination).is_err());
            assert!(source.exists());
            assert_eq!(
                fs::read(if directory {
                    destination.join("data")
                } else {
                    destination
                })
                .unwrap(),
                b"owned"
            );
        }
    }
    #[test]
    fn destination_appearing_after_admission_is_not_overwritten() {
        let temp = Temp::new();
        let source = temp.0.join("source");
        let destination = temp.0.join("marker");
        fs::write(&source, b"our marker").unwrap();
        assert!(!destination.exists());
        fs::write(&destination, b"competing marker").unwrap();
        assert!(durable_move_noreplace(&source, &destination).is_err());
        assert_eq!(fs::read(&source).unwrap(), b"our marker");
        assert_eq!(fs::read(&destination).unwrap(), b"competing marker");
    }
    #[test]
    fn rejects_relative_and_nul_paths_before_any_move() {
        use std::os::windows::ffi::OsStringExt;
        let temp = Temp::new();
        let source = temp.0.join("source");
        let destination = temp.0.join("destination");
        fs::write(&source, b"retained").unwrap();
        assert_eq!(
            durable_move_noreplace(Path::new("relative"), &destination)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::InvalidInput
        );
        let mut wide: Vec<u16> = destination.as_os_str().encode_wide().collect();
        wide.push(0);
        wide.extend("suffix".encode_utf16());
        let nul = PathBuf::from(std::ffi::OsString::from_wide(&wide));
        assert_eq!(
            durable_move_noreplace(&source, &nul).unwrap_err().kind(),
            std::io::ErrorKind::InvalidInput
        );
        assert_eq!(fs::read(source).unwrap(), b"retained");
        assert!(!destination.exists());
    }
}

/// Read the system directory from Windows, without trusting environment variables.
/// The result owns its UTF-16 path; no OS buffer or pointer escapes the call.
pub fn system_directory() -> std::io::Result<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;

    // SAFETY: This declaration matches the Windows API's system ABI: a writable
    // UTF-16 buffer, its capacity in u16 elements, and a u32 result length.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    }
    let mut buffer = vec![0u16; 32768];
    // SAFETY: The exclusive, initialized buffer has exactly the advertised
    // capacity, which fits u32. Windows writes synchronously and retains no
    // pointer. The buffer stays live and is not shared with another thread.
    let length = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) };
    if length == 0 {
        return Err(std::io::Error::last_os_error());
    }
    let length = length as usize;
    if length >= buffer.len() || buffer[length] != 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Windows system directory exceeds the bounded buffer or lacks a terminator",
        ));
    }
    Ok(std::path::PathBuf::from(std::ffi::OsString::from_wide(
        &buffer[..length],
    )))
}

#[cfg(test)]
mod system_directory_tests {
    #[test]
    fn system_directory_is_an_owned_absolute_existing_directory() {
        let first = super::system_directory().unwrap();
        let second = super::system_directory().unwrap();
        assert!(first.is_absolute());
        assert!(first.is_dir());
        assert_eq!(first, second);
    }
}
