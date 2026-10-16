use super::*;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-budget-capture-close-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], ""),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        workspace::initialize(&path).unwrap();
        Metadata::open_controlled(&path, true, &alive())
            .unwrap()
            .close()
            .unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn alive() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn input(bytes: &[u8]) -> CaptureInput<'_> {
    CaptureInput {
        bytes,
        maximum_bytes: bytes.len(),
        media_type: "text/plain",
        provenance: serde_json::json!({"kind":"fixture","locator":"capture-controlled"}),
        complete: true,
        truncated: false,
    }
}
struct ObserveCommitted {
    reader: rusqlite::Connection,
}
impl CaptureAccounting for ObserveCommitted {
    fn check(&self) -> Result<()> {
        let count: i64 = self.reader.query_row(
            "SELECT count(*) FROM documents WHERE key LIKE 'capture/%'",
            [],
            |row| row.get(0),
        )?;
        if count > 0 {
            Err(ToolkitError::new(
                "records.metadata_limit",
                "post-commit observer stopped",
                3,
            ))
        } else {
            Ok(())
        }
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
}
#[test]
fn actual_budgeted_capture_preserves_committed_id_on_post_commit_stop() {
    let f = Fixture::new();
    let invocation = alive();
    let meter = ObserveCommitted {
        reader: rusqlite::Connection::open(f.0.join("state/toolkit.sqlite")).unwrap(),
    };
    let error = commit_capture_budgeted(&f.0, input(b"body"), &invocation, &meter).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.metadata_limit", 3)
    );
    assert_eq!(error.details["schema"], "lycheedev.capture-commit-error.v1");
    assert_eq!(error.details["capturePublication"]["state"], "committed");
    let id = error.details["capturePublication"]["id"].as_str().unwrap();
    assert_eq!(error.details["completedCaptures"], serde_json::json!([id]));
    assert!(error.message.contains(id));
    let capture = super::super::read_capture(&f.0, id).unwrap();
    assert_eq!(capture["id"], id);
    invocation.check().unwrap();
    // An attempted continuation encounters the same sticky meter before work;
    // no implicit retry or second immutable manifest is created.
    assert_eq!(
        commit_capture_budgeted(&f.0, input(b"body"), &invocation, &meter)
            .unwrap_err()
            .code,
        "records.metadata_limit"
    );
    assert_eq!(
        meter
            .reader
            .query_row(
                "SELECT count(*) FROM documents WHERE key LIKE 'capture/%'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    meter.reader.close().unwrap();
}
#[test]
fn actual_verified_capture_has_same_post_commit_state_without_rescanning_body() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"verified body";
    let value = super::super::publish_blob_bounded(&f.0, bytes, bytes.len(), &invocation).unwrap();
    let blob = super::super::open_verified_blob_bounded(
        &f.0,
        &value,
        bytes.len() as u64,
        &invocation,
        |_| Ok(()),
    )
    .unwrap();
    let meter = ObserveCommitted {
        reader: rusqlite::Connection::open(f.0.join("state/toolkit.sqlite")).unwrap(),
    };
    let error = commit_verified_capture_budgeted(
        &blob,
        CaptureHeader {
            media_type: "text/plain",
            provenance: serde_json::json!({"kind":"fixture","locator":"verified"}),
            complete: true,
            truncated: false,
        },
        &invocation,
        &meter,
    )
    .unwrap_err();
    assert_eq!(error.details["capturePublication"]["state"], "committed");
    assert_eq!(
        error.details["completedCaptures"].as_array().unwrap().len(),
        1
    );
    blob.close().unwrap();
    meter.reader.close().unwrap();
}
#[test]
fn capture_completion_preserves_all_cleanup_events_and_never_claims_unknown_commit() {
    let value = serde_json::json!({"id":format!("CAP-{}","a".repeat(64))});
    let primary = ToolkitError::new("records.metadata_limit", "budget", 3)
        .with_details(serde_json::json!({"used":17}));
    let close = ToolkitError::new("vault.close", "same close failure", 5);
    let result = crate::metadata::historical_write::finish_historical::<()>(
        Err(primary.clone()),
        Err(close.clone()),
    );
    let result = crate::metadata::historical_write::finish_historical(result, Err(close));
    let error = capture_result(result, value.clone(), CaptureWriteState::Committed).unwrap_err();
    assert_eq!(
        error.details["primary"]["details"]["primary"]["details"]["primary"],
        serde_json::to_value(&primary).unwrap()
    );
    assert_eq!(
        error.details["primary"]["details"]["cleanup"]["message"],
        "same close failure"
    );
    assert_eq!(
        error.details["primary"]["details"]["primary"]["details"]["cleanup"]["message"],
        "same close failure"
    );
    let unknown = capture_result(Err(primary), value, CaptureWriteState::Unknown).unwrap_err();
    assert_eq!(unknown.details["capturePublication"]["state"], "unknown");
    assert_eq!(unknown.details["completedCaptures"], serde_json::json!([]));
    assert!(unknown.message.contains("do not replay"));
}
