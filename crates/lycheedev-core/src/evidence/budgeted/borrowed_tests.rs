//! Real SQLite/capture regressions for one request-owned connection. These tests
//! never replace publication or suppress the caller's checked final close.
use super::*;
use crate::control::TrackedCancellation;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-borrowed-captures-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], ""),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        workspace::initialize(&path).unwrap();
        Self(path)
    }
    fn writer(&self, control: &OperationControl) -> Metadata {
        Metadata::open_controlled(&self.0, true, control).unwrap()
    }
    fn reader(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.0.join("state/toolkit.sqlite")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
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
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn input(stage: usize) -> CaptureInput<'static> {
    let bytes: &[u8] = [b"source".as_slice(), b"artifact", b"manifest"][stage];
    CaptureInput {
        bytes,
        maximum_bytes: 64,
        media_type: "application/octet-stream",
        provenance: serde_json::json!({"kind":"fixture","locator":format!("stage:{stage}")}),
        complete: true,
        truncated: false,
    }
}
fn count(reader: &rusqlite::Connection) -> usize {
    reader
        .query_row(
            "SELECT count(*) FROM documents WHERE key LIKE 'capture/%'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}
fn save(
    f: &Fixture,
    writer: &mut Metadata,
    stage: usize,
    ids: &[String],
    control: &OperationControl,
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    let dependencies: Vec<_> = ids.iter().map(String::as_str).collect();
    commit_capture_budgeted_with_dependencies_and_metadata(
        &f.0,
        writer,
        input(stage),
        &dependencies,
        control,
        meter,
    )
}

#[test]
fn borrowed_capture_writer_commits_three_independent_visible_transactions() {
    let f = Fixture::new();
    let control = control();
    let mut writer = f.writer(&control);
    #[cfg(windows)]
    {
        // The original request owner itself denies delete-sharing throughout
        // the request, even before its first borrowed capture transaction.
        assert!(
            std::fs::rename(
                f.0.join("state/toolkit.sqlite"),
                f.0.join("state/replaced.sqlite")
            )
            .is_err()
        );
    }
    let reader = f.reader();
    let mut ids = Vec::new();
    for stage in 0..3 {
        let saved = save(&f, &mut writer, stage, &ids, &control, &Meter).unwrap();
        ids.push(saved["id"].as_str().unwrap().to_owned());
        assert_eq!(count(&reader), stage + 1);
        writer.check_capture_binding(&f.0).unwrap();
        // Another connection can become a writer between capture calls. Codec
        // and output therefore cannot inherit a retained SQLite transaction.
        reader.execute_batch("BEGIN IMMEDIATE; ROLLBACK;").unwrap();
        assert_eq!(
            super::super::read_capture(&f.0, ids.last().unwrap()).unwrap(),
            saved
        );
    }
    writer.close().unwrap();
    assert_eq!(count(&reader), 3);
    reader.close().unwrap();
    #[cfg(windows)]
    {
        let database = f.0.join("state/toolkit.sqlite");
        let moved = f.0.join("state/closed.sqlite");
        std::fs::rename(&database, &moved).unwrap();
        std::fs::rename(&moved, &database).unwrap();
    }
}

#[test]
fn borrowed_capture_rechecks_deleted_dependency_in_each_transaction() {
    let f = Fixture::new();
    let control = control();
    let mut writer = f.writer(&control);
    let source = save(&f, &mut writer, 0, &[], &control, &Meter).unwrap();
    let id = source["id"].as_str().unwrap().to_owned();
    let mut remover = f.writer(&control);
    assert!(remover.delete(&format!("capture/{id}")).unwrap());
    remover.close().unwrap();
    let error = save(&f, &mut writer, 1, &[id], &control, &Meter).unwrap_err();
    assert_eq!(error.code, "vault.record_missing");
    writer.check_capture_binding(&f.0).unwrap();
    writer.close().unwrap();
    let reader = f.reader();
    assert_eq!(count(&reader), 0);
    reader.close().unwrap();
}

#[test]
fn borrowed_capture_rejects_different_workspace_and_read_only_owner() {
    let first = Fixture::new();
    let second = Fixture::new();
    let control = control();
    let mut writer = first.writer(&control);
    let error = save(&second, &mut writer, 0, &[], &control, &Meter).unwrap_err();
    assert!(error.message.contains("different workspace"));
    assert!(!second.0.join("state/toolkit.sqlite").exists());
    writer.close().unwrap();
    let mut readonly = Metadata::open_controlled(&first.0, false, &control).unwrap();
    let error = save(&first, &mut readonly, 0, &[], &control, &Meter).unwrap_err();
    assert!(error.message.contains("read-only"));
    readonly.close().unwrap();
    let reader = first.reader();
    assert_eq!(count(&reader), 0);
    reader.close().unwrap();
}

#[test]
fn borrowed_capture_retains_live_schema_version_authentication() {
    let f = Fixture::new();
    let control = control();
    let mut writer = f.writer(&control);
    let changer = f.reader();
    changer.execute_batch("PRAGMA user_version=2;").unwrap();
    let error = save(&f, &mut writer, 0, &[], &control, &Meter).unwrap_err();
    assert_eq!(error.code, "vault.schema_newer");
    assert_eq!(count(&changer), 0);
    changer.execute_batch("PRAGMA user_version=1;").unwrap();
    save(&f, &mut writer, 0, &[], &control, &Meter).unwrap();
    writer.close().unwrap();
    changer.close().unwrap();
}

struct StopAfterCommit {
    reader: rusqlite::Connection,
    threshold: usize,
    cancel: Option<TrackedCancellation>,
    control: OperationControl,
}
impl CaptureAccounting for StopAfterCommit {
    fn check(&self) -> Result<()> {
        if count(&self.reader) >= self.threshold {
            if let Some(cancel) = &self.cancel {
                cancel.cancel();
                self.control.check()
            } else {
                Err(ToolkitError::new(
                    "records.metadata_limit",
                    "post-commit budget stop",
                    3,
                ))
            }
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
fn borrowed_source_artifact_manifest_keep_each_post_commit_stop_and_never_replay() {
    for stage in 0..3 {
        for cancel in [false, true] {
            let f = Fixture::new();
            let token = TrackedCancellation::new();
            let control =
                OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
            let mut writer = f.writer(&control);
            let mut ids = Vec::new();
            for prior in 0..stage {
                let saved = save(&f, &mut writer, prior, &ids, &control, &Meter).unwrap();
                ids.push(saved["id"].as_str().unwrap().to_owned());
            }
            let meter = StopAfterCommit {
                reader: f.reader(),
                threshold: stage + 1,
                cancel: cancel.then_some(token),
                control: control.clone(),
            };
            let error = save(&f, &mut writer, stage, &ids, &control, &meter).unwrap_err();
            assert_eq!(
                error.code,
                if cancel {
                    "command.cancelled"
                } else {
                    "records.metadata_limit"
                }
            );
            assert_eq!(error.details["capturePublication"]["state"], "committed");
            let id = error.details["capturePublication"]["id"].as_str().unwrap();
            assert_eq!(error.details["completedCaptures"], serde_json::json!([id]));
            assert_eq!(count(&meter.reader), stage + 1);
            assert!(save(&f, &mut writer, stage, &ids, &control, &meter).is_err());
            assert_eq!(count(&meter.reader), stage + 1);
            // Already committed identities survive even though the same sticky
            // cancellation cannot be used for further production work.
            for prior in ids.iter().map(String::as_str).chain(std::iter::once(id)) {
                assert_eq!(
                    super::super::read_capture(&f.0, prior).unwrap()["id"],
                    prior
                );
            }
            writer.close().unwrap();
            meter.reader.close().unwrap();
        }
    }
}

#[test]
fn borrowed_source_artifact_manifest_insert_failure_keeps_earlier_commits_only() {
    for stage in 0..3 {
        let f = Fixture::new();
        let control = control();
        let mut writer = f.writer(&control);
        let mut ids = Vec::new();
        for prior in 0..stage {
            let saved = save(&f, &mut writer, prior, &ids, &control, &Meter).unwrap();
            ids.push(saved["id"].as_str().unwrap().to_owned());
        }
        let reader = f.reader();
        reader.execute_batch("CREATE TRIGGER reject_capture BEFORE INSERT ON documents WHEN NEW.key LIKE 'capture/%' BEGIN SELECT RAISE(ABORT,'fixture capture insert'); END;").unwrap();
        let error = save(&f, &mut writer, stage, &ids, &control, &Meter).unwrap_err();
        assert!(error.message.contains("fixture capture insert"));
        assert!(error.details.get("capturePublication").is_none());
        assert_eq!(count(&reader), stage);
        writer.check_capture_binding(&f.0).unwrap();
        for id in ids {
            assert_eq!(super::super::read_capture(&f.0, &id).unwrap()["id"], id);
        }
        writer.close().unwrap();
        reader.close().unwrap();
    }
}

#[test]
fn borrowed_verified_capture_records_commit_before_late_cancellation() {
    let f = Fixture::new();
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let mut writer = f.writer(&control);
    let reference = super::super::publish_blob_bounded(&f.0, b"source", 6, &control).unwrap();
    let blob = super::super::open_verified_blob_bounded(&f.0, &reference, 6, &control, |_| Ok(()))
        .unwrap();
    let meter = StopAfterCommit {
        reader: f.reader(),
        threshold: 1,
        cancel: Some(token),
        control: control.clone(),
    };
    let error = commit_verified_capture_budgeted_with_metadata(
        &blob,
        &mut writer,
        CaptureHeader {
            media_type: "application/octet-stream",
            provenance: serde_json::json!({"kind":"fixture","locator":"source"}),
            complete: true,
            truncated: false,
        },
        &control,
        &meter,
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.details["capturePublication"]["state"], "committed");
    assert_eq!(count(&meter.reader), 1);
    assert_eq!(blob.close().unwrap_err().code, "command.cancelled");
    writer.close().unwrap();
    meter.reader.close().unwrap();
}
