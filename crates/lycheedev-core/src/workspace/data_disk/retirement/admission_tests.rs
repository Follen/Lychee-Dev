use super::super::*;
use super::{Fixture, manifest_path, reference};

fn limit(f: &Fixture, bytes: u64) {
    let mut config = crate::workspace::default_config();
    config["dataCache"]["maxBytes"] = serde_json::json!(bytes);
    storage::atomic_json(&f.0.join("config.json"), &config).unwrap();
}
fn usage(f: &Fixture) -> u64 {
    let resources = f.resources(&OperationControl::default());
    let owner = resources.reserve_derived_cache(Default::default()).unwrap();
    let bytes = owner.report().existing.total().unwrap();
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
    bytes
}
fn peak(bytes: u64) -> AdditionalPeak {
    AdditionalPeak {
        objects: bytes,
        ..Default::default()
    }
}
fn admit(f: &Fixture, bytes: u64) -> Result<Option<DerivedRetirementReport>> {
    let resources = f.resources(&OperationControl::default());
    let result = resources
        .reserve_derived_cache_with_retirement(
            peak(bytes),
            &|| Ok(()),
            |_| Ok(()),
            reference,
            |_| {},
        )
        .and_then(|(owner, report)| owner.finish(Ok(report)));
    crate::metadata::historical_write::finish_historical(result, resources.close())
}

#[test]
fn automatic_admission_no_pressure_does_not_project_or_retire() {
    let f = Fixture::new();
    let (_, keys) = f.publish(b"no pressure", &["no-pressure"]);
    // Existing admission need not interpret manifests when no eviction occurs.
    fs::write(manifest_path(&f, &keys[0]), b"unknown future manifest").unwrap();
    assert!(admit(&f, 1).unwrap().is_none());
    assert_eq!(
        fs::read(manifest_path(&f, &keys[0])).unwrap(),
        b"unknown future manifest"
    );
}
#[test]
fn automatic_admission_reclaims_then_holds_same_quota_for_new_peak() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"cold candidate", &["cold"]);
    let before = usage(&f);
    limit(&f, before);
    let resources = f.resources(&OperationControl::default());
    let committed = Cell::new(0);
    let (owner, report) = resources
        .reserve_derived_cache_with_retirement(
            peak(1),
            &|| Ok(()),
            |_| Ok(()),
            reference,
            |_| committed.set(committed.get() + 1),
        )
        .unwrap();
    let report = report.unwrap();
    assert!(report.complete);
    assert_eq!(report.removed_objects, 1);
    assert_eq!(committed.get(), 1);
    assert!(report.receipt_allocation.is_none());
    assert!(owner.lease.is_some());
    assert_eq!(
        owner.report().admitted_peak_bytes,
        owner.report().existing.total().unwrap() + 1
    );
    assert!(owner.report().coverage.automatic_eviction_enabled);
    assert!(!owner.report().coverage.total_data_coverage_complete);
    assert!(!f.paths().object(&object.sha256).unwrap().exists());
    assert!(!manifest_path(&f, &keys[0]).exists());
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn automatic_admission_existing_overlimit_never_deletes() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"over-limit", &["over-limit"]);
    limit(&f, usage(&f) - 1);
    assert!(admit(&f, 1).is_err());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(manifest_path(&f, &keys[0]).exists());
}
#[test]
fn automatic_admission_impossible_peak_never_deletes() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"impossible", &["impossible"]);
    let before = usage(&f);
    limit(&f, before);
    assert!(admit(&f, before + 1).is_err());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(manifest_path(&f, &keys[0]).exists());
}
#[test]
fn automatic_admission_retained_other_data_impossible_never_deletes() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"retained", &["retained"]);
    fs::write(
        f.paths().exports.join("protected-export"),
        b"retained output",
    )
    .unwrap();
    let before = usage(&f);
    limit(&f, before);
    assert!(admit(&f, before).is_err());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(manifest_path(&f, &keys[0]).exists());
    assert_eq!(
        fs::read(f.paths().exports.join("protected-export")).unwrap(),
        b"retained output"
    );
}
#[test]
fn automatic_admission_malformed_manifest_blocks_all_deletions() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"malformed", &["malformed"]);
    fs::write(manifest_path(&f, &keys[0]), b"not a manifest").unwrap();
    limit(&f, usage(&f));
    assert!(admit(&f, 1).is_err());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert_eq!(
        fs::read(manifest_path(&f, &keys[0])).unwrap(),
        b"not a manifest"
    );
}
#[test]
fn automatic_admission_active_reader_refuses_growth_then_retry_reclaims() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"pinned cold", &["pinned"]);
    limit(&f, usage(&f));
    let resources = f.resources(&OperationControl::default());
    let check: DerivedReadCheck = std::sync::Arc::new(|| Ok(()));
    let reader = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap()
        .open_pinned_object(&object.sha256, object.bytes)
        .unwrap()
        .unwrap();
    let error = resources
        .reserve_derived_cache_with_retirement(peak(1), &|| Ok(()), |_| Ok(()), reference, |_| {})
        .err()
        .unwrap();
    assert_eq!(error.details["retirement"]["skippedInUse"], 1);
    assert_eq!(error.details["retirement"]["reclaimedBytes"], 0);
    assert!(!error.message.contains("existing data is preserved"));
    assert!(manifest_path(&f, &keys[0]).exists());
    reader.close().unwrap();
    resources.close().unwrap();
    assert!(admit(&f, 1).unwrap().unwrap().complete);
}
#[test]
fn automatic_admission_partial_reclaim_then_rejection_retains_facts_and_charge() {
    let f = Fixture::new();
    let (cold, _) = f.publish(b"cold group", &["cold-group"]);
    let (pinned, keys) = f.publish(b"pinned group", &["pinned-group"]);
    let before = usage(&f);
    limit(&f, before);
    let resources = f.resources(&OperationControl::default());
    let check: DerivedReadCheck = std::sync::Arc::new(|| Ok(()));
    let reader = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap()
        .open_pinned_object(&pinned.sha256, pinned.bytes)
        .unwrap()
        .unwrap();
    let committed = Cell::new(0);
    let error = resources
        .reserve_derived_cache_with_retirement(
            peak(before),
            &|| Ok(()),
            |_| Ok(()),
            reference,
            |_| committed.set(committed.get() + 1),
        )
        .err()
        .unwrap();
    assert_eq!(committed.get(), 1);
    assert_eq!(error.details["retirement"]["removedObjects"], 1);
    assert!(
        error.details["retirement"]["reclaimedBytes"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(!error.message.contains("existing data is preserved"));
    assert!(!f.paths().object(&cold.sha256).unwrap().exists());
    assert!(manifest_path(&f, &keys[0]).exists());
    reader.close().unwrap();
    resources.close().unwrap();
}

#[test]
fn automatic_admission_unknown_scope_floor_prevents_useless_deletion() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"canonical", &["canonical"]);
    let unknown = f.paths().objects.join("unknown-protected");
    fs::write(&unknown, [7u8; 4096]).unwrap();
    let before = usage(&f);
    limit(&f, before);
    assert!(admit(&f, before).is_err());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(manifest_path(&f, &keys[0]).exists());
    assert_eq!(fs::read(unknown).unwrap(), vec![7u8; 4096]);
}

