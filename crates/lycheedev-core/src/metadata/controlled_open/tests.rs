use super::*;
use crate::control::TrackedCancellation;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Instant,
};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-metadata-open-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&root).unwrap();
        let home = root.join("home");
        let fixture = Self { root, home };
        crate::workspace::initialize(&fixture.home).unwrap();
        fixture
    }
    fn database(&self) -> PathBuf {
        self.home.join("state/toolkit.sqlite")
    }
    fn seed(&self, version: i32) {
        let connection = Connection::open(self.database()).unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        connection.close().unwrap();
    }
    fn assert_uninitialized(&self) {
        let connection = Connection::open(self.database()).unwrap();
        let version: i32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        let tables: i64 = connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((version, tables), (0, 0));
        connection.close().unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let removed = std::fs::remove_dir_all(&self.root);
        if !std::thread::panicking() {
            removed.unwrap();
        }
    }
}
fn error(result: Result<Metadata>) -> ToolkitError {
    match result {
        Ok(metadata) => {
            metadata.close().unwrap();
            panic!("expected controlled open to fail");
        }
        Err(error) => error,
    }
}

#[test]
fn pre_stopped_open_has_no_workspace_or_filesystem_side_effects() {
    let f = Fixture::new();
    let absent = f.root.join("never-created");
    let invalid = f.root.join("invalid-workspace");
    std::fs::create_dir(&invalid).unwrap();
    std::fs::write(invalid.join("workspace.json"), b"invalid").unwrap();
    let cancelled = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(10), cancelled.clone());
    cancelled.cancel();
    let expired = OperationControl::uncancelable(Instant::now());
    for (control, code) in [
        (&control, "command.cancelled"),
        (&expired, "command.deadline"),
    ] {
        for home in [&absent, &invalid, &f.home] {
            for write in [false, true] {
                assert_eq!(
                    error(Metadata::open_controlled(home, write, control)).code,
                    code
                );
            }
        }
    }
    assert!(!absent.exists());
    assert!(!invalid.join("state").exists());
    assert!(!invalid.join("locks").exists());
    assert!(!f.database().exists());
    assert_eq!(std::fs::read_dir(f.home.join("locks")).unwrap().count(), 0);
}

#[test]
fn readonly_missing_preserves_compatibility_and_does_not_initialize() {
    let f = Fixture::new();
    let old = error(Metadata::open(&f.home, false));
    let controlled = error(Metadata::open_controlled(
        &f.home,
        false,
        &OperationControl::default(),
    ));
    assert_eq!(
        serde_json::to_value(&controlled).unwrap(),
        serde_json::to_value(&old).unwrap()
    );
    assert_eq!(
        (controlled.code.as_str(), controlled.exit_code),
        ("vault.record_missing", 3)
    );
    assert!(!f.database().exists());
    assert_eq!(std::fs::read_dir(f.home.join("state")).unwrap().count(), 0);
    assert_eq!(std::fs::read_dir(f.home.join("locks")).unwrap().count(), 0);
}

#[test]
fn controlled_schema_versions_match_existing_open_diagnostics() {
    for version in [-1, 0, 1, 2] {
        for write in [false, true] {
            let old = Fixture::new();
            let new = Fixture::new();
            old.seed(version);
            new.seed(version);
            match (
                Metadata::open(&old.home, write),
                Metadata::open_controlled(&new.home, write, &OperationControl::default()),
            ) {
                (Ok(old), Ok(new)) => {
                    old.close().unwrap();
                    new.close().unwrap();
                }
                (Err(old), Err(new)) => assert_eq!(
                    serde_json::to_value(old).unwrap(),
                    serde_json::to_value(new).unwrap()
                ),
                _ => panic!("compatibility differs for version {version}, write {write}"),
            }
            let connection = Connection::open(new.database()).unwrap();
            let actual: i32 = connection
                .pragma_query_value(None, "user_version", |r| r.get(0))
                .unwrap();
            assert_eq!(actual, if version == 0 && write { 1 } else { version });
            connection.close().unwrap();
        }
    }
}

