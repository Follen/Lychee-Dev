use std::{os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
    core::PCWSTR,
};
pub mod destination;
pub use destination::{Destination, Directory};
pub fn durable_replace(source: &Path, destination: &Path) -> crate::Result<()> {
    let src: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let dst: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        MoveFileExW(
            PCWSTR(src.as_ptr()),
            PCWSTR(dst.as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(|e| e.to_string())
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
    fn wide(path: &Path) -> std::io::Result<Vec<u16>> {
        if !path.is_absolute() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "durable move requires absolute paths",
            ));
        }
        let mut units: Vec<u16> = path.as_os_str().encode_wide().collect();
        if units.contains(&0) || units.len() >= 32767 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "durable move path contains NUL or exceeds Windows path bound",
            ));
        }
        units.push(0);
        Ok(units)
    }
    let source = wide(source)?;
    let destination = wide(destination)?;
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