#[test]
fn automatic_admission_budget_failure_before_retirement_preserves_every_file() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"budget failure", &["budget-failure"]);
    limit(&f, usage(&f));
    let resources = f.resources(&OperationControl::default());
    let error = resources
        .reserve_derived_cache_with_retirement(
            peak(1),
            &|| Ok(()),
            |_| {
                Err::<(), _>(ToolkitError::new(
                    "test.retirement_budget",
                    "denied memory",
                    3,
                ))
            },
            reference,
            |_| {},
        )
        .err()
        .unwrap();
    assert_eq!(error.code, "test.retirement_budget");
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(manifest_path(&f, &keys[0]).exists());
    resources.close().unwrap();
}
#[test]
fn automatic_admission_cancelled_original_control_preserves_every_file() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"cancelled", &["cancelled"]);
    limit(&f, usage(&f));
    let resources = f.resources(&OperationControl::default());
    let check = || {
        Err(ToolkitError::new(
            "test.cancelled",
            "original caller stopped",
            3,
        ))
    };
    let error = resources
        .reserve_derived_cache_with_retirement(peak(1), &check, |_| Ok(()), reference, |_| {})
        .err()
        .unwrap();
    assert_eq!(error.code, "test.cancelled");
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(manifest_path(&f, &keys[0]).exists());
    resources.close().unwrap();
}

#[test]
fn automatic_admission_cancellation_after_first_manifest_commits_partial_receipt_once() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"cancel between references", &["cancel-a", "cancel-b"]);
    limit(&f, usage(&f));
    let first = manifest_path(&f, &keys[0]);
    let second = manifest_path(&f, &keys[1]);
    let resources = f.resources(&OperationControl::default());
    let committed = Cell::new(0);
    let check = || {
        if !first.exists() {
            Err(ToolkitError::new(
                "test.after_manifest",
                "cancelled after first retirement",
                3,
            ))
        } else {
            Ok(())
        }
    };
    let error = resources
        .reserve_derived_cache_with_retirement(
            peak(1),
            &check,
            |_| Ok(()),
            reference,
            |_| committed.set(committed.get() + 1),
        )
        .err()
        .unwrap();
    assert_eq!(error.code, "test.after_manifest");
    assert_eq!(committed.get(), 1);
    assert_eq!(error.details["retirement"]["removedGenerations"], 1);
    assert_eq!(error.details["retirement"]["removedObjects"], 0);
    assert!(!first.exists());
    assert!(second.exists());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    resources.close().unwrap();
}