#[test]
fn controlled_open_retains_existing_unversioned_table_compatibility() {
    let f = Fixture::new();
    let connection = Connection::open(f.database()).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('retained')",
        )
        .unwrap();
    connection.close().unwrap();
    let metadata = Metadata::open_controlled(&f.home, true, &OperationControl::default()).unwrap();
    let retained: String = metadata
        .connection
        .query_row("SELECT value FROM unrelated", [], |r| r.get(0))
        .unwrap();
    assert_eq!(retained, "retained");
    metadata.close().unwrap();
}

#[test]
fn controlled_open_returns_same_metadata_and_readonly_remains_readonly() {
    let f = Fixture::new();
    let mut metadata =
        Metadata::open_controlled(&f.home, true, &OperationControl::default()).unwrap();
    let synchronous: i64 = metadata
        .connection
        .pragma_query_value(None, "synchronous", |r| r.get(0))
        .unwrap();
    let timeout: i64 = metadata
        .connection
        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
        .unwrap();
    assert_eq!((synchronous, timeout), (2, 0));
    metadata.put("cache/first", &true, None).unwrap();
    metadata.close().unwrap();
    let mut read = Metadata::open_controlled(&f.home, false, &OperationControl::default()).unwrap();
    assert!(read.read::<bool>("cache/first").unwrap());
    assert_eq!(
        read.put("cache/second", &true, None).unwrap_err().code,
        "vault.metadata"
    );
    read.close().unwrap();
    let mut legacy = Metadata::open(&f.home, true).unwrap();
    let timeout: i64 = legacy
        .connection
        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
        .unwrap();
    assert_eq!(timeout, 5000);
    legacy.put("cache/second", &true, None).unwrap();
    legacy.close().unwrap();
    // The close is consuming and leaves no reader lock or live connection.
    std::fs::remove_file(f.database()).unwrap();
}

