//! Current DataFusion operator pool boundary used by the session Engine.
use super::super::error;
use super::*;
use crate::resource::{Budget, Cost};
use datafusion::execution::memory_pool::MemoryConsumer;
use lycheedev_core::control::TrackedCancellation;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn budget() -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        None,
    ))
}

#[test]
fn original_operator_pool_enforces_try_grow_releases_and_disables_spill() {
    let budget = budget();
    let (context, owner) = runtime::context(budget.clone(), 4096, 1).unwrap();
    let runtime = context.runtime_env();
    assert!(!runtime.disk_manager.tmp_files_enabled());
    let reservation =
        MemoryConsumer::new("provider-qualification").register(&runtime.memory_pool);
    reservation.try_grow(4096).unwrap();
    assert!(reservation.try_grow(1).is_err());
    assert_eq!(runtime.memory_pool.reserved(), 4096);
    drop((context, owner));
    assert!(budget.refundable_retained_bytes() >= 4096);
    drop(reservation);
    assert_eq!(runtime.memory_pool.reserved(), 0);
    drop(runtime);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn cancelled_or_unadmitted_operator_pool_does_not_grant_new_reservations() {
    let token = TrackedCancellation::new();
    let budget = Arc::new(Budget::new(
        Cost {
            retained_bytes: 32768,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(60),
        Some(token.clone()),
    ));
    assert!(runtime::context(budget.clone(), 65536, 1).is_err());
    assert_eq!(budget.refundable_retained_bytes(), 0);
    let (context, owner) = runtime::context(budget.clone(), 4096, 1).unwrap();
    let reservation =
        MemoryConsumer::new("cancelled-qualification").register(&context.runtime_env().memory_pool);
    token.cancel();
    assert_eq!(
        error(reservation.try_grow(1).unwrap_err()).code,
        "operation.cancelled"
    );
    assert_eq!(reservation.size(), 0);
    drop((reservation, context, owner));
    assert_eq!(budget.refundable_retained_bytes(), 0);
}
