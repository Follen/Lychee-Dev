//! Real messages go ONLY to message-only HWNDs constructed by this process.
//! No window discovery, foreground activation, SendInput, chat or game access.
#![cfg(windows)]
use lycheedev_windows::{
    Process, WindowIdentity,
    mailbox_input::{
        InputError, KeyboardActivity, MailboxAction, MailboxBindings, MailboxControl,
        MailboxWindow, send_mailbox_action, uptime_millis,
    },
};
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::w,
};

struct OwnedWindow {
    hwnd: HWND,
    _thread: PhantomData<Rc<()>>,
}
impl OwnedWindow {
    fn new() -> Self {
        // SAFETY: STATIC is a Windows-owned class; all strings are static
        // UTF-16. HWND_MESSAGE creates an invisible, non-activating window on
        // this test's OS thread. No pointer or external HWND is supplied.
        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("LycheeDev formal mailbox fixture"),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
        }
        .unwrap();
        Self {
            hwnd,
            _thread: PhantomData,
        }
    }
    fn identity(&self) -> WindowIdentity {
        WindowIdentity {
            process: Process::open(std::process::id())
                .unwrap()
                .identity()
                .clone(),
            hwnd: self.hwnd.0 as usize,
        }
    }
    fn target(&self) -> MailboxWindow {
        let process = Process::open(std::process::id()).unwrap();
        let executable = process
            .image_path()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap();
        MailboxWindow::bind(self.identity(), &executable, &self.class_name()).unwrap()
    }
    fn class_name(&self) -> String {
        let mut class = [0u16; 256];
        // SAFETY: This thread owns the live HWND; the initialized writable
        // UTF-16 output slice remains valid for this synchronous bounded read.
        let length = unsafe { GetClassNameW(self.hwnd, &mut class) };
        assert!(length > 0);
        String::from_utf16(&class[..length as usize]).unwrap()
    }
    fn queued(&self) -> Vec<(u32, usize, u32)> {
        let mut messages = Vec::new();
        loop {
            let mut message = MSG::default();
            // SAFETY: MSG is initialized writable storage valid for this
            // call; the filter is exclusively this thread's own live HWND.
            // Peek removes scalar queued messages without dispatching them.
            let found = unsafe {
                PeekMessageW(
                    &mut message,
                    Some(self.hwnd),
                    WM_KEYFIRST,
                    WM_KEYLAST,
                    PM_REMOVE,
                )
                .as_bool()
            };
            if !found {
                return messages;
            }
            assert_eq!(message.hwnd, self.hwnd);
            messages.push((message.message, message.wParam.0, message.lParam.0 as u32));
        }
    }
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        // SAFETY: This !Send/!Sync guard exclusively owns a HWND created on
        // this same thread. No callback or queued borrowed pointer is retained.
        let destroyed = unsafe { DestroyWindow(self.hwnd) };
        if !std::thread::panicking() {
            destroyed.expect("destroy owned fixture window");
        }
    }
}
fn foreground() -> HWND {
    // SAFETY: Parameterless read-only observation; no memory or ownership is
    // transferred and the returned opaque handle is never targeted by tests.
    unsafe { GetForegroundWindow() }
}
#[derive(Clone)]
struct Control {
    deadline: Instant,
    cancelled: Arc<Mutex<Option<Instant>>>,
}
impl Control {
    fn new() -> Self {
        Self {
            deadline: Instant::now() + Duration::from_secs(10),
            cancelled: Arc::new(Mutex::new(None)),
        }
    }
    fn cancel(&self) {
        let mut value = self.cancelled.lock().unwrap();
        if value.is_none() {
            *value = Some(Instant::now());
        }
    }
}
impl MailboxControl for Control {
    type Error = String;
    fn deadline(&self) -> Instant {
        self.deadline
    }
    fn ordered_child(&self, deadline: Instant) -> Result<Self, String> {
        Ok(Self {
            deadline: deadline.min(self.deadline),
            cancelled: self.cancelled.clone(),
        })
    }
    fn check(&self) -> Result<(), String> {
        let cancellation = self.cancelled.lock().unwrap();
        if cancellation.is_some_and(|at| at < self.deadline) {
            Err("fixture.original_cancelled".into())
        } else if Instant::now() >= self.deadline {
            Err("fixture.original_deadline".into())
        } else {
            Ok(())
        }
    }
}
fn bindings() -> MailboxBindings {
    MailboxBindings::new("ALT-CTRL-F12", "ALT-CTRL-F11").unwrap()
}

fn quiet_keyboard(control: &Control) -> KeyboardActivity {
    let keyboard = KeyboardActivity::new(control.deadline()).unwrap();
    loop {
        control.check().unwrap();
        match keyboard.check() {
            Ok(()) => return keyboard,
            Err(error)
                if matches!(
                    error.as_str(),
                    "desktop.keyboard_active" | "desktop.key_already_held"
                ) =>
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => panic!("passive keyboard fixture failed: {error}"),
        }
    }
}