#[derive(Clone, Copy, Debug)]
enum Blocked {
    Version,
    Begin,
    Commit,
}
impl Blocked {
    fn hold(self, fixture: &Fixture) -> Connection {
        fixture.seed(0);
        let blocker = Connection::open(fixture.database()).unwrap();
        blocker.busy_timeout(Duration::ZERO).unwrap();
        blocker
            .execute_batch(match self {
                Self::Version => "BEGIN EXCLUSIVE",
                Self::Begin => "BEGIN IMMEDIATE",
                Self::Commit => "BEGIN; SELECT * FROM sqlite_master",
            })
            .unwrap();
        blocker
    }
}
fn cancellation_under_contention(point: Blocked) {
    let f = Fixture::new();
    let blocker = point.hold(&f);
    let cancellation = TrackedCancellation::new();
    let control = OperationControl::tracked(
        Instant::now() + Duration::from_secs(3),
        cancellation.clone(),
    );
    let start = Instant::now();
    let (send, receive) = mpsc::sync_channel(1);
    let mut observed = false;
    let stopped = std::thread::scope(|scope| {
        // Only actual SQLITE_BUSY wakes cancellation; no timing-only guess.
        let worker = scope.spawn(move || {
            let received = receive.recv_timeout(Duration::from_secs(2));
            if received.is_ok() {
                cancellation.cancel();
            }
            received
        });
        let result = Metadata::open_controlled_observed(&f.home, true, &control, || {
            if !observed {
                observed = true;
                send.try_send(()).unwrap();
            }
        });
        drop(send);
        worker.join().unwrap().unwrap();
        error(result)
    });
    assert!(observed, "no SQLite contention observed at {point:?}");
    assert_eq!(
        (stopped.code.as_str(), stopped.exit_code),
        ("command.cancelled", 7)
    );
    assert!(start.elapsed() < Duration::from_secs(3));
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    f.assert_uninitialized();
    // Rollback and both close paths must permit a subsequent healthy open.
    Metadata::open_controlled(&f.home, true, &OperationControl::default())
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn cancellation_during_real_sqlite_schema_read_closes_connection() {
    cancellation_under_contention(Blocked::Version);
}

#[test]
fn cancellation_during_real_sqlite_begin_preserves_empty_schema() {
    cancellation_under_contention(Blocked::Begin);
}

#[test]
fn cancellation_during_real_sqlite_commit_rolls_back_table_and_version() {
    cancellation_under_contention(Blocked::Commit);
}

#[test]
fn original_deadline_bounds_schema_read_begin_and_commit_contention() {
    for point in [Blocked::Version, Blocked::Begin, Blocked::Commit] {
        let f = Fixture::new();
        let blocker = point.hold(&f);
        let deadline = Instant::now() + Duration::from_millis(300);
        let control = OperationControl::uncancelable(deadline);
        let mut observed = false;
        let stopped = error(Metadata::open_controlled_observed(
            &f.home,
            true,
            &control,
            || observed = true,
        ));
        assert!(observed, "no SQLite contention observed at {point:?}");
        assert_eq!(
            (stopped.code.as_str(), stopped.exit_code),
            ("command.deadline", 5)
        );
        assert_eq!(control.deadline(), deadline);
        assert!(Instant::now() < deadline + Duration::from_secs(2));
        blocker.execute_batch("ROLLBACK").unwrap();
        blocker.close().unwrap();
        f.assert_uninitialized();
        Metadata::open_controlled(&f.home, true, &OperationControl::default())
            .unwrap()
            .close()
            .unwrap();
    }
}

#[test]
fn sqlite_schema_error_rolls_back_and_allows_recovery() {
    let f = Fixture::new();
    let connection = Connection::open(f.database()).unwrap();
    connection
        .execute_batch("CREATE TABLE holder(value); CREATE INDEX documents ON holder(value)")
        .unwrap();
    connection.close().unwrap();
    let failed = error(Metadata::open_controlled(
        &f.home,
        true,
        &OperationControl::default(),
    ));
    assert_eq!(failed.code, "vault.metadata");
    let connection = Connection::open(f.database()).unwrap();
    let version: i32 = connection
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 0);
    connection
        .execute_batch("BEGIN EXCLUSIVE; DROP INDEX documents; COMMIT")
        .unwrap();
    connection.close().unwrap();
    Metadata::open_controlled(&f.home, true, &OperationControl::default())
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn concurrent_controlled_initializers_join_and_release_schema_lease() {
    let f = Fixture::new();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(3));
    let outcomes = std::thread::scope(|scope| {
        let home = &f.home;
        let control = &control;
        let first = scope.spawn(move || {
            Metadata::open_controlled(home, true, control).and_then(Metadata::close)
        });
        let second = Metadata::open_controlled(home, true, control).and_then(Metadata::close);
        (first.join().unwrap(), second)
    });
    outcomes.0.unwrap();
    outcomes.1.unwrap();
    Metadata::open_controlled(&f.home, false, &OperationControl::default())
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn cleanup_combination_preserves_primary_rollback_and_close_causes() {
    // Typed-cause composition model only: not a claim of OS/SQLite close-fault execution.
    let primary = ToolkitError::new("command.cancelled", "cancelled", 7)
        .with_details(serde_json::json!({"original": true}));
    let rollback = ToolkitError::new("vault.metadata", "rollback failed", 5)
        .with_details(serde_json::json!({"step": "rollback"}));
    let close = ToolkitError::new("vault.metadata", "close failed", 5)
        .with_details(serde_json::json!({"step": "close"}));
    let result: Result<()> = finish_historical(Err(primary.clone()), Err(rollback.clone()));
    let combined = finish_historical(result, Err(close.clone())).unwrap_err();
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
        serde_json::to_value(rollback).unwrap()
    );
    assert_eq!(
        combined.details["cleanup"],
        serde_json::to_value(close).unwrap()
    );
}
