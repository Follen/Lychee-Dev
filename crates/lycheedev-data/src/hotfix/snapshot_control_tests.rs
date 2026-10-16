//! Effective SQL uses the same XFTH decoder with its caller's extra control.
use super::*;
use lycheedev_core::control::OperationControl;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

fn version8() -> Vec<u8> {
    let mut raw = b"XFTH".to_vec();
    raw.extend(8u32.to_le_bytes());
    raw.extend(69933u32.to_le_bytes());
    raw.resize(44, 0);
    raw.extend(b"XFTH");
    raw.resize(72, 0);
    raw
}
#[test]
fn snapshot_layout_original_stop_precedes_format_and_independent_budget() {
    let control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() - Duration::from_secs(1),
        None,
    );
    assert_eq!(
        cache_layout_with_check(b"invalid", 0, Some(&budget), || control.check())
            .unwrap_err()
            .code,
        "command.deadline"
    );
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
}
#[test]
fn snapshot_walk_checks_before_charging_or_delivering_each_record() {
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    );
    let seen = Cell::new(0);
    let error = walk_with_check(
        b"bad record",
        0,
        9,
        Some(&budget),
        || Err(fault("fixture.original_stop", 7, "stop before record")),
        |_| {
            seen.set(seen.get() + 1);
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.original_stop");
    assert_eq!(seen.get(), 0);
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
}
#[test]
fn snapshot_v8_both_layout_probes_share_original_check_and_never_hide_stop() {
    let calls = Cell::new(0);
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    );
    let error = cache_layout_with_check(&version8(), 69933, Some(&budget), || {
        calls.set(calls.get() + 1);
        if calls.get() >= 2 {
            Err(fault("fixture.original_stop", 7, "original layout stop"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "fixture.original_stop");
    assert_eq!(
        calls.get(),
        3,
        "Both existing layout probes retain their original order"
    );
    assert_eq!(budget.snapshot().unwrap(), Cost::default());
}
#[test]
fn snapshot_layout_noop_check_preserves_legacy_result_and_work() {
    let a = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    );
    let b = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    );
    let raw = version8();
    assert_eq!(
        cache_layout(&raw, 69933, Some(&a)).map_err(|e| e.code),
        cache_layout_with_check(&raw, 69933, Some(&b), || Ok(())).map_err(|e| e.code)
    );
    assert_eq!(a.snapshot().unwrap(), b.snapshot().unwrap());
}
