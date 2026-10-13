//! One caller-owned deadline and cooperative cancellation token for the whole operation.
use crate::{Result, ToolkitError};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
#[derive(Debug, Clone)]
pub struct OperationControl {
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    parent_cancellations: Vec<Arc<AtomicBool>>,
}
impl Default for OperationControl {
    fn default() -> Self {
        Self::new(
            Instant::now() + Duration::from_secs(180),
            Arc::new(AtomicBool::new(false)),
        )
    }
}
impl OperationControl {
    pub fn new(deadline: Instant, cancel: Arc<AtomicBool>) -> Self {
        Self {
            deadline,
            cancel,
            parent_cancellations: Vec::new(),
        }
    }
    /// A request can retire independently while retaining the caller's lifetime and cancellation.
    pub fn child(&self, deadline: Instant, cancel: Arc<AtomicBool>) -> Self {
        let mut parents = self.parent_cancellations.clone();
        parents.push(Arc::clone(&self.cancel));
        Self {
            deadline: deadline.min(self.deadline),
            cancel,
            parent_cancellations: parents,
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
        self.inherit_cancellation();
        if self.cancel.load(Ordering::Acquire) {
            return Err(ToolkitError::new(
                "command.cancelled",
                "operation cancelled by the caller",
                7,
            ));
        }
        if Instant::now() >= self.deadline {
            return Err(ToolkitError::new(
                "command.deadline",
                "operation deadline exhausted",
                5,
            ));
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
    /// Requested heap storage needed when retaining this control's ancestry.
    pub fn clone_allocation_bytes(&self) -> usize {
        self.parent_cancellations.len() * std::mem::size_of::<Arc<AtomicBool>>()
    }
    pub fn token(&self) -> &AtomicBool {
        self.inherit_cancellation();
        &self.cancel
    }
    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.inherit_cancellation();
        Arc::clone(&self.cancel)
    }
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
}
