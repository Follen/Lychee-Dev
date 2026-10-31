//! The injected result follows a real, single CloseHandle. The child is always
//! reaped through an independently held process handle before assertions.
use super::*;
use std::{
    cell::{Cell, RefCell},
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
};

type Observer = Box<dyn FnMut(&'static str, Result<()>) -> Result<()>>;
thread_local! {
    static CLOSE: RefCell<Option<Observer>> = RefCell::new(None);
    static PID: Cell<u32> = const { Cell::new(0) };
    static SPAWN: RefCell<Option<OwnedHandle>> = const { RefCell::new(None) };
}
pub(super) fn record_spawn(pid: u32) {
    PID.set(pid);
    // Hold independent retirement authority before the tested launch can fail.
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    }
    .unwrap();
    SPAWN.with_borrow_mut(|slot| {
        assert!(slot.is_none());
        *slot = Some(unsafe { OwnedHandle::from_raw_handle(handle.0) });
    });
}
pub(super) fn observe_close(kind: &'static str, result: Result<()>) -> Result<()> {
    CLOSE.with_borrow_mut(|observer| match observer {
        Some(observer) => observer(kind, result),
        None => result,
    })
}
struct Observation;
impl Observation {
    fn install(kind: &'static str) -> Self {
        CLOSE.with_borrow_mut(|observer| {
            assert!(observer.is_none());
            *observer = Some(Box::new(move |actual, result| {
                result?;
                if actual == kind {
                    Err(format!("fixture.{kind}_close"))
                } else {
                    Ok(())
                }
            }));
        });
        Self
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        CLOSE.with_borrow_mut(|observer| *observer = None);
    }
}

struct Reap(Option<OwnedHandle>);
impl Reap {
    fn new() -> Self {
        Self(Some(
            SPAWN
                .with_borrow_mut(|slot| slot.take())
                .expect("original child handle"),
        ))
    }
    fn running(&self) -> bool {
        unsafe {
            WaitForSingleObject(HANDLE(self.0.as_ref().unwrap().as_raw_handle()), 0) == WAIT_TIMEOUT
        }
    }
    fn finish(&mut self) {
        let Some(handle) = self.0.take() else { return };
        let raw = HANDLE(handle.as_raw_handle());
        if unsafe { WaitForSingleObject(raw, 0) } == WAIT_TIMEOUT {
            unsafe { TerminateProcess(raw, 7) }.unwrap();
        }
        assert_eq!(unsafe { WaitForSingleObject(raw, 5000) }, WAIT_OBJECT_0);
        crate::guarded_spawn::close_owned_handle(handle).unwrap();
    }
}
impl Drop for Reap {
    fn drop(&mut self) {
        self.finish();
    }
}

#[test]
#[ignore = "child fixture only; invoked by detached checked-close parent regressions"]
fn owned_child_fixture() {
    assert_eq!(
        std::env::var("LYCHEEDEV_DETACHED_CLOSE_FIXTURE").expect("required child mode"),
        "wait"
    );
    // Parent owns the exact process and terminates it; this does not touch desktop input.
    loop {
        std::thread::park();
    }
}
fn launch() -> Result<DetachedChild> {
    let exe = std::env::current_exe().unwrap();
    spawn_hidden_no_inherit(
        &exe,
        &[
            "--exact".into(),
            "detached::checked_close_tests::owned_child_fixture".into(),
            "--ignored".into(),
        ],
        exe.parent().unwrap(),
        &[("LYCHEEDEV_DETACHED_CLOSE_FIXTURE".into(), "wait".into())],
    )
}

#[test]
fn startup_thread_close_failure_cannot_return_a_live_child() {
    let observation = Observation::install("detached_thread");
    let result = launch();
    drop(observation);
    let pid = PID.get();
    assert_ne!(pid, 0);
    // If a regression returns a live child, clean it before asserting so the
    // test itself never relies on failure-time Drop.
    let mut reap = Reap::new();
    let error = match result {
        Ok(child) => {
            child
                .terminate_and_close(Instant::now() + Duration::from_secs(2))
                .unwrap();
            None
        }
        Err(error) => Some(error),
    };
    let retired = !reap.running();
    reap.finish();
    assert!(
        error
            .as_deref()
            .is_some_and(|error| error.contains("fixture.detached_thread_close")),
        "{error:?}"
    );
    assert!(retired, "startup failure must reap its original child");
}

#[test]
fn authenticated_handoff_preserves_live_daemon_but_reports_process_close_failure() {
    let child = launch().unwrap();
    let mut reap = Reap::new();
    let observation = Observation::install("detached_process");
    let result = child.handoff();
    drop(observation);
    let running = reap.running();
    reap.finish();
    assert!(
        running,
        "authenticated handoff must not terminate the daemon even if the handle close fails"
    );
    assert!(
        result
            .err()
            .as_deref()
            .is_some_and(|error| error.contains("fixture.detached_process_close"))
    );
}

#[test]
fn terminated_child_checked_close_failure_still_reaps_exact_original_process() {
    let child = launch().unwrap();
    let mut reap = Reap::new();
    assert!(reap.running());
    let observation = Observation::install("detached_process");
    let result = child.terminate_and_close(Instant::now() + Duration::from_secs(2));
    drop(observation);
    let retired = !reap.running();
    reap.finish();
    assert!(retired);
    assert!(
        result
            .err()
            .as_deref()
            .is_some_and(|error| error.contains("fixture.detached_process_close"))
    );
}
