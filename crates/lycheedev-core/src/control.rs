//! One caller-owned deadline and cooperative cancellation token for the whole operation.
use crate::{Result, ToolkitError};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
/// Ordered controls compare cancellation's real monotonic linearization time
/// with their immutable deadline. Legacy AtomicBool writers have no timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopCause {
    Cancelled,
    DeadlineExceeded,
}
#[derive(Debug)]
struct CancellationState {
    token: Arc<AtomicBool>,
    at: Mutex<Option<Instant>>,
    // Access is serialized with `at`; the atomic also supports legacy readers.
    raw_exposed: AtomicBool,
}
/// Cancellation with an immutable first timestamp. Unlike a raw AtomicBool,
/// this can distinguish cancellation before a deadline from a later signal.
/// There are no observers, timers or threads to outlive a request.
#[derive(Debug, Clone)]
pub struct TrackedCancellation {
    state: Arc<CancellationState>,
}
impl Default for TrackedCancellation {
    fn default() -> Self {
        Self::new()
    }
}
impl TrackedCancellation {
    pub fn new() -> Self {
        Self {
            state: Arc::new(CancellationState {
                token: Arc::new(AtomicBool::new(false)),
                at: Mutex::new(None),
                raw_exposed: AtomicBool::new(false),
            }),
        }
    }
    pub fn cancel(&self) {
        // The timestamp is the cancellation's linearization point, recorded
        // under the same lock used to compare it with a control's deadline.
        let mut at = self.state.at.lock().unwrap_or_else(|p| p.into_inner());
        if at.is_none() {
            *at = Some(Instant::now());
        }
        self.state.token.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.state.token.load(Ordering::Acquire)
    }
    fn expose_raw(&self) {
        let _at = self.state.at.lock().unwrap_or_else(|p| p.into_inner());
        self.state.raw_exposed.store(true, Ordering::Release);
    }
}

