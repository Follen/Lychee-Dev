//! Use DataFusion's original bounded pool policy, with a preadmitted lifetime
//! allowance and disabled spilling. This is deliberately NOT a whole-query
//! allocator guarantee: DF's infallible grow() and unregistered output buffers
//! need operator-by-operator proof before any production engine migration.
use super::super::{QueryOwner, aggregate, external, numeric, unsupported};
use crate::resource::{Budget, OwnedRetainedReservation};
use datafusion::{
    common::Result,
    execution::{
        context::SessionContext,
        disk_manager::{DiskManagerBuilder, DiskManagerMode},
        memory_pool::{GreedyMemoryPool, MemoryLimit, MemoryPool, MemoryReservation},
        runtime_env::RuntimeEnvBuilder,
        session_state::SessionStateBuilder,
    },
    prelude::SessionConfig,
};
use std::{fmt, sync::Arc};

pub(super) fn context(
    budget: Arc<Budget>,
    operator_bytes: usize,
    batch_rows: usize,
) -> Result<(SessionContext, Arc<QueryOwner>)> {
    if operator_bytes == 0 || operator_bytes > 128 << 20 || batch_rows == 0 || batch_rows > 65_536 {
        return Err(unsupported(
            "Invalid DataFusion qualification runtime bound",
        ));
    }
    // The allowance is charged before constructing the runtime and is owned
    // by the actual pool, so surviving TaskContexts/reservations keep it live.
    let allocation = budget
        .reserve_retained(operator_bytes)
        .map_err(external)?
        .into_owned(&budget)
        .map_err(external)?;
    let pool = Arc::new(OperatorPool {
        inner: GreedyMemoryPool::new(operator_bytes),
        budget: budget.clone(),
        _allocation: allocation,
    });
    let owner = QueryOwner::new(budget);
    owner.retain(16 << 10)?;
    let runtime = RuntimeEnvBuilder::new()
        .with_memory_pool(pool)
        .with_disk_manager_builder(
            DiskManagerBuilder::default().with_mode(DiskManagerMode::Disabled),
        )
        .build_arc()?;
    let mut functions = aggregate::functions(&owner);
    functions.push(datafusion::functions_aggregate::count::count_udaf());
    let state = SessionStateBuilder::new()
        .with_runtime_env(runtime)
        .with_config(
            SessionConfig::new()
                .with_target_partitions(1)
                .with_batch_size(batch_rows),
        )
        // Registering COUNT's UDAF alone does not lower SQL COUNT(*) in
        // DataFusion 54. Keep its upstream aggregate planner without enabling
        // unrelated default functions, file formats, or table factories.
        .with_expr_planners(vec![
            Arc::new(numeric::NumericPlanner {
                owner: owner.clone(),
            }),
            Arc::new(datafusion::functions_aggregate::planner::AggregateFunctionPlanner),
        ])
        .with_aggregate_functions(functions)
        .build();
    Ok((SessionContext::new_with_state(state), owner))
}
struct OperatorPool {
    inner: GreedyMemoryPool,
    budget: Arc<Budget>,
    _allocation: OwnedRetainedReservation,
}
impl fmt::Debug for OperatorPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.inner, f)
    }
}
impl fmt::Display for OperatorPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.inner, f)
    }
}
impl MemoryPool for OperatorPool {
    fn name(&self) -> &str {
        "PreadmittedGreedyQualificationPool"
    }
    fn grow(&self, reservation: &MemoryReservation, additional: usize) {
        // The upstream trait requires this to succeed. Do not pretend a
        // fallible after-allocation check would enforce a hard allocation cap.
        self.inner.grow(reservation, additional);
    }
    fn shrink(&self, reservation: &MemoryReservation, bytes: usize) {
        self.inner.shrink(reservation, bytes);
    }
    fn try_grow(&self, reservation: &MemoryReservation, additional: usize) -> Result<()> {
        self.budget.check().map_err(external)?;
        self.inner.try_grow(reservation, additional)
    }
    fn reserved(&self) -> usize {
        self.inner.reserved()
    }
    fn memory_limit(&self) -> MemoryLimit {
        self.inner.memory_limit()
    }
}
