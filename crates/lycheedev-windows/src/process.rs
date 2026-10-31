use crate::Result;
mod lifetime;
pub use lifetime::{ProcessLifetime, observe_lifetime};
mod runtime_module;
pub use runtime_module::{
    RuntimeBudget, RuntimeControl, RuntimeCost, RuntimeImageGuard, RuntimeImageLayout,
    RuntimeImageSection, RuntimeLimits, RuntimeModule, RuntimeModuleIdentity, RuntimeReadFailure,
    RuntimeReadKind, RuntimeUsage,
};
use serde::{Deserialize, Serialize};
use std::os::windows::io::{IntoRawHandle, RawHandle};
use windows::Win32::{
    Foundation::*,
    System::{Diagnostics::ToolHelp::*, Memory::*, ProcessStatus::*, Threading::*},
    UI::WindowsAndMessaging::*,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProcessIdentity {
    pub pid: u32,
    pub created: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProcessMemory {
    pub identity: ProcessIdentity,
    pub private_commit: u64,
    pub working_set: u64,
    pub peak_working_set: u64,
    pub peak_private_commit: u64,
    pub scope: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct WindowIdentity {
    pub process: ProcessIdentity,
    pub hwnd: usize,
}
struct Handle(HANDLE);
impl IntoRawHandle for Handle {
    fn into_raw_handle(self) -> RawHandle {
        let raw = self.0.0;
        std::mem::forget(self);
        raw
    }
}
impl Handle {
    fn close(self) -> Result<()> {
        let result = crate::guarded_spawn::close_owned_handle(self);
        #[cfg(test)]
        let result = checked_query_tests::observe_close(result);
        result.map_err(|error| format!("process.handle_close: {error}"))
    }
    fn finish<T>(self, result: Result<T>) -> Result<T> {
        finish_query(result, self.close())
    }
}
pub(crate) fn finish_query<T>(result: Result<T>, close: Result<()>) -> Result<T> {
    match (result, close) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(error),
        (Err(mut primary), Err(cleanup)) => {
            primary.push('\n');
            primary.push_str(&cleanup);
            Err(primary)
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        let result = unsafe { CloseHandle(self.0) }.map_err(|error| error.to_string());
        #[cfg(test)]
        let result = checked_query_tests::observe_close(result);
        let _ = result;
    }
}
#[cfg(test)]
#[path = "process/checked_query_tests.rs"]
pub(crate) mod checked_query_tests;
pub struct Process {
    handle: Handle,
    identity: ProcessIdentity,
}
fn finish_process_wait(
    check: &mut impl FnMut() -> Result<()>,
    read_exit: impl FnOnce() -> Result<u32>,
) -> Result<u32> {
    check()?;
    let exit = read_exit()?;
    check()?;
    Ok(exit)
}
impl Process {
    /// Consume the original process owner once. An uncertain close is never retried.
    pub fn close(self) -> Result<()> {
        self.handle.close()
    }
    /// Preserve a query failure together with the original owner's close failure.
    pub fn finish<T>(self, result: Result<T>) -> Result<T> {
        self.handle.finish(result)
    }
    pub fn memory(&self) -> Result<ProcessMemory> {
        self.verify()?;
        let mut memory = PROCESS_MEMORY_COUNTERS_EX {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            ..Default::default()
        };
        unsafe {
            GetProcessMemoryInfo(
                self.handle.0,
                &mut memory as *mut _ as *mut PROCESS_MEMORY_COUNTERS,
                std::mem::size_of_val(&memory) as u32,
            )
            .map_err(|e| e.to_string())?;
        }
        self.verify()?;
        Ok(ProcessMemory{identity:self.identity.clone(),private_commit:memory.PrivateUsage as u64,working_set:memory.WorkingSetSize as u64,peak_working_set:memory.PeakWorkingSetSize as u64,peak_private_commit:memory.PeakPagefileUsage as u64,
            scope:"held process handle; GetProcessMemoryInfo PROCESS_MEMORY_COUNTERS_EX; PrivateUsage commit distinct from WorkingSetSize RSS".into()})
    }
    pub fn parent_pid(&self) -> Result<u32> {
        self.verify()?;
        unsafe {
            let snapshot =
                Handle(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.to_string())?);
            let result = (|| {
                let mut entry = PROCESSENTRY32W {
                    dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                    ..Default::default()
                };
                Process32FirstW(snapshot.0, &mut entry).map_err(|e| e.to_string())?;
                for _ in 0..65536 {
                    if entry.th32ProcessID == self.identity.pid {
                        self.verify()?;
                        if entry.th32ParentProcessID == 0 {
                            return Err("process.parent_missing".into());
                        }
                        return Ok(entry.th32ParentProcessID);
                    }
                    if Process32NextW(snapshot.0, &mut entry).is_err() {
                        return Err("process.parent_missing".into());
                    }
                }
                Err("process.inventory_budget".into())
            })();
            snapshot.finish(result)
        }
    }
    pub fn identity(&self) -> &ProcessIdentity {
        &self.identity
    }
    /// Read the complete caller-owned control without exporting a writable
    /// cancellation token. `check` is synchronous and called on each wait slice;
    /// callers can preserve ancestor cancellation and deadline event ordering.
    /// The closure must be bounded and the deadline cannot extend its control.
    pub fn wait_exit_with_control(
        &self,
        deadline: std::time::Instant,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<u32> {
        // SAFETY: `self` owns this process handle for the entire borrow. The
        // identity helper writes only its own initialized FILETIME buffers.
        if unsafe { identity(self.handle.0, self.identity.pid)? } != self.identity {
            return Err("process.identity_changed".into());
        }
        loop {
            check()?;
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                // A cancellation may have linearized before the deadline
                // between the first check and this clock read. Let the caller
                // resolve its actual cause before applying this wait's limit.
                check()?;
                return Err("command.deadline: process exit wait".into());
            }
            // SAFETY: The borrowed owned handle stays open, and a bounded wait
            // does not transfer ownership or retain any caller memory.
            let state =
                unsafe { WaitForSingleObject(self.handle.0, left.as_millis().clamp(1, 50) as u32) };
            if state == WAIT_OBJECT_0 {
                return finish_process_wait(&mut check, || {
                    let mut exit = 0;
                    // SAFETY: `exit` is a writable u32 for this synchronous call;
                    // the borrowed process handle has not been closed or reopened.
                    unsafe {
                        GetExitCodeProcess(self.handle.0, &mut exit).map_err(|e| e.to_string())?;
                    }
                    Ok(exit)
                });
            }
            if state != WAIT_TIMEOUT {
                return Err("process.wait_failed".into());
            }
        }
    }
    pub fn open(pid: u32) -> Result<Self> {
        unsafe {
            let handle = Handle(
                OpenProcess(
                    PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_SYNCHRONIZE,
                    false,
                    pid,
                )
                .map_err(|e| e.to_string())?,
            );
            match identity(handle.0, pid) {
                Ok(identity) => Ok(Self { handle, identity }),
                Err(error) => handle.finish(Err(error)),
            }
        }
    }
    pub fn verify(&self) -> Result<()> {
        unsafe {
            if WaitForSingleObject(self.handle.0, 0) != WAIT_TIMEOUT {
                return Err("process.exited".into());
            }
            if identity(self.handle.0, self.identity.pid)? != self.identity {
                return Err("process.identity_changed".into());
            }
            Ok(())
        }
    }
    pub fn image_path(&self) -> Result<std::path::PathBuf> {
        unsafe {
            self.verify()?;
            let mut text = vec![0u16; 32768];
            let mut len = text.len() as u32;
            QueryFullProcessImageNameW(
                self.handle.0,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(text.as_mut_ptr()),
                &mut len,
            )
            .map_err(|e| e.to_string())?;
            Ok(std::path::PathBuf::from(String::from_utf16_lossy(
                &text[..len as usize],
            )))
        }
    }
}

#[cfg(test)]
mod wait_exit_tests {
    use super::*;
    use std::{
        cell::Cell,
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };

    #[test]
    fn cancellation_at_completion_boundaries_stops_controlled_exit() {
        for before_exit_read in [true, false] {
            let cancelled = AtomicBool::new(false);
            let reads = Cell::new(0);
            let mut check = || {
                if cancelled.load(Ordering::Acquire) {
                    Err("fixture.late_cancel".into())
                } else {
                    Ok(())
                }
            };
            if before_exit_read {
                cancelled.store(true, Ordering::Release);
            }
            let result = finish_process_wait(&mut check, || {
                reads.set(reads.get() + 1);
                cancelled.store(true, Ordering::Release);
                Ok(42)
            });
            assert_eq!(result, Err("fixture.late_cancel".into()));
            assert_eq!(reads.get(), if before_exit_read { 0 } else { 1 });
        }
    }
    #[test]
    fn wait_exit_control_observes_parent_without_a_raw_token() {
        let own = Process::open(std::process::id()).unwrap();
        let cancelled = AtomicBool::new(false);
        let mut checks = 0;
        let result = own.wait_exit_with_control(Instant::now() + Duration::from_secs(5), || {
            checks += 1;
            if checks == 2 {
                cancelled.store(true, Ordering::Release);
            }
            if cancelled.load(Ordering::Acquire) {
                Err("fixture.parent_cancelled".into())
            } else {
                Ok(())
            }
        });
        assert_eq!(result, Err("fixture.parent_cancelled".into()));
        assert_eq!(checks, 2);
        assert_eq!(own.identity().pid, std::process::id());
        own.verify().unwrap();
    }

    #[test]
    fn wait_exit_control_preserves_caller_stop_cause() {
        let own = Process::open(std::process::id()).unwrap();
        let result = own.wait_exit_with_control(Instant::now(), || {
            Err("fixture.deadline_before_cancel".into())
        });
        assert_eq!(result, Err("fixture.deadline_before_cancel".into()));
        let mut checks = 0;
        let result = own.wait_exit_with_control(Instant::now(), || {
            checks += 1;
            if checks == 1 {
                Ok(())
            } else {
                Err("fixture.cancel_before_clock_read".into())
            }
        });
        assert_eq!(result, Err("fixture.cancel_before_clock_read".into()));
        assert_eq!(checks, 2);
    }
}
fn readable(p: PAGE_PROTECTION_FLAGS) -> bool {
    p.0 & PAGE_GUARD.0 == 0 && matches!(p.0 & 0xff, 0x02 | 0x04 | 0x08 | 0x20 | 0x40 | 0x80)
}

unsafe fn identity(h: HANDLE, pid: u32) -> Result<ProcessIdentity> {
    unsafe {
        let (mut c, mut e, mut k, mut u) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u).map_err(|e| e.to_string())?;
        Ok(ProcessIdentity {
            pid,
            created: (c.dwHighDateTime as u64) << 32 | c.dwLowDateTime as u64,
        })
    }
}
impl WindowIdentity {
    pub fn verify(&self) -> Result<()> {
        let p = Process::open(self.process.pid)?;
        let result = (|| {
            if p.identity != self.process {
                return Err("window.process_reused".into());
            }
            unsafe {
                let mut pid = 0;
                let h = HWND(self.hwnd as *mut _);
                if !IsWindow(Some(h)).as_bool() {
                    return Err("window.closed".into());
                }
                GetWindowThreadProcessId(h, Some(&mut pid));
                if pid != self.process.pid {
                    return Err("window.reused".into());
                }
                Ok(())
            }
        })();
        p.finish(result)
    }
}
