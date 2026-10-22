use super::*;
use crate::{fault, resource::Cost};
use lycheedev_core::{control::TrackedCancellation, storage};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
fn home() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/remote-admitted-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    lycheedev_core::workspace::initialize(&home).unwrap();
    home
}
#[test]
fn admitted_workspace_holds_root_and_real_slots_without_preparing_metadata_or_cdn() {
    let home = home();
    let control = OperationControl::default();
    let owner = OpenWorkspace::open(&home, &control, None)
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    assert_eq!(owner.home(), std::path::absolute(&home).unwrap());
    assert_eq!(owner.download_workers(), 8);
    assert!(!home.join("state/toolkit.sqlite").exists());
    fs::write(home.join("own-metadata-update"), b"allowed").unwrap();
    let moved = home.with_extension("moved");
    assert!(fs::rename(&home, &moved).is_err());
    owner.finish(Ok(())).unwrap();
    fs::rename(&home, &moved).unwrap();
    fs::rename(&moved, &home).unwrap();
    OpenWorkspace::open(&home, &control, None)
        .unwrap()
        .acquire(Policy::Measurement)
        .unwrap()
        .finish(Ok(()))
        .unwrap();
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn failed_admission_and_independent_budget_stop_release_root_and_slots() {
    let home = home();
    let control = OperationControl::default();
    let token = TrackedCancellation::new();
    let independent =
        OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    let budget = Budget::controlled(Cost::defaults(), independent.deadline(), independent);
    let owner = OpenWorkspace::open(&home, &control, Some(&budget))
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    token.cancel();
    assert_eq!(owner.check().unwrap_err().code, "command.cancelled");
    assert_eq!(owner.finish(Ok(())).unwrap_err().code, "command.cancelled");
    let moved = home.with_extension("moved");
    fs::rename(&home, &moved).unwrap();
    fs::rename(&moved, &home).unwrap();
    OpenWorkspace::open(&home, &control, None)
        .unwrap()
        .acquire(Policy::Measurement)
        .unwrap()
        .finish(Ok(()))
        .unwrap();
    fs::write(home.join("config.json"), b"invalid json").unwrap();
    assert!(
        OpenWorkspace::open(&home, &control, None)
            .and_then(|workspace| workspace.acquire(Policy::Ordinary))
            .is_err()
    );
    fs::rename(&home, &moved).unwrap();
    fs::rename(&moved, &home).unwrap();
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn admitted_finish_preserves_each_cleanup_event_and_deduplicates_stop_observations() {
    let primary = fault("test.operation", 5, "operation");
    let close = fault("test.close", 5, "close");
    let stop = fault("command.cancelled", 7, "cancelled");
    let error = finish::<()>(
        Err(primary),
        [close.clone(), close],
        Err(stop.clone()),
        Err(stop),
    )
    .unwrap_err();
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 4);
    assert_eq!(causes[1], causes[2]);
    assert_eq!(causes[3]["code"], "command.cancelled");
}

#[test]
fn opened_metadata_is_idle_and_nested_writers_do_not_deadlock_while_admitted() {
    let home = home();
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let opened = OpenWorkspace::open_metadata(&home, &control, Some(&budget)).unwrap();
    assert!(home.join("state/toolkit.sqlite").exists());
    // Merely opening Metadata does not grant a Data slot or block another owner.
    data_resources::acquire(&home, Policy::Measurement, &control)
        .unwrap()
        .close()
        .unwrap();
    let owner = opened.acquire(Policy::Ordinary).unwrap();
    let mut nested =
        Metadata::open_controlled_with_check(&home, true, &control, || budget.check()).unwrap();
    nested
        .create_controlled_with_check("test/outer-idle", &true, &control, || budget.check())
        .unwrap();
    nested.close().unwrap();
    let read = Metadata::read_document_raw_controlled_with_check(
        &home,
        "test/outer-idle",
        &control,
        || budget.check(),
    )
    .unwrap();
    assert_eq!(read.bytes.as_slice(), b"true");
    owner.finish(Ok(())).unwrap();
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn metadata_open_failure_precedes_busy_admission_and_checked_teardown_frees_root() {
    let home = home();
    let control = OperationControl::default();
    let held = data_resources::acquire(&home, Policy::Measurement, &control).unwrap();
    fs::create_dir_all(home.join("state")).unwrap();
    fs::write(home.join("state/toolkit.sqlite"), b"not a sqlite database").unwrap();
    let expected = Metadata::open_controlled(&home, true, &control)
        .err()
        .expect("corrupt SQLite must fail");
    let error = OpenWorkspace::open_metadata(&home, &control, None)
        .err()
        .expect("Metadata must fail before lease wait");
    assert_eq!(error.code, expected.code);
    assert_eq!(error.message, expected.message);
    control.check().unwrap();
    held.close().unwrap();
    let moved = home.with_extension("released");
    fs::rename(&home, &moved).unwrap();
    fs::remove_dir_all(moved).unwrap();
}

#[test]
fn independent_budget_deadline_bounds_open_with_a_held_metadata_schema_lease() {
    let home = home();
    let held = storage::ExclusiveLock::resource(&home.join("locks"), "metadata-schema").unwrap();
    let control = OperationControl::default();
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_millis(50),
        None,
    );
    let error = OpenWorkspace::open_metadata(&home, &control, Some(&budget))
        .err()
        .expect("schema lease is held");
    control.check().unwrap();
    let stop = budget.check().unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        (stop.code.as_str(), stop.exit_code)
    );
    assert!(!home.join("state/toolkit.sqlite").exists());
    // The waiter does not release a different owner's schema lock.
    assert!(storage::ExclusiveLock::resource(&home.join("locks"), "metadata-schema").is_err());
    held.close().unwrap();
    OpenWorkspace::open_metadata(&home, &control, None)
        .unwrap()
        .finish(Ok(()))
        .unwrap();
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn open_then_failed_acquire_keeps_each_typed_cleanup_cause_in_reverse_order() {
    // Fault injection at the existing aggregation seam; actual owner-release
    // coverage is separate. Equal values from different closes remain events.
    let primary = fault("test.operation", 5, "operation");
    let lease = fault("test.lease", 5, "lease");
    let metadata = fault("test.metadata", 5, "metadata");
    let directory = fault("test.directory", 5, "directory");
    let stop = fault("command.cancelled", 7, "stopped");
    let error = finish::<()>(
        Err(primary.clone()),
        [lease.clone(), metadata.clone(), directory.clone()],
        Err(stop.clone()),
        Err(stop.clone()),
    )
    .unwrap_err();
    assert_eq!(
        error.details["causes"],
        serde_json::json!([primary, lease, metadata, directory, stop])
    );
}

#[test]
fn stop_after_metadata_open_closes_database_and_directory_before_failed_acquire_returns() {
    let home = home();
    let control = OperationControl::default();
    let held = data_resources::acquire(&home, Policy::Measurement, &control).unwrap();
    let short = OperationControl::default();
    let opened = OpenWorkspace::open_metadata(&home, &short, None).unwrap();
    short.tracked_cancel_handle().unwrap().cancel();
    let error = opened
        .acquire(Policy::Ordinary)
        .err()
        .expect("original stop precedes the held resource wait");
    assert_eq!(error.code, short.check().unwrap_err().code);
    held.close().unwrap();
    let moved = home.with_extension("after-failed-acquire");
    fs::rename(&home, &moved).unwrap();
    // Both the held root and database handle must be closed for Windows rename.
    fs::rename(
        moved.join("state/toolkit.sqlite"),
        moved.join("state/closed.sqlite"),
    )
    .unwrap();
    fs::remove_dir_all(moved).unwrap();
}

#[test]
fn actual_admitted_cancel_with_successful_cleanup_returns_one_original_stop() {
    let home = home();
    let control = OperationControl::default();
    let owner = OpenWorkspace::open_metadata(&home, &control, None)
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    let original = control.check().unwrap_err();
    let error = owner.finish::<()>(Err(original.clone())).unwrap_err();
    assert!(same(&error, &original));
    assert!(
        error.details.is_null(),
        "successful resource closes cannot manufacture another stop event"
    );
    let fresh = OperationControl::default();
    data_resources::acquire(&home, Policy::Measurement, &fresh)
        .unwrap()
        .close()
        .unwrap();
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn actual_admitted_cleanup_retains_distinct_original_and_independent_budget_stops() {
    let home = home();
    let control = OperationControl::default();
    let cancelled = TrackedCancellation::new();
    let budget = Budget::new(
        Cost::defaults(),
        control.deadline(),
        Some(cancelled.clone()),
    );
    let owner = OpenWorkspace::open_metadata(&home, &control, Some(&budget))
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    cancelled.cancel();
    let original = control.check().unwrap_err();
    let independent = budget.check().unwrap_err();
    let error = owner.finish::<()>(Err(original.clone())).unwrap_err();
    assert_eq!(
        error.details["causes"],
        serde_json::json!([original, independent])
    );
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn adaptive_query_observes_only_after_actual_queued_admission() {
    let home = home();
    let original = OperationControl::default();
    let held = data_resources::acquire(&home, Policy::Measurement, &original).unwrap();
    let calls = std::sync::Arc::new(AtomicU64::new(0));
    let available = std::sync::Arc::new(AtomicU64::new(4 << 30));
    let budget = Budget::query_with_probe(original.deadline(), original.clone(), {
        let calls = calls.clone();
        let available = available.clone();
        std::sync::Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(available.load(Ordering::SeqCst))
        })
    });
    // This tests admission, not a complete request finishing within 50 ms.
    // Prepare the real owner (including the first durable SQLite schema commit)
    // before starting the unchanged short deadline for the blocked acquire.
    let opened = OpenWorkspace::open_metadata(&home, &original, Some(&budget)).unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    let opened = OpenWorkspace {
        control: &short,
        ..opened
    };
    let error = opened
        .acquire(Policy::Ordinary)
        .err()
        .expect("held measurement blocks admission");
    assert_eq!(error.code, "command.deadline");
    assert_eq!(error.exit_code, 5);
    assert_eq!(short.check().unwrap_err().code, "command.deadline");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // The failed waiter must not release the other owner's real kernel lock.
    let still_held = storage::ExclusiveLock::resource(
        &home.join("locks/data-resources-v1"),
        "admission",
    )
    .err()
    .expect("measurement still owns the admission gate");
    assert_eq!(still_held.code, "workspace.busy");
    original.check().unwrap();
    available.store(512 << 20, Ordering::SeqCst);
    held.close().unwrap();
    let owner = OpenWorkspace::open_metadata(&home, &original, Some(&budget))
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    assert!(std::ptr::eq(owner.budget().unwrap(), &budget));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(budget.memory_snapshot().unwrap().ceiling_bytes, 128 << 20);
    owner.finish(Ok(())).unwrap();
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn adaptive_probe_runs_under_real_owner_and_reuse_never_reacquires() {
    let home = home();
    let control = OperationControl::default();
    let calls = std::sync::Arc::new(AtomicU64::new(0));
    let budget = Budget::query_with_probe(control.deadline(), control.clone(), {
        let home = home.clone();
        let calls = calls.clone();
        std::sync::Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
            let error = data_resources::acquire(&home, Policy::Measurement, &short)
                .err()
                .expect("memory observation requires an already-held real slot");
            assert_eq!(error.code, "command.deadline");
            Ok(4 << 30)
        })
    });
    let owner = OpenWorkspace::open_metadata(&home, &control, Some(&budget))
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    budget
        .initialize_admitted(owner.resources.as_ref().unwrap())
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    budget.charge(Cost::retained(1)).unwrap();
    assert_eq!(
        owner
            .budget()
            .unwrap()
            .logical_snapshot()
            .unwrap()
            .retained_bytes,
        1
    );
    owner.finish(Ok(())).unwrap();
    data_resources::acquire(&home, Policy::Measurement, &control)
        .unwrap()
        .close()
        .unwrap();
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn failed_adaptive_activation_closes_real_slots_metadata_and_root() {
    for fail_probe in [false, true] {
        let home = home();
        let control = OperationControl::default();
        let budget = Budget::query_with_probe(
            control.deadline(),
            control.clone(),
            std::sync::Arc::new(move || {
                if fail_probe {
                    Err(fault("test.memory_status", 5, "probe unavailable"))
                } else {
                    Ok(128 << 20)
                }
            }),
        );
        budget
            .safety_only()
            .charge(Cost::retained(64 << 20))
            .unwrap();
        let error = OpenWorkspace::open_metadata(&home, &control, Some(&budget))
            .unwrap()
            .acquire(Policy::Ordinary)
            .err()
            .expect("fresh ceiling/probe failure denies activation");
        assert_eq!(error.code, "records.query_resource_budget");
        if fail_probe {
            assert_eq!(error.details["cause"]["code"], "test.memory_status");
        }
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 64 << 20);
        assert!(budget.memory_snapshot().is_err());
        control.check().unwrap();
        data_resources::acquire(&home, Policy::Measurement, &control)
            .unwrap()
            .close()
            .unwrap();
        let moved = home.with_extension("failed-adaptive-closed");
        fs::rename(&home, &moved).unwrap();
        fs::rename(
            moved.join("state/toolkit.sqlite"),
            moved.join("state/closed.sqlite"),
        )
        .unwrap();
        fs::remove_dir_all(moved).unwrap();
    }
}

