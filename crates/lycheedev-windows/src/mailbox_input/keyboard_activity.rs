//! Passive WH_KEYBOARD_LL lifetime. The callback never dereferences event data
//! and retains only the latest monotonic timestamp. No text or key log exists.
use super::keyboard_state::KeyboardState;
use std::{
    cell::OnceCell,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    sync::{Arc, mpsc},
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows::Win32::{
    Foundation::{
        HANDLE, HINSTANCE, LPARAM, LRESULT, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT, WPARAM,
    },
    System::{
        LibraryLoader::GetModuleHandleW,
        Threading::{CreateEventW, INFINITE, SetEvent, WaitForSingleObject},
    },
    UI::{Input::KeyboardAndMouse::GetAsyncKeyState, WindowsAndMessaging::*},
};

const START_LIMIT: Duration = Duration::from_secs(2);
const CLOSE_MILLIS: u32 = 1000;
thread_local! {
    // Each dedicated native thread installs at most one hook. OnceCell permits
    // immutable, nonblocking callback access without shared mutable references.
    static ACTIVE: OnceCell<Arc<KeyboardState>> = const { OnceCell::new() };
}

// SAFETY: Registered only as a WH_KEYBOARD_LL callback on the dedicated native
// thread. Its process-lived function address uses the Windows ABI; arguments
// stay opaque scalars and no panic-capable user code or retained data is invoked.
unsafe extern "system" fn keyboard_hook(code: i32, message: WPARAM, data: LPARAM) -> LRESULT {
    if code >= 0
        && matches!(
            message.0 as u32,
            WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP
        )
    {
        // No allocation, lock, key-state query or user callback runs here.
        // The only retained event information is the current timestamp.
        let _ = ACTIVE.try_with(|slot| {
            if let Some(state) = slot.get() {
                state.note(Instant::now());
            }
        });
    }
    // SAFETY: These three scalar parameters are passed through unchanged from
    // Windows. `data` is opaque and is NEVER dereferenced or retained. The
    // process-lived callback always chains, including negative hook codes.
    unsafe { CallNextHookEx(None, code, message, data) }
}

fn os_error(operation: &str, error: windows::core::Error) -> String {
    format!("{operation}: hresult=0x{:08x}", error.code().0 as u32)
}
fn stop_event() -> Result<OwnedHandle, String> {
    // SAFETY: Unnamed manual-reset event with default security; no pointers or
    // inherited access are supplied. Success returns one owned kernel handle.
    let handle = unsafe { CreateEventW(None, true, false, None) }
        .map_err(|e| os_error("desktop.keyboard_stop_event", e))?;
    // SAFETY: Transfer the unique successful CreateEvent handle exactly once.
    // OwnedHandle closes it; worker ownership uses its safe try_clone API.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
}

/// Kept across all known-zero retries in one native backend invocation. The
/// stop event and worker's separate owned clone make timeout/drop memory-safe;
/// an unconfirmed join is an error, never reported as successful cleanup.
pub struct KeyboardActivity {
    stop: OwnedHandle,
    state: Arc<KeyboardState>,
    worker: Option<JoinHandle<Result<(), String>>>,
    close_attempted: bool,
}
impl KeyboardActivity {
    /// Installation is bounded by the original caller deadline and a two-second
    /// startup cap. No 500ms wait occurs here or while any publication lock is held.
    pub fn new(deadline: Instant) -> Result<Self, String> {
        let now = Instant::now();
        if now >= deadline {
            return Err("desktop.keyboard_start_deadline".into());
        }
        let stop = stop_event()?;
        let worker_stop = stop
            .try_clone()
            .map_err(|e| format!("desktop.keyboard_stop_clone: {e}"))?;
        let state = Arc::new(KeyboardState::new(now));
        let worker_state = state.clone();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = std::thread::Builder::new()
            .name("lycheedev-keyboard-activity".into())
            .spawn(move || run(worker_stop, worker_state, ready_tx))
            .map_err(|e| format!("desktop.keyboard_thread: {e}"))?;
        let mut activity = Self {
            stop,
            state,
            worker: Some(worker),
            close_attempted: false,
        };
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(START_LIMIT);
        let started = match ready_rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err("desktop.keyboard_start_deadline".into()),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("desktop.keyboard_start_disconnected".into())
            }
        };
        if let Err(error) = started {
            return match activity.close() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(format!("{error}\n{cleanup}")),
            };
        }
        Ok(activity)
    }

    /// Try-only: every keyboard VK must be released and the most recent event
    /// (including startup) must be at least 500ms old. GetAsyncKeyState is queried
    /// outside the hook because Windows invokes hooks before updating its state.
    pub fn check(&self) -> Result<(), String> {
        self.state.check(Instant::now(), |vk| {
            // SAFETY: Bounded scalar virtual-key query only; no pointer is
            // supplied, and no physical or synthetic key state is modified.
            unsafe { GetAsyncKeyState(i32::from(vk)) < 0 }
        })
    }

    /// Signal an independent kernel event and join only after the OS thread
    /// handle becomes signaled. Failure retains the JoinHandle for a later close;
    /// no dead PID, elapsed age or fabricated successful cleanup is used.
    pub fn close(&mut self) -> Result<(), String> {
        self.close_attempted = true;
        self.state.stop();
        let Some(worker) = self.worker.as_ref() else {
            return Ok(());
        };
        // SAFETY: This parent-owned event handle remains live throughout the
        // synchronous call; the worker has a distinct owned handle to the same
        // event. Signaling neither closes a handle nor accesses worker memory.
        unsafe { SetEvent(HANDLE(self.stop.as_raw_handle())) }
            .map_err(|e| os_error("desktop.keyboard_stop", e))?;
        // SAFETY: JoinHandle owns the thread handle. It is only borrowed for a
        // bounded wait and is not closed or consumed until that wait returns.
        let status = unsafe { WaitForSingleObject(HANDLE(worker.as_raw_handle()), CLOSE_MILLIS) };
        if status == WAIT_TIMEOUT {
            return Err("desktop.keyboard_close_deadline".into());
        }
        if status == WAIT_FAILED {
            return Err(os_error(
                "desktop.keyboard_join_wait",
                windows::core::Error::from_thread(),
            ));
        }
        if status != WAIT_OBJECT_0 {
            return Err(format!("desktop.keyboard_join_status: {}", status.0));
        }
        // The signaled native thread is already terminated; join cannot wait for
        // its message loop. Taking this sole handle makes close idempotent.
        self.worker
            .take()
            .expect("worker was borrowed above")
            .join()
            .map_err(|_| "desktop.keyboard_thread_panicked".to_string())?
    }
}
impl Drop for KeyboardActivity {
    fn drop(&mut self) {
        // Checked paths call close explicitly. A timeout here never closes the
        // worker's event or frees its Arc; standard JoinHandle detachment remains
        // memory-safe until the already-signaled worker exits. It is not success.
        if !self.close_attempted {
            let _ = self.close();
        }
    }
}

