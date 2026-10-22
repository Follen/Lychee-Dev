use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::Path,
};
use windows::Win32::{Foundation::HANDLE, System::IO::DeviceIoControl};
pub fn reparse_writer(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .share_mode(7)
        .custom_flags(0x02200000)
        .open(path)
}
pub fn set_junction(file: &File, target: &Path) -> io::Result<()> {
    let absolute = std::path::absolute(target)?;
    let print: Vec<u16> = absolute.as_os_str().encode_wide().collect();
    let substitute: Vec<u16> = std::ffi::OsString::from(format!("\\??\\{}", absolute.display()))
        .encode_wide()
        .collect();
    let size = 16 + (substitute.len() + print.len() + 2) * 2;
    let mut bytes = vec![0u8; size];
    bytes[..4].copy_from_slice(&0xA0000003u32.to_le_bytes());
    bytes[4..6].copy_from_slice(&((size - 8) as u16).to_le_bytes());
    bytes[10..12].copy_from_slice(&((substitute.len() * 2) as u16).to_le_bytes());
    bytes[12..14].copy_from_slice(&((substitute.len() * 2 + 2) as u16).to_le_bytes());
    bytes[14..16].copy_from_slice(&((print.len() * 2) as u16).to_le_bytes());
    let mut at = 16;
    for unit in substitute
        .iter()
        .copied()
        .chain(Some(0))
        .chain(print.iter().copied())
        .chain(Some(0))
    {
        bytes[at..at + 2].copy_from_slice(&unit.to_le_bytes());
        at += 2;
    }
    let mut returned = 0u32;
    unsafe {
        DeviceIoControl(
            HANDLE(file.as_raw_handle()),
            0x000900A4,
            Some(bytes.as_ptr().cast()),
            bytes.len() as u32,
            None,
            0,
            Some(&mut returned),
            None,
        )
    }
    .map_err(|error| io::Error::from_raw_os_error((error.code().0 as u32 & 0xffff) as i32))
}
