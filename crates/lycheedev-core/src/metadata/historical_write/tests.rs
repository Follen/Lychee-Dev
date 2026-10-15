use super::*;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "historical-cas-{}-{}-{}",
            std::process::id(),
            crate::storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        crate::workspace::initialize(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn change<'a>(key: &'a str, generation: i64, bytes: &'a [u8]) -> HistoricalMutation<'a> {
    HistoricalMutation {
        key,
        expected_generation: generation,
        bytes,
    }
}
#[test]
fn terminal_work_and_ownership_commit_in_one_sqlite_transaction() {
    let f = Fixture::new();
    let c = OperationControl::default();
    Metadata::with_historical_write(&f.0, &c, |w| {
        w.commit(&[
            change("work/OP", 0, br#"{"generation":1}"#),
            change("ownership/window", 0, br#"{"operationId":"OP"}"#),
        ])?;
        w.commit(&[
            change("work/OP", 1, br#"{"generation":2}"#),
            change("ownership/window", 1, br#"{"operationId":""}"#),
        ])?;
        assert_eq!(w.document("work/OP")?.generation, 2);
        assert_eq!(w.document("ownership/window")?.generation, 2);
        Ok(())
    })
    .unwrap();
}
#[test]
fn stale_second_generation_rolls_back_already_updated_work() {
    let f = Fixture::new();
    let c = OperationControl::default();
    Metadata::with_historical_write(&f.0, &c, |w| {
        w.commit(&[change("work", 0, b"1"), change("owner", 0, b"1")])?;
        let e = w
            .commit(&[change("work", 1, b"2"), change("owner", 2, b"2")])
            .unwrap_err();
        assert_eq!(e.code, "vault.generation_conflict");
        assert_eq!(e.message, "vault.generation_conflict: owner");
        assert_eq!(w.document("work")?.bytes, b"1");
        assert_eq!(w.document("work")?.generation, 1);
        assert_eq!(w.document("owner")?.bytes, b"1");
        Ok(())
    })
    .unwrap();
}
#[test]
fn generation_zero_does_not_accept_identical_existing_bytes() {
    let f = Fixture::new();
    let c = OperationControl::default();
    Metadata::with_historical_write(&f.0, &c, |w| {
        w.commit(&[change("key", 0, b"null")])?;
        assert_eq!(
            w.commit(&[change("key", 0, b"null")]).unwrap_err().code,
            "vault.generation_conflict"
        );
        assert_eq!(w.document("key")?.generation, 1);
        Ok(())
    })
    .unwrap();
}
#[test]
fn recreated_key_uses_deleted_generation_instead_of_resetting() {
    let f = Fixture::new();
    let c = OperationControl::default();
    Metadata::with_historical_write(&f.0, &c, |w| {
        w.connection.execute(
            "INSERT INTO deleted_generations(key,generation)VALUES('key',17)",
            [],
        )?;
        w.commit(&[change("key", 0, b"null")])?;
        assert_eq!(w.document("key")?.generation, 18);
        Ok(())
    })
    .unwrap();
}
#[test]
fn mutation_admission_checks_count_duplicate_keys_generation_json_and_size() {
    let f = Fixture::new();
    let c = OperationControl::default();
    Metadata::with_historical_write(&f.0, &c, |w| {
        assert_eq!(
            w.commit(&[]).unwrap_err().message,
            "vault: invalid mutation count"
        );
        for changes in [
            vec![change("a", 0, b"0"), change("a", 0, b"1")],
            vec![change("", 0, b"0")],
            vec![change("a", -1, b"0")],
            vec![change("a", i64::MAX, b"0")],
            vec![change("a", 0, b"{")],
        ] {
            assert_eq!(
                w.commit(&changes).unwrap_err().message,
                "vault: invalid mutation"
            );
        }
        let large = vec![b' '; 1_048_577];
        assert!(w.commit(&[change("key", 0, &large)]).is_err());
        assert!(w.document("a").is_err());
        Ok(())
    })
    .unwrap();
}
#[test]
fn raw_invalid_utf8_inside_json_string_remains_byte_exact() {
    let f = Fixture::new();
    let c = OperationControl::default();
    let raw = b"{\"evidence\":\"\xff\"}";
    Metadata::with_historical_write(&f.0, &c, |w| {
        w.commit(&[change("work", 0, raw)])?;
        assert_eq!(w.document("work")?.bytes, raw);
        Ok(())
    })
    .unwrap();
}
#[test]
fn cancellation_at_begin_or_commit_rolls_back_all_mutations() {
    for at_commit in [false, true] {
        let f = Fixture::new();
        let c = OperationControl::default();
        let stopped = Metadata::with_historical_write(&f.0, &c, |w| {
            w.commit(&[change("work", 0, b"1"), change("owner", 0, b"1")])?;
            if at_commit {
                w.connection.pragma_update(None, "journal_mode", "DELETE")?;
            }
            let block = Connection::open(f.0.join("state/toolkit.sqlite"))?;
            if at_commit {
                block.execute_batch("BEGIN; SELECT * FROM documents;")?;
            } else {
                block.execute_batch("BEGIN IMMEDIATE")?;
            }
            let mut busy = false;
            let e = w
                .commit_observed(
                    &[change("work", 1, b"2"), change("owner", 1, b"2")],
                    &mut || {
                        busy = true;
                        c.tracked_cancel_handle().unwrap().cancel();
                    },
                )
                .unwrap_err();
            assert!(busy);
            assert_eq!(e.code, "command.cancelled");
            block.execute_batch("ROLLBACK")?;
            let raw: Vec<u8> = w.connection.query_row(
                "SELECT value FROM documents WHERE key='work'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(raw, b"1");
            let generation: i64 = w.connection.query_row(
                "SELECT generation FROM documents WHERE key='owner'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(generation, 1);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(stopped.code, "command.cancelled");
    }
}
#[test]
fn cancelled_entry_creates_neither_database_nor_lease_namespace() {
    let f = Fixture::new();
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    let before: Vec<_> = std::fs::read_dir(f.0.join("locks"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    let e = Metadata::with_historical_write(&f.0, &c, |_| Ok(())).unwrap_err();
    assert_eq!(e.code, "command.cancelled");
    let after: Vec<_> = std::fs::read_dir(f.0.join("locks"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(before, after);
}
#[test]
fn first_typed_failure_survives_checked_close_failure() {
    let primary = ToolkitError::new("command.cancelled", "context canceled", 7)
        .with_details(serde_json::json!({"original":1}));
    let e = finish_historical::<()>(
        Err(primary),
        Err(ToolkitError::io(std::io::Error::other("close failure"))),
    )
    .unwrap_err();
    assert_eq!(e.code, "command.cancelled");
    assert_eq!(e.details["primary"]["details"]["original"], 1);
    assert_eq!(e.details["cleanup"]["code"], "external.io");
    assert!(e.message.contains("close failure"));
}
