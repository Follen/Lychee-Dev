#![forbid(unsafe_code)]
use super::*;
use std::cell::Cell;
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_INVALID_HANDLE, ERROR_NOT_ENOUGH_MEMORY, WAIT_ABANDONED, WAIT_EVENT,
};

struct Fake {
    exists: Result<bool>,
    creation: Result<u64>,
    state: Result<WaitState>,
    closed: Result<()>,
    calls: Vec<&'static str>,
}
impl Default for Fake {
    fn default() -> Self {
        Self {
            exists: Ok(true),
            creation: Ok(123),
            state: Ok(WaitState::Running),
            closed: Ok(()),
            calls: Vec::new(),
        }
    }
}
impl Backend for Fake {
    type Owner = ();
    fn open(&mut self, pid: u32) -> Result<Option<()>> {
        assert_eq!(pid, 17);
        self.calls.push("open");
        self.exists.clone().map(|exists| exists.then_some(()))
    }
    fn created(&mut self, _: &()) -> Result<u64> {
        self.calls.push("times");
        self.creation.clone()
    }
    fn wait(&mut self, _: &()) -> Result<WaitState> {
        self.calls.push("wait");
        self.state.clone()
    }
    fn close(&mut self, _: ()) -> Result<()> {
        self.calls.push("close");
        self.closed.clone()
    }
}
fn expected() -> ProcessIdentity {
    ProcessIdentity {
        pid: 17,
        created: 123,
    }
}
fn observe(fake: &mut Fake) -> Result<ProcessLifetime> {
    observe_with(&expected(), &mut || Ok(()), fake)
}
fn native_error(code: u32) -> Error {
    Error::from_hresult(HRESULT::from_win32(code))
}
#[test]
fn running_and_signaled_lifetimes_have_exact_bounded_os_order() {
    for (state, expected) in [
        (WaitState::Running, ProcessLifetime::Running),
        (WaitState::Exited, ProcessLifetime::Exited),
    ] {
        let mut fake = Fake {
            state: Ok(state),
            ..Fake::default()
        };
        assert_eq!(observe(&mut fake).unwrap(), expected);
        assert_eq!(fake.calls, ["open", "times", "wait", "close"]);
    }
}
#[test]
fn only_open_invalid_parameter_is_absence_all_other_errors_keep_original_code() {
    assert_eq!(
        opened::<()>(Err(native_error(ERROR_INVALID_PARAMETER.0))).unwrap(),
        None
    );
    for code in [
        ERROR_ACCESS_DENIED.0,
        ERROR_INVALID_HANDLE.0,
        ERROR_NOT_ENOUGH_MEMORY.0,
        0,
    ] {
        let error = native_error(code);
        let detail = error.to_string();
        assert_eq!(
            opened::<()>(Err(error)).unwrap_err(),
            format!("process.lifetime_open: {detail}")
        );
    }
    assert_eq!(opened(Ok(42)).unwrap(), Some(42));
}
#[test]
fn absent_pid_never_reads_waits_or_closes_a_nonexistent_handle() {
    let mut fake = Fake {
        exists: Ok(false),
        ..Fake::default()
    };
    assert_eq!(observe(&mut fake).unwrap(), ProcessLifetime::Absent);
    assert_eq!(fake.calls, ["open"]);
}
#[test]
fn reused_pid_uses_original_creation_and_never_waits_on_replacement() {
    let mut fake = Fake {
        creation: Ok(124),
        state: Err("must not wait".into()),
        ..Fake::default()
    };
    assert_eq!(observe(&mut fake).unwrap(), ProcessLifetime::PidReused);
    assert_eq!(fake.calls, ["open", "times", "close"]);
}
#[test]
fn missing_identity_is_not_process_end_and_cancellation_precedes_validation() {
    for expected in [
        ProcessIdentity {
            pid: 0,
            created: 123,
        },
        ProcessIdentity {
            pid: 17,
            created: 0,
        },
    ] {
        let mut fake = Fake::default();
        assert_eq!(
            observe_with(&expected, &mut || Ok(()), &mut fake).unwrap_err(),
            "desktop.identity_changed"
        );
        assert_eq!(
            observe_with(
                &expected,
                &mut || Err("original.cancelled".into()),
                &mut fake
            )
            .unwrap_err(),
            "original.cancelled"
        );
        assert!(fake.calls.is_empty());
    }
}
#[test]
fn pre_cancelled_request_never_opens_a_process() {
    let mut fake = Fake::default();
    assert_eq!(
        observe_with(
            &expected(),
            &mut || Err("ancestor.cancelled".into()),
            &mut fake
        )
        .unwrap_err(),
        "ancestor.cancelled"
    );
    assert!(fake.calls.is_empty());
}
#[test]
fn open_failure_and_post_open_control_error_both_survive_without_close() {
    let mut fake = Fake {
        exists: Err("native access denied 5".into()),
        ..Fake::default()
    };
    let mut checks = 0;
    let error = observe_with(
        &expected(),
        &mut || {
            checks += 1;
            if checks == 2 {
                Err("original.deadline".into())
            } else {
                Ok(())
            }
        },
        &mut fake,
    )
    .unwrap_err();
    assert_eq!(error, "native access denied 5\noriginal.deadline");
    assert_eq!(fake.calls, ["open"]);
}
#[test]
fn absence_cannot_be_issued_after_cancellation_during_open() {
    let mut fake = Fake {
        exists: Ok(false),
        ..Fake::default()
    };
    let mut checks = 0;
    assert_eq!(
        observe_with(
            &expected(),
            &mut || {
                checks += 1;
                if checks == 2 {
                    Err("original.cancelled".into())
                } else {
                    Ok(())
                }
            },
            &mut fake
        )
        .unwrap_err(),
        "original.cancelled"
    );
    assert_eq!(fake.calls, ["open"]);
}
#[test]
fn every_post_open_cancel_point_closes_once_and_never_returns_exit() {
    for cancelled_at in 2..=6 {
        let mut fake = Fake {
            state: Ok(WaitState::Exited),
            ..Fake::default()
        };
        let mut checks = 0;
        let error = observe_with(
            &expected(),
            &mut || {
                checks += 1;
                if checks >= cancelled_at {
                    Err("original.cancelled".into())
                } else {
                    Ok(())
                }
            },
            &mut fake,
        )
        .unwrap_err();
        assert_eq!(error, "original.cancelled", "check {cancelled_at}");
        assert_eq!(
            fake.calls.iter().filter(|&&call| call == "close").count(),
            1
        );
        if cancelled_at == 2 {
            assert_eq!(fake.calls, ["open", "close"]);
        }
        if cancelled_at == 3 {
            assert_eq!(fake.calls, ["open", "times", "close"]);
        }
        assert_eq!(fake.calls.last(), Some(&"close"));
    }
}
#[test]
fn creation_mismatch_is_not_issued_after_post_query_cancellation() {
    let mut fake = Fake {
        creation: Ok(124),
        ..Fake::default()
    };
    let mut checks = 0;
    assert_eq!(
        observe_with(
            &expected(),
            &mut || {
                checks += 1;
                if checks == 3 {
                    Err("original.deadline".into())
                } else {
                    Ok(())
                }
            },
            &mut fake
        )
        .unwrap_err(),
        "original.deadline"
    );
    assert_eq!(fake.calls, ["open", "times", "close"]);
}
#[test]
fn times_error_including_invalid_parameter_never_becomes_absence() {
    let detail = os_error("times", native_error(ERROR_INVALID_PARAMETER.0));
    let mut fake = Fake {
        creation: Err(detail.clone()),
        ..Fake::default()
    };
    assert_eq!(observe(&mut fake).unwrap_err(), detail);
    assert_eq!(fake.calls, ["open", "times", "close"]);
}
#[test]
fn failed_wait_preserves_os_error_and_unexpected_states_are_not_exit() {
    assert_eq!(
        waited(WAIT_OBJECT_0, || panic!("no last error read")).unwrap(),
        WaitState::Exited
    );
    assert_eq!(
        waited(WAIT_TIMEOUT, || panic!("no last error read")).unwrap(),
        WaitState::Running
    );
    for state in [WAIT_ABANDONED, WAIT_EVENT(7)] {
        assert_eq!(
            waited(state, || panic!("no last error read")).unwrap_err(),
            "desktop.identity_changed"
        );
    }
    let detail = os_error("wait", native_error(ERROR_INVALID_HANDLE.0));
    let mut reads = 0;
    let error = waited(WAIT_FAILED, || {
        reads += 1;
        native_error(ERROR_INVALID_HANDLE.0)
    })
    .unwrap_err();
    assert_eq!(reads, 1);
    assert_eq!(error, detail);
    let mut fake = Fake {
        state: Err(error),
        ..Fake::default()
    };
    assert_eq!(observe(&mut fake).unwrap_err(), detail);
    assert_eq!(fake.calls, ["open", "times", "wait", "close"]);
}
#[test]
fn original_os_failure_close_and_control_causes_survive_in_order_once() {
    let mut fake = Fake {
        creation: Err("original times error".into()),
        closed: Err("close error".into()),
        ..Fake::default()
    };
    let mut checks = 0;
    let error = observe_with(
        &expected(),
        &mut || {
            checks += 1;
            match checks {
                3 | 4 => Err("ancestor.cancelled".into()),
                5 => Err("deadline.expired".into()),
                _ => Ok(()),
            }
        },
        &mut fake,
    )
    .unwrap_err();
    assert_eq!(
        error,
        "original times error\nancestor.cancelled\nclose error\ndeadline.expired"
    );
    assert_eq!(fake.calls, ["open", "times", "close"]);
}
#[test]
fn close_failure_blocks_every_successful_opened_lifetime() {
    for (created, state) in [
        (123, WaitState::Running),
        (123, WaitState::Exited),
        (124, WaitState::Running),
    ] {
        let mut fake = Fake {
            creation: Ok(created),
            state: Ok(state),
            closed: Err("checked close failed".into()),
            ..Fake::default()
        };
        assert_eq!(observe(&mut fake).unwrap_err(), "checked close failed");
        assert_eq!(
            fake.calls.iter().filter(|&&call| call == "close").count(),
            1
        );
    }
}
#[test]
fn checked_close_disarms_drop_before_success_or_failure_without_retry() {
    struct Owner<'a>(&'a Cell<usize>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    for fails in [false, true] {
        let drops = Cell::new(0);
        let owner = Owner(&drops);
        let mut calls = 0;
        let result = consume_close(
            owner,
            |_| HANDLE::default(),
            |_| {
                calls += 1;
                assert_eq!(drops.get(), 0);
                if fails {
                    Err("close failed".into())
                } else {
                    Ok(())
                }
            },
        );
        assert_eq!(result.is_err(), fails);
        assert_eq!(calls, 1);
        assert_eq!(drops.get(), 0);
    }
}