#[derive(Debug, Clone)]
pub struct OperationControl {
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    parent_cancellations: Vec<Arc<AtomicBool>>,
    // Root-to-leaf order prevents cycles when related controls are observed
    // concurrently. Each child appends one distinct cancellation state.
    ordered: Option<Vec<TrackedCancellation>>,
    first_stop: Arc<Mutex<Option<StopCause>>>,
    stopped: Arc<AtomicBool>,
}
impl Default for OperationControl {
    fn default() -> Self {
        Self::uncancelable(Instant::now() + Duration::from_secs(180))
    }
}
impl OperationControl {
    pub fn new(deadline: Instant, cancel: Arc<AtomicBool>) -> Self {
        Self {
            deadline,
            cancel,
            parent_cancellations: Vec::new(),
            ordered: None,
            first_stop: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }
    /// No external cancellation writer exists until a handle is explicitly
    /// requested. A tracked handle may subsequently cancel this control.
    pub fn uncancelable(deadline: Instant) -> Self {
        Self::tracked(deadline, TrackedCancellation::new())
    }
    pub fn tracked(deadline: Instant, cancellation: TrackedCancellation) -> Self {
        Self {
            deadline,
            cancel: cancellation.state.token.clone(),
            parent_cancellations: Vec::new(),
            ordered: Some(vec![cancellation]),
            first_stop: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }
    /// An ordered child retains every ancestor cancellation timestamp and the
    /// parent's deadline. No raw-token observation is promoted to a timestamp.
    pub fn ordered_child(&self, deadline: Instant) -> Result<Self> {
        self.stop_cause()?; // Also verifies cancellation provenance.
        let mut ordered = self.ordered.clone().ok_or_else(unordered)?;
        let local = TrackedCancellation::new();
        let mut parents = self.parent_cancellations.clone();
        parents.push(self.cancel.clone());
        ordered.push(local.clone());
        Ok(Self {
            deadline: deadline.min(self.deadline),
            cancel: local.state.token.clone(),
            parent_cancellations: parents,
            ordered: Some(ordered),
            first_stop: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
        })
    }
    pub fn tracked_cancel_handle(&self) -> Result<TrackedCancellation> {
        self.stop_cause()?;
        self.ordered
            .as_ref()
            .and_then(|lineage| lineage.last())
            .cloned()
            .ok_or_else(unordered)
    }
    /// Return the actual earliest stop, even when the backend was blocked until
    /// after both events. Holding all timestamp locks while reading the clock
    /// prevents a concurrent pre-deadline cancellation from being missed.
    pub fn stop_cause(&self) -> Result<Option<StopCause>> {
        // Already established history cannot be erased by a later signal or
        // even later legacy-handle exposure. Release this lock before taking
        // timestamp locks to preserve the common acquisition order below.
        if self.stopped.load(Ordering::Acquire) {
            return Ok(*self.first_stop.lock().unwrap_or_else(|p| p.into_inner()));
        }
        let lineage = self.ordered.as_ref().ok_or_else(unordered)?;
        // Ordinary live checks do not allocate or lock. A stop decision takes
        // the full timestamp lock set below; an alive observation can linearize
        // before a concurrently arriving cancellation or legacy-handle escape.
        if lineage.iter().all(|c| {
            !c.state.raw_exposed.load(Ordering::Acquire) && !c.state.token.load(Ordering::Acquire)
        }) && Instant::now() < self.deadline
        {
            return Ok(None);
        }
        let stamps: Vec<_> = lineage
            .iter()
            .map(|c| c.state.at.lock().unwrap_or_else(|p| p.into_inner()))
            .collect();
        if lineage
            .iter()
            .any(|c| c.state.raw_exposed.load(Ordering::Acquire))
        {
            return Err(unordered());
        }
        let earliest = stamps.iter().filter_map(|at| **at).min();
        let now = Instant::now();
        let mut first = self.first_stop.lock().unwrap_or_else(|p| p.into_inner());
        if first.is_none() {
            *first = if earliest.is_some_and(|at| at < self.deadline) {
                Some(StopCause::Cancelled)
            } else if now >= self.deadline {
                Some(StopCause::DeadlineExceeded)
            } else {
                None
            };
        }
        if first.is_some() {
            self.stopped.store(true, Ordering::Release);
        }
        Ok(*first)
    }
    /// A request can retire independently while retaining the caller's lifetime and cancellation.
    pub fn child(&self, deadline: Instant, cancel: Arc<AtomicBool>) -> Self {
        let mut parents = self.parent_cancellations.clone();
        parents.push(Arc::clone(&self.cancel));
        Self {
            deadline: deadline.min(self.deadline),
            cancel,
            parent_cancellations: parents,
            ordered: None,
            first_stop: Arc::new(Mutex::new(None)),
            stopped: Arc::new(AtomicBool::new(false)),
        }
    }
    fn inherit_cancellation(&self) {
        if self
            .parent_cancellations
            .iter()
            .any(|token| token.load(Ordering::Acquire))
        {
            self.cancel.store(true, Ordering::Release);
        }
    }
    pub fn check(&self) -> Result<()> {
        // Keep the ubiquitous live check allocation/lock free. Once a stop is
        // established its historical cause wins even if a legacy writer resets
        // the raw boolean afterward.
        if !self.stopped.load(Ordering::Acquire)
            && !self.cancel.load(Ordering::Acquire)
            && self
                .parent_cancellations
                .iter()
                .all(|c| !c.load(Ordering::Acquire))
            && Instant::now() < self.deadline
        {
            return Ok(());
        }
        // The legacy surface retains its cancellation-first behavior. Exact
        // consumers must call stop_cause/ordered_child and reject raw access.
        if let Ok(cause) = self.stop_cause() {
            return match cause {
                Some(StopCause::Cancelled) => Err(cancelled()),
                Some(StopCause::DeadlineExceeded) => Err(deadline_exceeded()),
                None => Ok(()),
            };
        }
        self.inherit_cancellation();
        if self.cancel.load(Ordering::Acquire) {
            return Err(cancelled());
        }
        if Instant::now() >= self.deadline {
            return Err(deadline_exceeded());
        }
        Ok(())
    }
    pub fn remaining(&self) -> Result<Duration> {
        self.check()?;
        Ok(self.deadline.saturating_duration_since(Instant::now()))
    }
    pub fn deadline(&self) -> Instant {
        self.deadline
    }
    /// Clones share one stop scope. Equal timestamps do not imply the same
    /// request: independent cleanup controls must never replace its cause.
    pub fn same_scope(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.first_stop, &other.first_stop)
    }
    /// Requested heap storage needed when retaining this control's ancestry.
    pub fn clone_allocation_bytes(&self) -> usize {
        self.parent_cancellations.len() * std::mem::size_of::<Arc<AtomicBool>>()
            + self.ordered.as_ref().map_or(0, |lineage| {
                lineage.len() * std::mem::size_of::<TrackedCancellation>()
            })
    }
    pub fn token(&self) -> &AtomicBool {
        if let Some(local) = self.ordered.as_ref().and_then(|lineage| lineage.last()) {
            local.expose_raw();
        }
        self.inherit_cancellation();
        &self.cancel
    }
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        if let Some(local) = self.ordered.as_ref().and_then(|lineage| lineage.last()) {
            local.expose_raw();
        }
        self.inherit_cancellation();
        Arc::clone(&self.cancel)
    }
}

