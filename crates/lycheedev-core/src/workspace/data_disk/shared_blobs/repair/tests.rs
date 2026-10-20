use super::*;
use crate::workspace;
use std::cell::Cell;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-decoded-repair-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn limit(&self, bytes: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(bytes);
        storage::atomic_json(&self.0.join("config.json"), &config).unwrap();
    }
    fn reference(&self) -> BlobRef {
        BlobRef {
            sha256: storage::sha256(b"body"),
            bytes: 4,
        }
    }
    fn body(&self) -> PathBuf {
        evidence::blob_path(&self.0, &self.reference().sha256).unwrap()
    }
    fn record(&self, suffix: &str) -> PathBuf {
        DataCachePaths::new(&self.0)
            .shared_blobs()
            .join(format!("{}.{suffix}", self.reference().sha256))
    }
    fn publish(&self) -> BlobRef {
        publish_shared_blob_with_check(
            &self.0,
            b"body",
            4,
            &OperationControl::default(),
            &|| Ok(()),
        )
        .unwrap()
    }
    fn missing(&self) -> BlobRef {
        let reference = self.publish();
        fs::remove_file(self.body()).unwrap();
        reference
    }
    fn rebuild(&self) -> Result<BlobRef> {
        rebuild_shared_blob_with_check(
            &self.0,
            b"body",
            4,
            &self.reference(),
            &OperationControl::default(),
            &|| Ok(()),
        )
    }
    fn usage(&self) -> DerivedUsage {
        let control = OperationControl::default();
        let owner = DerivedReservation::acquire_inner(
            None,
            &self.0,
            0,
            AdditionalPeak::default(),
            &control,
            None,
            true,
        )
        .unwrap();
        let usage = owner.report.existing;
        owner.finish(Ok(())).unwrap();
        usage
    }
    fn pair(&self) -> (Vec<u8>, Vec<u8>) {
        (
            fs::read(self.record("intent")).unwrap(),
            fs::read(self.record("complete")).unwrap(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn stopped() -> ToolkitError {
    ToolkitError::new("fixture.repair_stopped", "original budget stopped", 7)
}

#[test]
fn missing_body_rebuild_preserves_retained_capture_original_pair_and_exact_peak() {
    let f = Fixture::new();
    f.publish();
    let capture = evidence::commit_capture(
        &f.0,
        b"body",
        "application/octet-stream",
        serde_json::json!({"kind":"repair-evidence", "locator":"fixture"}),
        true,
        false,
    )
    .unwrap();
    let keep_invocation = crate::Invocation {
        command: "evidence keep".into(),
        positional: Some(capture["id"].as_str().unwrap().into()),
        flags: Default::default(),
        home: f.0.clone(),
        cwd: f.0.clone(),
        control: OperationControl::default(),
    };
    evidence::dispatch(&keep_invocation).unwrap();
    let key = format!("evidence/keep/{}", capture["id"].as_str().unwrap());
    let kept = crate::metadata::Metadata::read_document_raw_controlled_with_check(
        &f.0,
        &key,
        &OperationControl::default(),
        || Ok(()),
    )
    .unwrap();
    let before = f.pair();
    fs::remove_file(f.body()).unwrap();
    f.limit(263); // original80 + completed4 + repair48 + declaration128 + stage4 = 264.
    assert_eq!(
        f.rebuild().unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
    assert!(!f.record("repair.intent").exists());
    f.limit(264);
    f.rebuild().unwrap();
    assert_eq!(f.pair(), before);
    assert_eq!(fs::read(f.body()).unwrap(), b"body");
    assert_eq!(f.usage().total().unwrap(), 84);
    assert!(!f.record("repair.intent").exists());
    assert_eq!(
        evidence::read_capture(&f.0, capture["id"].as_str().unwrap()).unwrap(),
        capture
    );
    assert_eq!(
        evidence::read_blob(&f.0, &capture["blob"], 4).unwrap(),
        b"body"
    );
    let after = crate::metadata::Metadata::read_document_raw_controlled_with_check(
        &f.0,
        &key,
        &OperationControl::default(),
        || Ok(()),
    )
    .unwrap();
    assert_eq!(after.bytes, kept.bytes);
    assert_eq!(after.generation, kept.generation);
}
#[test]
fn ordinary_publisher_stays_strict_for_missing_completed_body() {
    let f = Fixture::new();
    f.missing();
    let error =
        publish_shared_blob_with_check(&f.0, b"body", 4, &OperationControl::default(), &|| Ok(()))
            .unwrap_err();
    assert_eq!(error.code, "vault.data_shared_blob_integrity");
    assert!(!f.body().exists());
    assert!(!f.record("repair.intent").exists());
}
#[test]
fn rebuild_rejects_wrong_final_sha_size_and_present_corruption() {
    let f = Fixture::new();
    let expected = f.missing();
    for bytes in [b"fake".as_slice(), b"short".as_slice()] {
        let error = rebuild_shared_blob_with_check(
            &f.0,
            bytes,
            10,
            &expected,
            &OperationControl::default(),
            &|| Ok(()),
        )
        .unwrap_err();
        assert_eq!(error.code, "vault.data_shared_blob_integrity");
        assert!(!f.body().exists());
        assert!(!f.record("repair.intent").exists());
    }
    fs::write(f.body(), b"fake").unwrap();
    assert!(f.rebuild().is_err());
    assert_eq!(fs::read(f.body()).unwrap(), b"fake");
}
#[test]
fn rebuild_rejects_partial_mismatched_and_unrelated_missing_attribution() {
    for fault in [
        "intent",
        "complete",
        "wrong-complete",
        "wrong-intent",
        "unrelated",
    ] {
        let f = Fixture::new();
        if fault == "unrelated" {
            let other = publish_shared_blob_with_check(
                &f.0,
                b"other",
                5,
                &OperationControl::default(),
                &|| Ok(()),
            )
            .unwrap();
            f.missing();
            fs::remove_file(evidence::blob_path(&f.0, &other.sha256).unwrap()).unwrap();
        } else {
            f.missing();
            match fault {
                "intent" | "complete" => fs::remove_file(f.record(fault)).unwrap(),
                "wrong-complete" => fs::write(f.record("complete"), [0; 32]).unwrap(),
                "wrong-intent" => {
                    let mut raw = fs::read(f.record("intent")).unwrap();
                    raw[8] = 5;
                    fs::write(f.record("intent"), raw).unwrap();
                }
                _ => unreachable!(),
            }
        }
        assert_eq!(
            f.rebuild().unwrap_err().code,
            "vault.data_shared_blob_integrity",
            "{fault}"
        );
        assert!(!f.body().exists());
        assert!(!f.record("repair.intent").exists());
    }
}
#[test]
fn old_repair_marker_keeps_stage_peak_and_never_authorizes_retry() {
    let f = Fixture::new();
    f.publish();
    fs::write(
        f.record("repair.intent"),
        fs::read(f.record("intent")).unwrap(),
    )
    .unwrap();
    // Synthetic crash residue: its charge is the retained repair B, not a scan
    // of arbitrary shared blob directories. No file is adopted or deleted.
    let stage = f
        .body()
        .parent()
        .unwrap()
        .join(".lycheedev-export-crash-fixture");
    fs::write(&stage, b"body").unwrap();
    assert_eq!(f.usage().total().unwrap(), 136);
    assert_eq!(f.usage().retained_shared_peak_bytes, 4);
    assert_eq!(
        f.rebuild().unwrap_err().code,
        "vault.data_shared_blob_recovery_required"
    );
    fs::remove_file(f.body()).unwrap();
    assert_eq!(
        f.rebuild().unwrap_err().code,
        "vault.data_shared_blob_recovery_required"
    );
    assert_eq!(fs::read(stage).unwrap(), b"body");
    assert!(f.record("repair.intent").exists());
}
#[test]
fn historical_missing_alias_without_attribution_uses_existing_cold_enrollment() {
    let f = Fixture::new();
    evidence::publish_blob(&f.0, b"body", 4).unwrap();
    fs::remove_file(f.body()).unwrap();
    f.limit(216);
    f.rebuild().unwrap();
    assert_eq!(f.usage().total().unwrap(), 84);
    assert!(!f.record("repair.intent").exists());
}
#[test]
fn cancelled_before_repair_body_withdraws_only_new_marker_and_allows_clean_retry() {
    let f = Fixture::new();
    let reference = f.missing();
    let before = f.pair();
    let seen = Cell::new(false);
    let error = rebuild_shared_blob_with_check(
        &f.0,
        b"body",
        4,
        &reference,
        &OperationControl::default(),
        &|| {
            if f.record("repair.intent").exists() {
                seen.set(true);
                Err(stopped())
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.repair_stopped");
    assert!(seen.get());
    assert_eq!(f.pair(), before);
    assert!(!f.record("repair.intent").exists());
    assert!(!f.body().exists());
    f.rebuild().unwrap();
}
#[test]
fn published_repair_late_stop_keeps_marker_body_and_publication_receipt() {
    let f = Fixture::new();
    let reference = f.missing();
    let before = f.pair();
    let error = rebuild_shared_blob_with_check(
        &f.0,
        b"body",
        4,
        &reference,
        &OperationControl::default(),
        &|| {
            if f.body().exists() {
                Err(stopped())
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.repair_stopped");
    assert_eq!(f.pair(), before);
    assert_eq!(fs::read(f.body()).unwrap(), b"body");
    assert!(f.record("repair.intent").exists());
    assert!(error.details.to_string().contains("shared-blob-rebuild"));
    assert_eq!(f.usage().total().unwrap(), 264);
    f.rebuild().unwrap();
    assert_eq!(f.usage().total().unwrap(), 84);
    assert!(!f.record("repair.intent").exists());
}
#[test]
fn concurrent_winner_after_repair_marker_is_verified_without_overwrite() {
    for corrupt in [false, true] {
        let f = Fixture::new();
        let reference = f.missing();
        let seen = Cell::new(false);
        let result = rebuild_shared_blob_with_check(
            &f.0,
            b"body",
            4,
            &reference,
            &OperationControl::default(),
            &|| {
                if f.record("repair.intent").exists() && !seen.replace(true) {
                    // A different source owner publishes between quota and canonical
                    // lock. Deliberate corrupt fixture tests the full winner check.
                    if corrupt {
                        fs::write(f.body(), b"fake")?;
                    } else {
                        evidence::publish_blob(&f.0, b"body", 4)?;
                    }
                }
                Ok(())
            },
        );
        assert!(seen.get());
        if corrupt {
            assert!(result.is_err());
            assert_eq!(fs::read(f.body()).unwrap(), b"fake");
        } else {
            result.unwrap();
            assert_eq!(fs::read(f.body()).unwrap(), b"body");
        }
        assert!(!f.record("repair.intent").exists());
    }
}

#[test]
fn final_rebuild_rescan_does_not_retain_the_initial_missing_body_exception() {
    let f = Fixture::new();
    let reference = f.missing();
    let saw_marker = Cell::new(false);
    let removed = Cell::new(false);
    let error = rebuild_shared_blob_with_check(
        &f.0,
        b"body",
        4,
        &reference,
        &OperationControl::default(),
        &|| {
            if f.record("repair.intent").exists() {
                saw_marker.set(true);
            } else if saw_marker.get() && !removed.get() && f.body().exists() {
                fs::remove_file(f.body())?;
                removed.set(true);
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert!(removed.get());
    assert_eq!(error.code, "vault.data_shared_blob_integrity");
    let receipts = error.details.to_string();
    assert!(receipts.contains("shared-blob-rebuild"));
    assert!(receipts.contains("shared-blob-repair-settlement"));
    assert!(receipts.contains("withdrawalComplete"));
    assert!(!f.record("repair.intent").exists());
}

#[test]
fn repair_name_fails_unchanged_legacy_intent_decoder_until_checked_withdrawal() {
    let f = Fixture::new();
    let reference = f.publish();
    let raw = intent(&reference).unwrap();
    // This is the exact existing decoder retained from19e, exercised rather
    // than substituting a permissive model or merely inferring its result.
    let normal = format!("{}.intent", reference.sha256);
    assert!(same_blob(&decode(&normal, &raw).unwrap(), &reference));
    let name = format!("{}.repair.intent", reference.sha256);
    let control = OperationControl::default();
    let checks = Checks {
        control: &control,
        supplemental: None,
    };
    let directory = Directory::open_controlled(
        &DataCachePaths::new(&f.0).shared_blobs(),
        &mut Controlled(&checks),
    )
    .unwrap();
    let mut marker = QuotaIntent::create(&directory, &mut Controlled(&checks)).unwrap();
    assert_eq!(
        marker
            .publish_repair_record(&raw, &mut Controlled(&checks))
            .unwrap(),
        StagedPublication::Published
    );
    assert!(marker.close().is_empty());
    assert!(close_directory(directory).is_empty());
    let published = fs::read(f.record("repair.intent")).unwrap();
    assert_eq!(published, raw);
    assert!(name.ends_with(".intent")); // Both old/new inventory dispatch here.
    assert_eq!(
        decode(&name, &published).unwrap_err().code,
        "vault.data_shared_blob_integrity"
    );
    // New inventory understands precisely this suffix and retains the stage B.
    assert_eq!(f.usage().total().unwrap(), 136);
    assert_eq!(
        f.rebuild().unwrap_err().code,
        "vault.data_shared_blob_recovery_required"
    );
    // A separate real clean repair withdraws its own marker and leaves exactly
    // the original two ordinary records, which the old decoder still accepts.
    let clean = Fixture::new();
    clean.missing();
    clean.rebuild().unwrap();
    assert_eq!(
        fs::read_dir(DataCachePaths::new(&clean.0).shared_blobs())
            .unwrap()
            .count(),
        2
    );
    assert!(same_blob(
        &decode(&normal, &fs::read(clean.record("intent")).unwrap()).unwrap(),
        &reference
    ));
}

#[test]
fn new_repair_inventory_rejects_noncanonical_or_extra_prefix_marker_names() {
    for prefix in ["x", "extra."] {
        let f = Fixture::new();
        let reference = f.publish();
        let directory = DataCachePaths::new(&f.0).shared_blobs();
        fs::write(
            directory.join(format!("{prefix}{}.repair.intent", reference.sha256)),
            intent(&reference).unwrap(),
        )
        .unwrap();
        let control = OperationControl::default();
        let error = DerivedReservation::acquire_inner(
            None,
            &f.0,
            0,
            AdditionalPeak::default(),
            &control,
            None,
            true,
        )
        .err()
        .expect("malformed repair was admitted");
        assert_eq!(error.code, "vault.data_shared_blob_integrity");
        assert_eq!(fs::read(f.body()).unwrap(), b"body");
    }
}
