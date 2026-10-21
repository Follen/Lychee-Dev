use super::*;
use crate::control::TrackedCancellation;
use std::sync::atomic::{AtomicU64, Ordering};
fn home() -> std::path::PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "lychee-removal-txn-{}-{}-{}",
        std::process::id(),
        crate::storage::utc_now().replace([':', '.'], "_"),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    crate::workspace::initialize(&path).unwrap();
    path
}
fn seed_historical_capture(home: &std::path::Path) {
    let metadata = Metadata::open(home, true).unwrap();
    // Model an existing historical CAP without routing a new reserved-key
    // mutation after the removal writer has acquired its write lease.
    metadata
        .connection
        .execute(
            INSERT_DOCUMENT,
            params!["capture/fixture", b"null".as_slice()],
        )
        .unwrap();
    metadata.close().unwrap();
}
#[test]
fn removal_scan_and_recovery_share_a_real_sqlite_writer_transaction() {
    let home = home();
    seed_historical_capture(&home);
    let control = OperationControl::default();
    Metadata::with_historical_write(&home, &control, |writer| {
        writer.with_write_lease(|writer| {
            let other = Connection::open(home.join("state/toolkit.sqlite"))?;
            other.busy_timeout(Duration::ZERO)?;
            let mut state = RemovalCommit::NotCommitted;
            let mut calls = 0;
            writer.removal_transaction(&mut state, |tx| {
                calls += 1;
                let mut rows = 0;
                tx.visit_raw_page("capture/", "", |_, _, _| {
                    rows += 1;
                    Ok(())
                })?;
                assert_eq!(rows, 1);
                let error = other
                    .execute("INSERT INTO documents VALUES('capture/concurrent',1,'null')", [])
                    .unwrap_err();
                assert!(matches!(error, rusqlite::Error::SqliteFailure(ref e, _) if e.code == rusqlite::ErrorCode::DatabaseBusy));
                tx.put_recovery("evidence/remove/fixture", 0, b"null")?;
                tx.delete_manifest("capture/fixture", 1)
            })?;
            assert_eq!(calls, 1);
            assert_eq!(state, RemovalCommit::Committed);
            assert_eq!(writer.document("capture/fixture").unwrap_err().code, "vault.record_missing");
            assert_eq!(writer.document("evidence/remove/fixture")?.generation, 1);
            let deleted: i64 = writer.connection.query_row(
                "SELECT generation FROM deleted_generations WHERE key='capture/fixture'",
                [],
                |row| row.get(0),
            )?;
            assert_eq!(deleted, 1);
            other.execute("INSERT INTO documents VALUES('capture/concurrent',1,'null')", [])?;
            other.close().map_err(|(_, error)| ToolkitError::from(error))?;
            Ok(())
        })
    })
    .unwrap();
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
fn cancellation_on_actual_sqlite_busy_does_not_enter_or_replay_removal() {
    let home = home();
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let result = Metadata::with_historical_write(&home, &control, |writer| {
        writer.with_write_lease(|writer| {
            let other = Connection::open(home.join("state/toolkit.sqlite"))?;
            other.execute_batch("BEGIN IMMEDIATE")?;
            let mut state = RemovalCommit::NotCommitted;
            let mut calls = 0;
            let mut busy = 0;
            let result = writer.removal_transaction_observed(
                &mut state,
                |_| {
                    calls += 1;
                    Ok(())
                },
                &mut || {
                    busy += 1;
                    token.cancel();
                },
            );
            assert_eq!(calls, 0);
            assert_eq!(busy, 1);
            assert_eq!(state, RemovalCommit::NotCommitted);
            other.execute_batch("ROLLBACK")?;
            other.close().map_err(|(_, e)| ToolkitError::from(e))?;
            result
        })
    });
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert!(control.stop_cause().is_ok());
    std::fs::remove_dir_all(home).unwrap();
}
#[test]
fn failed_second_cas_rolls_back_tombstones_and_recovery_without_replaying_callback() {
    let home = home();
    seed_historical_capture(&home);
    let control = OperationControl::default();
    Metadata::with_historical_write(&home, &control, |writer| {
        writer.with_write_lease(|writer| {
            let mut state = RemovalCommit::NotCommitted;
            let mut calls = 0;
            let error = writer
                .removal_transaction(&mut state, |tx| {
                    calls += 1;
                    tx.put_recovery("evidence/remove/fixture", 0, b"null")?;
                    tx.delete_manifest("capture/fixture", 1)?;
                    tx.delete_manifest("evidence/keep/absent", 1)
                })
                .unwrap_err();
            assert_eq!(error.code, "vault.generation_conflict");
            assert_eq!(calls, 1);
            assert_eq!(state, RemovalCommit::NotCommitted);
            assert_eq!(writer.document("capture/fixture")?.generation, 1);
            assert_eq!(
                writer.document("evidence/remove/fixture").unwrap_err().code,
                "vault.record_missing"
            );
            let count: i64 = writer.connection.query_row(
                "SELECT count(*) FROM deleted_generations",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(count, 0);
            Ok(())
        })
    })
    .unwrap();
    std::fs::remove_dir_all(home).unwrap();
}
