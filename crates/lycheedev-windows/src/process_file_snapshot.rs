//! Copy only explicitly selected disk files already held by one bound process.
//! No remote heap reads, file-position changes, writes or process termination.
use crate::{ProcessIdentity, Result, process::finish_query};
use std::{ffi::c_void, os::windows::ffi::OsStrExt, path::Path};
use windows::{
    Win32::{
        Foundation::*,
        Globalization::CompareStringOrdinal,
        Storage::FileSystem::{
            FILE_NAME_NORMALIZED, FILE_TYPE_DISK, GetFileSizeEx, GetFileType,
            GetFinalPathNameByHandleW,
        },
        System::{
            Diagnostics::Debug::ReadProcessMemory,
            Memory::{
                CreateFileMappingW, FILE_MAP_READ, MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile,
                PAGE_READONLY, UnmapViewOfFile,
            },
            Threading::{
                GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_DUP_HANDLE,
                PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION, PROCESS_SYNCHRONIZE,
                QueryFullProcessImageNameW, WaitForSingleObject,
            },
        },
    },
    core::{PCWSTR, PWSTR},
};
const MAX_FILE: usize = 16 << 20;
const MAX_TABLE: usize = 8 << 20;

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
#[repr(C)]
#[derive(Clone, Copy)]
struct Entry {
    handle: usize,
    handle_count: usize,
    pointer_count: usize,
    access: u32,
    object_type: u32,
    attributes: u32,
    reserved: u32,
}
struct Handle(Option<HANDLE>);
impl Handle {
    fn raw(&self) -> HANDLE {
        self.0.expect("live snapshot handle")
    }
    fn close(&mut self) -> Result<()> {
        self.0.take().map_or(Ok(()), |handle| {
            // SAFETY: this is the original unique owner; take prevents retries.
            unsafe { CloseHandle(handle) }.map_err(|_| "file snapshot handle release failed".into())
        })
    }
    fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        finish_query(result, self.close())
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
struct View(Option<MEMORY_MAPPED_VIEW_ADDRESS>);
impl View {
    fn close(&mut self) -> Result<()> {
        self.0.take().map_or(Ok(()), |view| {
            // SAFETY: take consumes this original successful mapping once.
            unsafe { UnmapViewOfFile(view) }
                .map_err(|_| "file snapshot mapping release failed".into())
        })
    }
    fn finish<T>(mut self, result: Result<T>) -> Result<T> {
        finish_query(result, self.close())
    }
}
impl Drop for View {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

// No Debug/Serialize: snapshots may contain private application state.
pub struct FileSnapshots(Vec<Option<Vec<u8>>>);
impl FileSnapshots {
    pub fn get(&self, index: usize) -> Option<&[u8]> {
        self.0.get(index)?.as_deref()
    }
}
fn path_units(path: &Path) -> Result<Vec<u16>> {
    let path = std::path::absolute(path).map_err(|_| "file snapshot path is invalid")?;
    Ok(normalize(path.as_os_str().encode_wide().collect()))
}
fn normalize(mut path: Vec<u16>) -> Vec<u16> {
    let unc = [92, 92, 63, 92, 85, 78, 67, 92];
    if path.starts_with(&unc) {
        path.splice(..8, [92, 92]);
    } else if path.starts_with(&[92, 92, 63, 92]) {
        path.drain(..4);
    }
    path
}
fn same(a: &[u16], b: &[u16]) -> bool {
    // SAFETY: both slices are live UTF-16 paths; Windows compares explicit
    // lengths with ordinal case rules, preserving unpaired UTF-16 too.
    unsafe { CompareStringOrdinal(a, b, true) }.0 == 2
}
fn process_matches(handle: HANDLE, identity: &ProcessIdentity, executable: &[u16]) -> Result<()> {
    let (mut created, mut exit, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: all out structures and the held process handle remain live.
    unsafe { GetProcessTimes(handle, &mut created, &mut exit, &mut kernel, &mut user) }
        .map_err(|_| "file snapshot process identity is unavailable")?;
    let stamp = ((created.dwHighDateTime as u64) << 32) | created.dwLowDateTime as u64;
    if stamp != identity.created || unsafe { WaitForSingleObject(handle, 0) } != WAIT_TIMEOUT {
        return Err("file snapshot process identity has ended or changed".into());
    }
    let mut path = vec![0; 32768];
    let mut length = path.len() as u32;
    // SAFETY: path owns the entire output capacity and the original handle is
    // held throughout validation and subsequent file-handle inspection.
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
    }
    .map_err(|_| "file snapshot process image is unavailable")?;
    path.truncate(length as usize);
    if !same(&normalize(path), executable) {
        return Err("file snapshot process image changed".into());
    }
    Ok(())
}
fn table(process: HANDLE, check: &mut impl FnMut() -> Result<()>) -> Result<Vec<Entry>> {
    if std::mem::size_of::<usize>() != 8 {
        return Err("file snapshots require Windows amd64".into());
    }
    let mut size = 65536usize;
    loop {
        check()?;
        // Native handle snapshots contain ULONG_PTR fields, so the allocation
        // is pointer-aligned. The class-51 header contains two ULONG_PTRs.
        let mut buffer = vec![0usize; size / 8];
        let mut needed = 0;
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                51,
                buffer.as_mut_ptr().cast(),
                size as u32,
                &mut needed,
            )
        };
        if status.is_ok() {
            let count = buffer[0];
            if count > (size - 16) / std::mem::size_of::<Entry>() {
                return Err("file snapshot handle table is invalid".into());
            }
            let mut entries = Vec::with_capacity(count);
            for index in 0..count {
                check()?;
                // SAFETY: the count is checked against this aligned allocation;
                // every native entry fits and Entry is Copy with integer fields.
                entries.push(unsafe {
                    std::ptr::read(
                        buffer
                            .as_ptr()
                            .cast::<u8>()
                            .add(16 + index * std::mem::size_of::<Entry>())
                            .cast::<Entry>(),
                    )
                });
            }
            return Ok(entries);
        }
        // Only buffer-size races may retry; all other native errors are terminal.
        if status.0 as u32 != 0xc0000004 && status.0 as u32 != 0xc0000023 {
            return Err("file snapshot handle inspection is unavailable".into());
        }
        size = (size * 2).max(needed as usize).next_multiple_of(8);
        if size > MAX_TABLE {
            return Err("file snapshot handle table exceeds its budget".into());
        }
    }
}
fn copy_file(file: HANDLE, check: &mut impl FnMut() -> Result<()>) -> Result<Vec<u8>> {
    check()?;
    let mut size = 0i64;
    unsafe { GetFileSizeEx(file, &mut size) }.map_err(|_| "file snapshot size is unavailable")?;
    if size < 0 || size as u64 > MAX_FILE as u64 {
        return Err("file snapshot exceeds its budget".into());
    }
    if size == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: the held duplicate grants read access; this creates a read-only
    // file-backed mapping without reopening a path or moving the shared cursor.
    let mapping = Handle(Some(
        unsafe { CreateFileMappingW(file, None, PAGE_READONLY, 0, 0, PCWSTR::null()) }
            .map_err(|_| "read-only file mapping is unavailable")?,
    ));
    let result = (|| {
        let address = unsafe { MapViewOfFile(mapping.raw(), FILE_MAP_READ, 0, 0, size as usize) };
        if address.Value.is_null() {
            return Err("read-only file view is unavailable".into());
        }
        let view = View(Some(address));
        let result = (|| {
            check()?;
            let mut bytes = vec![0; size as usize];
            let mut copied = 0;
            // SAFETY: this reads only our own successful file-backed view, not
            // remote process memory. The OS copy reports mapping/truncation
            // failures instead of creating a Rust slice over mutable file pages.
            unsafe {
                ReadProcessMemory(
                    GetCurrentProcess(),
                    address.Value,
                    bytes.as_mut_ptr().cast(),
                    bytes.len(),
                    Some(&mut copied),
                )
            }
            .map_err(|_| "file snapshot changed or could not be copied")?;
            if copied != bytes.len() {
                return Err("file snapshot copy was incomplete".into());
            }
            check()?;
            Ok(bytes)
        })();
        view.finish(result)
    })();
    mapping.finish(result)
}
/// The application must first authorize the process role and exact file paths.
/// This verifies original creation/image on one held handle; it cannot authorize
/// a later process using a cached PID. At most two files of 16 MiB are copied.
pub fn snapshot_process_files(
    identity: &ProcessIdentity,
    executable: &Path,
    paths: &[&Path],
    mut check: impl FnMut() -> Result<()>,
) -> Result<FileSnapshots> {
    check()?;
    if identity.pid == 0
        || identity.created == 0
        || !executable.is_absolute()
        || paths.is_empty()
        || paths.len() > 2
        || paths.iter().any(|p| !p.is_absolute())
    {
        return Err("file snapshot requires exact process and file identities".into());
    }
    let executable = path_units(executable)?;
    let targets = paths
        .iter()
        .map(|p| path_units(p))
        .collect::<Result<Vec<_>>>()?;
    let source = Handle(Some(
        unsafe {
            OpenProcess(
                PROCESS_QUERY_INFORMATION | PROCESS_DUP_HANDLE | PROCESS_SYNCHRONIZE,
                false,
                identity.pid,
            )
        }
        .map_err(|_| "file snapshot process is not accessible")?,
    ));
    let result = (|| {
        process_matches(source.raw(), identity, &executable)?;
        let entries = table(source.raw(), &mut check)?;
        let mut result = FileSnapshots(vec![None; paths.len()]);
        for entry in entries {
            check()?;
            if entry.access & 1 == 0 {
                continue;
            }
            let mut duplicate = HANDLE::default();
            // SAFETY: source is the original held process; only read-data and
            // read-attributes rights are requested for this non-inherited copy.
            if unsafe {
                DuplicateHandle(
                    source.raw(),
                    HANDLE(entry.handle as *mut c_void),
                    GetCurrentProcess(),
                    &mut duplicate,
                    0x81,
                    false,
                    DUPLICATE_HANDLE_OPTIONS(0),
                )
            }
            .is_err()
            {
                continue;
            }
            let duplicate = Handle(Some(duplicate));
            let inspected = (|| {
                if unsafe { GetFileType(duplicate.raw()) } != FILE_TYPE_DISK {
                    return Ok(None);
                }
                let mut path = vec![0; 32768];
                let length = unsafe {
                    GetFinalPathNameByHandleW(duplicate.raw(), &mut path, FILE_NAME_NORMALIZED)
                } as usize;
                if length == 0 || length >= path.len() {
                    return Ok(None);
                }
                path.truncate(length);
                let path = normalize(path);
                let Some(index) = targets.iter().position(|target| same(&path, target)) else {
                    return Ok(None);
                };
                if result.0[index].is_none() {
                    return copy_file(duplicate.raw(), &mut check)
                        .map(|bytes| Some((index, bytes)));
                }
                Ok(None)
            })();
            if let Some((index, bytes)) = duplicate.finish(inspected)? {
                result.0[index] = Some(bytes);
            }
            if result.0.iter().all(Option::is_some) {
                break;
            }
        }
        process_matches(source.raw(), identity, &executable)?;
        check()?;
        Ok(result)
    })();
    source.finish(result)
}

#[cfg(test)]
mod tests;
