use super::*;
use crate::workspace::{
    self,
    data_resources::{Policy, acquire},
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(limit: u64) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-data-disk-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        workspace::initialize(&home).unwrap();
        let fixture = Self(home);
        fixture.limit(limit);
        fixture
    }
    fn limit(&self, limit: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(limit);
        storage::atomic_json(&self.0.join("config.json"), &config).unwrap();
    }
    fn resources(&self, control: &OperationControl) -> DataResources {
        acquire(&self.0, Policy::Ordinary, control).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn peak(bytes: u64) -> AdditionalPeak {
    AdditionalPeak {
        objects: bytes,
        ..AdditionalPeak::default()
    }
}

#[test]
fn exact_decimal_default_and_old_generic_budget_are_independent() {
    let f = Fixture::new(workspace::DEFAULT_DATA_CACHE_MAX_BYTES);
    let control = OperationControl::default();
    let resources = f.resources(&control);
    assert_eq!(resources.data_cache_max_bytes(), 5_000_000_000);
    let owner = resources
        .reserve_derived_cache(peak(5_000_000_000))
        .unwrap();
    assert_eq!(owner.report().admitted_peak_bytes, 5_000_000_000);
    assert!(!owner.report().coverage.total_data_coverage_complete);
    assert!(!owner.report().coverage.automatic_eviction_enabled);
    owner.finish(Ok(())).unwrap();
    assert_eq!(
        resources
            .reserve_derived_cache(peak(5_000_000_001))
            .err()
            .unwrap()
            .code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(
        workspace::config(&f.0).unwrap()["cache"]["maxBytes"],
        20u64 << 30
    );
    resources.close().unwrap();
}
#[test]
fn each_simultaneous_category_charges_checked_peak_and_overflow_is_rejected() {
    let f = Fixture::new(15);
    let resources = f.resources(&OperationControl::default());
    let requested = AdditionalPeak {
        objects: 1,
        generations: 2,
        staging: 3,
        spill: 4,
        export_temporary: 5,
    };
    let owner = resources.reserve_derived_cache(requested).unwrap();
    assert_eq!(owner.report().admitted_peak_bytes, 15);
    owner.finish(Ok(())).unwrap();
    assert_eq!(
        resources
            .reserve_derived_cache(AdditionalPeak {
                spill: 5,
                ..requested
            })
            .err()
            .unwrap()
            .code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(
        AdditionalPeak {
            objects: u64::MAX,
            staging: 1,
            ..AdditionalPeak::default()
        }
        .total()
        .unwrap_err()
        .code,
        "vault.data_cache_size_overflow"
    );
    resources.close().unwrap();
}
#[test]
fn current_reserved_bytes_replace_allowance_and_remaining_files_charge_next_owner() {
    let f = Fixture::new(10);
    let resources = f.resources(&OperationControl::default());
    let owner = resources.reserve_derived_cache(peak(10)).unwrap();
    let path = owner.paths().staging.join("fixture-owned-stage");
    fs::write(&path, b"0123456789").unwrap();
    assert_eq!(owner.verify_usage().unwrap().total().unwrap(), 10);
    assert_eq!(owner.report().admitted_peak_bytes, 10); // Not 10 actual + 10 reserved.
    owner.finish(Ok(())).unwrap();
    assert_eq!(
        resources.reserve_derived_cache(peak(1)).err().unwrap().code,
        "vault.data_cache_budget_exceeded"
    );
    let owner = resources.reserve_derived_cache(peak(0)).unwrap();
    assert_eq!(owner.report().existing.temporary_namespace_bytes, 10);
    owner.finish(Ok(())).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"0123456789");
    resources.close().unwrap();
}
#[test]
fn evidence_and_existing_generic_raw_paths_are_preserved_and_explicitly_unattributed() {
    let f = Fixture::new(1);
    let paths = [
        "blobs/evidence-body",
        "cache/objects/legacy-body",
        "indexes/rawsource/legacy",
        "tmp/legacy-spool",
    ];
    for path in paths {
        let path = f.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"protected existing bytes").unwrap();
    }
    let resources = f.resources(&OperationControl::default());
    let owner = resources.reserve_derived_cache(peak(1)).unwrap();
    assert_eq!(owner.report().existing.total().unwrap(), 0);
    assert_eq!(owner.report().coverage.not_yet_attributed.len(), 4);
    owner.finish(Ok(())).unwrap();
    assert!(resources.reserve_derived_cache(peak(2)).is_err());
    for path in paths {
        assert_eq!(
            fs::read(f.0.join(path)).unwrap(),
            b"protected existing bytes"
        );
    }
    resources.close().unwrap();
}
#[test]
fn generic_cache_enumeration_and_prune_inputs_do_not_adopt_data_roots() {
    let f = Fixture::new(32);
    let resources = f.resources(&OperationControl::default());
    let owner = resources.reserve_derived_cache(peak(4)).unwrap();
    let body = owner.paths().object(&storage::sha256(b"body")).unwrap();
    fs::create_dir_all(body.parent().unwrap()).unwrap();
    fs::write(&body, b"body").unwrap();
    owner.finish(Ok(())).unwrap();
    let inv = crate::Invocation {
        home: f.0.clone(),
        command: "cache status".into(),
        positional: None,
        flags: Default::default(),
        cwd: f.0.clone(),
        control: OperationControl::default(),
    };
    let status = crate::cache::dispatch(&inv).unwrap();
    assert_eq!(status.data["objectBytes"], 0);
    let inv = crate::Invocation {
        home: f.0.clone(),
        command: "cache prune".into(),
        positional: None,
        flags: [("--uncommitted".into(), "true".into())].into(),
        cwd: f.0.clone(),
        control: OperationControl::default(),
    };
    crate::cache::dispatch(&inv).unwrap();
    assert_eq!(fs::read(body).unwrap(), b"body");
    resources.close().unwrap();
}
#[test]
fn snapshot_independent_content_path_and_invalid_keys() {
    let paths = DataCachePaths::new(Path::new("home"));
    let digest = storage::sha256(b"one physical content");
    assert_eq!(
        paths.object(&digest).unwrap(),
        Path::new("home/cache/data-derived-v1/objects")
            .join(&digest[..2])
            .join(&digest[2..])
    );
    assert_eq!(
        paths.object(&digest).unwrap(),
        paths.clone().object(&digest).unwrap()
    );
    for bad in [
        "../escape".to_owned(),
        "a".repeat(63),
        digest.to_uppercase(),
    ] {
        assert!(paths.object(&bad).is_err());
    }
}
#[test]
fn existing_overage_and_live_config_shrink_reject_new_work_without_deleting() {
    let f = Fixture::new(8);
    let resources = f.resources(&OperationControl::default());
    let owner = resources.reserve_derived_cache(peak(8)).unwrap();
    let file = owner.paths().generations.join("unknown-generation");
    fs::write(&file, b"12345678").unwrap();
    owner.finish(Ok(())).unwrap();
    f.limit(7);
    let error = resources.reserve_derived_cache(peak(0)).err().unwrap();
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    assert_eq!(error.details["reservation"]["configuredMaxBytes"], 7);
    assert_eq!(fs::read(file).unwrap(), b"12345678");
    resources.close().unwrap();
}
#[test]
fn writer_overrun_is_reported_and_files_remain_for_explicit_recovery() {
    let f = Fixture::new(10);
    let resources = f.resources(&OperationControl::default());
    let owner = resources.reserve_derived_cache(peak(3)).unwrap();
    let path = owner.paths().spill.join("unknown-spill");
    fs::write(&path, b"four").unwrap();
    let error = owner.verify_usage().unwrap_err();
    assert_eq!(error.code, "vault.data_cache_reservation_exceeded");
    assert_eq!(
        owner.finish::<()>(Err(error)).unwrap_err().code,
        "vault.data_cache_reservation_exceeded"
    );
    assert_eq!(fs::read(path).unwrap(), b"four");
    resources.close().unwrap();
}
#[test]
fn cancellation_releases_quota_before_reporting_original_stop() {
    let f = Fixture::new(10);
    let control = OperationControl::default();
    let resources = f.resources(&control);
    let owner = resources.reserve_derived_cache(peak(10)).unwrap();
    control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(owner.finish(Ok(())).unwrap_err().code, "command.cancelled");
    let lease = ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE).unwrap();
    lease.close().unwrap();
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}
#[test]
fn original_cancel_before_reservation_does_not_prepare_namespaces() {
    let f = Fixture::new(10);
    let control = OperationControl::default();
    let resources = f.resources(&control);
    control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(
        resources
            .reserve_derived_cache(peak(10))
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    for root in resources.data_cache_paths().roots() {
        assert!(!root.exists());
    }
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}
#[test]
fn held_quota_blocks_second_writer_then_drop_releases_without_deletion() {
    let f = Fixture::new(10);
    let first = f.resources(&OperationControl::default());
    let owner = first.reserve_derived_cache(peak(10)).unwrap();
    let second = f.resources(&OperationControl::uncancelable(
        Instant::now() + Duration::from_millis(60),
    ));
    assert_eq!(
        second.reserve_derived_cache(peak(1)).err().unwrap().code,
        "command.deadline"
    );
    assert_eq!(second.close().unwrap_err().code, "command.deadline");
    let path = owner.paths().spill.join("retain-on-drop");
    fs::write(&path, b"keep").unwrap();
    drop(owner);
    let owner = first.reserve_derived_cache(peak(6)).unwrap();
    assert_eq!(owner.report().existing.total().unwrap(), 4);
    owner.finish(Ok(())).unwrap();
    assert_eq!(fs::read(path).unwrap(), b"keep");
    first.close().unwrap();
}
#[test]
fn checked_cleanup_preserves_repeated_close_events_and_consumes_owners_once() {
    let f = Fixture::new(10);
    let control = OperationControl::default();
    let resources = f.resources(&control);
    let mut owner = resources.reserve_derived_cache(peak(1)).unwrap();
    let home = std::path::absolute(&f.0).unwrap();
    let expected_directories = [
        "locks",
        "cache/data-derived-v1",
        "indexes/data-derived-v1",
        "tmp/data-derived-v1",
        "state/data",
        "cache/data-derived-v1/objects",
        "indexes/data-derived-v1/generations",
        "tmp/data-derived-v1/staging",
        "tmp/data-derived-v1/spill",
        "tmp/data-derived-v1/exports",
        "indexes/data-derived-v1/shared-blobs-v1",
        "indexes/data-derived-v1/recency-v1",
    ]
    .map(|path| home.join(path));
    assert_eq!(
        owner
            .directories
            .iter()
            .map(|d| d.path())
            .collect::<Vec<_>>(),
        expected_directories
            .iter()
            .map(PathBuf::as_path)
            .collect::<Vec<_>>()
    );
    control.cancel_handle().store(true, Ordering::Release);
    let mut directory_calls = 0;
    let mut lease_calls = 0;
    let mut closed_directories = Vec::new();
    let lease_closed = Cell::new(false);
    let cleanup = owner.cleanup_with(
        |directory| {
            directory_calls += 1;
            // Quota outlives all managed roots, then closes before its parent.
            assert_eq!(
                lease_closed.get(),
                directory.path() == expected_directories[0]
            );
            closed_directories.push(directory.path().to_owned());
            assert!(close_directory(directory).is_empty());
            vec![ToolkitError::new("test.close", "same close event", 5)]
        },
        |lease| {
            lease_calls += 1;
            assert!(!lease_closed.replace(true));
            lease.close()?;
            Err(ToolkitError::new("test.close", "same close event", 5))
        },
    );
    // One locks parent, four roots (including private metadata), and seven children.
    assert_eq!(directory_calls, expected_directories.len());
    assert_eq!(
        closed_directories,
        expected_directories[1..]
            .iter()
            .rev()
            .chain(&expected_directories[..1])
            .cloned()
            .collect::<Vec<_>>()
    );
    assert_eq!(lease_calls, 1);
    assert!(lease_closed.get());
    assert_eq!(cleanup.len(), expected_directories.len() + 1);
    assert!(owner.directories.is_empty());
    assert!(owner.lease.is_none());
    assert!(owner.cleanup().is_empty());
    let primary = ToolkitError::new("test.work", "operation failure", 4);
    let error = join::<()>(
        Err(primary),
        cleanup,
        &Checks {
            control: &control,
            supplemental: None,
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "test.work");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), expected_directories.len() + 3);
    assert_eq!(causes[0]["code"], "test.work");
    for cause in &causes[1..causes.len() - 1] {
        assert_eq!(cause["code"], "test.close");
        assert_eq!(cause["message"], "same close event");
    }
    assert_eq!(causes.last().unwrap()["code"], "command.cancelled");
    drop(owner);
    ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}

#[test]
fn independent_budget_interrupts_held_quota_wait_without_releasing_other_owner() {
    let f = Fixture::new(16);
    let first = f.resources(&OperationControl::default());
    let second = f.resources(&OperationControl::default());
    let held = first.reserve_derived_cache(peak(16)).unwrap();
    let contended = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = contended.clone();
    ExclusiveLock::set_blob_wait_observer(move || observed.set(true));
    let check = || {
        if contended.get() {
            Err(ToolkitError::new(
                "test.wait_budget",
                "independent wait budget",
                3,
            ))
        } else {
            Ok(())
        }
    };
    assert_eq!(
        second
            .reserve_derived_cache_with_check(peak(1), &check)
            .err()
            .unwrap()
            .code,
        "test.wait_budget"
    );
    assert!(
        contended.get(),
        "budget stop must follow actual quota contention"
    );
    assert!(ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE).is_err());
    held.finish(Ok(())).unwrap();
    second
        .reserve_derived_cache(peak(16))
        .unwrap()
        .finish(Ok(()))
        .unwrap();
    second.close().unwrap();
    first.close().unwrap();
}