#[test]
fn formal_reload_and_invoke_use_distinct_function_keys_without_focus_or_chat() {
    let window = OwnedWindow::new();
    let other = OwnedWindow::new();
    let target = window.target();
    for (action, key) in [(MailboxAction::Reload, 0x7a), (MailboxAction::Invoke, 0x7b)] {
        let previous_foreground = foreground();
        let control = Control::new();
        let mut keyboard = quiet_keyboard(&control);
        let started = Instant::now();
        let receipt =
            send_mailbox_action(&target, bindings(), action, &control, &keyboard, |_| Ok(()))
                .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(240));
        assert_eq!(foreground(), previous_foreground);
        assert_eq!(receipt.messages_queued, 6);
        assert!(receipt.submission_complete && receipt.release_complete);
        assert_eq!(&window.identity(), target.identity());
        let packets = window.queued();
        assert_eq!(packets.len(), 6);
        let keys = [0x11, 0x12, key, key, 0x12, 0x11];
        let messages = [
            WM_KEYDOWN,
            WM_SYSKEYDOWN,
            WM_SYSKEYDOWN,
            WM_SYSKEYUP,
            WM_SYSKEYUP,
            WM_KEYUP,
        ];
        for (i, (message, vk, bits)) in packets.into_iter().enumerate() {
            assert_eq!((message, vk), (messages[i], keys[i]));
            assert_eq!(bits & 0xffff, 1);
            assert_eq!(bits & (1 << 29) != 0, (1..5).contains(&i));
            assert_eq!(bits & (3 << 30), if i >= 3 { 3 << 30 } else { 0 });
        }
        assert!(other.queued().is_empty());
        keyboard.close().unwrap();
    }
}
#[test]
fn formal_cancelled_chord_releases_only_injected_control_on_owned_window() {
    let window = OwnedWindow::new();
    let control = Control::new();
    let mut keyboard = quiet_keyboard(&control);
    let mut checks = 0;
    let failure = send_mailbox_action(
        &window.target(),
        bindings(),
        MailboxAction::Invoke,
        &control,
        &keyboard,
        |child| {
            checks += 1;
            if checks == 3 {
                child.cancel();
            }
            child.check()
        },
    )
    .unwrap_err();
    keyboard.close().unwrap();
    assert!(
        matches!(failure.cause, Some(InputError::Guard(ref error)) if error == "fixture.original_cancelled")
    );
    assert_eq!(failure.receipt.messages_queued, 2);
    assert!(!failure.receipt.submission_complete);
    assert!(failure.receipt.release_complete);
    let packets = window.queued();
    assert_eq!(
        packets
            .iter()
            .map(|(m, vk, _)| (*m, *vk))
            .collect::<Vec<_>>(),
        [(WM_KEYDOWN, 0x11), (WM_KEYUP, 0x11)]
    );
    assert_eq!(checks, 3);
}
#[test]
fn changed_owned_process_identity_cannot_construct_input_target() {
    let window = OwnedWindow::new();
    let mut target = window.identity();
    target.process.created ^= 1;
    let executable = std::env::current_exe()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap();
    let error = MailboxWindow::bind(target, &executable, "STATIC").unwrap_err();
    assert_eq!(error, "window.process_reused");
    assert!(window.queued().is_empty());
}
#[test]
fn mismatched_executable_or_class_cannot_construct_input_target() {
    let window = OwnedWindow::new();
    let executable = std::env::current_exe()
        .unwrap()
        .into_os_string()
        .into_string()
        .unwrap();
    let actual_class = window.class_name();
    for (exe, class) in [
        (executable.as_str(), "LycheeDevWrongClass"),
        ("C:\\wrong-fixture-path.exe", actual_class.as_str()),
    ] {
        let error = MailboxWindow::bind(window.identity(), exe, class).unwrap_err();
        assert_eq!(error, "desktop.identity_changed");
        assert!(window.queued().is_empty());
    }
}
#[test]
fn busy_input_mutex_does_not_run_fresh_guard_or_queue_stale_input() {
    let window = OwnedWindow::new();
    let target = window.target();
    let name = format!(
        "Local\\LycheeDev.Input.{}.{}.{}",
        target.identity().process.pid,
        target.identity().process.created,
        target.identity().hwnd
    );
    let (ready_tx, ready_rx) = mpsc::sync_channel(0);
    let (release_tx, release_rx) = mpsc::sync_channel(0);
    // Only this test's fixed mutex is acquired. The worker cannot send keys.
    let thread = std::thread::spawn(move || {
        use windows::{
            Win32::{
                Foundation::{CloseHandle, WAIT_OBJECT_0},
                System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject},
            },
            core::PCWSTR,
        };
        let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: this fixture owns the named handle and releases its same-thread ownership before closing it.
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(name.as_ptr())) }.unwrap();
        let acquired = unsafe { WaitForSingleObject(handle, 0) };
        if acquired != WAIT_OBJECT_0 {
            unsafe { CloseHandle(handle) }.unwrap();
            panic!("fixture mutex unavailable");
        }
        ready_tx.send(()).unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(5));
        unsafe { ReleaseMutex(handle) }.unwrap();
        unsafe { CloseHandle(handle) }.unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let control = Control::new();
    let mut keyboard = KeyboardActivity::new(control.deadline()).unwrap();
    let failure = send_mailbox_action(
        &target,
        bindings(),
        MailboxAction::Invoke,
        &control,
        &keyboard,
        |_| panic!("busy must precede authorization"),
    )
    .unwrap_err();
    // Release/reap before assertions so an assertion failure strands no worker.
    release_tx.send(()).unwrap();
    thread.join().unwrap();
    keyboard.close().unwrap();
    assert!(
        matches!(failure.cause, Some(InputError::Platform(ref value)) if value == "desktop.input_busy")
    );
    assert_eq!(failure.receipt.messages_queued, 0);
    assert!(window.queued().is_empty());
}
#[test]
fn uptime_is_monotonic_boot_relative_observation() {
    let first = uptime_millis();
    let second = uptime_millis();
    assert!(second >= first);
}
