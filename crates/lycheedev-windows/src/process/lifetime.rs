//! Read-only OS lifetime observations for the fixed Go process-end contract.
//! No window, executable memory, input, termination or wait-for-exit authority.
use super::{Handle, ProcessIdentity};
use crate::Result;
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, GetLastError, HANDLE, WAIT_FAILED,
            WAIT_OBJECT_0, WAIT_TIMEOUT,
        },
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
            WaitForSingleObject,
        },
    },
    core::{Error, HRESULT},
};

/// An observation of the selected PID and original creation counter, not a
/// restorable capability. Only the three terminal variants prove that the
/// originally selected lifetime ended. Errors never imply termination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProcessLifetime {
    Running,
    Absent,
    Exited,
    PidReused,
}

/// Observe the original process once, without waiting or consulting a HWND.
/// The caller retains its original cancellation/deadline through `check`, which
/// must be bounded and synchronous. Every ordinary return after opening a
/// handle checked-closes it exactly once, even on cancellation or OS failure.
/// A failed close or control check prevents issuance of a lifetime result;
/// independent failures are joined without replacing the first cause.
#[forbid(unsafe_code)]
pub fn observe_lifetime(
    expected: &ProcessIdentity,
    mut check: impl FnMut() -> Result<()>,
) -> Result<ProcessLifetime> {
    observe_with(expected, &mut check, &mut Windows)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WaitState {
    Running,
    Exited,
}
trait Backend {
    type Owner;
    fn open(&mut self, pid: u32) -> Result<Option<Self::Owner>>;
    fn created(&mut self, owner: &Self::Owner) -> Result<u64>;
    fn wait(&mut self, owner: &Self::Owner) -> Result<WaitState>;
    fn close(&mut self, owner: Self::Owner) -> Result<()>;
}

// All lifecycle business/control orchestration is safe Rust. Only the Windows
// backend's individual synchronous OS calls below cross the FFI boundary.
#[forbid(unsafe_code)]
fn observe_with<B: Backend>(
    expected: &ProcessIdentity,
    check: &mut impl FnMut() -> Result<()>,
    backend: &mut B,
) -> Result<ProcessLifetime> {
    check()?;
    if expected.pid == 0 || expected.created == 0 {
        return Err("desktop.identity_changed".into());
    }
    let opened = backend.open(expected.pid);
    let opened_control = check();
    let owner = match opened {
        Err(error) => return join(Err(error), opened_control),
        Ok(None) => return join(Ok(ProcessLifetime::Absent), opened_control),
        Ok(Some(owner)) => owner,
    };
    let observation = (|| {
        opened_control?;
        let created = backend.created(&owner);
        let created = join(created, check())?;
        if created != expected.created {
            return Ok(ProcessLifetime::PidReused);
        }
        let state = backend.wait(&owner);
        Ok(match join(state, check())? {
            WaitState::Running => ProcessLifetime::Running,
            WaitState::Exited => ProcessLifetime::Exited,
        })
    })();
    // Control failures never skip cleanup or reset the caller's deadline. Take
    // the same checks around close on success and every post-open error path.
    let before_close = check();
    let close = backend.close(owner);
    let after_close = check();
    join(join(join(observation, before_close), close), after_close)
}

#[forbid(unsafe_code)]
fn join<T>(result: Result<T>, next: Result<()>) -> Result<T> {
    match (result, next) {
        (result, Ok(())) => result,
        (Ok(_), Err(error)) => Err(error),
        (Err(mut first), Err(next)) => {
            for cause in next.lines() {
                if !first.lines().any(|prior| prior == cause) {
                    first.push('\n');
                    first.push_str(cause);
                }
            }
            Err(first)
        }
    }
}
fn os_error(operation: &str, error: Error) -> String {
    format!("process.lifetime_{operation}: {error}")
}
fn opened<T>(result: windows::core::Result<T>) -> Result<Option<T>> {
    match result {
        Ok(owner) => Ok(Some(owner)),
        Err(error) if error.code() == HRESULT::from_win32(ERROR_INVALID_PARAMETER.0) => Ok(None),
        Err(error) => Err(os_error("open", error)),
    }
}
fn waited(
    state: windows::Win32::Foundation::WAIT_EVENT,
    error: impl FnOnce() -> Error,
) -> Result<WaitState> {
    match state {
        WAIT_OBJECT_0 => Ok(WaitState::Exited),
        WAIT_TIMEOUT => Ok(WaitState::Running),
        WAIT_FAILED => Err(os_error("wait", error())),
        _ => Err("desktop.identity_changed".into()),
    }
}
// Disarm ownership before making the one checked close attempt. Retrying even
// a failed close could close a reused OS handle. Drop is only an unwind fallback.
fn consume_close<T>(
    owner: T,
    raw: impl FnOnce(&T) -> HANDLE,
    close: impl FnOnce(HANDLE) -> Result<()>,
) -> Result<()> {
    let handle = raw(&owner);
    std::mem::forget(owner);
    close(handle)
}
struct Windows;
impl Backend for Windows {
    type Owner = Handle;
    fn open(&mut self, pid: u32) -> Result<Option<Handle>> {
        // SAFETY: scalar PID/access bits contain no pointers. The successful
        // handle has one new owner; it is non-inheritable and requests only
        // process query plus synchronization, never VM read/write or terminate.
        opened(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                false,
                pid,
            )
        })
        .map(|owner| owner.map(Handle))
    }
    fn created(&mut self, owner: &Handle) -> Result<u64> {
        let (mut created, mut exited, mut kernel, mut user) = (
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
            FILETIME::default(),
        );
        // SAFETY: owner holds this exact process handle for the entire borrow;
        // all four initialized FILETIME buffers are distinct, writable and live
        // through the synchronous call, which retains none of their pointers.
        unsafe { GetProcessTimes(owner.0, &mut created, &mut exited, &mut kernel, &mut user) }
            .map_err(|error| os_error("times", error))?;
        Ok((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
    }
    fn wait(&mut self, owner: &Handle) -> Result<WaitState> {
        // SAFETY: the unique owner keeps the borrowed handle valid; timeout 0
        // only observes its signaled state, retaining no pointers or ownership.
        let state = unsafe { WaitForSingleObject(owner.0, 0) };
        // Read the thread's last error immediately on WAIT_FAILED, before any
        // caller control check or other OS operation can replace it.
        waited(state, || {
            // SAFETY: GetLastError reads only this thread's last-error scalar.
            // The lazy callback runs solely for WAIT_FAILED, before any other
            // OS call or caller control check can replace that error.
            let error = unsafe { GetLastError() };
            Error::from_hresult(HRESULT::from_win32(error.0))
        })
    }
    fn close(&mut self, owner: Handle) -> Result<()> {
        consume_close(
            owner,
            |owner| owner.0,
            |handle| {
                // SAFETY: this is the consumed unique owned handle. Its Drop has
                // been disarmed and no references remain; this is its sole close
                // attempt, even if Windows reports failure.
                unsafe { CloseHandle(handle) }.map_err(|error| os_error("close", error))
            },
        )
    }
}

#[cfg(test)]
mod tests;
