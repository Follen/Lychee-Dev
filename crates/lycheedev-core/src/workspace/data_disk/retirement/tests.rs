use super::*;
use crate::{
    evidence,
    metadata::Metadata,
    workspace::{
        self,
        data_resources::{Policy, acquire},
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-derived-retirement-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&root).unwrap();
        Self(root)
    }
    fn resources(&self, control: &OperationControl) -> DataResources {
        acquire(&self.0, Policy::Ordinary, control).unwrap()
    }
    fn paths(&self) -> DataCachePaths {
        DataCachePaths::new(&self.0)
    }
    fn publish(&self, raw: &[u8], labels: &[&str]) -> (DerivedObject, Vec<String>) {
        let resources = self.resources(&OperationControl::default());
        let owner = resources
            .reserve_derived_cache(AdditionalPeak {
                objects: raw.len() as u64,
                staging: raw.len() as u64,
                generations: 4096,
                ..Default::default()
            })
            .unwrap();
        let mut writer = owner.begin_object(raw.len() as u64).unwrap();
        writer.write_bytes(raw).unwrap();
        let object = writer.commit().unwrap();
        let bytes = serde_json::to_vec(&object).unwrap();
        let mut keys = Vec::new();
        for label in labels {
            let key = storage::sha256(label.as_bytes());
            owner.publish_generation(&key, &bytes).unwrap();
            keys.push(key);
        }
        owner.finish(Ok(())).unwrap();
        resources.close().unwrap();
        keys.sort();
        (object, keys)
    }
    fn prune(&self, options: DerivedRetirementOptions) -> Result<DerivedRetirementReport> {
        let resources = self.resources(&OperationControl::default());
        let result =
            resources.retire_derived_cache_with_check(options, &|| Ok(()), |_| Ok(()), reference);
        let cleanup = resources.close();
        crate::metadata::historical_write::finish_historical(result, cleanup)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn reference(_: &str, bytes: &[u8]) -> Result<DerivedObject> {
    let value: evidence::BlobRef = serde_json::from_slice(bytes)?;
    Ok(DerivedObject {
        sha256: value.sha256,
        bytes: value.bytes,
    })
}
fn manifest_path(f: &Fixture, key: &str) -> PathBuf {
    f.paths().generations.join(format!("{key}.json"))
}

#[test]
fn published_orphan_is_reclaimable_but_unknown_names_remain_counted() {
    let f = Fixture::new();
    let raw = b"committed before generation crash";
    let (object, _) = f.publish(raw, &[]);
    let unknown = f.paths().objects.join("foreign-not-cas");
    fs::write(&unknown, b"protected unknown bytes").unwrap();
    let report = f.prune(DerivedRetirementOptions::default()).unwrap();
    assert_eq!(report.removed_objects, 1);
    assert_eq!(report.removed_orphan_objects, 1);
    assert_eq!(report.removed_generations, 0);
    assert_eq!(report.reclaimed_bytes, raw.len() as u64);
    assert_eq!(report.after_bytes, b"protected unknown bytes".len() as u64);
    assert_eq!(report.unattributed_scope_bytes, report.after_bytes);
    assert!(!report.complete);
    assert!(!f.paths().object(&object.sha256).unwrap().exists());
    assert_eq!(fs::read(unknown).unwrap(), b"protected unknown bytes");
}
#[test]
fn all_shared_manifest_references_retire_before_one_physical_object() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"shared object", &["one", "two", "three"]);
    let before = keys
        .iter()
        .map(|key| fs::metadata(manifest_path(&f, key)).unwrap().len())
        .sum::<u64>()
        + object.bytes;
    let report = f
        .prune(DerivedRetirementOptions {
            max_objects: 1,
            ..Default::default()
        })
        .unwrap();
    assert!(report.complete);
    assert_eq!(report.before_bytes, before);
    assert_eq!(report.reclaimed_bytes, before);
    assert_eq!(report.after_bytes, 0);
    assert_eq!(report.attempted_objects, 1);
    assert_eq!(report.removed_objects, 1);
    assert_eq!(report.removed_generations, 3);
    assert_eq!(
        report.deletions.iter().map(|d| d.kind).collect::<Vec<_>>(),
        ["generation", "generation", "generation", "object"]
    );
    for key in keys {
        assert!(!manifest_path(&f, &key).exists());
    }
    assert!(!f.paths().object(&object.sha256).unwrap().exists());
}
#[test]
fn active_owned_reader_skips_body_and_references_including_dry_run_then_retries() {
    let f = Fixture::new();
    let (object, keys) = f.publish(&vec![42; 131073], &["active"]);
    let resources = f.resources(&OperationControl::default());
    let check: DerivedReadCheck = Arc::new(|| Ok(()));
    let reader = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap()
        .open_pinned_object(&object.sha256, object.bytes)
        .unwrap()
        .unwrap();
    for dry_run in [false, true] {
        let report = resources
            .retire_derived_cache_with_check(
                DerivedRetirementOptions {
                    dry_run,
                    ..Default::default()
                },
                &|| Ok(()),
                |_| Ok(()),
                reference,
            )
            .unwrap();
        assert_eq!(report.skipped_in_use, 1);
        assert_eq!(report.attempted_objects, 0);
        assert_eq!(report.reclaimed_bytes, 0);
        assert!(!report.complete);
        assert!(manifest_path(&f, &keys[0]).exists());
    }
    reader.close().unwrap();
    resources.close().unwrap();
    assert!(f.prune(Default::default()).unwrap().complete);
}
#[test]
fn partial_shared_group_consumes_one_attempt_and_keeps_remaining_references() {
    let f = Fixture::new();
    let mut bodies = [b"first-group".as_slice(), b"second-group".as_slice()];
    bodies.sort_by_key(|bytes| storage::sha256(bytes));
    let (object, keys) = f.publish(bodies[0], &["reference-a", "reference-b"]);
    let (other, others) = f.publish(bodies[1], &["other-reference"]);
    let directory = Directory::open(&f.paths().generations).unwrap();
    let pinned = directory
        .open_file(OsStr::new(&format!("{}.json", keys[1])))
        .unwrap();
    let report = f
        .prune(DerivedRetirementOptions {
            max_objects: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(report.attempted_objects, 1);
    assert_eq!(report.removed_generations, 1);
    assert_eq!(report.removed_objects, 0);
    assert_eq!(report.skipped_in_use, 1);
    assert!(report.truncated);
    assert!(!manifest_path(&f, &keys[0]).exists());
    assert!(manifest_path(&f, &keys[1]).exists());
    assert!(manifest_path(&f, &others[0]).exists());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(f.paths().object(&other.sha256).unwrap().exists());
    storage::close_file(pinned).unwrap();
    directory.close().unwrap();
    let retry = f.prune(Default::default()).unwrap();
    assert!(retry.complete);
    assert_eq!(retry.removed_objects, 2);
    assert_eq!(retry.removed_generations, 2);
}
#[test]
fn unknown_or_conflicting_manifest_blocks_every_deletion_before_mutation() {
    for unknown in [true, false] {
        let f = Fixture::new();
        let (object, keys) = f.publish(b"ordinary", &["valid"]);
        let path = if unknown {
            f.paths().generations.join("future-format.bin")
        } else {
            manifest_path(&f, &storage::sha256(b"conflicting"))
        };
        let bytes = if unknown {
            b"future generation".to_vec()
        } else {
            serde_json::to_vec(
                &serde_json::json!({"sha256":object.sha256,"bytes":object.bytes + 1}),
            )
            .unwrap()
        };
        fs::write(&path, &bytes).unwrap();
        let error = f.prune(Default::default()).unwrap_err();
        assert_eq!(error.code, "vault.data_cache_integrity");
        assert_eq!(error.details["result"]["reclaimedBytes"], 0);
        assert!(manifest_path(&f, &keys[0]).exists());
        assert!(f.paths().object(&object.sha256).unwrap().exists());
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn canonical_same_length_corruption_is_rejected_and_never_reclaimed() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"original", &[]);
    let path = f.paths().object(&object.sha256).unwrap();
    fs::write(&path, b"modified").unwrap();
    let error = f.prune(Default::default()).unwrap_err();
    assert_eq!(error.code, "vault.data_cache_integrity");
    assert_eq!(error.details["result"]["reclaimedBytes"], 0);
    assert_eq!(fs::read(path).unwrap(), b"modified");
}
#[test]
fn cancellation_after_manifest_retirement_keeps_counted_orphan_for_retry() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"retain until retry", &["cancelled"]);
    let path = manifest_path(&f, &keys[0]);
    let control = OperationControl::default();
    let cancel = control.cancel_handle();
    let resources = f.resources(&control);
    let result = resources.retire_derived_cache_with_check(
        Default::default(),
        &|| {
            if !path.exists() {
                cancel.store(true, Ordering::Release);
            }
            control.check()
        },
        |_| Ok(()),
        reference,
    );
    let error = result.unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.details["result"]["removedGenerations"], 1);
    assert_eq!(error.details["result"]["removedObjects"], 0);
    assert_eq!(error.details["result"]["afterBytes"], object.bytes);
    assert!(!path.exists());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    assert!(resources.close_cleanup().is_empty());
    let fresh = f.resources(&OperationControl::default());
    let check = || Ok(());
    let read = fresh.read_derived_cache_with_check(&check).unwrap();
    assert_eq!(read.report().existing.object_namespace_bytes, object.bytes);
    read.finish(Ok(())).unwrap();
    fresh.close().unwrap();
    let retry = f.prune(Default::default()).unwrap();
    assert!(retry.complete);
    assert_eq!(retry.removed_orphan_objects, 1);
}
#[test]
fn allocation_refusal_precedes_deletion_and_releases_original_quota() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"bounded", &["budget"]);
    let resources = f.resources(&OperationControl::default());
    let error = resources
        .retire_derived_cache_with_check(
            Default::default(),
            &|| Ok(()),
            |_| -> Result<()> {
                Err(ToolkitError::new(
                    "test.retirement_budget",
                    "injected original budget refusal",
                    3,
                ))
            },
            reference,
        )
        .unwrap_err();
    assert_eq!(error.code, "test.retirement_budget");
    assert_eq!(error.details["result"]["reclaimedBytes"], 0);
    ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
        .unwrap()
        .close()
        .unwrap();
    assert!(manifest_path(&f, &keys[0]).exists());
    assert!(f.paths().object(&object.sha256).unwrap().exists());
    resources.close().unwrap();
}
#[test]
fn explicit_dry_run_authenticates_all_identity_and_reports_only_planned_deletions() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"dry run", &["dry"]);
    let manifest = fs::read(manifest_path(&f, &keys[0])).unwrap();
    let report = f
        .prune(DerivedRetirementOptions {
            dry_run: true,
            ..Default::default()
        })
        .unwrap();
    assert!(report.complete && report.dry_run);
    assert_eq!(report.after_bytes, 0);
    assert!(report.deletions.iter().all(|d| d.state == "planned"));
    assert_eq!(fs::read(manifest_path(&f, &keys[0])).unwrap(), manifest);
    assert_eq!(
        fs::read(f.paths().object(&object.sha256).unwrap()).unwrap(),
        b"dry run"
    );
}
#[test]
fn same_sha_cap_and_keep_remain_separately_owned_after_derived_retirement() {
    let f = Fixture::new();
    let raw = b"same bytes in separate physical namespaces";
    let (object, _) = f.publish(raw, &["cap-derived"]);
    let capture = evidence::commit_capture(
        &f.0,
        raw,
        "application/octet-stream",
        serde_json::json!({"kind":"test","locator":"fixture:retirement"}),
        true,
        false,
    )
    .unwrap();
    assert_eq!(capture["blob"]["sha256"], object.sha256);
    let id = capture["id"].as_str().unwrap();
    let invocation = crate::Invocation {
        command: "evidence keep".into(),
        positional: Some(id.into()),
        flags: Default::default(),
        home: f.0.clone(),
        cwd: f.0.clone(),
        control: OperationControl::default(),
    };
    evidence::dispatch(&invocation).unwrap();
    let key = format!("evidence/keep/{id}");
    let metadata = Metadata::open(&f.0, false).unwrap();
    let before: serde_json::Value = metadata.read(&key).unwrap();
    metadata.close().unwrap();
    assert!(f.prune(Default::default()).unwrap().complete);
    assert_eq!(evidence::read_capture(&f.0, id).unwrap(), capture);
    assert_eq!(
        evidence::read_blob(&f.0, &capture["blob"], raw.len()).unwrap(),
        raw
    );
    let metadata = Metadata::open(&f.0, false).unwrap();
    assert_eq!(metadata.read::<serde_json::Value>(&key).unwrap(), before);
    metadata.close().unwrap();
}
#[test]
fn injected_close_error_preserves_requested_disposition_without_fake_reclaim() {
    let f = Fixture::new();
    let mut report = f.prune(Default::default()).unwrap();
    let (object, _) = f.publish(b"close diagnostic", &[]);
    report.before_bytes = object.bytes;
    report.after_bytes = object.bytes;
    report.data_before_bytes = object.bytes;
    report.data_after_bytes = object.bytes;
    let path = f.paths().object(&object.sha256).unwrap();
    let directory = Directory::open(path.parent().unwrap()).unwrap();
    let control = OperationControl::default();
    let checks = Checks {
        control: &control,
        supplemental: None,
    };
    let held = authenticate(
        &directory,
        OsStr::new(&object.sha256[2..]),
        &object.sha256,
        object.bytes,
        &checks,
    )
    .unwrap()
    .unwrap();
    let error = discard_with_close(
        held,
        "orphan-object",
        &object.sha256,
        object.bytes,
        &mut report,
        &checks,
        |file| {
            storage::close_file(file)?;
            Err(std::io::Error::other(
                "injected close diagnostic after real close",
            ))
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "external.io");
    assert_eq!(report.reclaimed_bytes, 0);
    assert_eq!(report.after_bytes, object.bytes);
    assert_eq!(report.deletions[0].state, "requested-close-unconfirmed");
    directory.close().unwrap();
}

#[test]
fn returned_receipts_keep_their_allocation_owner_through_serialization() {
    struct Allocation(Arc<AtomicUsize>, usize);
    impl Drop for Allocation {
        fn drop(&mut self) {
            self.0.fetch_sub(self.1, Ordering::SeqCst);
        }
    }
    let f = Fixture::new();
    f.publish(b"guard lifetime", &["guard"]);
    let resources = f.resources(&OperationControl::default());
    let retained = Arc::new(AtomicUsize::new(0));
    let report = resources
        .retire_derived_cache_with_check(
            Default::default(),
            &|| Ok(()),
            |bytes| {
                retained.fetch_add(bytes, Ordering::SeqCst);
                Ok(Allocation(retained.clone(), bytes))
            },
            reference,
        )
        .unwrap();
    assert!(retained.load(Ordering::SeqCst) > 0);
    let json = serde_json::to_value(&report).unwrap();
    assert!(json.get("receiptAllocation").is_none());
    assert!(retained.load(Ordering::SeqCst) > 0);
    drop(report);
    assert_eq!(retained.load(Ordering::SeqCst), 0);
    resources.close().unwrap();
}

#[test]
fn remaining_byte_target_is_inclusive_and_config_shrink_still_allows_retirement() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"over the reduced limit", &["shrunk"]);
    let before = object.bytes + fs::metadata(manifest_path(&f, &keys[0])).unwrap().len();
    let mut config = workspace::default_config();
    config["dataCache"]["maxBytes"] = serde_json::json!(1);
    storage::atomic_json(&f.0.join("config.json"), &config).unwrap();
    let no_change = f
        .prune(DerivedRetirementOptions {
            target_bytes: before,
            ..Default::default()
        })
        .unwrap();
    assert!(no_change.complete);
    assert_eq!(no_change.before_bytes, before);
    assert_eq!(no_change.after_bytes, before);
    assert_eq!(no_change.attempted_objects, 0);
    assert_eq!(no_change.reclaimed_bytes, 0);
    assert!(manifest_path(&f, &keys[0]).exists());
    let retired = f.prune(Default::default()).unwrap();
    assert!(retired.complete);
    assert_eq!(retired.after_bytes, 0);
    assert_eq!(retired.reclaimed_bytes, before);
}
#[test]
fn enumeration_chain_denies_the_original_ancestor_reparse_writer_until_checked_close() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    f.publish(b"enumeration identity", &["enumeration"]);
    let resources = f.resources(&OperationControl::default());
    let control = OperationControl::default();
    let owner = DerivedReservation::acquire_inner(
        Some(&resources),
        &f.0,
        resources.data_cache_max_bytes(),
        AdditionalPeak::default(),
        &control,
        None,
        true,
    )
    .unwrap();
    let writer = |path: &Path| {
        fs::OpenOptions::new()
            .write(true)
            .share_mode(7)
            .custom_flags(0x02200000)
            .open(path)
    };
    let ancestor = f.0.join("cache");
    // Same desired write access as the existing junction fixture. Ordinary
    // Directory ancestry permits it before the stronger retirement pins.
    storage::close_file(writer(&ancestor).unwrap()).unwrap();
    let mut held = Vec::new();
    hold_enumeration_ancestors(&owner, &mut held).unwrap();
    let error = writer(&ancestor).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(32));
    for branch in ["indexes", "tmp", "state"] {
        assert_eq!(
            writer(&f.0.join(branch)).unwrap_err().raw_os_error(),
            Some(32)
        );
    }
    while let Some(file) = held.pop() {
        storage::close_file(file).unwrap();
    }
    storage::close_file(writer(&ancestor).unwrap()).unwrap();
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn referenced_missing_body_remains_an_explicit_integrity_stop_without_manifest_deletion() {
    let f = Fixture::new();
    let (object, keys) = f.publish(b"fixture missing body", &["missing"]);
    fs::remove_file(f.paths().object(&object.sha256).unwrap()).unwrap();
    let error = f.prune(Default::default()).unwrap_err();
    assert_eq!(error.code, "vault.data_cache_integrity");
    assert!(error.message.contains("absent canonical Data object"));
    assert_eq!(error.details["result"]["reclaimedBytes"], 0);
    assert!(manifest_path(&f, &keys[0]).exists());
}
