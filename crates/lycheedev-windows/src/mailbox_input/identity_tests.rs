//! Identity failures use real reads/posts on this test's own message-only HWND.
//! The expected identity is fault-injected after one down; Windows window class
//! mutation/reuse itself is not claimed or attempted.
use super::*;
use std::time::{Duration, Instant};
use windows::{Win32::UI::WindowsAndMessaging::*, core::w};

struct OwnedWindow(HWND);
impl OwnedWindow {
    fn new() -> Self {
        // SAFETY: Static class/title pointers are immutable and process-lived;
        // HWND_MESSAGE creates an invisible window on this test thread. No
        // external HWND, borrowed callback or user desktop target is involved.
        Self(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("LycheeDev identity fault fixture"),
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
            .unwrap()
        })
    }
    fn target(&self) -> MailboxWindow {
        let process = Process::open(std::process::id()).unwrap();
        let executable = process
            .image_path()
            .unwrap()
            .into_os_string()
            .into_string()
            .unwrap();
        let mut class = [0u16; 256];
        // SAFETY: This thread owns the live HWND; Windows writes at most the
        // length of the initialized UTF-16 slice and retains no pointer.
        let length = unsafe { GetClassNameW(self.0, &mut class) };
        assert!(length > 0);
        MailboxWindow::bind(
            WindowIdentity {
                process: process.identity().clone(),
                hwnd: self.0.0 as usize,
            },
            &executable,
            &String::from_utf16(&class[..length as usize]).unwrap(),
        )
        .unwrap()
    }
    fn packets(&self) -> Vec<(u32, usize)> {
        let mut messages = Vec::new();
        loop {
            let mut message = MSG::default();
            // SAFETY: MSG is writable initialized storage; the filter is only
            // the live HWND owned by this thread. No messages are dispatched.
            if !unsafe {
                PeekMessageW(
                    &mut message,
                    Some(self.0),
                    WM_KEYFIRST,
                    WM_KEYLAST,
                    PM_REMOVE,
                )
                .as_bool()
            } {
                return messages;
            }
            messages.push((message.message, message.wParam.0));
        }
    }
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        // SAFETY: Created and exclusively held on this same OS thread, with
        // no outstanding callback or pointer. Only this owned HWND is destroyed.
        let result = unsafe { DestroyWindow(self.0) };
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
struct Control(Instant);
impl MailboxControl for Control {
    type Error = String;
    fn deadline(&self) -> Instant {
        self.0
    }
    fn ordered_child(&self, deadline: Instant) -> Result<Self, String> {
        Ok(Self(deadline.min(self.0)))
    }
    fn check(&self) -> Result<(), String> {
        if Instant::now() >= self.0 {
            Err("fixture.deadline".into())
        } else {
            Ok(())
        }
    }
}
struct ChangeExpectedAfterDown<'a> {
    inner: WindowsBackend<'a>,
    change_class: bool,
    posts: usize,
}
impl Backend for ChangeExpectedAfterDown<'_> {
    fn now(&self) -> Instant {
        self.inner.now()
    }
    fn sleep(&mut self, duration: Duration) {
        self.inner.sleep(duration);
    }
    fn verify(&mut self) -> Result<(), String> {
        self.inner.verify()
    }
    fn minimized(&mut self) -> bool {
        self.inner.minimized()
    }
    fn keyboard_ready(&mut self) -> Result<(), String> {
        // This fixture isolates identity rejection on its own HWND. Readiness
        // faults are covered separately by the engine and passive monitor tests.
        Ok(())
    }
    fn scan(&mut self, vk: u16) -> u32 {
        self.inner.scan(vk)
    }
    fn acquire(&mut self) -> Result<(), String> {
        self.inner.acquire()
    }
    fn finish_lock(&mut self) -> Vec<String> {
        self.inner.finish_lock()
    }
    fn post(&mut self, message: u32, vk: u16, bits: u32) -> Result<(), String> {
        self.inner.post(message, vk, bits)?;
        self.posts += 1;
        if self.posts == 1 {
            // Fault injection changes the expected identity, not any OS/global
            // state. All following verifies use the actual production checker.
            if self.change_class {
                self.inner.target.class.push(b'X'.into());
            } else {
                self.inner.target.executable.push(b'X'.into());
            }
        }
        Ok(())
    }
}
#[test]
fn class_and_executable_mismatch_block_normal_and_emergency_posts_on_owned_hwnd() {
    for change_class in [true, false] {
        let window = OwnedWindow::new();
        let target = window.target();
        let control = Control(Instant::now() + Duration::from_secs(5));
        let mut keyboard = KeyboardActivity::new(control.deadline()).unwrap();
        let mut backend = ChangeExpectedAfterDown {
            inner: WindowsBackend {
                target,
                keyboard: &keyboard,
                lock: None,
                _thread: PhantomData,
            },
            change_class,
            posts: 0,
        };
        let bindings = MailboxBindings::new("ALT-CTRL-F12", "ALT-CTRL-F11").unwrap();
        let failure = engine::send(
            &mut backend,
            &control,
            bindings,
            MailboxAction::Invoke,
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(
            matches!(failure.cause, Some(InputError::Platform(ref value)) if value == "desktop.identity_changed")
        );
        assert_eq!(failure.cleanup_errors, ["desktop.identity_changed"]);
        assert_eq!(failure.receipt.messages_queued, 1);
        assert!(!failure.receipt.submission_complete && !failure.receipt.release_complete);
        assert_eq!(backend.posts, 1);
        assert!(backend.inner.lock.is_none());
        assert_eq!(window.packets(), [(WM_KEYDOWN, 0x11)]);
        drop(backend);
        keyboard.close().unwrap();
    }
}

#[test]
fn short_process_close_failure_blocks_owned_window_identity() {
    use crate::process::checked_query_tests::CloseObservation;
    let window = OwnedWindow::new();
    let target = window.target();
    let observer = CloseObservation::install(|result| {
        result?;
        Err("fixture.process_close".into())
    });
    let result = target.identity.verify();
    drop(observer);
    assert!(result.unwrap_err().contains("fixture.process_close"));
    assert!(window.packets().is_empty());
}
#[test]
fn short_process_close_failure_cannot_seal_a_mailbox_window() {
    use crate::process::checked_query_tests::CloseObservation;
    let window = OwnedWindow::new();
    let target = window.target();
    let observer = CloseObservation::install(|result| {
        result?;
        Err("fixture.process_close".into())
    });
    let result = MailboxWindow::bind(
        target.identity.clone(),
        &String::from_utf16(&target.executable).unwrap(),
        &String::from_utf16(&target.class).unwrap(),
    );
    drop(observer);
    assert!(result.unwrap_err().contains("fixture.process_close"));
    assert!(window.packets().is_empty());
}
#[test]
fn mailbox_query_close_failure_preserves_the_primary_identity_rejection() {
    use crate::process::checked_query_tests::CloseObservation;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let window = OwnedWindow::new();
    let mut target = window.target();
    target.class.push(u16::from(b'X'));
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let observer = CloseObservation::install(move |result| {
        result?;
        // Two nested window-identity proofs finish before the outer image/class
        // query owner. Fault that retained owner, after its real OS close.
        if observed.fetch_add(1, Ordering::SeqCst) >= 2 {
            Err("fixture.outer_process_close".into())
        } else {
            Ok(())
        }
    });
    let result = target.verify();
    drop(observer);
    assert!(calls.load(Ordering::SeqCst) >= 3);
    let error = result.unwrap_err();
    assert!(error.contains("desktop.identity_changed"));
    assert!(error.contains("fixture.outer_process_close"));
    assert!(window.packets().is_empty());
}
