use super::*;
use crate::workspace::{
    data_disk::{
        AdditionalPeak, DataCachePaths, DerivedUsage, publish_shared_blob_stream_with_check,
        publish_shared_blob_with_check,
    },
    data_resources::{self, Policy},
};
use lycheedev_windows::filesystem::{
    Directory,
    destination::{FileControl, FileCost, QuotaIntent, StagedPublication},
};
use std::io::Cursor;

struct Meter;
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
}
fn publish(f: &Fixture, body: &[u8]) -> BlobRef {
    publish_shared_blob_with_check(
        &f.home,
        body,
        body.len(),
        &OperationControl::default(),
        &|| Ok(()),
    )
    .unwrap()
}
fn usage(f: &Fixture) -> DerivedUsage {
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.home, Policy::Ordinary, &control).unwrap();
    let owner = resources
        .reserve_derived_cache(AdditionalPeak::default())
        .unwrap();
    assert!(!owner.report().coverage.total_data_coverage_complete);
    let usage = owner.report().existing;
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
    usage
}
fn record(f: &Fixture, digest: &str, extension: &str) -> PathBuf {
    DataCachePaths::new(&f.home)
        .shared_blobs()
        .join(format!("{digest}.{extension}"))
}
fn recovery(f: &Fixture, id: &str) -> Option<RawDocument> {
    Metadata::with_historical_write(&f.home, &OperationControl::default(), |writer| {
        optional(writer.document(&format!("evidence/remove/{id}")))
    })
    .unwrap()
}
fn assert_shared(receipt: &Value, id: &str) {
    assert_eq!(
        receipt,
        &json!({"captureId":id,"manifestRemoved":true,"retentionRemoved":false,
            "blobRemoved":false,"blobShared":true,"blobMissing":false})
    );
}

#[test]
fn decoded_stream_capture_remove_retains_data_body_and_admission_with_or_without_active_reader() {
    for active_reader in [false, true] {
        let f = Fixture::new();
        let control = OperationControl::default();
        let mut blob = publish_shared_blob_stream_with_check(
            &f.home,
            &mut Cursor::new(b"body"),
            4,
            4,
            &control,
            || Ok(()),
            &Meter,
        )
        .unwrap();
        let reference = blob.reference().clone();
        let capture = commit_verified_capture_budgeted(
            &blob,
            CaptureHeader {
                media_type: "text/plain",
                provenance: json!({"kind":"fixture","locator":"decoded-stream"}),
                complete: true,
                truncated: false,
            },
            &control,
            &Meter,
        )
        .unwrap();
        // The real decoded-cache shape references the body, not a CAP ID.
        f.raw(
            "decoded-v1/fixture",
            &serde_json::to_vec(&json!({"blob":reference})).unwrap(),
        );
        let before_intent = fs::read(record(&f, &reference.sha256, "intent")).unwrap();
        let before_complete = fs::read(record(&f, &reference.sha256, "complete")).unwrap();
        let before = usage(&f);
        assert_eq!(before.shared_blob_bytes, 4);
        assert_eq!(before.total().unwrap(), 84);
        let receipt = if active_reader {
            let result = run(&f.home, id(&capture), &f.inv(id(&capture)).control).unwrap();
            blob.rewind().unwrap();
            let mut bytes = [0; 4];
            assert_eq!(blob.read_checked(&mut bytes).unwrap(), 4);
            assert_eq!(&bytes, b"body");
            blob.close().unwrap();
            result
        } else {
            blob.close().unwrap();
            run(&f.home, id(&capture), &f.inv(id(&capture)).control).unwrap()
        };
        assert_shared(&receipt, id(&capture));
        assert!(read_capture(&f.home, id(&capture)).is_err());
        assert_eq!(
            run(&f.home, id(&capture), &f.inv(id(&capture)).control).unwrap(),
            receipt
        );
        assert_eq!(
            fs::read(blob_path(&f.home, &reference.sha256).unwrap()).unwrap(),
            b"body"
        );
        assert_eq!(
            fs::read(record(&f, &reference.sha256, "intent")).unwrap(),
            before_intent
        );
        assert_eq!(
            fs::read(record(&f, &reference.sha256, "complete")).unwrap(),
            before_complete
        );
        assert_eq!(usage(&f), before);
        let reused = publish(&f, b"body");
        assert_eq!(reused.sha256, reference.sha256);
        assert_eq!(reused.bytes, reference.bytes);
        assert_eq!(usage(&f), before);
        // This is an actual later growth admission, not just successful replay.
        publish(&f, b"next");
        assert_eq!(usage(&f).shared_blob_bytes, 8);
        f.cleanup();
    }
}

