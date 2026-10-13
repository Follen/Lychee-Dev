//! PostMessage is exercised only on an invisible message-only window owned by this test process.
use lycheedev_windows::{Process, WindowIdentity, input::chord};
use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::w,
};
struct Window(HWND);
impl Window {
    fn new() -> Self {
        Self(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("LycheeDev input fixture"),
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
    fn identity(&self) -> WindowIdentity {
        WindowIdentity {
            process: Process::open(std::process::id())
                .unwrap()
                .identity()
                .clone(),
            hwnd: self.0.0 as usize,
        }
    }
    fn queued(&self) -> Vec<(u32, usize, isize)> {
        let mut out = Vec::new();
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, Some(self.0), WM_KEYFIRST, WM_KEYLAST, PM_REMOVE).as_bool()
            {
                out.push((msg.message, msg.wParam.0, msg.lParam.0));
            }
        }
        out
    }
}
impl Drop for Window {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}
#[test]
fn directed_chord_releases_owned_modifiers() {
    let window = Window::new();
    let receipt = chord(&window.identity(), &[0x11, 0x12, 0x7b], || Ok(())).unwrap();
    assert_eq!(receipt.messages_queued, 6);
    assert!(receipt.submission_complete && receipt.release_complete);
    let events = window.queued();
    assert_eq!(events.len(), 6);
    assert_eq!(events[0].1, 0x11);
    assert_eq!(events[5].1, 0x11);
    assert_eq!(events[5].0, WM_KEYUP);
    assert_eq!(events[5].2 as u32 & (1 << 29), 0);
}
#[test]
fn failure_after_first_down_preserves_partial_and_releases() {
    let window = Window::new();
    let mut guards = 0;
    let failure = chord(&window.identity(), &[0x11, 0x12, 0x7b], || {
        guards += 1;
        if guards == 5 {
            Err("fixture.guard_changed".into())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(failure.receipt.messages_queued, 2);
    assert!(!failure.receipt.submission_complete);
    assert!(failure.receipt.release_complete);
    let events = window.queued();
    assert_eq!(events.len(), 2);
    assert_eq!((events[0].0, events[1].0), (WM_KEYDOWN, WM_KEYUP));
}
#[test]
fn zero_send_is_distinct_from_uncertain_delivery() {
    let window = Window::new();
    let failure = chord(&window.identity(), &[0x11, 0x12, 0x7b], || {
        Err("fixture.blocked".into())
    })
    .unwrap_err();
    assert_eq!(failure.receipt.messages_queued, 0);
    assert!(window.queued().is_empty());
}
#[test]
fn fixed_reload_has_only_literal_bounded_steps_and_no_retry() {
    let window = Window::new();
    let mut waits = 0;
    let receipt = lycheedev_windows::input::fixed_reload(
        &window.identity(),
        |_| Ok(()),
        |duration| {
            assert!([50, 150].contains(&duration.as_millis()));
            waits += 1;
            Ok(())
        },
    )
    .unwrap();
    assert!(receipt.submission_complete && receipt.release_complete);
    assert_eq!(receipt.messages_queued, 17);
    assert_eq!(waits, 11);
    let events = window.queued();
    assert_eq!(events.len(), 17);
    assert_eq!(
        events
            .iter()
            .filter(|e| e.0 == WM_CHAR)
            .map(|e| e.1 as u8)
            .collect::<Vec<_>>(),
        b"/reload"
    );
    let mut failed = false;
    let failure = lycheedev_windows::input::fixed_reload(
        &window.identity(),
        |step| {
            if step == 5 && !failed {
                failed = true;
                Err("fixture.stop_before_text".into())
            } else {
                Ok(())
            }
        },
        |_| Ok(()),
    )
    .unwrap_err();
    assert_eq!(failure.receipt.messages_queued, 8);
    assert!(failure.receipt.release_complete);
    assert!(!failure.receipt.submission_complete);
    assert!(window.queued().iter().all(|e| e.0 != WM_CHAR));
}
