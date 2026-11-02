//! Session admission is independent from a completed request's deadline. Every
//! executing request first reserves its entire available allowance; the serial
//! session cannot overcommit through concurrently retained result/cache owners.
use crate::{
    fault,
    resource::{Budget, Cost, OwnedRetainedReservation},
};
use lycheedev_core::{Result, control::OperationControl};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(super) struct SessionMemory {
    pub budget: Arc<Budget>,
    limit: usize,
}
pub(super) struct RequestLifetime {
    pub budget: Arc<Budget>,
    // Deliberately conservative: source/cache and result sharing retain this
    // full admission together. This is a reservation, not a claimed RSS value.
    session: Mutex<OwnedRetainedReservation>,
}
impl SessionMemory {
pub fn begin_bounded(&self, work:u64, control:OperationControl, max_bytes:usize)->Result<Arc<RequestLifetime>> {
    control.check()?;
    let used=usize::try_from(self.budget.snapshot()?.retained_bytes).map_err(|_|bound())?;
    let available=self.limit.checked_sub(used).ok_or_else(bound)?;
    let allowance=available.min(max_bytes);
    if allowance==0 {return Err(bound());}
    let reservation=self.budget.reserve_retained(allowance)?.into_owned(&self.budget)?;
    let mut costs=Cost::defaults();costs.retained_bytes=allowance as u64;costs.decode_work=work;
    let budget=Arc::new(Budget::controlled(costs,control.deadline(),control).with_query_phase_profile());
    Ok(Arc::new(RequestLifetime{budget,session:Mutex::new(reservation)}))
}

    pub fn new(limit: usize) -> Result<Self> {
        if limit == 0 || limit > 8usize << 30 {
            return Err(bound());
        }
        let mut costs = Cost::defaults();
        costs.retained_bytes = limit as u64;
        Ok(Self {
            budget: Arc::new(Budget::new(
                costs,
                Instant::now()
                    .checked_add(Duration::from_secs(365 * 24 * 3600))
                    .ok_or_else(bound)?,
                None,
            )),
            limit,
        })
    }
    pub fn begin(&self, work: u64, control: OperationControl) -> Result<Arc<RequestLifetime>> {
        control.check()?;
        let used = usize::try_from(self.budget.snapshot()?.retained_bytes).map_err(|_| bound())?;
        let available = self.limit.checked_sub(used).ok_or_else(bound)?;
        if available == 0 {
            return Err(bound());
        }
        let reservation = self
            .budget
            .reserve_retained(available)?
            .into_owned(&self.budget)?;
        let mut costs = Cost::defaults();
        costs.retained_bytes = available as u64;
        costs.decode_work = work;
        let budget = Arc::new(
            Budget::controlled(costs, control.deadline(), control).with_query_phase_profile(),
        );
        Ok(Arc::new(RequestLifetime {
            budget,
            session: Mutex::new(reservation),
        }))
    }
}
fn bound() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Session retained-memory admission exhausted",
    )
}

impl RequestLifetime {
    pub fn settle(&self) -> Result<usize> {
        let bytes = usize::try_from(self.budget.snapshot()?.retained_bytes).map_err(|_| bound())?;
        self.session.lock().map_err(|_| bound())?.shrink_to(bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovery_metadata_refunds_while_first_source_cache_survives() {
        let session = SessionMemory::new(10000).unwrap();
        let lifetime = session.begin(10000, OperationControl::default()).unwrap();
        let source = lifetime
            .budget
            .reserve_retained(1000)
            .unwrap()
            .into_owned(&lifetime.budget)
            .unwrap();
        let metadata = lifetime
            .budget
            .reserve_retained(2000)
            .unwrap()
            .into_owned(&lifetime.budget)
            .unwrap();
        let info = super::super::api::DatasetInfo {
            _allocation: Some(metadata),
            _lifetime: Some(lifetime.clone()),
            table: super::super::api::TableRequest {
                name: "T".into(),
                mode: super::super::api::SourceMode::Static,
                hotfix_captures: vec![],
            },
            columns: vec![],
            row_count: 0,
            provenance: serde_json::json!({"complete":true}),
        };
        lifetime.settle().unwrap();
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 3000);
        assert!(serde_json::to_string(&info).unwrap().contains("complete"));
        drop(info);
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 1000);
        drop(source);
        drop(lifetime);
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 0);
    }
    #[test]
    fn request_controls_are_independent_and_live_owners_share_session_limit() {
        let session = SessionMemory::new(10000).unwrap();
        let first = session.begin(100, OperationControl::default()).unwrap();
        let retained = first
            .budget
            .reserve_retained(1000)
            .unwrap()
            .into_owned(&first.budget)
            .unwrap();
        first.settle().unwrap();
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 1000);
        let second = session.begin(200, OperationControl::default()).unwrap();
        assert!(second.budget.reserve_retained(9001).is_err());
        assert!(second.budget.charge(Cost::work(201)).is_err());
        second.settle().unwrap();
        drop(second);
        drop(retained);
        drop(first);
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 0);
    }
    #[test]
    fn failed_request_refunds_whole_session_allowance() {
        let session = SessionMemory::new(1000).unwrap();
        {
            let _request = session.begin(10, OperationControl::default()).unwrap();
            assert!(session.begin(10, OperationControl::default()).is_err());
        }
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 0);
        assert!(session.begin(10, OperationControl::default()).is_ok());
    }
}

impl std::fmt::Debug for RequestLifetime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RequestLifetime").finish_non_exhaustive()
    }
}

/// Reconcile on every request exit, including errors with an already-installed
/// source cache. It never runs an expired request's work/control again.
pub(super) struct RequestExecution(pub Arc<RequestLifetime>);
impl Drop for RequestExecution {
    fn drop(&mut self) { let _ = self.0.settle(); }
}