#[test]
fn data_pending_attribution_preserves_body_and_retained_peak_on_capture_remove() {
    let f = Fixture::new();
    let reference = publish(&f, b"body");
    let capture = f.capture(b"body", "pending");
    // A retained, already closed intent without completion is a crash fixture.
    fs::remove_file(record(&f, &reference.sha256, "complete")).unwrap();
    let before = usage(&f);
    assert_eq!(before.shared_blob_bytes, 0);
    assert_eq!(before.retained_shared_peak_bytes, 8);
    let receipt = run(&f.home, id(&capture), &f.inv(id(&capture)).control).unwrap();
    assert_shared(&receipt, id(&capture));
    assert_eq!(
        fs::read(blob_path(&f.home, &reference.sha256).unwrap()).unwrap(),
        b"body"
    );
    assert_eq!(usage(&f), before);
    assert!(!record(&f, &reference.sha256, "complete").exists());
    f.cleanup();
}

#[test]
fn absent_data_attribution_preserves_real_deletion_without_creating_ownership_directories() {
    for existing_parent in [false, true] {
        let f = Fixture::new();
        let paths = DataCachePaths::new(&f.home);
        if existing_parent {
            fs::create_dir_all(paths.shared_blobs().parent().unwrap()).unwrap();
        }
        let capture = f.capture(b"body", "not-data-owned");
        let path = blob_path(&f.home, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
        let receipt = run(&f.home, id(&capture), &f.inv(id(&capture)).control).unwrap();
        assert_eq!(receipt["blobRemoved"], true);
        assert_eq!(receipt["blobShared"], false);
        assert!(!path.exists());
        assert!(!paths.shared_blobs().exists());
        f.cleanup();
    }
}

#[test]
fn invalid_data_attribution_or_body_preserves_capture_and_never_prepares_removal() {
    for fault in [
        "orphan-complete",
        "magic",
        "digest",
        "length",
        "completion",
        "body-length",
        "missing-body",
    ] {
        let f = Fixture::new();
        let reference = publish(&f, b"body");
        let capture = f.capture(b"body", fault);
        let intent = record(&f, &reference.sha256, "intent");
        let complete = record(&f, &reference.sha256, "complete");
        let body = blob_path(&f.home, &reference.sha256).unwrap();
        match fault {
            "orphan-complete" => fs::remove_file(&intent).unwrap(),
            "magic" | "digest" | "length" => {
                let mut raw = fs::read(&intent).unwrap();
                match fault {
                    "magic" => raw[0] ^= 1,
                    "digest" => raw[16] ^= 1,
                    _ => raw[8..16].copy_from_slice(&5u64.to_le_bytes()),
                }
                fs::write(&intent, &raw).unwrap();
                fs::write(&complete, hex::decode(storage::sha256(&raw)).unwrap()).unwrap();
            }
            "completion" => fs::write(&complete, [0; 32]).unwrap(),
            "body-length" => fs::write(&body, b"bad").unwrap(),
            _ => fs::remove_file(&body).unwrap(),
        }
        let error = run(&f.home, id(&capture), &f.inv(id(&capture)).control).unwrap_err();
        assert_eq!(error.code, "vault.data_shared_blob_integrity", "{fault}");
        assert!(read_capture(&f.home, id(&capture)).is_ok(), "{fault}");
        assert!(recovery(&f, id(&capture)).is_none(), "{fault}");
        assert_eq!(body.exists(), fault != "missing-body", "{fault}");
        f.cleanup();
    }
}

#[test]
fn data_retention_does_not_claim_content_health_or_remove_same_length_corruption() {
    let f = Fixture::new();
    let reference = publish(&f, b"body");
    let capture = f.capture(b"body", "damaged-shared-content");
    let before = usage(&f);
    let path = blob_path(&f.home, &reference.sha256).unwrap();
    fs::write(&path, b"evil").unwrap();
    // A valid independent owner protects its bytes even when content is bad.
    // Removing a CAP only removes that reference, not the other owner's body.
    let receipt = run(&f.home, id(&capture), &f.inv(id(&capture)).control).unwrap();
    assert_shared(&receipt, id(&capture));
    assert_eq!(fs::read(&path).unwrap(), b"evil");
    assert_eq!(usage(&f), before);
    // The actual reader still rejects the content with its original SHA check.
    let error = match open_verified_blob_bounded(
        &f.home,
        &serde_json::to_value(&reference).unwrap(),
        reference.bytes,
        &OperationControl::default(),
        |_| Ok(()),
    ) {
        Ok(blob) => {
            blob.close().unwrap();
            panic!("corrupt body must fail the real reader");
        }
        Err(error) => error,
    };
    assert_eq!(error.code, "vault.blob_integrity");
    assert_eq!(fs::read(&path).unwrap(), b"evil");
    f.cleanup();
}

#[test]
fn pending_removal_rechecks_data_reference_added_after_prepare_and_preserves_replay() {
    let f = Fixture::new();
    let capture = f.capture(b"body", "prepare-first");
    let id = id(&capture);
    let control = f.inv(id).control;
    let key = format!("evidence/remove/{id}");
    let mut progress = Progress::default();
    Metadata::with_historical_write(&f.home, &control, |writer| {
        writer.with_write_lease(|writer| {
            let row = writer.document(&format!("capture/{id}"))?;
            let capture = dependencies::inspect_capture_projection(&row.bytes, id, &control)?;
            let lease = historical_lease(
                &f.home,
                &format!("blob:{}", capture.signed_blob().0),
                &control,
            )?;
            let result = prepare(&f.home, writer, &key, id, &capture, &control, &mut progress);
            finish_historical(result, lease.close())
        })
    })
    .unwrap();
    let pending = recovery(&f, id).unwrap();
    assert!(
        decode(&pending.bytes, id, &control)
            .unwrap()
            .capture
            .is_some()
    );
    assert!(read_capture(&f.home, id).is_err());
    let reference = publish(&f, b"body");
    let before = usage(&f);
    let receipt = run(&f.home, id, &control).unwrap();
    assert_shared(&receipt, id);
    assert!(
        decode(&recovery(&f, id).unwrap().bytes, id, &control)
            .unwrap()
            .capture
            .is_none()
    );
    assert_eq!(run(&f.home, id, &control).unwrap(), receipt);
    assert_eq!(
        fs::read(blob_path(&f.home, &reference.sha256).unwrap()).unwrap(),
        b"body"
    );
    assert_eq!(usage(&f), before);
    f.cleanup();
}

struct Native<'a>(&'a OperationControl);
impl FileControl for Native<'_> {
    type Error = ToolkitError;
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
    fn charge(&mut self, _: FileCost) -> Result<()> {
        self.0.check()
    }
}

