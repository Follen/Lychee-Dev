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
