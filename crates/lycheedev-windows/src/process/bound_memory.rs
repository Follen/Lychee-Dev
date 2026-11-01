//! Read-only same-user process memory bound to an original lifetime, executable
//! and exact loaded module. This capability has no write/input/retirement right.
use super::{Handle, Process, ProcessIdentity, identity};
use crate::Result;
use std::{
    ffi::OsString,
    os::windows::ffi::OsStringExt,
    path::{Path, PathBuf},
};
use windows::{
    Win32::{
        Foundation::*,
        Security::*,
        System::{
            Diagnostics::{Debug::ReadProcessMemory, ToolHelp::*},
            Threading::*,
        },
    },
    core::PWSTR,
};

pub const ACCESS_DENIED: &str = "process.memory_access_denied";
pub struct BoundMemory {
    process: Process,
    base: u64,
    size: u64,
}
fn fault() -> String {
    "process.bound_memory_unavailable".into()
}
pub(super) fn same_path(left: &Path, right: &Path) -> bool {
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
}
pub(super) fn user_sid(process: HANDLE) -> Result<Vec<u8>> {
    let mut token = HANDLE::default();
    // SAFETY: the original process handle is borrowed; the API initializes only
    // this owned token output. TOKEN_QUERY permits metadata, not impersonation.
    unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) }.map_err(|_| fault())?;
    let token = Handle(token);
    let result = (|| {
        let mut needed = 0;
        let _ = unsafe { GetTokenInformation(token.0, TokenUser, None, 0, &mut needed) };
        if !(std::mem::size_of::<TOKEN_USER>()..=65536).contains(&(needed as usize)) {
            return Err(fault());
        }
        let mut storage = vec![0u64; (needed as usize).div_ceil(8)];
        // SAFETY: aligned owned storage has the requested byte capacity and
        // stays live until the SID copy below is complete.
        unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(storage.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .map_err(|_| fault())?;
        let sid = unsafe { (*storage.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let start = storage.as_ptr() as usize;
        let end = start + storage.len() * 8;
        let at = sid.0 as usize;
        if at < start || at > end.saturating_sub(8) {
            return Err(fault());
        }
        // GetLengthSid reads the fixed SID header. Verify the resulting complete
        // byte range before constructing any Rust slice into the native buffer.
        let size = unsafe { GetLengthSid(sid) } as usize;
        if !(8..=68).contains(&size) || size > end - at {
            return Err(fault());
        }
        Ok(unsafe { std::slice::from_raw_parts(sid.0.cast::<u8>(), size) }.to_vec())
    })();
    token.finish(result)
}
pub(super) fn image(process: &Process) -> Result<PathBuf> {
    let mut path = vec![0u16; 32768];
    let mut size = path.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            process.handle.0,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut size,
        )
    }
    .map_err(|_| fault())?;
    path.truncate(size as usize);
    PathBuf::from(OsString::from_wide(&path))
        .canonicalize()
        .map_err(|_| fault())
}
impl BoundMemory {
    pub fn open(
        expected: &ProcessIdentity,
        executable: &Path,
        module: &Path,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        check()?;
        if expected.pid == 0 || !executable.is_absolute() || !module.is_absolute() {
            return Err(fault());
        }
        let executable = executable.canonicalize().map_err(|_| fault())?;
        let module = module.canonicalize().map_err(|_| fault())?;
        // SAFETY: only query/read/synchronize rights, without inheritance. An
        // access-denied result is distinguished for an explicit same-user UAC
        // channel operation; it never licenses another login or a target swap.
        let handle = Handle(
            unsafe {
                OpenProcess(
                    PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
                    false,
                    expected.pid,
                )
            }
            .map_err(|error| {
                if error.code() == ERROR_ACCESS_DENIED.to_hresult() {
                    ACCESS_DENIED.into()
                } else {
                    fault()
                }
            })?,
        );
        let actual = match unsafe { identity(handle.0, expected.pid) } {
            Ok(actual) => actual,
            Err(error) => return handle.finish(Err(error)),
        };
        let process = Process {
            handle,
            identity: actual,
        };
        let opened = (|| {
            check()?;
            process.verify()?;
            if process.identity() != expected || !same_path(&image(&process)?, &executable) {
                return Err(fault());
            }
            if user_sid(process.handle.0)? != user_sid(unsafe { GetCurrentProcess() })? {
                return Err("process.memory_user_mismatch".into());
            }
            check()?;
            let snapshot = Handle(
                unsafe {
                    CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, expected.pid)
                }
                .map_err(|_| fault())?,
            );
            let found = (|| {
                let mut entry = MODULEENTRY32W {
                    dwSize: std::mem::size_of::<MODULEENTRY32W>() as u32,
                    ..Default::default()
                };
                unsafe { Module32FirstW(snapshot.0, &mut entry) }.map_err(|_| fault())?;
                let mut found = None;
                for _ in 0..4096 {
                    check()?;
                    let count = entry
                        .szExePath
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExePath.len());
                    let path = PathBuf::from(OsString::from_wide(&entry.szExePath[..count]));
                    if path
                        .canonicalize()
                        .ok()
                        .is_some_and(|path| same_path(&path, &module))
                    {
                        if found.is_some() {
                            return Err(fault());
                        }
                        let base = entry.modBaseAddr as u64;
                        let size = entry.modBaseSize as u64;
                        if size == 0
                            || size > 512 << 20
                            || base < 0x10000
                            || base
                                .checked_add(size)
                                .is_none_or(|end| end > 0x0000_8000_0000_0000)
                        {
                            return Err(fault());
                        }
                        found = Some((base, size));
                    }
                    match unsafe { Module32NextW(snapshot.0, &mut entry) } {
                        Ok(()) => {}
                        Err(error) if error.code() == ERROR_NO_MORE_FILES.to_hresult() => {
                            return found.ok_or_else(fault);
                        }
                        Err(_) => return Err(fault()),
                    }
                }
                Err(fault())
            })();
            let found = snapshot.finish(found)?;
            process.verify()?;
            check()?;
            Ok(found)
        })();
        match opened {
            Ok((base, size)) => Ok(Self {
                process,
                base,
                size,
            }),
            Err(error) => process.finish(Err(error)),
        }
    }
    pub fn module_base(&self) -> u64 {
        self.base
    }
    pub fn module_size(&self) -> u64 {
        self.size
    }
    pub fn verify(&self, mut check: impl FnMut() -> Result<()>) -> Result<()> {
        check()?;
        self.process.verify()?;
        check()
    }
    pub fn read(
        &self,
        address: u64,
        size: usize,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Vec<u8>> {
        check()?;
        if !(1..=1 << 20).contains(&size)
            || address < 0x10000
            || address
                .checked_add(size as u64)
                .is_none_or(|end| end > 0x0000_8000_0000_0000)
        {
            return Err("process.memory_read_range".into());
        }
        self.process.verify()?;
        let mut output = vec![0u8; size];
        let mut count = 0;
        // SAFETY: the held original read-only process handle remains live; the
        // output is a uniquely borrowed initialized allocation of `size` bytes.
        // Remote addresses are never dereferenced as Rust pointers. Require the
        // complete OS copy and original lifetime before returning its bytes.
        unsafe {
            ReadProcessMemory(
                self.process.handle.0,
                address as usize as *const _,
                output.as_mut_ptr().cast(),
                size,
                Some(&mut count),
            )
        }
        .map_err(|_| fault())?;
        if count != size {
            return Err(fault());
        }
        self.process.verify()?;
        check()?;
        Ok(output)
    }
    pub fn close(self) -> Result<()> {
        self.process.close()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn own_process_reads_require_the_original_lifetime_image_and_module_and_release_all_handles() {
        let process = Process::open(std::process::id()).unwrap();
        let expected = process.identity().clone();
        process.close().unwrap();
        let image = std::env::current_exe().unwrap();
        let memory = BoundMemory::open(&expected, &image, &image, || Ok(())).unwrap();
        assert!(memory.module_size() > 2);
        assert_eq!(
            memory.read(memory.module_base(), 2, || Ok(())).unwrap(),
            b"MZ"
        );
        let content = Box::new(*b"synthetic-owned-memory-no-credentials");
        assert_eq!(
            memory
                .read(content.as_ptr() as u64, content.len(), || Ok(()))
                .unwrap(),
            *content
        );
        assert!(memory.read(1, 1, || Ok(())).is_err());
        assert!(
            memory
                .read(memory.module_base(), (1 << 20) + 1, || Ok(()))
                .is_err()
        );
        assert_eq!(
            memory
                .read(content.as_ptr() as u64, content.len(), || Err(
                    "original-cancel".into()
                ))
                .unwrap_err(),
            "original-cancel"
        );
        memory.close().unwrap();
        let mut stale = expected.clone();
        stale.created += 1;
        assert!(BoundMemory::open(&stale, &image, &image, || Ok(())).is_err());
        let different = crate::filesystem::system_directory()
            .unwrap()
            .join("cmd.exe");
        assert!(BoundMemory::open(&expected, &different, &image, || Ok(())).is_err());
        assert!(BoundMemory::open(&expected, &image, &different, || Ok(())).is_err());
        assert!(
            matches!(BoundMemory::open(&expected,&image,&image,||Err("early-cancel".into())),Err(error) if error=="early-cancel")
        );
    }
}