#[test]
fn active_original_intent_fails_closed_then_closed_intent_protects_without_quota_lock() {
    let f = Fixture::new();
    let reference = publish(&f, b"body");
    let capture = f.capture(b"body", "live-intent");
    let raw: [u8; 48] = fs::read(record(&f, &reference.sha256, "intent"))
        .unwrap()
        .try_into()
        .unwrap();
    fs::remove_file(record(&f, &reference.sha256, "intent")).unwrap();
    fs::remove_file(record(&f, &reference.sha256, "complete")).unwrap();
    let control = OperationControl::tracked(
        Instant::now() + Duration::from_secs(5),
        TrackedCancellation::new(),
    );
    let quota = historical_lease(&f.home, "data-derived-v1:quota", &control).unwrap();
    let directory = Directory::open_shared_controlled(
        &DataCachePaths::new(&f.home).shared_blobs(),
        &mut Native(&control),
    )
    .unwrap();
    let mut intent = QuotaIntent::create(&directory, &mut Native(&control)).unwrap();
    assert_eq!(
        intent.publish_record(&raw, &mut Native(&control)).unwrap(),
        StagedPublication::Published
    );
    // The live WRITE/DELETE owner and immutable Reader cannot share on Windows.
    // This is an actual native sharing failure, not an injected absent record.
    let error = run(&f.home, id(&capture), &control).unwrap_err();
    assert_eq!(
        error.code,
        ToolkitError::io(std::io::Error::from_raw_os_error(32)).code
    );
    assert!(read_capture(&f.home, id(&capture)).is_ok());
    assert!(recovery(&f, id(&capture)).is_none());
    assert!(intent.close().is_empty());
    // Quota remains held: a blob -> quota acquisition here would time out.
    let receipt = run(&f.home, id(&capture), &control).unwrap();
    assert_shared(&receipt, id(&capture));
    assert_eq!(control.stop_cause().unwrap(), None);
    directory.close().unwrap();
    quota.close().unwrap();
    assert_eq!(usage(&f).retained_shared_peak_bytes, 8);
    f.cleanup();
}