fn run(
    stop: OwnedHandle,
    state: Arc<KeyboardState>,
    ready: mpsc::SyncSender<Result<(), String>>,
) -> Result<(), String> {
    struct Stopped(Arc<KeyboardState>);
    impl Drop for Stopped {
        fn drop(&mut self) {
            self.0.stop();
        }
    }
    let _stopped = Stopped(state.clone());
    ACTIVE
        .with(|slot| slot.set(state.clone()))
        .map_err(|_| "desktop.keyboard_thread_reused".to_string())?;
    // SAFETY: Null module name asks only for this executable's module handle.
    // It is borrowed, process-lived, and never unloaded or freed by this code.
    let module =
        unsafe { GetModuleHandleW(None) }.map_err(|e| os_error("desktop.keyboard_module", e));
    let hook = module.and_then(|module| {
        // SAFETY: This process-lived callback is used by a low-level keyboard
        // hook on this dedicated message-loop thread. Module handle is borrowed
        // from this executable. No Rust stack pointer or caller data is retained.
        unsafe {
            SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(keyboard_hook),
                Some(HINSTANCE(module.0)),
                0,
            )
        }
        .map_err(|e| os_error("desktop.keyboard_hook", e))
    });
    let hook = match hook {
        Ok(hook) => hook,
        Err(error) => {
            let _ = ready.send(Err(error.clone()));
            return Err(error);
        }
    };
    state.note(Instant::now());
    let _ = ready.send(Ok(()));
    let result = pump(&stop);
    // SAFETY: This exact successful hook is unhooked once on its installer
    // thread, after the message loop has stopped. Callback code is process-lived
    // and TLS owns its state through the complete unhook attempt.
    let unhook =
        unsafe { UnhookWindowsHookEx(hook) }.map_err(|e| os_error("desktop.keyboard_unhook", e));
    match (result, unhook) {
        (Err(a), Err(b)) => Err(format!("{a}\n{b}")),
        (Err(e), _) | (_, Err(e)) => Err(e),
        _ => Ok(()),
    }
}
fn pump(stop: &OwnedHandle) -> Result<(), String> {
    loop {
        // SAFETY: The local HANDLE array borrows the worker-owned event only
        // for this wait. Message input and stop are event-driven; the event stays
        // alive for the loop. INPUTAVAILABLE includes previously observed input.
        let status = unsafe {
            MsgWaitForMultipleObjectsEx(
                Some(&[HANDLE(stop.as_raw_handle())]),
                INFINITE,
                QS_ALLINPUT,
                MWMO_INPUTAVAILABLE,
            )
        };
        if status == WAIT_OBJECT_0 {
            return Ok(());
        }
        if status == WAIT_FAILED {
            return Err(os_error(
                "desktop.keyboard_messages",
                windows::core::Error::from_thread(),
            ));
        }
        if status.0 != WAIT_OBJECT_0.0 + 1 {
            return Err(format!("desktop.keyboard_message_status: {}", status.0));
        }
        // Bound each drain and let the stop event take precedence even under a
        // continuous event stream. No timer polls for keyboard quietness.
        for _ in 0..256 {
            // SAFETY: Zero-time observation of the worker's live stop handle;
            // no ownership transfer or pointer retention occurs.
            let stopped = unsafe { WaitForSingleObject(HANDLE(stop.as_raw_handle()), 0) };
            if stopped == WAIT_OBJECT_0 {
                return Ok(());
            }
            if stopped == WAIT_FAILED {
                return Err(os_error(
                    "desktop.keyboard_stop_wait",
                    windows::core::Error::from_thread(),
                ));
            }
            let mut message = MSG::default();
            // SAFETY: MSG is initialized writable stack storage for this
            // synchronous thread-local pump. Windows retains no pointer to it.
            if !unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
                break;
            }
            if message.message == WM_QUIT {
                return Ok(());
            }
            // There are no windows or caller callbacks on this dedicated thread.
            // PeekMessage itself dispatches the sent low-level hook callbacks;
            // other posted thread messages carry no work and are discarded.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn passive_monitor_installs_and_closes_without_input() {
        let mut monitor = KeyboardActivity::new(Instant::now() + START_LIMIT).unwrap();
        assert!(monitor.worker.is_some());
        // Exact startup quietness is tested with a controlled clock in
        // keyboard_state; scheduling delay on this host cannot make this flaky.
        monitor.close().unwrap();
        assert!(monitor.worker.is_none());
        assert_eq!(
            monitor.check().unwrap_err(),
            "desktop.keyboard_monitor_closed"
        );
        monitor.close().unwrap();
    }
    #[test]
    fn expired_start_deadline_never_installs_a_listener() {
        assert!(matches!(
            KeyboardActivity::new(Instant::now()).err().as_deref(),
            Some("desktop.keyboard_start_deadline")
        ));
    }
}