fn unordered() -> ToolkitError {
    ToolkitError::new(
        "control.cancellation_order_unavailable",
        "raw cancellation token has no verifiable event order",
        5,
    )
}
fn cancelled() -> ToolkitError {
    ToolkitError::new("command.cancelled", "operation cancelled by the caller", 7)
}
fn deadline_exceeded() -> ToolkitError {
    ToolkitError::new("command.deadline", "operation deadline exhausted", 5)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn request_cancellation_does_not_retire_parent_and_parent_deadline_is_preserved() {
        let parent_token = Arc::new(AtomicBool::new(false));
        let parent = OperationControl::new(
            Instant::now() + Duration::from_secs(5),
            parent_token.clone(),
        );
        let first = parent.child(
            Instant::now() + Duration::from_secs(90),
            Arc::new(AtomicBool::new(false)),
        );
        assert_eq!(first.deadline(), parent.deadline());
        first.cancel_handle().store(true, Ordering::Release);
        assert_eq!(first.check().unwrap_err().exit_code, 7);
        assert!(parent.check().is_ok());
        let second = parent.child(
            Instant::now() + Duration::from_secs(1),
            Arc::new(AtomicBool::new(false)),
        );
        assert!(second.deadline() < parent.deadline());
        let nested = second.child(parent.deadline(), Arc::new(AtomicBool::new(false)));
        assert_eq!(nested.deadline(), second.deadline());
        parent_token.store(true, Ordering::Release);
        assert_eq!(nested.check().unwrap_err().code, "command.cancelled");
        assert!(nested.cancel_handle().load(Ordering::Acquire));
        let expired = OperationControl::new(Instant::now(), Arc::new(AtomicBool::new(false)));
        assert_eq!(
            expired
                .child(parent.deadline(), Arc::new(AtomicBool::new(false)))
                .check()
                .unwrap_err()
                .exit_code,
            5
        );
    }
    #[test]
    fn tracked_cancel_before_deadline_stays_cancelled_after_both_events() {
        let cancellation = TrackedCancellation::new();
        let control = OperationControl::tracked(
            Instant::now() + Duration::from_millis(200),
            cancellation.clone(),
        );
        let signal = cancellation.clone();
        let worker = std::thread::spawn(move || signal.cancel());
        worker.join().unwrap();
        let first = *cancellation.state.at.lock().unwrap();
        assert!(
            first.unwrap() < control.deadline(),
            "test must establish cancellation first"
        );
        std::thread::sleep(
            control.deadline().saturating_duration_since(Instant::now()) + Duration::from_millis(5),
        );
        assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
        cancellation.cancel();
        assert_eq!(*cancellation.state.at.lock().unwrap(), first);
        assert_eq!(control.check().unwrap_err().code, "command.cancelled");
    }

    #[test]
    fn deadline_before_late_cancel_is_known_without_a_timer_observer() {
        let cancellation = TrackedCancellation::new();
        let control = OperationControl::tracked(
            Instant::now() + Duration::from_millis(40),
            cancellation.clone(),
        );
        let signal = cancellation.clone();
        let deadline = control.deadline();
        let worker = std::thread::spawn(move || {
            std::thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(20),
            );
            signal.cancel();
        });
        // Deliberately do not check the control until both events occurred.
        worker.join().unwrap();
        assert!(cancellation.state.at.lock().unwrap().unwrap() > deadline);
        assert_eq!(
            control.stop_cause().unwrap(),
            Some(StopCause::DeadlineExceeded)
        );
        assert_eq!(control.check().unwrap_err().code, "command.deadline");
        cancellation.cancel();
        assert_eq!(
            control.stop_cause().unwrap(),
            Some(StopCause::DeadlineExceeded)
        );
    }

    #[test]
    fn ordered_ancestry_preserves_local_and_parent_stop_order() {
        let parent_signal = TrackedCancellation::new();
        let parent = OperationControl::tracked(
            Instant::now() + Duration::from_secs(2),
            parent_signal.clone(),
        );
        let first = parent.ordered_child(parent.deadline()).unwrap();
        first.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(first.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert!(parent.check().is_ok());
        let child = parent
            .ordered_child(Instant::now() + Duration::from_millis(20))
            .unwrap();
        let grandchild = child.ordered_child(parent.deadline()).unwrap();
        assert_eq!(grandchild.deadline(), child.deadline());
        std::thread::sleep(
            child.deadline().saturating_duration_since(Instant::now()) + Duration::from_millis(5),
        );
        parent_signal.cancel();
        assert_eq!(parent.stop_cause().unwrap(), Some(StopCause::Cancelled));
        assert_eq!(
            grandchild.stop_cause().unwrap(),
            Some(StopCause::DeadlineExceeded)
        );
    }

    #[test]
    fn raw_tokens_remain_compatible_but_never_claim_ordered_evidence() {
        let raw = Arc::new(AtomicBool::new(false));
        let legacy = OperationControl::new(Instant::now() + Duration::from_secs(1), raw.clone());
        assert_eq!(
            legacy.stop_cause().unwrap_err().code,
            "control.cancellation_order_unavailable"
        );
        raw.store(true, Ordering::Release);
        assert_eq!(legacy.check().unwrap_err().code, "command.cancelled");
        let owned = OperationControl::default();
        assert_eq!(owned.stop_cause().unwrap(), None);
        let child = owned.ordered_child(owned.deadline()).unwrap();
        let raw = owned.cancel_handle();
        assert!(owned.stop_cause().is_err());
        assert!(child.stop_cause().is_err());
        raw.store(true, Ordering::Release);
        assert_eq!(owned.check().unwrap_err().code, "command.cancelled");
        assert_eq!(child.check().unwrap_err().code, "command.cancelled");
    }

    #[test]
    fn ordered_controls_release_all_owned_state_without_background_workers() {
        let cancellation = TrackedCancellation::new();
        let weak = Arc::downgrade(&cancellation.state);
        let control = OperationControl::tracked(
            Instant::now() + Duration::from_secs(1),
            cancellation.clone(),
        );
        assert!(control.same_scope(&control.clone()));
        assert!(!control.same_scope(&OperationControl::uncancelable(control.deadline())));
        let child = control.ordered_child(control.deadline()).unwrap();
        let child_state = Arc::downgrade(&child.tracked_cancel_handle().unwrap().state);
        let signal = cancellation.clone();
        let worker = std::thread::spawn(move || signal.cancel());
        worker.join().unwrap();
        drop(child);
        assert!(child_state.upgrade().is_none());
        drop(control);
        drop(cancellation);
        assert!(weak.upgrade().is_none());
        let expired = OperationControl::uncancelable(Instant::now());
        assert_eq!(
            expired.stop_cause().unwrap(),
            Some(StopCause::DeadlineExceeded)
        );
    }
}
