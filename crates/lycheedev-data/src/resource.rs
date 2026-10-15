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
#[derive(Default, Clone, Copy, Debug, Serialize, PartialEq, Eq)]
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
    logical: Cost,
    logical_scratch: u64,
    once: BTreeSet<String>,
}
pub struct Budget {
    limits: Cost,
    state: Arc<Mutex<State>>,
    safety_only: bool,
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
/// Go logical scratch is separate from Rust allocation ownership and is never
/// included in the public lifetime-usage snapshot.
pub(crate) struct LogicalScratch<'a> {
    budget: &'a Budget,
    bytes: u64,
}
impl Drop for LogicalScratch<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.budget.state.lock() {
            state.logical_scratch -= self.bytes;
        }
    }
}
impl RetainedReservation<'_> {
    /// Extend only this owner's refundable allocation. Failed admission leaves
    /// both the guard and the shared ledger unchanged.
    pub(crate) fn grow(&mut self, bytes: usize) -> Result<()> {
        let bytes = u64::try_from(bytes).map_err(|_| {
            fault(
                "records.query_resource_budget",
                3,
                "Retained allocation overflow",
            )
        })?;
        let total = self
            .bytes
            .checked_add(bytes)
            .filter(|total| *total <= self.budget.limits.retained_bytes)
            .ok_or_else(|| {
                fault(
                    "records.query_resource_budget",
                    3,
                    "Retained allocation overflow",
                )
            })?;
        self.budget.charge_split(
            Cost::default(),
            Cost {
                retained_bytes: bytes,
                ..Default::default()
            },
        )?;
        self.bytes = total;
        Ok(())
    }
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
            state: Arc::new(Mutex::new(State::default())),
            safety_only: false,
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
    /// A view sharing the same hard safety caps, cancellation and allocations,
    /// but excluding Rust-only request/executor/publication costs from Go usage.
    pub(crate) fn safety_only(&self) -> Self {
        Self {
            limits: self.limits,
            state: self.state.clone(),
            safety_only: true,
            deadline: self.deadline,
            cancel: self.cancel.clone(),
            control: self.control.clone(),
        }
    }
    pub(crate) fn logical_snapshot(&self) -> Result<Cost> {
        self.state
            .lock()
            .map(|s| s.logical)
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))
    }
    /// Both ledgers are checked before either changes. Safety retained bytes
    /// may exceed Go's allocation estimate without inflating metadata usage.
    pub(crate) fn charge_split(&self, logical: Cost, safety: Cost) -> Result<()> {
        self.check()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        let logical = if self.safety_only {
            Cost::default()
        } else {
            logical
        };
        let next_logical = self.admitted_logical(&state, logical)?;
        let next_safety = self.admitted(state.used, safety)?;
        state.logical = next_logical;
        state.used = next_safety;
        Ok(())
    }
    pub(crate) fn reserve_logical_scratch(&self, bytes: u64) -> Result<LogicalScratch<'_>> {
        self.check()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        let bytes = if self.safety_only { 0 } else { bytes };
        let current = state.logical.retained_bytes + state.logical_scratch;
        if bytes > self.limits.retained_bytes - current {
            return Err(fault(
                "records.query_resource_budget",
                3,
                "Whole query scratch_bytes budget exceeded",
            ));
        }
        state.logical_scratch += bytes;
        Ok(LogicalScratch {
            budget: self,
            bytes,
        })
    }
    fn admitted_logical(&self, state: &State, cost: Cost) -> Result<Cost> {
        let next = self.admitted(state.logical, cost)?;
        if state.logical_scratch > self.limits.retained_bytes - next.retained_bytes {
            return Err(fault(
                "records.query_resource_budget",
                3,
                "Whole query scratch_bytes budget exceeded",
            ));
        }
        Ok(next)
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
    /// Remaining admitted transfer budget. A concurrent reader may observe
    /// the same snapshot; atomic charging still rejects overspend.
    pub(crate) fn remaining_network_bytes(&self) -> Result<u64> {
        self.check()?;
        let state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        self.limits
            .network_bytes
            .checked_sub(state.used.network_bytes)
            .ok_or_else(|| {
                fault(
                    "records.internal",
                    8,
                    "Network resource ledger exceeds its limit",
                )
            })
    }
    /// Record body bytes already consumed, including a read that completed
    /// concurrently with cancellation/deadline. This intentionally does not
    /// call check(): it admits no future work and cannot refund observed I/O.
    /// A limit failure retains the ordinary atomic-charge rule (no mutation).
    pub(crate) fn charge_observed_network_bytes(&self, bytes: u64) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        self.admit(
            &mut state,
            Cost {
                network_bytes: bytes,
                ..Cost::default()
            },
        )
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
        self.charge_split(Cost::default(), Cost::retained(bytes))?;
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
        let logical = if self.safety_only {
            Cost::default()
        } else {
            cost
        };
        let next_logical = self.admitted_logical(&state, logical)?;
        let next_safety = self.admitted(state.used, cost)?;
        state.logical = next_logical;
        state.used = next_safety;
        Ok(())
    }
    fn admitted(&self, mut next: Cost, cost: Cost) -> Result<Cost> {
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
        Ok(next)
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
    #[test]
    fn growing_reservation_releases_only_owned_bytes_after_failed_admission() {
        let budget = Budget::new(
            Cost {
                retained_bytes: 100,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let unrelated = budget.reserve_retained(30).unwrap();
        let mut owned = budget.reserve_retained(10).unwrap();
        owned.grow(40).unwrap();
        budget
            .charge(Cost {
                metadata_bytes: 17,
                decode_work: 3,
                ..Default::default()
            })
            .unwrap();
        assert!(owned.grow(21).is_err());
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 80);
        assert!(owned.grow(usize::MAX).is_err());
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 80);
        drop(owned);
        let used = budget.snapshot().unwrap();
        assert_eq!(used.retained_bytes, 30);
        assert_eq!(used.metadata_bytes, 17);
        assert_eq!(used.decode_work, 3);
        drop(unrelated);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
        let mut persistent = budget.reserve_retained(10).unwrap();
        persistent.grow(20).unwrap();
        persistent.commit();
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 30);
    }
}

