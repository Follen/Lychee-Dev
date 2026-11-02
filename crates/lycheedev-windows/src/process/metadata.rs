//! Process names/parents and limited-query image/command-line metadata. No
//! remote-memory, input, elevation or termination capability is opened here.
use super::{Handle, Process, ProcessIdentity, finish_query, identity};
use crate::Result;
use std::{
    ffi::{OsString, c_void},
    os::windows::ffi::OsStringExt,
    path::PathBuf,
};
use windows::{
    Win32::{
        Foundation::*,
        System::{Diagnostics::ToolHelp::*, Threading::*},
    },
    core::{HRESULT, PCWSTR, PWSTR},
};

pub struct Row {
    pub pid: u32,
    pub parent: u32,
    pub name: String,
}
// Command lines may contain private application arguments: no Debug/Serialize.
pub struct Metadata {
    pub identity: ProcessIdentity,
    pub image: PathBuf,
    pub arguments: Option<Vec<OsString>>,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct UnicodeString {
    length: u16,
    capacity: u16,
    buffer: *const u16,
}
#[link(name = "ntdll")]
unsafe extern "system" {
    fn NtQueryInformationProcess(
        process: HANDLE,
        class: u32,
        buffer: *mut c_void,
        size: u32,
        needed: *mut u32,
    ) -> NTSTATUS;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn CommandLineToArgvW(command: PCWSTR, count: *mut i32) -> *mut PWSTR;
}

/// Enumerate a complete bounded OS snapshot. An enumeration error never means
/// that an application is absent and cannot license a replacement login.
pub fn rows(mut check: impl FnMut() -> Result<()>) -> Result<Vec<Row>> {
    check()?;
    let snapshot = Handle(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
            .map_err(|_| "process metadata inventory is unavailable")?,
    );
    let result = (|| {
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if let Err(error) = unsafe { Process32FirstW(snapshot.0, &mut entry) } {
            return if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) {
                Ok(Vec::new())
            } else {
                Err("process metadata inventory failed".into())
            };
        }
        let mut rows = Vec::new();
        loop {
            check()?;
            if rows.len() >= 65536 {
                return Err("process metadata inventory exceeds its budget".into());
            }
            let length = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            rows.push(Row {
                pid: entry.th32ProcessID,
                parent: entry.th32ParentProcessID,
                name: String::from_utf16_lossy(&entry.szExeFile[..length]),
            });
            match unsafe { Process32NextW(snapshot.0, &mut entry) } {
                Ok(()) => {}
                Err(error) if error.code() == HRESULT::from_win32(ERROR_NO_MORE_FILES.0) => break,
                Err(_) => return Err("process metadata inventory ended incompletely".into()),
            }
        }
        check()?;
        Ok(rows)
    })();
    snapshot.finish(result)
}
fn arguments(text: &[u16]) -> Result<Vec<OsString>> {
    if text.len() > 65536 || text.contains(&0) {
        return Err("process metadata command line is invalid".into());
    }
    let mut input = text.to_vec();
    input.push(0);
    let mut count = 0;
    // SAFETY: the input has one trailing NUL and lives through the call. Shell32
    // returns its own complete argv/string allocation for LocalFree.
    let argv = unsafe { CommandLineToArgvW(PCWSTR(input.as_ptr()), &mut count) };
    if argv.is_null() {
        return Err("process metadata arguments are unavailable".into());
    }
    let result = (|| {
        if !(1..=4096).contains(&count) {
            return Err("process metadata argument count exceeds its budget".into());
        }
        let mut values = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            // SAFETY: Shell32 guarantees count valid terminated strings in this
            // still-owned allocation. Preserve Windows paths as UTF-16.
            let value = unsafe { *argv.add(index) };
            values.push(OsString::from_wide(unsafe { value.as_wide() }));
        }
        Ok(values)
    })();
    // SAFETY: this consumes the original Shell32 allocation once on every path.
    let remainder = unsafe { LocalFree(Some(HLOCAL(argv.cast()))) };
    finish_query(
        result,
        if remainder.0.is_null() {
            Ok(())
        } else {
            Err("process metadata argv release failed".into())
        },
    )
}
fn command_line(handle: HANDLE) -> Result<Vec<OsString>> {
    let mut needed = 0;
    // SAFETY: the sizing call supplies no output storage; only needed is written.
    let _ = unsafe { NtQueryInformationProcess(handle, 60, std::ptr::null_mut(), 0, &mut needed) };
    if !(std::mem::size_of::<UnicodeString>()..=131072).contains(&(needed as usize)) {
        return Err("process metadata command line length is unavailable".into());
    }
    let mut storage = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
    let size = storage.len() * std::mem::size_of::<usize>();
    // SAFETY: storage is pointer-aligned and owns the declared byte capacity.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            60,
            storage.as_mut_ptr().cast(),
            size as u32,
            &mut needed,
        )
    };
    if !status.is_ok() {
        return Err("process metadata command line query failed".into());
    }
    let string = unsafe { std::ptr::read(storage.as_ptr().cast::<UnicodeString>()) };
    let start = storage.as_ptr() as usize;
    let offset = (string.buffer as usize).checked_sub(start);
    let length = string.length as usize;
    if length % 2 != 0
        || string.capacity < string.length
        || string.buffer as usize % 2 != 0
        || offset.is_none_or(|offset| offset > size || length > size - offset)
    {
        return Err("process metadata command line layout is invalid".into());
    }
    // SAFETY: the complete UTF-16 range was verified inside this still-owned
    // allocation; copy before releasing native-query storage.
    let text = unsafe { std::slice::from_raw_parts(string.buffer, length / 2) }.to_vec();
    arguments(&text)
}
pub fn inspect(
    pid: u32,
    with_arguments: bool,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Metadata> {
    check()?;
    if pid == 0 {
        return Err("process metadata requires a process identity".into());
    }
    let handle = Handle(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        }
        .map_err(|_| "process metadata is not accessible")?,
    );
    let process = match unsafe { identity(handle.0, pid) } {
        Ok(identity) => Process { handle, identity },
        Err(error) => return handle.finish(Err(error)),
    };
    let result = (|| {
        check()?;
        process.verify()?;
        let mut path = vec![0; 32768];
        let mut length = path.len() as u32;
        // SAFETY: the original limited-query handle and output capacity remain
        // live. No VM_READ right or process-memory API is used.
        unsafe {
            QueryFullProcessImageNameW(
                process.handle.0,
                PROCESS_NAME_WIN32,
                PWSTR(path.as_mut_ptr()),
                &mut length,
            )
        }
        .map_err(|_| "process metadata image is unavailable")?;
        path.truncate(length as usize);
        let args = if with_arguments {
            Some(command_line(process.handle.0)?)
        } else {
            None
        };
        check()?;
        process.verify()?;
        Ok(Metadata {
            identity: process.identity.clone(),
            image: PathBuf::from(OsString::from_wide(&path)),
            arguments: args,
        })
    })();
    process.handle.finish(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_process_metadata_matches_independent_creation_and_runtime_arguments_without_memory_rights()
     {
        let own = Process::open(std::process::id()).unwrap();
        let expected = own.identity().clone();
        own.close().unwrap();
        let observed = inspect(expected.pid, true, || Ok(())).unwrap();
        assert_eq!(observed.identity, expected);
        assert_eq!(
            observed.image.canonicalize().unwrap(),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        assert_eq!(
            observed.arguments.unwrap(),
            std::env::args_os().collect::<Vec<_>>()
        );
        let all = rows(|| Ok(())).unwrap();
        assert_eq!(all.iter().filter(|row| row.pid == expected.pid).count(), 1);
        assert!(
            all.iter()
                .find(|row| row.pid == expected.pid)
                .unwrap()
                .parent
                > 0
        );
        assert!(
            inspect(expected.pid, false, || Ok(()))
                .unwrap()
                .arguments
                .is_none()
        );
    }
    #[test]
    fn windows_argument_rules_preserve_quoted_unicode_profiles_and_cancellation() {
        let text = r#"program.exe --utility-sub-type=network.mojom.NetworkService "--user-data-dir=C:\中文 space\EBWebView" "" "slash\\""#;
        assert_eq!(
            arguments(&text.encode_utf16().collect::<Vec<_>>()).unwrap(),
            vec![
                OsString::from("program.exe"),
                "--utility-sub-type=network.mojom.NetworkService".into(),
                r"--user-data-dir=C:\中文 space\EBWebView".into(),
                "".into(),
                "slash\\".into()
            ]
        );
        assert!(
            inspect(std::process::id(), true, || Err(
                "synthetic-original-cancel".into()
            ))
            .err()
            .unwrap()
            .contains("synthetic-original-cancel")
        );
        assert!(rows(|| Err("synthetic-original-cancel".into())).is_err());
        assert!(arguments(&[b'a' as u16, 0, b'b' as u16]).is_err());
    }
}
