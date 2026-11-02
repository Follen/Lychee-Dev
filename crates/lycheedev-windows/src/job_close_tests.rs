//! Exercise a real configured Job but fail before assigning this test process.
//! The actual handle is consumed once before a cleanup result is injected.
use super::*;
use std::cell::Cell;
thread_local! {
    static FAIL: Cell<bool> = const { Cell::new(false) };
    static CLOSED: Cell<usize> = const { Cell::new(0) };
}
pub(super) fn before_assignment() -> crate::Result<()> {
    if FAIL.get() {
        Err("fixture.job_startup".into())
    } else {
        Ok(())
    }
}
pub(super) fn observe_close(result: crate::Result<()>) -> crate::Result<()> {
    result?;
    if FAIL.get() {
        CLOSED.set(CLOSED.get() + 1);
        Err("fixture.broker_job_close".into())
    } else {
        Ok(())
    }
}
struct Observation;
impl Observation {
    fn install() -> Self {
        assert!(!FAIL.replace(true));
        CLOSED.set(0);
        Self
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        FAIL.set(false);
    }
}

#[test]
fn startup_error_preserves_configured_job_checked_close_failure() {
    let observation = Observation::install();
    let result = BrokerJob::own_current_process(2 << 30);
    drop(observation);
    assert_eq!(
        CLOSED.get(),
        1,
        "the real unassigned Job was closed exactly once"
    );
    let error = result
        .err()
        .expect("fixture rejects before attaching current process");
    assert!(error.contains("fixture.job_startup"));
    assert!(error.contains("fixture.broker_job_close"), "{error}");
}
