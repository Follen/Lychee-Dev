//! Fault results follow the real one-shot CloseHandle on this test's query
//! owners. No invalid handle or second close is manufactured.
use super::*;
use std::cell::RefCell;
type Observer = Box<dyn FnMut(Result<()>) -> Result<()>>;
thread_local! { static CLOSE: RefCell<Option<Observer>> = RefCell::new(None); }
pub(crate) fn observe_close(result: Result<()>) -> Result<()> {
    CLOSE.with_borrow_mut(|observer| match observer {
        Some(observer) => observer(result),
        None => result,
    })
}
pub(crate) struct CloseObservation;
impl CloseObservation {
    pub(crate) fn install(observer: impl FnMut(Result<()>) -> Result<()> + 'static) -> Self {
        CLOSE.with_borrow_mut(|current| {
            assert!(current.is_none());
            *current = Some(Box::new(observer));
        });
        Self
    }
}
impl Drop for CloseObservation {
    fn drop(&mut self) {
        CLOSE.with_borrow_mut(|observer| *observer = None);
    }
}
#[test]
fn parent_snapshot_cannot_succeed_after_its_checked_close_failure() {
    let own = Process::open(std::process::id()).unwrap();
    let observer = CloseObservation::install(|result| {
        result?;
        Err("fixture.snapshot_close".into())
    });
    let result = own.parent_pid();
    drop(observer);
    assert!(result.unwrap_err().contains("fixture.snapshot_close"));
}
