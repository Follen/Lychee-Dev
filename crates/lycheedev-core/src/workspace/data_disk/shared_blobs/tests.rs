use super::*;
use crate::workspace::{
    self,
    data_resources::{self, Policy},
};
use std::{
    cell::Cell,
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new(limit: u64) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-shared-data-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        let f = Self(home);
        f.limit(limit);
        f
    }
    fn limit(&self, limit: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(limit);
        storage::atomic_json(&self.0.join("config.json"), &config).unwrap();
    }
    fn publish(&self, bytes: &[u8]) -> Result<BlobRef> {
        publish_shared_blob_with_check(
            &self.0,
            bytes,
            bytes.len(),
            &OperationControl::default(),
            &|| Ok(()),
        )
    }
    fn usage(&self) -> DerivedUsage {
        let control = OperationControl::default();
        let resources = data_resources::acquire(&self.0, Policy::Ordinary, &control).unwrap();
        let owner = resources
            .reserve_derived_cache(AdditionalPeak::default())
            .unwrap();
        let usage = owner.report().existing;
        assert!(!owner.report().coverage.total_data_coverage_complete);
        owner.finish(Ok(())).unwrap();
        resources.close().unwrap();
        usage
    }
    fn paths(&self) -> DataCachePaths {
        DataCachePaths::new(&self.0)
    }
    fn receipt(&self, bytes: &[u8], suffix: &str) -> PathBuf {
        self.paths()
            .shared_blobs()
            .join(format!("{}.{suffix}", storage::sha256(bytes)))
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn stop() -> ToolkitError {
    ToolkitError::new(
        "fixture.budget_cancelled",
        "independent Budget cancelled",
        7,
    )
}
fn stages(home: &Path) -> usize {
    fs::read_dir(home.join("blobs"))
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            if !e.file_type().unwrap().is_dir() {
                return 0;
            }
            fs::read_dir(e.path())
                .unwrap()
                .filter(|e| {
                    e.as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".lycheedev-")
                })
                .count()
        })
        .sum()
}
#[test]
fn exact_peak_counts_body_stage_and_records_then_releases_only_finished_peak() {
    let f = Fixture::new(215);
    assert_eq!(
        f.publish(b"body").unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
    assert!(!f.receipt(b"body", "intent").exists());
    f.limit(216);
    let blob = f.publish(b"body").unwrap();
    assert_eq!(blob.bytes, 4);
    let usage = f.usage();
    assert_eq!(usage.shared_blob_bytes, 4);
    assert_eq!(usage.retained_shared_peak_bytes, 0);
    assert_eq!(usage.generation_namespace_bytes, 80);
    assert_eq!(usage.total().unwrap(), 84);
    assert_eq!(f.publish(b"body").unwrap().sha256, blob.sha256);
    assert_eq!(f.usage(), usage);
}
#[test]
fn preexisting_source_and_evidence_body_requires_new_attribution_capacity_and_is_never_deleted() {
    let f = Fixture::new(215);
    let control = OperationControl::default();
    let original = evidence::publish_blob_bounded(&f.0, b"body", 4, &control).unwrap();
    let capture = evidence::commit_capture(
        &f.0,
        b"body",
        "application/octet-stream",
        serde_json::json!({"kind":"fixture-source-evidence","locator":"fixture"}),
        true,
        false,
    )
    .unwrap();
    assert_eq!(
        f.publish(b"body").unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
    let path = evidence::blob_path(&f.0, original["sha256"].as_str().unwrap()).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"body");
    f.limit(216);
    f.publish(b"body").unwrap();
    assert_eq!(f.usage().shared_blob_bytes, 4);
    assert_eq!(
        evidence::read_capture(&f.0, capture["id"].as_str().unwrap()).unwrap(),
        capture
    );
    assert_eq!(fs::read(path).unwrap(), b"body");
}
#[test]
fn derived_and_registered_shared_bytes_use_the_same_admission_limit() {
    let f = Fixture::new(220);
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources
        .reserve_derived_cache(AdditionalPeak {
            objects: 4,
            staging: 4,
            ..Default::default()
        })
        .unwrap();
    let mut writer = owner.begin_object(4).unwrap();
    writer.write_bytes(b"data").unwrap();
    writer.commit().unwrap();
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
    f.publish(b"body").unwrap();
    assert_eq!(f.usage().total().unwrap(), 88);
    // Settled usage is unchanged; the declaration is transient peak only.
    f.limit(92);
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    assert_eq!(
        resources
            .reserve_derived_cache(AdditionalPeak {
                spill: 5,
                ..Default::default()
            })
            .err()
            .unwrap()
            .code,
        "vault.data_cache_budget_exceeded"
    );
    let owner = resources
        .reserve_derived_cache(AdditionalPeak {
            spill: 4,
            ..Default::default()
        })
        .unwrap();
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn configuration_shrink_allows_only_verified_zero_growth_reuse() {
    let f = Fixture::new(216);
    let blob = f.publish(b"body").unwrap();
    f.limit(1);
    assert_eq!(f.publish(b"body").unwrap().sha256, blob.sha256);
    assert_eq!(
        f.publish(b"next").unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(
        fs::read(evidence::blob_path(&f.0, &blob.sha256).unwrap()).unwrap(),
        b"body"
    );
}
#[test]
fn cancellation_after_intent_before_body_withdraws_original_intent_and_permits_retry() {
    let f = Fixture::new(216);
    let observed = Cell::new(false);
    let error =
        publish_shared_blob_with_check(&f.0, b"body", 4, &OperationControl::default(), &|| {
            if f.receipt(b"body", "intent").exists() {
                observed.set(true);
                Err(stop())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, "fixture.budget_cancelled");
    assert!(observed.get());
    assert!(!f.receipt(b"body", "intent").exists());
    assert_eq!(f.usage().total().unwrap(), 0);
    f.publish(b"body").unwrap();
}
#[test]
fn cancelled_real_body_stage_cleanup_releases_intent_without_error_code_inference() {
    let f = Fixture::new(216);
    let observed = Cell::new(false);
    let error =
        publish_shared_blob_with_check(&f.0, b"body", 4, &OperationControl::default(), &|| {
            if stages(&f.0) != 0 {
                observed.set(true);
                Err(stop())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(observed.get());
    assert_eq!(error.code, "fixture.budget_cancelled");
    assert_eq!(stages(&f.0), 0);
    assert!(!f.receipt(b"body", "intent").exists());
    assert_eq!(f.usage().total().unwrap(), 0);
    f.publish(b"body").unwrap();
}
#[test]
fn published_body_late_failure_retains_intent_and_allows_read_only_pending_reuse() {
    let f = Fixture::new(216);
    let path = evidence::blob_path(&f.0, &storage::sha256(b"body")).unwrap();
    let error =
        publish_shared_blob_with_check(&f.0, b"body", 4, &OperationControl::default(), &|| {
            if path.exists() { Err(stop()) } else { Ok(()) }
        })
        .unwrap_err();
    assert_eq!(error.code, "fixture.budget_cancelled");
    assert!(error.details.to_string().contains("shared-blob"));
    assert!(f.receipt(b"body", "intent").exists());
    assert!(!f.receipt(b"body", "complete").exists());
    let usage = f.usage();
    assert_eq!(usage.retained_shared_peak_bytes, 8);
    assert_eq!(usage.total().unwrap(), 56 + 128);
    f.limit(1);
    f.publish(b"body").unwrap();
    assert!(!f.receipt(b"body", "complete").exists());
    f.limit(216);
    assert_eq!(f.usage().total().unwrap(), 56); // Empty v2 declaration is authenticated and retired.
    assert_eq!(fs::read(path).unwrap(), b"body");
}
#[test]
fn synthetic_crash_pending_missing_body_retains_peak_and_never_replays() {
    let f = Fixture::new(100);
    f.usage();
    let reference = BlobRef {
        sha256: storage::sha256(b"body"),
        bytes: 4,
    };
    fs::write(f.receipt(b"body", "intent"), intent(&reference).unwrap()).unwrap();
    assert_eq!(
        f.publish(b"body").unwrap_err().code,
        "vault.data_shared_blob_recovery_required"
    );
    assert_eq!(f.usage().retained_shared_peak_bytes, 8);
    assert!(
        !evidence::blob_path(&f.0, &reference.sha256)
            .unwrap()
            .exists()
    );
}
#[test]
fn changed_registered_body_length_blocks_unrelated_growth_without_repair() {
    let f = Fixture::new(1024);
    let reference = f.publish(b"body").unwrap();
    let path = evidence::blob_path(&f.0, &reference.sha256).unwrap();
    fs::write(&path, b"expanded corruption").unwrap();
    assert_eq!(
        f.publish(b"next").unwrap_err().code,
        "vault.data_shared_blob_integrity"
    );
    assert_eq!(fs::read(path).unwrap(), b"expanded corruption");
    assert!(!f.receipt(b"next", "intent").exists());
}
#[test]
fn mismatched_marker_or_same_length_corruption_cannot_authorize_reuse() {
    let f = Fixture::new(1024);
    let reference = f.publish(b"body").unwrap();
    fs::write(f.receipt(b"body", "complete"), [0; 32]).unwrap();
    assert_eq!(
        f.publish(b"next").unwrap_err().code,
        "vault.data_shared_blob_integrity"
    );
    fs::write(
        f.receipt(b"body", "complete"),
        completion(&intent(&reference).unwrap()),
    )
    .unwrap();
    fs::write(
        evidence::blob_path(&f.0, &reference.sha256).unwrap(),
        b"evil",
    )
    .unwrap();
    assert_eq!(
        f.publish(b"body").unwrap_err().code,
        "vault.data_cache_integrity"
    );
}
#[test]
fn simultaneous_same_sha_publishers_charge_one_body_and_one_record_pair() {
    let f = Fixture::new(216);
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let home = f.0.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                publish_shared_blob_with_check(
                    &home,
                    b"body",
                    4,
                    &OperationControl::default(),
                    &|| Ok(()),
                )
                .unwrap()
            })
        })
        .collect();
    barrier.wait();
    for handle in handles {
        assert_eq!(handle.join().unwrap().bytes, 4);
    }
    assert_eq!(f.usage().total().unwrap(), 84);
    assert_eq!(fs::read_dir(f.paths().shared_blobs()).unwrap().count(), 2);
}
#[test]
fn unregistered_evidence_is_reported_outside_coverage_and_never_pruned() {
    let f = Fixture::new(216);
    let blob = evidence::publish_blob_bounded(
        &f.0,
        b"unregistered evidence",
        100,
        &OperationControl::default(),
    )
    .unwrap();
    f.publish(b"body").unwrap();
    assert_eq!(f.usage().total().unwrap(), 84);
    assert_eq!(
        evidence::read_blob(&f.0, &blob, 100).unwrap(),
        b"unregistered evidence"
    );
}

#[test]
fn completion_stage_failure_keeps_pending_claim_then_clean_retry_recovers() {
    let f = Fixture::new(216);
    let body = evidence::blob_path(&f.0, &storage::sha256(b"body")).unwrap();
    let observed = Cell::new(false);
    let error =
        publish_shared_blob_with_check(&f.0, b"body", 4, &OperationControl::default(), &|| {
            if body.exists()
                && fs::read_dir(f.paths().shared_blobs())?.any(|entry| {
                    entry.is_ok_and(|entry| {
                        entry
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".lycheedev-cache-")
                    })
                })
            {
                observed.set(true);
                Err(stop())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert!(
        observed.get(),
        "fault must follow actual completion stage creation"
    );
    assert_eq!(error.code, "fixture.budget_cancelled");
    assert_eq!(fs::read(body).unwrap(), b"body");
    assert!(!f.receipt(b"body", "complete").exists());
    assert_eq!(fs::read_dir(f.paths().shared_blobs()).unwrap().count(), 2);
    assert_eq!(f.usage().retained_shared_peak_bytes, 8);
    f.publish(b"body").unwrap();
    assert!(f.receipt(b"body", "complete").exists());
    assert_eq!(f.usage().retained_shared_peak_bytes, 0);
}
#[test]
fn waiting_shared_writer_observes_independent_budget_and_does_not_release_active_derived_lease() {
    let f = Fixture::new(216);
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let held = resources
        .reserve_derived_cache(AdditionalPeak::default())
        .unwrap();
    let contended = std::rc::Rc::new(Cell::new(false));
    let observed = contended.clone();
    ExclusiveLock::set_blob_wait_observer(move || observed.set(true));
    let error = publish_shared_blob_with_check(&f.0, b"body", 4, &control, &|| {
        if contended.get() { Err(stop()) } else { Ok(()) }
    })
    .unwrap_err();
    assert!(contended.get());
    assert_eq!(error.code, "fixture.budget_cancelled");
    assert!(control.check().is_ok());
    assert!(ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE).is_err());
    assert!(!f.receipt(b"body", "intent").exists());
    held.finish(Ok(())).unwrap();
    resources.close().unwrap();
    f.publish(b"body").unwrap();
}

fn closed_body_pending(f: &Fixture, body: &[u8]) -> BlobRef {
    f.usage();
    let reference = BlobRef {
        sha256: storage::sha256(body),
        bytes: body.len() as u64,
    };
    let published = evidence::publish_blob(&f.0, body, body.len()).unwrap();
    assert_eq!(published["sha256"], reference.sha256);
    fs::write(f.receipt(body, "intent"), intent(&reference).unwrap()).unwrap();
    reference
}

#[test]
fn pending_closed_publication_recovers_only_after_full_body_and_clean_shard_proof() {
    let f = Fixture::new(10000);
    let body = b"durable body before completion";
    let reference = closed_body_pending(&f, body);
    assert_eq!(f.usage().retained_shared_peak_bytes, 2 * reference.bytes);
    let reused = f.publish(body).unwrap();
    assert_eq!(reused.sha256, reference.sha256);
    assert_eq!(reused.bytes, reference.bytes);
    let usage = f.usage();
    assert_eq!(usage.retained_shared_peak_bytes, 0);
    assert_eq!(usage.shared_blob_bytes, reference.bytes);
    assert_eq!(
        fs::read(f.receipt(body, "complete")).unwrap(),
        completion(&intent(&reference).unwrap())
    );
    assert_eq!(
        fs::read(evidence::blob_path(&f.0, &reference.sha256).unwrap()).unwrap(),
        body
    );
}

#[test]
fn pending_recovery_never_removes_or_discounts_unknown_shared_stage() {
    let f = Fixture::new(10000);
    let body = b"retain stage";
    let reference = closed_body_pending(&f, body);
    let path = evidence::blob_path(&f.0, &reference.sha256).unwrap();
    let unknown = path.parent().unwrap().join(".lycheedev-unknown-stage");
    fs::write(&unknown, b"other domain or interrupted writer").unwrap();
    f.publish(body).unwrap();
    assert!(!f.receipt(body, "complete").exists());
    assert_eq!(f.usage().retained_shared_peak_bytes, 2 * reference.bytes);
    assert_eq!(
        fs::read(unknown).unwrap(),
        b"other domain or interrupted writer"
    );
}

#[test]
fn pending_recovery_reserves_completion_before_growth_and_preserves_zero_growth_reuse() {
    let f = Fixture::new(10000);
    let body = b"protected pending";
    let reference = closed_body_pending(&f, body);
    let before = f.usage();
    f.limit(before.total().unwrap());
    let reused = f.publish(body).unwrap();
    assert_eq!(reused.sha256, reference.sha256);
    assert_eq!(reused.bytes, reference.bytes);
    assert!(!f.receipt(body, "complete").exists());
    assert_eq!(f.usage(), before);
    f.limit(before.total().unwrap() + COMPLETE_BYTES as u64);
    f.publish(body).unwrap();
    assert_eq!(f.usage().retained_shared_peak_bytes, 0);
}
