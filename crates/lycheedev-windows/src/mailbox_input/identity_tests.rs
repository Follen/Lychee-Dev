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
struct ChangeExpectedAfterDown {
    inner: WindowsBackend,
    change_class: bool,
    posts: usize,
}
impl Backend for ChangeExpectedAfterDown {
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
    fn pressed(&mut self, vk: u16) -> bool {
        self.inner.pressed(vk)
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
        let mut backend = ChangeExpectedAfterDown {
            inner: WindowsBackend {
                target,
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
    }
}
