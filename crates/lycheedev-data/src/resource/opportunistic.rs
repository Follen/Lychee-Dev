//! Optional CPU work uses only already-admitted private headroom. It cannot
//! grow either memory allowance or turn a valid serial operation into a refusal.
use super::*;

impl Budget {
    /// Select and reserve optional CPU workers atomically from this exact
    /// shared ledger. Zero means backpressure: use the original serial path.
    /// Unlike reserve_retained, this never probes memory or grows an allowance.
    /// Selection is O(1), even when the OS reports a very large CPU count.
    pub(crate) fn try_reserve_workers(
        &self,
        requested: usize,
        bytes_per_worker: usize,
    ) -> Result<(usize, Option<RetainedReservation<'_>>)> {
        self.check()?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| fault("records.internal", 8, "Query resource lock poisoned"))?;
        // Waiting for another owner is a cancellation boundary too. check()
        // does not acquire this ledger mutex or replace the original control.
        self.check()?;
        if state.pending.is_some() {
            return Err(adaptive::not_admitted());
        }
        let remaining = state
            .safety_limits
            .retained_bytes
            .checked_sub(state.used.retained_bytes)
            .ok_or_else(|| fault("records.internal", 8, "Retained ledger exceeds its limit"))?;
        if bytes_per_worker == 0 {
            return Err(fault(
                "records.internal",
                8,
                "Worker storage allowance is zero",
            ));
        }
        let count = (requested as u64).min(remaining / bytes_per_worker as u64) as usize;
        if count == 0 {
            return Ok((0, None));
        }
        // Division above proves both multiplication and addition fit the
        // current allowance. Existing owners and the Go ledger stay unchanged.
        let bytes = count as u64 * bytes_per_worker as u64;
        state.used.retained_bytes += bytes;
        #[cfg(test)]
        {
            state.refundable_retained_bytes += bytes;
        }
        Ok((
            count,
            Some(RetainedReservation {
                budget: self,
                bytes,
                committed: false,
            }),
        ))
    }

    #[cfg(test)]
    fn try_reserve_retained(&self, bytes: usize) -> Result<Option<RetainedReservation<'_>>> {
        Ok(self.try_reserve_workers(1, bytes)?.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    #[test]
    fn optional_workers_share_private_headroom_without_changing_logical_usage() {
        let budget = Budget::new(
            Cost {
                retained_bytes: 100,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let foreign = budget.reserve_retained(60).unwrap();
        let logical = budget.logical_snapshot().unwrap();
        let limits = budget.memory_snapshot().unwrap();
        let guard = budget.try_reserve_retained(40).unwrap().unwrap();
        assert!(
            budget
                .safety_only()
                .try_reserve_retained(1)
                .unwrap()
                .is_none()
        );
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 100);
        assert_eq!(budget.refundable_retained_bytes(), 100);
        assert_eq!(budget.logical_snapshot().unwrap(), logical);
        assert_eq!(budget.memory_snapshot().unwrap(), limits);
        drop(guard);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 60);
        drop(foreign);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }

    #[test]
    fn optional_worker_reservations_cannot_race_past_the_shared_limit() {
        let budget = Budget::new(
            Cost {
                retained_bytes: 100,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let ready = std::sync::Barrier::new(2);
        let held = std::sync::Barrier::new(2);
        let successes = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            for _ in 0..2 {
                scope.spawn(|| {
                    ready.wait();
                    let guard = budget.try_reserve_retained(60).unwrap();
                    if guard.is_some() {
                        successes.fetch_add(1, Ordering::SeqCst);
                    }
                    held.wait();
                    drop(guard);
                });
            }
        });
        assert_eq!(successes.load(Ordering::SeqCst), 1);
        assert_eq!(budget.refundable_retained_bytes(), 0);
        assert_eq!(budget.snapshot().unwrap(), Cost::default());
    }

    #[test]
    fn worker_count_is_selected_atomically_without_iterating_a_large_cpu_bound() {
        let budget = Budget::new(
            Cost {
                retained_bytes: 100,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let foreign = budget.reserve_retained(60).unwrap();
        let (count, guard) = budget.try_reserve_workers(usize::MAX, 11).unwrap();
        assert_eq!(count, 3);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 93);
        assert_eq!(budget.refundable_retained_bytes(), 93);
        assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
        assert_eq!(budget.try_reserve_workers(usize::MAX, 11).unwrap().0, 0);
        assert_eq!(budget.try_reserve_workers(0, 1).unwrap().0, 0);
        assert_eq!(
            budget.try_reserve_workers(1, 0).err().unwrap().code,
            "records.internal"
        );
        drop(guard);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 60);
        drop(foreign);
    }

    #[test]
    fn optional_workers_keep_admission_and_original_cancellation() {
        let control = OperationControl::default();
        let pending = Budget::query_controlled(control.deadline(), control.clone());
        assert_eq!(
            pending.try_reserve_retained(1).err().unwrap().code,
            "records.internal"
        );
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let guard = budget.try_reserve_retained(1024).unwrap().unwrap();
        control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(budget.try_reserve_retained(1).err().unwrap().exit_code, 7);
        drop(guard);
        assert_eq!(budget.refundable_retained_bytes(), 0);
        assert_eq!(budget.snapshot().unwrap(), Cost::default());
    }

    #[test]
    fn optional_workers_never_probe_or_grow_an_admitted_adaptive_allowance() {
        use lycheedev_core::{
            storage,
            workspace::{
                self,
                data_resources::{self, Policy},
            },
        };
        use std::sync::atomic::{AtomicU64, AtomicUsize};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/data-optional-worker-budget")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                storage::utc_now().replace([':', '.'], "-"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        workspace::initialize(&home).unwrap();
        let control = OperationControl::default();
        let admission = data_resources::acquire(&home, Policy::Ordinary, &control).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let probe = {
            let calls = calls.clone();
            Arc::new(move || {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(8 << 30)
            })
        };
        let budget = Budget::query_with_probe(control.deadline(), control, probe);
        budget.initialize_admitted(&admission).unwrap();
        let initial_calls = calls.load(Ordering::SeqCst);
        let memory = budget.memory_snapshot().unwrap();
        assert_eq!(memory.retained_bytes, 512 << 20);
        // The ceiling permits normal growth, but optional CPU parallelism is
        // not permitted to ask for that growth or a new host-memory sample.
        assert!(memory.ceiling_bytes > memory.retained_bytes);
        assert!(budget.try_reserve_retained(600 << 20).unwrap().is_none());
        let guard = budget.try_reserve_retained(8 << 20).unwrap().unwrap();
        assert_eq!(budget.memory_snapshot().unwrap(), memory);
        assert_eq!(budget.logical_snapshot().unwrap(), Cost::default());
        assert_eq!(calls.load(Ordering::SeqCst), initial_calls);
        drop(guard);
        assert_eq!(budget.snapshot().unwrap(), Cost::default());
        drop(budget);
        admission.close().unwrap();
        std::fs::remove_dir_all(&home).unwrap();
    }
}
