use super::*;
use crate::{
    control::TrackedCancellation,
    selection::{self, SelectionSpec, SourcePin},
};
use std::time::{Duration, Instant};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-capture-controlled-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        let f = Self(root);
        workspace::initialize(&f.home()).unwrap();
        f
    }
    fn home(&self) -> PathBuf {
        self.0.join("home")
    }
    fn count(&self) -> usize {
        if !self.home().join("state/toolkit.sqlite").exists() {
            return 0;
        }
        let m = Metadata::open(&self.home(), false).unwrap();
        let count = m.list::<Value>("capture/", 10).unwrap().len();
        m.close().unwrap();
        count
    }
    fn assert_lock_released(&self, bytes: &[u8]) {
        ExclusiveLock::resource(
            &self.home().join("locks"),
            &format!("blob:{}", storage::sha256(bytes)),
        )
        .unwrap()
        .close()
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let result = fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn input(bytes: &[u8]) -> CaptureInput<'_> {
    CaptureInput {
        bytes,
        maximum_bytes: bytes.len(),
        media_type: "application/json",
        provenance: serde_json::json!({"kind":"fixture","locator":"controlled-capture"}),
        complete: true,
        truncated: false,
    }
}
fn cancellation() -> (OperationControl, TrackedCancellation) {
    let signal = TrackedCancellation::new();
    (
        OperationControl::tracked(Instant::now() + Duration::from_secs(3), signal.clone()),
        signal,
    )
}
#[test]
fn capture_precancel_and_stop_before_blob_leave_no_publication() {
    let f = Fixture::new();
    let (c, s) = cancellation();
    s.cancel();
    let absent = f.0.join("absent");
    assert_eq!(
        commit_capture_bounded(&absent, input(b"{}"), &c)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(!absent.exists());
    let (c, s) = cancellation();
    let error = commit_internal_observed(&f.home(), input(b"{}"), Some(&c), |stage| {
        if matches!(stage, CapturePublication::BeforeBlob) {
            s.cancel();
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(fs::read_dir(f.home().join("blobs"))
        .unwrap()
        .next()
        .is_none());
    assert_eq!(f.count(), 0);
    f.assert_lock_released(b"{}");
}
#[test]
fn cancellation_after_blob_preserves_original_bytes_without_manifest_or_replay() {
    let f = Fixture::new();
    let bytes = b"{\"original\":true}";
    let (c, s) = cancellation();
    let mut published = 0;
    let error = commit_internal_observed(&f.home(), input(bytes), Some(&c), |stage| {
        if matches!(stage, CapturePublication::BlobPublished) {
            published += 1;
            s.cancel();
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(published, 1);
    assert_eq!(
        fs::read(blob_path(&f.home(), &storage::sha256(bytes)).unwrap()).unwrap(),
        bytes
    );
    assert_eq!(f.count(), 0);
    assert!(!f.home().join("state/toolkit.sqlite").exists());
    f.assert_lock_released(bytes);
    assert_eq!(
        commit_capture_bounded(&f.home(), input(bytes), &c)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(f.count(), 0);
}
#[test]
fn cancellation_after_manifest_preserves_exact_capture_and_separate_pin_transaction() {
    let f = Fixture::new();
    let pin = selection::pin_selection_controlled(
        &f.home(),
        SelectionSpec {
            source: Some(SourcePin {
                repository: "wow-ui-source".into(),
                product: "retail".into(),
                requested_ref: "fixture".into(),
                exact_commit: "b".repeat(40),
                parser_revision: "fixture".into(),
            }),
            ..Default::default()
        },
        &OperationControl::default(),
    )
    .unwrap();
    let bytes = b"{\"capture\":true}";
    let (c, s) = cancellation();
    let mut committed = 0;
    let error = commit_internal_observed(&f.home(), input(bytes), Some(&c), |stage| {
        if matches!(stage, CapturePublication::ManifestPublished) {
            committed += 1;
            s.cancel();
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(committed, 1);
    assert_eq!(f.count(), 1);
    let m = Metadata::open(&f.home(), false).unwrap();
    let capture = m.list::<Value>("capture/", 10).unwrap().pop().unwrap();
    let typed: CaptureRef = serde_json::from_value(capture.clone()).unwrap();
    assert_eq!(typed.id, capture_identity(&typed).unwrap());
    assert_eq!(
        m.generation(&format!("capture/{}", typed.id)).unwrap(),
        Some(1)
    );
    assert_eq!(m.generation(&format!("pin/{}", pin.id)).unwrap(), Some(1));
    m.close().unwrap();
    assert_eq!(
        super::super::read_capture(&f.home(), &typed.id).unwrap(),
        capture
    );
    assert_eq!(
        commit_capture_bounded(&f.home(), input(bytes), &c)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(f.count(), 1);
    assert_eq!(
        selection::load_pinned_controlled(&f.home(), &pin.id, &OperationControl::default())
            .unwrap()
            .id,
        pin.id
    );
    f.assert_lock_released(bytes);
}
#[test]
fn capture_metadata_contention_uses_original_deadline_and_retains_blob() {
    let f = Fixture::new();
    Metadata::open(&f.home(), true).unwrap().close().unwrap();
    let blocker = rusqlite::Connection::open(f.home().join("state/toolkit.sqlite")).unwrap();
    blocker.busy_timeout(Duration::ZERO).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let deadline = Instant::now() + Duration::from_millis(300);
    let c = OperationControl::uncancelable(deadline);
    let bytes = b"{\"deadline\":true}";
    let error = commit_capture_bounded(&f.home(), input(bytes), &c).unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert_eq!(c.deadline(), deadline);
    assert!(Instant::now() < deadline + Duration::from_secs(2));
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert_eq!(f.count(), 0);
    assert_eq!(
        fs::read(blob_path(&f.home(), &storage::sha256(bytes)).unwrap()).unwrap(),
        bytes
    );
    f.assert_lock_released(bytes);
    commit_capture_bounded(&f.home(), input(bytes), &OperationControl::default()).unwrap();
    assert_eq!(f.count(), 1);
}
#[test]
fn capture_blob_lock_contention_waits_for_original_stop_without_publication() {
    let f = Fixture::new();
    let bytes = b"{\"busy\":true}";
    let held = ExclusiveLock::resource(
        &f.home().join("locks"),
        &format!("blob:{}", storage::sha256(bytes)),
    )
    .unwrap();
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    ExclusiveLock::set_blob_wait_observer(move || token.cancel());
    let error = commit_capture_bounded(&f.home(), input(bytes), &control).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert_eq!(
        ExclusiveLock::resource(
            &f.home().join("locks"),
            &format!("blob:{}", storage::sha256(bytes)),
        )
        .err()
        .unwrap()
        .code,
        "workspace.busy",
    );
    assert_eq!(f.count(), 0);
    assert!(!blob_path(&f.home(), &storage::sha256(bytes))
        .unwrap()
        .exists());
    held.close().unwrap();
    f.assert_lock_released(bytes);
}
#[test]
fn capture_existing_blob_integrity_failure_closes_guard_without_overwriting() {
    let f = Fixture::new();
    let bytes = b"{\"expected\":true}";
    let path = blob_path(&f.home(), &storage::sha256(bytes)).unwrap();
    fs::create_dir(path.parent().unwrap()).unwrap();
    fs::write(&path, b"wrong").unwrap();
    let error =
        commit_capture_bounded(&f.home(), input(bytes), &OperationControl::default()).unwrap_err();
    assert_eq!(error.code, "vault.blob_integrity");
    assert_eq!(fs::read(&path).unwrap(), b"wrong");
    assert_eq!(f.count(), 0);
    f.assert_lock_released(bytes);
}
#[test]
fn capture_cleanup_join_keeps_full_primary_and_multiple_close_causes() {
    use crate::metadata::historical_write::finish_historical;
    let primary = ToolkitError::new("command.cancelled", "stopped", 7)
        .with_details(serde_json::json!({"source":"capture"}));
    let metadata = ToolkitError::new("vault.metadata", "close failed", 5);
    let lock = ToolkitError::new("external.io", "lock close failed", 5);
    let result: Result<()> = finish_historical(Err(primary.clone()), Err(metadata.clone()));
    let combined = finish_historical(result, Err(lock.clone())).unwrap_err();
    assert_eq!(
        (combined.code.as_str(), combined.exit_code),
        ("command.cancelled", 7)
    );
    assert_eq!(
        combined.details["primary"]["details"]["primary"],
        serde_json::to_value(primary).unwrap()
    );
    assert_eq!(
        combined.details["primary"]["details"]["cleanup"],
        serde_json::to_value(metadata).unwrap()
    );
    assert_eq!(
        combined.details["cleanup"],
        serde_json::to_value(lock).unwrap()
    );
}
