use super::*;
use crate::control::TrackedCancellation;
use std::{sync::atomic::{AtomicU64, Ordering}, time::Instant};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-metadata-consumers-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&root).unwrap();
        let fixture = Self(root);
        crate::workspace::initialize(&fixture.0.join("home")).unwrap();
        Metadata::open(&fixture.home(), true)
            .unwrap()
            .close()
            .unwrap();
        fixture
    }
    fn home(&self) -> std::path::PathBuf {
        self.0.join("home")
    }
    fn blocker(&self, sql: &str) -> Connection {
        let connection = Connection::open(self.home().join("state/toolkit.sqlite")).unwrap();
        connection.busy_timeout(Duration::ZERO).unwrap();
        connection.execute_batch(sql).unwrap();
        connection
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}

#[test]
fn controlled_read_cancelled_after_open_retries_actual_busy_and_restores_timeout() {
    let f = Fixture::new();
    let mut metadata =
        Metadata::open_controlled(&f.home(), true, &OperationControl::default()).unwrap();
    metadata.put("pin/key", &true, None).unwrap();
    let blocker = f.blocker("BEGIN EXCLUSIVE");
    let signal = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(3), signal.clone());
    let mut busy = false;
    let error = metadata
        .read_controlled_observed::<bool>("pin/key", &control, || {
            busy = true;
            signal.cancel();
        })
        .unwrap_err();
    assert!(busy);
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert!(
        metadata
            .read_controlled::<bool>("pin/key", &OperationControl::default())
            .unwrap()
    );
    let timeout: i64 = metadata
        .connection
        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 5000);
    metadata.close().unwrap();
}
#[test]
fn controlled_read_original_deadline_and_missing_record_diagnostics() {
    let f = Fixture::new();
    let metadata =
        Metadata::open_controlled(&f.home(), false, &OperationControl::default()).unwrap();
    let blocker = f.blocker("BEGIN EXCLUSIVE");
    let deadline = Instant::now() + Duration::from_millis(300);
    let control = OperationControl::uncancelable(deadline);
    let mut busy = false;
    let error = metadata
        .read_controlled_observed::<bool>("missing", &control, || busy = true)
        .unwrap_err();
    assert!(busy);
    assert_eq!(error.code, "command.deadline");
    assert_eq!(control.deadline(), deadline);
    assert!(Instant::now() < deadline + Duration::from_secs(2));
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    let old = metadata.read::<bool>("missing").unwrap_err();
    let new = metadata
        .read_controlled::<bool>("missing", &OperationControl::default())
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(old).unwrap(),
        serde_json::to_value(new).unwrap()
    );
    metadata.close().unwrap();
}
#[test]
fn controlled_put_commit_cancellation_explicitly_restores_autocommit() {
    let f = Fixture::new();
    let mut metadata =
        Metadata::open_controlled(&f.home(), true, &OperationControl::default()).unwrap();
    let blocker = f.blocker("BEGIN; SELECT * FROM documents");
    let signal = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(3), signal.clone());
    let mut busy = false;
    let error = metadata
        .put_controlled_observed("capture/stopped", &true, None, &control, || {
            busy = true;
            signal.cancel();
        })
        .unwrap_err();
    assert!(busy);
    assert_eq!(error.code, "command.cancelled");
    assert!(metadata.connection.is_autocommit());
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert_eq!(metadata.generation("capture/stopped").unwrap(), None);
    metadata
        .put_controlled("capture/stopped", &true, None, &OperationControl::default())
        .unwrap();
    assert_eq!(metadata.generation("capture/stopped").unwrap(), Some(1));
    metadata.close().unwrap();
}
#[test]
fn controlled_idempotence_and_conflict_both_release_transaction() {
    let f = Fixture::new();
    let mut metadata =
        Metadata::open_controlled(&f.home(), true, &OperationControl::default()).unwrap();
    let control = OperationControl::default();
    metadata
        .put_controlled("pin/id", &true, None, &control)
        .unwrap();
    metadata
        .put_controlled("pin/id", &true, None, &control)
        .unwrap();
    assert!(metadata.connection.is_autocommit());
    assert_eq!(
        metadata
            .put_controlled("pin/id", &false, None, &control)
            .unwrap_err()
            .code,
        "vault.generation_conflict"
    );
    assert!(metadata.connection.is_autocommit());
    assert_eq!(metadata.generation("pin/id").unwrap(), Some(1));
    let blocker = f.blocker("BEGIN EXCLUSIVE");
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    metadata.close().unwrap();
}