#[test]
fn uppercase_historical_capture_uses_canonical_data_blob_lease_and_signed_identity() {
    let f = Fixture::new();
    let old = f.capture(b"body", "uppercase");
    let mut capture: CaptureRef = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(
        run(&f.home, id(&old), &f.inv(id(&old)).control).unwrap()["blobRemoved"],
        true
    );
    let reference = publish(&f, b"body");
    capture.blob.sha256 = reference.sha256.to_ascii_uppercase();
    capture.id = capture_identity(&capture).unwrap();
    f.raw(
        &format!("capture/{}", capture.id),
        &storage::identity_json(&capture).unwrap(),
    );
    let control = OperationControl::default();
    let lease = historical_lease(&f.home, &format!("blob:{}", reference.sha256), &control).unwrap();
    let waiting = OperationControl::tracked(
        Instant::now() + Duration::from_secs(5),
        TrackedCancellation::new(),
    );
    let entered = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = entered.clone();
    let expected_key = format!("blob:{}", reference.sha256);
    // The test-only witness does not replace or control the actual lease.
    // Require time remaining at the exact call boundary, so slow metadata
    // admission cannot masquerade as waiting for the already held blob lock.
    BEFORE_BLOB_LEASE.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(move |key, control| {
            assert_eq!(key, expected_key);
            assert!(control.remaining().unwrap() >= Duration::from_secs(2));
            observed.set(true);
        }));
    });
    let result = run(&f.home, &capture.id, &waiting);
    assert!(entered.get());
    assert_eq!(result.unwrap_err().code, "command.deadline");
    assert_eq!(
        waiting.stop_cause().unwrap(),
        Some(StopCause::DeadlineExceeded)
    );
    assert!(read_capture(&f.home, &capture.id).is_ok());
    assert!(recovery(&f, &capture.id).is_none());
    lease.close().unwrap();
    let receipt = run(&f.home, &capture.id, &f.inv(&capture.id).control).unwrap();
    assert_shared(&receipt, &capture.id);
    assert_eq!(
        fs::read(blob_path(&f.home, &reference.sha256).unwrap()).unwrap(),
        b"body"
    );
    assert_eq!(usage(&f).shared_blob_bytes, 4);
    f.cleanup();
}
