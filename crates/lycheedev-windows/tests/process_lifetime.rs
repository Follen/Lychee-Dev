//! Only this test executable and its own stdin-controlled child are observed.
//! No game, foreign process, window, desktop input or network is required.
#![cfg(windows)]
use lycheedev_windows::{
    Process, ProcessIdentity,
    mailbox_input::equal_fold_utf16,
    process::{ProcessLifetime, observe_lifetime},
};
use std::{
    io::Read,
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

#[test]
#[ignore = "owned stdin-controlled child; invoked only by its parent fixture"]
fn lifetime_child_fixture() {
    assert_eq!(
        std::env::var("LYCHEEDEV_LIFETIME_CHILD").as_deref(),
        Ok("1")
    );
    let mut byte = [0; 1];
    // The parent closes its pipe to end this child. A parent crash also closes
    // the pipe; this fixture does not depend on a fixed sleep or outside signal.
    assert_eq!(std::io::stdin().read(&mut byte).unwrap(), 0);
}
struct OwnedChild(Child);
impl OwnedChild {
    fn new() -> Self {
        Self(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "lifetime_child_fixture",
                    "--ignored",
                    "--nocapture",
                ])
                .env("LYCHEEDEV_LIFETIME_CHILD", "1")
                .creation_flags(0x08000000)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    fn identity(&self) -> ProcessIdentity {
        Process::open(self.0.id()).unwrap().identity().clone()
    }
    fn stop(&mut self) {
        drop(self.0.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "owned child failed: {status}");
                return;
            }
            assert!(
                Instant::now() < deadline,
                "owned child did not exit after stdin closed"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        drop(self.0.stdin.take());
        // Panic fallback targets only the Child created above, never a PID
        // supplied by a caller. Reap before releasing our owned child handle.
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
#[test]
fn own_process_is_running_without_any_window_and_reuse_is_not_continuity() {
    let identity = Process::open(std::process::id())
        .unwrap()
        .identity()
        .clone();
    assert_eq!(
        observe_lifetime(&identity, || Ok(())).unwrap(),
        ProcessLifetime::Running
    );
    let wrong = ProcessIdentity {
        created: identity.created + 1,
        ..identity
    };
    assert_eq!(
        observe_lifetime(&wrong, || Ok(())).unwrap(),
        ProcessLifetime::PidReused
    );
}
#[test]
fn owned_child_natural_exit_is_observed_with_its_exact_lifetime_still_held() {
    let mut child = OwnedChild::new();
    let identity = child.identity();
    assert_eq!(
        observe_lifetime(&identity, || Ok(())).unwrap(),
        ProcessLifetime::Running
    );
    child.stop();
    // Child still owns its process handle, preserving the exited object/PID so
    // this assertion cannot pass merely through an absent PID or PID reuse.
    for _ in 0..3 {
        assert_eq!(
            observe_lifetime(&identity, || Ok(())).unwrap(),
            ProcessLifetime::Exited
        );
    }
}
#[test]
fn pre_cancelled_observation_does_not_promote_owned_child_exit() {
    let mut child = OwnedChild::new();
    let identity = child.identity();
    child.stop();
    assert_eq!(
        observe_lifetime(&identity, || Err("original.cancelled".into())).unwrap_err(),
        "original.cancelled"
    );
}
#[test]
fn public_utf16_equality_keeps_the_pinned_go_relation() {
    let folded = |left: &str, right: &str| {
        equal_fold_utf16(
            &left.encode_utf16().collect::<Vec<_>>(),
            &right.encode_utf16().collect::<Vec<_>>(),
        )
    };
    assert!(folded("C:\\KſΣẞ.exe", "c:\\Ksςß.EXE"));
    assert!(!folded("C:\\ß.exe", "c:\\SS.exe"));
    assert!(!folded("İ", "i"));
    assert!(!folded("é", "e\u{301}"));
    assert!(equal_fold_utf16(&[0xd800], &[0xfffd]));
}
