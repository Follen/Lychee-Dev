//! DataFusion integration candidate. The mature engine owns relational planning
//! and execution; owned extensions retain the established numeric contract.
//! This module is not a second SQL executor or an implicit default-route switch.
mod aggregate;
mod numeric;
mod provider;
mod values;

use crate::{
    fault,
    resource::{Budget, Cost, OwnedRetainedReservation},
};
use datafusion::common::{DataFusionError, Result as DfResult};
use lycheedev_core::ToolkitError;
use std::sync::{Arc, Mutex};

/// Candidate-owned function/state allocations remain charged while the query
/// owner lives. Tests retain it through conversion of all results. A production
/// stream must attach this owner to its output lifetime before using this seam;
/// native DataFusion plan/stream allocation accounting is not implemented here.
struct QueryOwner {
    budget: Arc<Budget>,
    allocations: Mutex<Vec<OwnedRetainedReservation>>,
}
impl std::fmt::Debug for QueryOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataFusionQueryOwner")
            .finish_non_exhaustive()
    }
}
impl QueryOwner {
    fn new(budget: Arc<Budget>) -> Arc<Self> {
        Arc::new(Self {
            budget,
            allocations: Mutex::new(Vec::new()),
        })
    }
    fn retain(&self, bytes: usize) -> DfResult<()> {
        self.budget.check().map_err(external)?;
        // Include the retained reservation/vector entry before growing it.
        let bytes = bytes.checked_add(128).ok_or_else(|| external(limit()))?;
        let allocation = self
            .budget
            .reserve_retained(bytes)
            .map_err(external)?
            .into_owned(&self.budget)
            .map_err(external)?;
        self.allocations
            .lock()
            .map_err(|_| external(limit()))?
            .push(allocation);
        Ok(())
    }
    fn check(&self) -> DfResult<()> {
        self.budget.check().map_err(external)
    }
    fn work(&self, units: usize) -> DfResult<()> {
        self.budget.charge(Cost::work(units)).map_err(external)
    }
}
fn external(error: ToolkitError) -> DataFusionError {
    DataFusionError::External(Box::new(error))
}
fn limit() -> ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "DataFusion query exceeds its admitted bound",
    )
}
fn unsupported(message: impl Into<String>) -> DataFusionError {
    external(fault("relational.unsupported", 3, message))
}
fn error(error: DataFusionError) -> ToolkitError {
    // DataFusion may add Context layers. Keep the original typed fault rather
    // than replacing cancellation/numeric errors with a generic SQL failure.
    let mut source: &(dyn std::error::Error + 'static) = &error;
    loop {
        if let Some(error) = source.downcast_ref::<ToolkitError>() {
            return error.clone();
        }
        match source.source() {
            Some(next) => source = next,
            None => break,
        }
    }
    fault("relational.datafusion", 4, error.to_string())
}
#[cfg(test)]
mod tests;
