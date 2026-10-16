use super::*;
use crate::control::{StopCause, TrackedCancellation};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
thread_local! { static AFTER_CLOSE:RefCell<Option<Box<dyn FnOnce()->Result<()>>>>=RefCell::new(None); }
pub(super) fn after_blob_close() -> Result<()> {
    let hook = AFTER_CLOSE.with(|slot| slot.borrow_mut().take());
    hook.map_or(Ok(()), |hook| hook())
}
fn with_after_close<T>(hook: impl FnOnce() -> Result<()> + 'static, run: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            AFTER_CLOSE.with(|slot| {
                slot.borrow_mut().take();
            });
        }
    }
    AFTER_CLOSE.with(|slot| {
        assert!(slot.borrow().is_none());
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    let _reset = Reset;
    run()
}
struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-remove-r40-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self { home }
    }
    fn inv(&self, id: &str) -> Invocation {
        Invocation {
            command: "evidence remove".into(),
            positional: Some(id.into()),
            cwd: self.home.clone(),
            home: self.home.clone(),
            flags: BTreeMap::new(),
            control: OperationControl::tracked(
                Instant::now() + Duration::from_secs(30),
                TrackedCancellation::new(),
            ),
        }
    }
    fn capture(&self, body: &[u8], locator: &str) -> Value {
        commit_capture_bounded(
            &self.home,
            CaptureInput {
                bytes: body,
                maximum_bytes: body.len(),
                media_type: "text/plain",
                provenance: json!({"kind":"fixture","locator":locator}),
                complete: true,
                truncated: false,
            },
            &OperationControl::default(),
        )
        .unwrap()
    }
    fn raw(&self, key: &str, body: &[u8]) {
        Metadata::with_historical_write(&self.home, &OperationControl::default(), |writer| {
            writer.commit(&[crate::metadata::historical_write::HistoricalMutation {
                key,
                expected_generation: 0,
                bytes: body,
            }])
        })
        .unwrap();
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.home).unwrap();
    }
}
fn id(capture: &Value) -> &str {
    capture["id"].as_str().unwrap()
}
#[test]
fn default_remove_accepts_future_retention_schema_but_keep_still_rejects_it() {
    let f = Fixture::new();
    let capture = f.capture(b"body", "future");
    let id = id(&capture);
    f.raw(
        &format!("evidence/keep/{id}"),
        format!(
            r#"{{"schema":"future","captureId":"{id}","blob":{},"keptAt":null}}"#,
            capture["blob"]
        )
        .as_bytes(),
    );
    let mut keep = f.inv(id);
    keep.command = "evidence keep".into();
    assert_eq!(
        super::super::dispatch(&keep).unwrap_err().code,
        "evidence.invalid_retention"
    );
    let inv = f.inv(id);
    let receipt = super::super::dispatch(&inv).unwrap().data;
    assert_eq!(
        receipt,
        json!({"captureId":id,"manifestRemoved":true,"retentionRemoved":true,"blobRemoved":true,"blobShared":false,"blobMissing":false})
    );
    assert_eq!(super::super::dispatch(&inv).unwrap().data, receipt);
    assert_eq!(inv.control.stop_cause().unwrap(), None);
    inv.control.ordered_child(inv.control.deadline()).unwrap();
    f.cleanup();
}
#[test]
fn removal_retention_validates_capture_and_signed_blob_not_ignored_schema_or_marshal_time() {
    for mismatch in ["capture", "digest", "length"] {
        let f = Fixture::new();
        let capture = f.capture(b"body", mismatch);
        let id = id(&capture);
        let mut keep = json!({"schema":"future","captureId":id,"blob":capture["blob"]});
        match mismatch {
            "capture" => keep["captureId"] = json!("other"),
            "digest" => keep["blob"]["sha256"] = json!("0".repeat(64)),
            _ => keep["blob"]["bytes"] = json!(-1),
        }
        f.raw(
            &format!("evidence/keep/{id}"),
            &serde_json::to_vec(&keep).unwrap(),
        );
        assert_eq!(
            run(&f.home, id, &f.inv(id).control).unwrap_err().code,
            "evidence.invalid_retention"
        );
        assert!(read_capture(&f.home, id).is_ok());
        f.cleanup();
    }
    let f = Fixture::new();
    let capture = f.capture(b"body", "time");
    let id = id(&capture);
    // Fixed Go time parser accepts +24:00, but Time.MarshalJSON rejects it.
    // Remove does not marshal this unused field; keep does.
    f.raw(
        &format!("evidence/keep/{id}"),
        format!(
            r#"{{"captureId":"{id}","blob":{},"keptAt":"2026-10-15T00:00:00+24:00"}}"#,
            capture["blob"]
        )
        .as_bytes(),
    );
    assert!(run(&f.home, id, &f.inv(id).control).is_ok());
    f.cleanup();
}
#[test]
fn raw_literal_reference_matches_keys_but_does_not_normalize_escaped_values() {
    for literal in [false, true] {
        let f = Fixture::new();
        let capture = f.capture(b"body", "reference");
        let id = id(&capture);
        let raw = if literal {
            format!(r#"{{"capture:{id}":0}}"#)
        } else {
            format!(r#"{{"id":"\u0043{}"}}"#, &id[1..])
        };
        f.raw("operation/reference", raw.as_bytes());
        let result = run(&f.home, id, &f.inv(id).control);
        if literal {
            assert_eq!(result.unwrap_err().code, "evidence.capture_referenced");
            assert!(read_capture(&f.home, id).is_ok());
        } else {
            assert_eq!(result.unwrap()["blobRemoved"], true);
        }
        f.cleanup();
    }
}
#[test]
fn sharing_only_uses_other_typed_capture_blobs_and_preserves_duplicate_object_merge() {
    for typed in [false, true] {
        let f = Fixture::new();
        let capture = f.capture(b"body", "sharing");
        let id = id(&capture);
        let key = if typed {
            "capture/unvalidated-historical"
        } else {
            "operation/blob-shaped"
        };
        let raw = format!(
            r#"{{"blob":{{"sha256":"{}"}},"BLOB":{{"bytes":4}},"mediaType":"text/plain"}}"#,
            capture["blob"]["sha256"].as_str().unwrap()
        );
        f.raw(key, raw.as_bytes());
        let receipt = run(&f.home, id, &f.inv(id).control).unwrap();
        assert_eq!(receipt["blobShared"], typed);
        assert_eq!(receipt["blobRemoved"], !typed);
        let path = blob_path(&f.home, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
        assert_eq!(path.exists(), typed);
        f.cleanup();
    }
}
#[test]
fn absent_original_file_or_shard_is_missing_after_checked_native_lookup() {
    for shard in [false, true] {
        let f = Fixture::new();
        let capture = f.capture(b"body", "missing");
        let id = id(&capture);
        let path = blob_path(&f.home, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
        fs::remove_file(&path).unwrap();
        if shard {
            fs::remove_dir(path.parent().unwrap()).unwrap();
        }
        let receipt = run(&f.home, id, &f.inv(id).control).unwrap();
        assert_eq!(receipt["blobMissing"], true);
        assert_eq!(receipt["blobRemoved"], false);
        f.cleanup();
    }
}
#[test]
fn delete_sharing_denial_preserves_pending_receipt_and_resume_is_idempotent() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let capture = f.capture(b"body", "deny-delete");
    let id = id(&capture);
    let path = blob_path(&f.home, capture["blob"]["sha256"].as_str().unwrap()).unwrap();
    let external = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    let error = run(&f.home, id, &f.inv(id).control).unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(error.details["result"]["manifestRemoved"], true);
    assert_eq!(error.details["resumeCaptureId"], id);
    assert!(read_capture(&f.home, id).is_err());
    assert!(path.exists());
    storage::close_file(external).unwrap();
    let result = run(&f.home, id, &f.inv(id).control).unwrap();
    assert_eq!(result["blobRemoved"], true);
    assert_eq!(run(&f.home, id, &f.inv(id).control).unwrap(), result);
    f.cleanup();
}
#[test]
fn successful_disposition_followed_by_injected_cleanup_failure_keeps_deleted_fact_and_recovery() {
    let f = Fixture::new();
    let capture = f.capture(b"body", "late-close");
    let id = id(&capture);
    let control = f.inv(id).control;
    let error = with_after_close(
        || {
            Err(ToolkitError::new(
                "fixture.close",
                "injected after real body close",
                5,
            ))
        },
        || run(&f.home, id, &control),
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.close");
    assert_eq!(error.details["result"]["blobRemoved"], true);
    assert_eq!(
        error.details["removalPublication"]["blobDisposition"],
        "deleted"
    );
    assert_eq!(
        error.details["removalPublication"]["recoveryComplete"],
        false
    );
    assert!(
        !blob_path(&f.home, capture["blob"]["sha256"].as_str().unwrap())
            .unwrap()
            .exists()
    );
    let resumed = run(&f.home, id, &control).unwrap();
    assert_eq!(resumed["blobRemoved"], true);
    assert_eq!(resumed["blobMissing"], false);
    f.cleanup();
}
#[test]
fn stop_after_real_delete_and_cleanup_failure_preserves_both_causes_without_replaying_mutation() {
    let f = Fixture::new();
    let capture = f.capture(b"body", "late-stop");
    let id = id(&capture);
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let error = with_after_close(
        move || {
            token.cancel();
            Err(ToolkitError::new("fixture.close", "late cleanup", 5))
        },
        || run(&f.home, id, &control),
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.close");
    assert_eq!(error.details["result"]["blobRemoved"], true);
    let raw = serde_json::to_string(&error.details).unwrap();
    assert!(raw.contains("command.cancelled"));
    assert!(raw.contains("fixture.close"));
    assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
    assert_eq!(
        run(&f.home, id, &f.inv(id).control).unwrap()["blobRemoved"],
        true
    );
    f.cleanup();
}
#[test]
fn references_after_first_thousand_rows_are_not_hidden_by_an_early_page() {
    let f = Fixture::new();
    let capture = f.capture(b"body", "page");
    let id = id(&capture);
    let connection = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    connection.execute_batch("BEGIN; WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<1001) INSERT INTO documents SELECT printf('aaa/%04d',x),1,'null' FROM n; COMMIT;").unwrap();
    connection.close().map_err(|(_, e)| e).unwrap();
    f.raw(
        "zzz/reference",
        serde_json::to_string(id).unwrap().as_bytes(),
    );
    let error = run(&f.home, id, &f.inv(id).control).unwrap_err();
    assert_eq!(error.code, "evidence.capture_referenced");
    assert_eq!(error.details["documentKey"], "zzz/reference");
    assert!(read_capture(&f.home, id).is_ok());
    f.cleanup();
}
#[test]
fn original_precancel_precedes_invalid_id_without_creating_a_recovery_record() {
    let f = Fixture::new();
    let token = TrackedCancellation::new();
    token.cancel();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), token);
    assert_eq!(
        run(&f.home, "invalid", &control).unwrap_err().code,
        "command.cancelled"
    );
    assert!(control.stop_cause().is_ok());
    f.cleanup();
}

#[test]
fn historical_missing_blob_above_production_payload_cap_is_not_a_read_limit() {
    let f = Fixture::new();
    let mut capture: CaptureRef =
        serde_json::from_value(f.capture(b"seed", "signed-large")).unwrap();
    capture.id.clear();
    capture.blob.bytes = MAX_CAPTURE_BYTES + 1;
    capture.blob.sha256 = "f".repeat(64);
    capture.id = capture_identity(&capture).unwrap();
    f.raw(
        &format!("capture/{}", capture.id),
        &storage::identity_json(&capture).unwrap(),
    );
    let receipt = run(&f.home, &capture.id, &f.inv(&capture.id).control).unwrap();
    assert_eq!(receipt["blobMissing"], true);
    f.cleanup();
}

#[test]
fn malformed_absent_selector_uses_manifest_lookup_after_workspace_admission() {
    let f = Fixture::new();
    let error = run(&f.home, "not-a-capture", &f.inv("not-a-capture").control).unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    f.cleanup();
}