#[cfg(test)]
mod logical_tests {
    use super::*;
    use std::time::Duration;
    fn budget() -> Budget {
        Budget::new(
            Cost {
                retained_bytes: 100,
                metadata_bytes: 50,
                decode_work: 100_000_000,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        )
    }
    #[test]
    fn split_charges_are_atomic_in_both_directions() {
        let b = budget();
        b.charge_split(Cost::retained(40), Cost::retained(80))
            .unwrap();
        assert!(
            b.charge_split(Cost::retained(1), Cost::retained(21))
                .is_err()
        );
        assert_eq!(b.logical_snapshot().unwrap().retained_bytes, 40);
        assert_eq!(b.snapshot().unwrap().retained_bytes, 80);
        assert!(
            b.charge_split(Cost::retained(61), Cost::retained(1))
                .is_err()
        );
        assert_eq!(b.logical_snapshot().unwrap().retained_bytes, 40);
        assert_eq!(b.snapshot().unwrap().retained_bytes, 80);
    }
    #[test]
    fn safety_view_and_scratch_never_release_another_owners_charge() {
        let b = budget();
        b.charge_split(Cost::retained(64), Cost::default()).unwrap();
        let safety = b.safety_only();
        let scratch = b.reserve_logical_scratch(36).unwrap();
        assert!(b.charge_split(Cost::retained(1), Cost::default()).is_err());
        safety.charge(Cost::retained(90)).unwrap();
        let reservation = safety.reserve_retained(10).unwrap();
        drop(scratch);
        assert!(safety.charge(Cost::retained(1)).is_err());
        drop(reservation);
        assert_eq!(b.snapshot().unwrap().retained_bytes, 90);
        assert_eq!(b.logical_snapshot().unwrap().retained_bytes, 64);
        assert!(safety.charge(Cost::retained(11)).is_err());
    }
    #[test]
    fn decode_limit_and_cancellation_apply_to_both_views() {
        let b = budget();
        b.charge(Cost::work(100_000_000)).unwrap();
        assert!(b.charge(Cost::work(1)).is_err());
        assert!(b.safety_only().charge(Cost::work(1)).is_err());
        assert_eq!(b.logical_snapshot().unwrap().decode_work, 100_000_000);
        let token = Arc::new(AtomicBool::new(true));
        let cancelled = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            Some(token),
        );
        assert_eq!(
            cancelled
                .safety_only()
                .charge(Cost::default())
                .unwrap_err()
                .code,
            "operation.cancelled"
        );
        assert_eq!(cancelled.logical_snapshot().unwrap(), Cost::default());
    }
}

#[cfg(test)]
mod network_observed_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn remaining_network_keeps_u64_max_and_rejects_corrupt_ledger() {
        let b = Budget::new(
            Cost {
                network_bytes: u64::MAX,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(5),
            None,
        );
        assert_eq!(b.remaining_network_bytes().unwrap(), u64::MAX);
        b.charge_observed_network_bytes(5).unwrap();
        assert_eq!(b.remaining_network_bytes().unwrap(), u64::MAX - 5);
        let b = Budget::new(
            Cost {
                network_bytes: 3,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(5),
            None,
        );
        b.state.lock().unwrap().used.network_bytes = 4;
        assert_eq!(
            b.remaining_network_bytes().unwrap_err().code,
            "records.internal"
        );
    }
    #[test]
    fn completed_network_read_is_charged_after_cancellation_but_overshoot_is_atomic() {
        let token = Arc::new(AtomicBool::new(true));
        let b = Budget::new(
            Cost {
                network_bytes: 3,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(5),
            Some(token),
        );
        assert!(
            b.charge(Cost {
                network_bytes: 1,
                ..Cost::default()
            })
            .is_err()
        );
        b.charge_observed_network_bytes(3).unwrap();
        assert_eq!(b.snapshot().unwrap().network_bytes, 3);
        assert_eq!(b.logical_snapshot().unwrap().network_bytes, 3);
        assert_eq!(
            b.charge_observed_network_bytes(1).unwrap_err().code,
            "records.query_resource_budget"
        );
        assert_eq!(b.snapshot().unwrap().network_bytes, 3);
    }
    #[test]
    fn concurrent_snapshots_do_not_grant_two_atomic_admissions() {
        let b = Budget::new(
            Cost {
                network_bytes: 3,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(5),
            None,
        );
        let same = b.safety_only();
        assert_eq!(b.remaining_network_bytes().unwrap(), 3);
        assert_eq!(same.remaining_network_bytes().unwrap(), 3);
        b.charge_observed_network_bytes(3).unwrap();
        assert!(same.charge_observed_network_bytes(3).is_err());
        assert_eq!(b.snapshot().unwrap().network_bytes, 3);
    }
}
