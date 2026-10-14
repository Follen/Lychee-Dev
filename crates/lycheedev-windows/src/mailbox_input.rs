//! Formal Go 3.2.0 mailbox input boundary. No foreground activation or chat.
//!
//! This is a low-level safety API, not a Live authorization capability. The
//! caller must supply a guard that rechecks its fresh INPUT witness, publication,
//! ownership and exact binding identity before every message. A saved DTO or
//! successful queue receipt cannot establish any of those facts.

mod engine;
mod equal_fold;
use crate::{Process, WindowIdentity};
use engine::Backend;
pub use engine::{
    CLEANUP_LIMIT, INPUT_LIMIT, InputError, InputFailure, InputReceipt, KEY_CADENCE, MailboxAction,
    MailboxBindings, MailboxControl,
};
pub(crate) use equal_fold::equal_fold_utf16;
use std::{
    marker::PhantomData,
    os::windows::ffi::OsStrExt,
    rc::Rc,
    time::{Duration, Instant},
};
use windows::{
    Win32::{
        Foundation::{
            CloseHandle, HANDLE, HWND, LPARAM, WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0,
            WAIT_TIMEOUT, WPARAM,
        },
        System::{
            SystemInformation::GetTickCount64,
            Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
        },
        UI::{
            Input::KeyboardAndMouse::{GetAsyncKeyState, MAPVK_VK_TO_VSC, MapVirtualKeyW},
            WindowsAndMessaging::{GetClassNameW, IsIconic, PostMessageW},
        },
    },
    core::PCWSTR,
};

/// Complete selected-window identity, sealed only after live OS validation.
/// This is not an INPUT witness: a fresh protocol guard is still mandatory.
/// No public fields or Deserialize permit restoration as an input capability.
#[derive(Clone, Debug)]
pub struct MailboxWindow {
    identity: WindowIdentity,
    executable: Vec<u16>,
    class: Vec<u16>,
}
impl MailboxWindow {
    /// Validate the original selection's executable and exact window class,
    /// in addition to PID, process creation counter and HWND. Never capture a
    /// replacement window's class/executable merely because its HWND matches.
    pub fn bind(identity: WindowIdentity, executable: &str, class: &str) -> Result<Self, String> {
        if identity.hwnd == 0
            || identity.process.pid == 0
            || identity.process.created == 0
            || executable.is_empty()
            || class.is_empty()
            || executable.len() > 32767 * 4
            || class.len() > 255 * 4
            || executable.contains('\0')
            || class.contains('\0')
        {
            return Err("desktop.identity_changed".into());
        }
        let target = Self {
            identity,
            executable: executable.encode_utf16().collect(),
            class: class.encode_utf16().collect(),
        };
        if target.executable.len() >= 32768 || target.class.len() >= 256 {
            return Err("desktop.identity_changed".into());
        }
        target.verify()?;
        Ok(target)
    }
    pub fn identity(&self) -> &WindowIdentity {
        &self.identity
    }
    fn verify(&self) -> Result<(), String> {
        self.identity.verify()?;
        let process = Process::open(self.identity.process.pid)?;
        if process.identity() != &self.identity.process {
            return Err("desktop.identity_changed".into());
        }
        let image = process.image_path()?;
        let actual_executable: Vec<u16> = image.as_os_str().encode_wide().collect();
        let mut class = [0u16; 256];
        // SAFETY: The selected HWND is a scalar OS handle already identity-
        // checked. The initialized 256-unit output slice stays writable/live
        // for this synchronous call; Windows retains no pointer to it.
        let length = unsafe { GetClassNameW(HWND(self.identity.hwnd as *mut _), &mut class) };
        if length <= 0 {
            return Err("desktop.identity_changed".into());
        }
        self.identity.verify()?;
        process.verify()?;
        if !equal_fold::equal_fold_utf16(&self.executable, &actual_executable)
            || !char::decode_utf16(self.class.iter().copied())
                .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER))
                .eq(char::decode_utf16(class[..length as usize].iter().copied())
                    .map(|value| value.unwrap_or(char::REPLACEMENT_CHARACTER)))
        {
            return Err("desktop.identity_changed".into());
        }
        Ok(())
    }
}

/// Current Windows boot-relative milliseconds, matching the fixed Go channel's
/// uptimeMillis/GetTickCount64 source. This observation alone is not proof that
/// a persisted INPUT sample is fresh.
pub fn uptime_millis() -> u64 {
    // SAFETY: Parameterless kernel clock read; no pointers, retained memory,
    // ownership, thread affinity or caller-provided buffer is involved.
    unsafe { GetTickCount64() }
}

/// Queue one of the two observed Ctrl+Alt+F1..F12 actions to this exact HWND.
/// Uses a try-only cross-process lock and a <=60s ordered child of `parent`.
/// Normal down AND up messages each have a 40ms cadence; emergency key release
/// follows Go's immediate reverse order within its independent one-second cap.
/// No key is retried, except releasing a down known to have been queued here.
/// The guard must use the supplied child control and remain bounded; it must
/// never treat binding/window data alone as authorization or reset its budget.
pub fn send_mailbox_action<C: MailboxControl>(
    target: &MailboxWindow,
    bindings: MailboxBindings,
    action: MailboxAction,
    parent: &C,
    guard: impl FnMut(&C) -> Result<(), C::Error>,
) -> Result<InputReceipt, InputFailure<C::Error>> {
    let mut backend = WindowsBackend {
        target: target.clone(),
        lock: None,
        _thread: PhantomData,
    };
    engine::send(&mut backend, parent, bindings, action, guard)
}

