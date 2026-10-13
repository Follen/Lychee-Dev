//! One query's shared conservative allocation/work/network ledger. Counters
//! describe admitted work, not process RSS or an optimizer's cost estimate.
use crate::fault;
use lycheedev_core::{Result, control::OperationControl};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};
#[derive(Default, Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cost {
    pub retained_bytes: u64,
    pub metadata_bytes: u64,
    pub decode_work: u64,
    pub network_requests: u64,
    pub network_bytes: u64,
}
impl Cost {
    pub fn retained(bytes: usize) -> Self {
        Self {
            retained_bytes: bytes as u64,
            ..Default::default()
        }
    }
    pub fn metadata(bytes: usize) -> Self {
        Self {
            retained_bytes: bytes as u64,
            metadata_bytes: bytes as u64,
            ..Default::default()
        }
    }
    pub fn work(work: usize) -> Self {
        Self {
            decode_work: work as u64,
            ..Default::default()
        }
    }
}
impl Cost {
    pub fn defaults() -> Self {
        Self {
            retained_bytes: 512 << 20,
            metadata_bytes: 128 << 20,
            decode_work: 100_000_000,
            network_requests: 4096,
            network_bytes: 1 << 30,
        }
    }
}
#[derive(Default)]
struct State {
    used: Cost,
    once: BTreeSet<String>,
}
pub struct Budget {
    limits: Cost,
    state: Mutex<State>,
    pub deadline: Instant,
    cancel: Option<Arc<AtomicBool>>,
    control: Option<OperationControl>,
}
/// A temporary allocation admission. Only its exact retained-byte charge is
/// released when the owning allocation is dropped; work/metadata never reset.
pub(crate) struct RetainedReservation<'a> {
    budget: &'a Budget,
    bytes: u64,
    committed: bool,
}
impl RetainedReservation<'_> {
    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}
impl Drop for RetainedReservation<'_> {
    fn drop(&mut self) {
        if !self.committed
            && let Ok(mut state) = self.budget.state.lock()
            && let Some(remaining) = state.used.retained_bytes.checked_sub(self.bytes)
        {
            state.used.retained_bytes = remaining;
        }
    }
}
impl Budget {
    pub fn new(limits: Cost, deadline: Instant, cancel: Option<Arc<AtomicBool>>) -> Self {
        Self {
            limits,
            state: Mutex::new(State::default()),
            deadline,
            cancel,
            control: None,
        }
    }
    pub fn controlled(limits: Cost, deadline: Instant, control: OperationControl) -> Self {
        Self {
            deadline: deadline.min(control.deadline()),
            control: Some(control),
            ..Self::new(limits, deadline, None)
        }
    }
    pub fn token(&self) -> Option<&AtomicBool> {
        self.control
            .as_ref()
            .map(OperationControl::token)
            .or(self.cancel.as_deref())
    }
    pub fn check(&self) -> Result<()> {
        if let Some(control) = &self.control {
            control.check()?;
        }
        if self
            .cancel
            .as_ref()
            .is_some_and(|v| v.load(Ordering::Acquire))
        {
            return Err(fault("operation.cancelled", 7, "Data query cancelled"));
        }
        if Instant::now() >= self.deadline {
            return Err(fault(
                "records.query_resource_budget",
                3,
                "Data query deadline exceeded",
            ));
        }
        Ok(())
    }
    pub fn snapshot(&self) -> Result<Cost> {
        let state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        Ok(state.used)
    }
    pub fn charge(&self, cost: Cost) -> Result<()> {
        self.check()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        self.admit(&mut state, cost)
    }
    pub(crate) fn reserve_retained(&self, bytes: usize) -> Result<RetainedReservation<'_>> {
        self.charge(Cost::retained(bytes))?;
        Ok(RetainedReservation {
            budget: self,
            bytes: bytes as u64,
            committed: false,
        })
    }
    pub fn charge_once(&self, key: &str, cost: Cost) -> Result<()> {
        self.check()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        if state.once.contains(key) {
            return Ok(());
        }
        if key.len() > 4096 || state.once.len() >= 4096 {
            return Err(fault(
                "records.query_resource_budget",
                3,
                "Resource identity count exceeded",
            ));
        }
        self.admit(&mut state, cost)?;
        state.once.insert(key.to_owned());
        Ok(())
    }
    fn admit(&self, state: &mut State, cost: Cost) -> Result<()> {
        let mut next = state.used;
        for (used, limit, add, name) in [
            (
                &mut next.retained_bytes,
                self.limits.retained_bytes,
                cost.retained_bytes,
                "retained_bytes",
            ),
            (
                &mut next.metadata_bytes,
                self.limits.metadata_bytes,
                cost.metadata_bytes,
                "metadata_bytes",
            ),
            (
                &mut next.decode_work,
                self.limits.decode_work,
                cost.decode_work,
                "decode_work",
            ),
            (
                &mut next.network_requests,
                self.limits.network_requests,
                cost.network_requests,
                "network_requests",
            ),
            (
                &mut next.network_bytes,
                self.limits.network_bytes,
                cost.network_bytes,
                "network_bytes",
            ),
        ] {
            if *used > limit || add > limit - *used {
                return Err(fault(
                    "records.query_resource_budget",
                    3,
                    format!("Whole query {name} budget exceeded"),
                ));
            }
            *used += add;
        }
        state.used = next;
        Ok(())
    }
}

#[cfg(test)]
mod temporary_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn failed_allocation_release_preserves_work_and_committed_ownership() {
        let budget = Budget::new(
            Cost {
                retained_bytes: 64,
                decode_work: 100,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(5),
            None,
        );
        budget.charge(Cost::retained(16)).unwrap();
        let reservation = budget.reserve_retained(40).unwrap();
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 56);
        assert!(budget.reserve_retained(9).is_err());
        budget.charge(Cost::work(3)).unwrap();
        drop(reservation);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 16);
        assert_eq!(budget.snapshot().unwrap().decode_work, 3);
        budget.reserve_retained(40).unwrap().commit();
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 56);
        assert!(budget.reserve_retained(9).is_err());
    }
}
