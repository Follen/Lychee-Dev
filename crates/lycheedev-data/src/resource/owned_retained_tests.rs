use super::*;
use std::time::Duration;
fn budget() -> Arc<Budget> {
    Arc::new(Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        OperationControl::default(),
    ))
}
#[test]
fn transfer_moves_one_refund_without_charging_work_or_either_ledger_again() {
    let b = budget();
    b.charge(Cost {
        decode_work: 3,
        network_requests: 2,
        network_bytes: 5,
        ..Default::default()
    })
    .unwrap();
    let reservation = b.reserve_retained(17).unwrap();
    let actual = b.snapshot().unwrap();
    let logical = b.logical_snapshot().unwrap();
    let owned = reservation.into_owned(&b).unwrap();
    assert_eq!(b.snapshot().unwrap(), actual);
    assert_eq!(b.logical_snapshot().unwrap(), logical);
    drop(owned);
    assert_eq!(b.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(b.snapshot().unwrap().decode_work, 3);
    assert_eq!(b.snapshot().unwrap().network_requests, 2);
    assert_eq!(b.snapshot().unwrap().network_bytes, 5);
    assert_eq!(b.logical_snapshot().unwrap(), logical);
}
#[test]
fn transfer_to_a_different_budget_refunds_only_its_original_failed_owner() {
    let original = budget();
    let other = budget();
    original.charge_observed_network_bytes(9).unwrap();
    let reservation = original.reserve_retained(17).unwrap();
    let error = reservation.into_owned(&other).err().unwrap();
    assert_eq!(error.code, "records.internal");
    assert_eq!(original.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(original.snapshot().unwrap().network_bytes, 9);
    assert_eq!(other.snapshot().unwrap(), Cost::default());
}
#[test]
fn transferred_owner_holds_original_budget_and_refunds_after_stop() {
    let b = budget();
    let weak = Arc::downgrade(&b);
    let owned = b.reserve_retained(17).unwrap().into_owned(&b).unwrap();
    let control = b.control.as_ref().unwrap().clone();
    drop(b);
    assert!(weak.upgrade().is_some());
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        weak.upgrade().unwrap().check().unwrap_err().code,
        "command.cancelled"
    );
    let observe = weak.upgrade().unwrap();
    drop(owned);
    assert_eq!(observe.snapshot().unwrap().retained_bytes, 0);
    drop(observe);
    assert!(weak.upgrade().is_none());
}