#[test]
fn adaptive_activation_preserves_independent_budget_deadline_before_probe() {
    let home = home();
    let original = OperationControl::default();
    let calls = std::sync::Arc::new(AtomicU64::new(0));
    let budget = Budget::query_with_probe(Instant::now(), original.clone(), {
        let calls = calls.clone();
        std::sync::Arc::new(move || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(4 << 30)
        })
    });
    let error = OpenWorkspace::open_metadata(&home, &original, Some(&budget))
        .err()
        .unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    original.check().unwrap();
    data_resources::acquire(&home, Policy::Measurement, &original)
        .unwrap()
        .close()
        .unwrap();
    fs::remove_dir_all(home).unwrap();
}

// Go: TestQueryAdmissionSharesBudgetAndPreservesLeaseOwner, using the actual
// lazy Rust Session and real workspace locks, with no network or fake session.
#[test]
fn adaptive_nested_session_shares_budget_and_preserves_outer_lease() {
    use crate::remote::session::{Options, Session};
    use lycheedev_core::selection::DataPin;
    use std::sync::Arc;
    let home = home();
    let control = OperationControl::default();
    let mut config = lycheedev_core::workspace::default_config();
    config["download"]["workers"] = serde_json::json!(1);
    storage::atomic_json(&home.join("config.json"), &config).unwrap();
    let pin = DataPin {
        product: "retail".into(),
        region: "cn".into(),
        language: "zhCN".into(),
        full_build: "12.1.5.70000".into(),
        build_config: "a".repeat(32),
        cdn_config: "b".repeat(32),
        definition_commit: "c".repeat(40),
    };
    let options = || Options {
        offline: true,
        key_file: None,
        variant: String::new(),
        metadata_bytes: 512 << 20,
        content_bytes: 128 << 20,
    };
    let budget = Arc::new(Budget::query_with_probe(
        control.deadline(),
        control.clone(),
        Arc::new(|| Ok(4 << 30)),
    ));
    let mut owner = OpenWorkspace::open(&home, &control, Some(&budget))
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    assert_eq!(budget.memory_snapshot().unwrap().concurrent_queries, 1);
    assert!(std::ptr::eq(owner.budget().unwrap(), budget.as_ref()));
    Session::borrowed(&mut owner, &pin, options(), &budget)
        .unwrap()
        .close()
        .unwrap();
    let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    assert_eq!(
        data_resources::acquire(&home, Policy::Ordinary, &short)
            .err()
            .unwrap()
            .code,
        "command.deadline"
    );
    owner.finish(Ok(())).unwrap();
    let fixed = Arc::new(Budget::controlled(
        Cost {
            retained_bytes: 4,
            metadata_bytes: 4,
            ..Cost::default()
        },
        control.deadline(),
        control.clone(),
    ));
    let mut owner = OpenWorkspace::open(&home, &control, Some(&fixed))
        .unwrap()
        .acquire(Policy::Ordinary)
        .unwrap();
    Session::borrowed(&mut owner, &pin, options(), &fixed)
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(fixed.memory_snapshot().unwrap().mode, "fixed");
    assert_eq!(fixed.memory_snapshot().unwrap().retained_bytes, 4);
    owner.finish(Ok(())).unwrap();
    data_resources::acquire(&home, Policy::Measurement, &control)
        .unwrap()
        .close()
        .unwrap();
    fs::remove_dir_all(home).unwrap();
}
