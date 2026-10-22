use super::*;
use lycheedev_core::control::TrackedCancellation;
use std::time::Duration;

fn budget() -> Budget {
    let budget = Budget::new(
        Cost {
            retained_bytes: 10,
            metadata_bytes: 20,
            decode_work: 30,
            network_requests: 40,
            network_bytes: 50,
        },
        Instant::now() + Duration::from_secs(30),
        None,
    );
    budget
        .charge(Cost {
            retained_bytes: 5,
            metadata_bytes: 6,
            decode_work: 7,
            network_requests: 8,
            network_bytes: 9,
        })
        .unwrap();
    budget
}
fn diagnostic(
    error: lycheedev_core::ToolkitError,
    dimension: &str,
    used: u64,
    requested: u64,
    limit: u64,
) {
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert_eq!(
        error.message,
        format!(
            "records.query_resource_budget: {dimension} (used={used} requested={requested} limit={limit})"
        )
    );
}
#[test]
fn fixed_rejections_report_all_five_dimensions_without_partial_charges() {
    let b = budget();
    let before = (b.snapshot().unwrap(), b.logical_snapshot().unwrap());
    for (cost, name, used, requested, limit) in [
        (
            Cost {
                retained_bytes: 6,
                ..Cost::default()
            },
            "retained_bytes",
            5,
            6,
            10,
        ),
        (
            Cost {
                retained_bytes: 1,
                metadata_bytes: 15,
                ..Cost::default()
            },
            "metadata_bytes",
            6,
            15,
            20,
        ),
        (
            Cost {
                retained_bytes: 1,
                metadata_bytes: 1,
                decode_work: 24,
                ..Cost::default()
            },
            "decode_work",
            7,
            24,
            30,
        ),
        (
            Cost {
                retained_bytes: 1,
                metadata_bytes: 1,
                decode_work: 1,
                network_requests: 33,
                ..Cost::default()
            },
            "network_requests",
            8,
            33,
            40,
        ),
        (
            Cost {
                retained_bytes: 1,
                metadata_bytes: 1,
                decode_work: 1,
                network_requests: 1,
                network_bytes: 42,
            },
            "network_bytes",
            9,
            42,
            50,
        ),
        (
            Cost {
                network_bytes: u64::MAX,
                ..Cost::default()
            },
            "network_bytes",
            9,
            u64::MAX,
            50,
        ),
    ] {
        diagnostic(b.charge(cost).unwrap_err(), name, used, requested, limit);
        assert_eq!(
            (b.snapshot().unwrap(), b.logical_snapshot().unwrap()),
            before
        );
    }
}
#[test]
fn scratch_rejections_report_live_retention_without_committing_growth() {
    let b = budget();
    let scratch = b.reserve_logical_scratch(3).unwrap();
    let before = (
        b.snapshot().unwrap(),
        b.logical_snapshot().unwrap(),
        b.memory_snapshot().unwrap(),
    );
    diagnostic(
        b.reserve_logical_scratch(3).err().unwrap(),
        "scratch_bytes",
        8,
        3,
        10,
    );
    diagnostic(
        b.charge(Cost::retained(3)).unwrap_err(),
        "retained_bytes",
        8,
        3,
        10,
    );
    assert_eq!(
        (
            b.snapshot().unwrap(),
            b.logical_snapshot().unwrap(),
            b.memory_snapshot().unwrap()
        ),
        before
    );
    drop(scratch);
    assert_eq!(b.remaining_retained().unwrap(), 5);
}
#[test]
fn safety_rejection_reports_its_ledger_and_does_not_commit_logical_charge() {
    let b = budget();
    let before = (b.snapshot().unwrap(), b.logical_snapshot().unwrap());
    diagnostic(
        b.charge_split(Cost::retained(1), Cost::retained(6))
            .unwrap_err(),
        "retained_bytes",
        5,
        6,
        10,
    );
    assert_eq!(
        (b.snapshot().unwrap(), b.logical_snapshot().unwrap()),
        before
    );
    diagnostic(
        b.safety_only().charge(Cost::retained(6)).unwrap_err(),
        "retained_bytes",
        5,
        6,
        10,
    );
    assert_eq!(
        (b.snapshot().unwrap(), b.logical_snapshot().unwrap()),
        before
    );
}
#[test]
fn observed_network_rejection_retains_accounting_and_diagnostic_after_cancel() {
    let cancelled = TrackedCancellation::new();
    let b = Budget::new(
        Cost {
            network_bytes: u64::MAX,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        Some(cancelled.clone()),
    );
    b.charge(Cost {
        network_bytes: u64::MAX,
        ..Cost::default()
    })
    .unwrap();
    let before = (b.snapshot().unwrap(), b.logical_snapshot().unwrap());
    cancelled.cancel();
    assert_eq!(
        b.charge(Cost::retained(1)).unwrap_err().code,
        "operation.cancelled"
    );
    diagnostic(
        b.charge_observed_network_bytes(1).unwrap_err(),
        "network_bytes",
        u64::MAX,
        1,
        u64::MAX,
    );
    assert_eq!(
        (b.snapshot().unwrap(), b.logical_snapshot().unwrap()),
        before
    );
}