struct MutexHandle {
    handle: HANDLE,
    owned: bool,
}
struct WindowsBackend {
    target: MailboxWindow,
    lock: Option<MutexHandle>,
    // A Win32 mutex must be released on its acquiring OS thread. Neither the
    // backend nor its guard can be sent/shared across threads or await points.
    _thread: PhantomData<Rc<()>>,
}
fn os_error(operation: &str, error: windows::core::Error) -> String {
    format!("{operation}: hresult=0x{:08x}", error.code().0 as u32)
}
impl Backend for WindowsBackend {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
    fn verify(&mut self) -> Result<(), String> {
        self.target.verify()
    }
    fn minimized(&mut self) -> bool {
        // SAFETY: HWND is an opaque numeric OS handle, never dereferenced by
        // Rust. Identity was checked immediately before this read; this call
        // retains no memory and does not change activation/focus.
        unsafe { IsIconic(HWND(self.target.identity.hwnd as *mut _)).as_bool() }
    }
    fn pressed(&mut self, vk: u16) -> bool {
        // SAFETY: Only bounded virtual-key integers are queried; this reads
        // physical key state without writing/releasing any user's input.
        unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
    }
    fn scan(&mut self, vk: u16) -> u32 {
        // SAFETY: Pure integer VK-to-scan-code mapping, no pointer/lifetime or
        // keyboard-state mutation. The engine only supplies Ctrl/Alt/F1..F12.
        unsafe { MapVirtualKeyW(u32::from(vk), MAPVK_VK_TO_VSC) }
    }
    fn post(&mut self, message: u32, vk: u16, bits: u32) -> Result<(), String> {
        // SAFETY: The engine has rechecked the exact HWND/process identity
        // before this synchronous call. WM_KEY*/WM_SYSKEY* carry scalar values
        // only, no borrowed pointer survives queueing. A true result records
        // submission, never dispatch; there is no foreground/global input API.
        unsafe {
            PostMessageW(
                Some(HWND(self.target.identity.hwnd as *mut _)),
                message,
                WPARAM(usize::from(vk)),
                LPARAM(bits as isize),
            )
        }
        .map_err(|error| os_error("desktop.post", error))
    }
    fn acquire(&mut self) -> Result<(), String> {
        let name: Vec<u16> = format!(
            "Local\\LycheeDev.Input.{}.{}.{}",
            self.target.identity.process.pid,
            self.target.identity.process.created,
            self.target.identity.hwnd
        )
        .encode_utf16()
        .chain(Some(0))
        .collect();
        // SAFETY: The name is a live, NUL-terminated UTF-16 vector for this
        // synchronous call; Windows copies it and returns an owned kernel
        // handle. No SECURITY_ATTRIBUTES pointer or initial ownership is used.
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }
            .map_err(|error| os_error("desktop.input_lock_create", error))?;
        self.lock = Some(MutexHandle {
            handle,
            owned: false,
        });
        // SAFETY: The just-created handle remains owned by this !Send backend;
        // a zero wait neither blocks nor lets a stale claim queue behind input.
        let status = unsafe { WaitForSingleObject(handle, 0) };
        if status == WAIT_OBJECT_0 || status == WAIT_ABANDONED {
            self.lock.as_mut().expect("handle assigned above").owned = true;
        }
        match status {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_ABANDONED => Err("desktop.input_abandoned".into()),
            WAIT_TIMEOUT => Err("desktop.input_busy".into()),
            WAIT_FAILED => Err(os_error(
                "desktop.input_lock_wait",
                windows::core::Error::from_thread(),
            )),
            _ => Err(format!("desktop.input_lock_status: {}", status.0)),
        }
    }
    fn finish_lock(&mut self) -> Vec<String> {
        let mut errors = Vec::new();
        if let Some(lock) = self.lock.take() {
            if lock.owned {
                // SAFETY: `owned` is set only for WAIT_OBJECT_0/ABANDONED on
                // this same OS thread. The !Send/!Sync backend owns the handle;
                // this explicit finish consumes its one release obligation.
                if let Err(error) = unsafe { ReleaseMutex(lock.handle) } {
                    errors.push(os_error("desktop.input_lock_release", error));
                }
            }
            // SAFETY: This handle came from CreateMutexW and has not been
            // closed. take() ensures exactly one close attempt, including when
            // wait/acquisition or ReleaseMutex failed. No borrower remains.
            if let Err(error) = unsafe { CloseHandle(lock.handle) } {
                errors.push(os_error("desktop.input_lock_close", error));
            }
        }
        errors
    }
}
impl Drop for WindowsBackend {
    fn drop(&mut self) {
        // Normal paths explicitly finish and return every error. This fallback
        // covers unwinding during allocation/FFI setup before that result exists.
        let _ = self.finish_lock();
    }
}

#[cfg(test)]
#[path = "mailbox_input/identity_tests.rs"]
mod identity_tests;
