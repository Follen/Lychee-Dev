//! Owned metadata-only windows; no game, image bytes, process memory or input.
use lycheedev_core::control::OperationControl;
use lycheedev_live::passive::{Source, WindowsSource};
use windows::{
    Win32::{Foundation::HWND, UI::WindowsAndMessaging::*},
    core::w,
};

struct OwnedWindow(HWND);
impl OwnedWindow {
    fn new(visible: bool) -> Self {
        let style = WS_POPUP
            | if visible {
                WS_VISIBLE
            } else {
                WINDOW_STYLE::default()
            };
        Self(
            unsafe {
                CreateWindowExW(
                    WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                    w!("STATIC"),
                    w!("Lychee passive owned window"),
                    style,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap(),
        )
    }
}
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        unsafe {
            DestroyWindow(self.0).unwrap();
        }
    }
}

#[test]
fn actual_windows_source_keeps_visible_owned_windows_and_excludes_hidden_window() {
    let first = OwnedWindow::new(true);
    let second = OwnedWindow::new(true);
    let hidden = OwnedWindow::new(false);
    let windows = WindowsSource.windows(&OperationControl::default()).unwrap();
    let a = windows
        .iter()
        .find(|window| window.identity.hwnd == first.0.0 as usize)
        .unwrap();
    let b = windows
        .iter()
        .find(|window| window.identity.hwnd == second.0.0 as usize)
        .unwrap();
    assert!(
        !windows
            .iter()
            .any(|window| window.identity.hwnd == hidden.0.0 as usize)
    );
    assert_eq!(a.identity.process.pid, std::process::id());
    assert_eq!(a.identity.process, b.identity.process);
    assert_ne!(a.identity.hwnd, b.identity.hwnd);
    assert_ne!(a.identity.process.created, 0);
    assert_eq!(a.title, "Lychee passive owned window");
    assert_eq!(a.class.to_ascii_lowercase(), "static");
    assert_eq!(
        std::fs::canonicalize(&a.executable).unwrap(),
        std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap()
    );
}

#[test]
fn native_enumeration_retains_the_control_cause() {
    let control = OperationControl::default();
    control
        .cancel_handle()
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(
        WindowsSource.windows(&control).unwrap_err().code,
        "command.cancelled"
    );
    let mut calls = 0;
    let result = lycheedev_windows::inventory::passive_windows(|| {
        calls += 1;
        if calls >= 2 {
            Err("fixture.cancelled_during_enumeration".into())
        } else {
            Ok(())
        }
    });
    assert_eq!(result.unwrap_err(), "fixture.cancelled_during_enumeration");
}
