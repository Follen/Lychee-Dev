//! The default and historical APIs share the current generation history.
//! Use real delete/recreate operations, not hand-seeded tombstones alone.
use super::*;
use crate::control::{OperationControl, TrackedCancellation};
use std::{
    cell::Cell,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-generation-contract-{}-{}-{}",
            std::process::id(),
            crate::storage::utc_now().replace([':', '.'], ""),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        ));
        crate::workspace::initialize(&path).unwrap();
        Self(path)
    }
    fn open(&self, controlled: bool, write: bool) -> Metadata {
        if controlled {
            Metadata::open_controlled(&self.0, write, &OperationControl::default()).unwrap()
        } else {
            Metadata::open(&self.0, write).unwrap()
        }
    }
    fn legacy_schema(&self) {
        let connection = Connection::open(self.0.join("state/toolkit.sqlite")).unwrap();
        connection.execute_batch("CREATE TABLE documents (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL CHECK(generation>0), value BLOB NOT NULL CHECK(length(value)<=1048576)) WITHOUT ROWID; PRAGMA user_version=1;").unwrap();
        connection
            .execute(
                "INSERT INTO documents VALUES('retained',7,?1)",
                [b"true".as_slice()],
            )
            .unwrap();
        connection.close().unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn tombstone(metadata: &Metadata, key: &str) -> Option<i64> {
    metadata
        .connection
        .query_row(
            "SELECT generation FROM deleted_generations WHERE key=?1",
            [key],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
}
fn tables(metadata: &Metadata) -> Vec<String> {
    let mut query = metadata
        .connection
        .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
        .unwrap();
    query
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}
fn change<'a>(key: &'a str, expected_generation: i64, bytes: &'a [u8]) -> OwnedMutation<'a> {
    OwnedMutation {
        key,
        expected_generation,
        bytes,
    }
}

#[test]
fn writable_opens_initialize_and_upgrade_retained_tombstone_schema() {
    for controlled in [false, true] {
        for legacy in [false, true] {
            let fixture = Fixture::new();
            if legacy {
                fixture.legacy_schema();
                for reader_controlled in [false, true] {
                    let metadata = fixture.open(reader_controlled, false);
                    assert_eq!(tables(&metadata), ["documents"]);
                    assert_eq!(metadata.generation("retained").unwrap(), Some(7));
                    metadata.close().unwrap();
                }
            }
            let metadata = fixture.open(controlled, true);
            assert_eq!(tables(&metadata), ["deleted_generations", "documents"]);
            let schema: String = metadata
                .connection
                .query_row(
                    "SELECT sql FROM sqlite_master WHERE name='deleted_generations'",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                schema,
                "CREATE TABLE deleted_generations (key TEXT PRIMARY KEY NOT NULL, generation INTEGER NOT NULL) WITHOUT ROWID"
            );
            let version: i32 = metadata
                .connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, 1);
            if legacy {
                assert!(metadata.read::<bool>("retained").unwrap());
                assert_eq!(metadata.generation("retained").unwrap(), Some(7));
            }
            metadata.close().unwrap();
            // Migration is idempotent and readonly handles see its result.
            fixture.open(!controlled, true).close().unwrap();
            let metadata = fixture.open(controlled, false);
            assert_eq!(tables(&metadata), ["deleted_generations", "documents"]);
            metadata.close().unwrap();
        }
    }
}

#[test]
fn historical_deletion_then_every_default_creator_rejects_old_cas() {
    for creator in 0..7 {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        Metadata::with_owned_write(&fixture.0, &control, |writer| {
            writer.commit(&[change("key", 0, b"false")])?;
            writer.commit(&[change("key", 1, b"true")])?;
            writer.delete(&[("key".into(), 2)])
        })
        .unwrap();
        let mut metadata = fixture.open(creator % 2 == 0, true);
        match creator {
            0 => metadata.put("key", &true, None).unwrap(),
            1 => metadata.create("key", &true).unwrap(),
            2 => metadata
                .put_controlled("key", &true, None, &control)
                .unwrap(),
            3 => metadata.create_controlled("key", &true, &control).unwrap(),
            4 => metadata
                .put_serialized("key", b"true", None, || Ok(()))
                .unwrap(),
            5 => metadata
                .put_serialized_referencing(
                    "key",
                    b"true",
                    &[],
                    &control,
                    || Ok(()),
                    |_| Ok(()),
                    |_, _| Ok(()),
                    || Ok(()),
                )
                .unwrap(),
            6 => {
                let state = Cell::new(CaptureWriteState::NotCommitted);
                metadata
                    .put_capture_serialized(
                        "key",
                        b"true",
                        &[],
                        &control,
                        || Ok(()),
                        |_| Ok(()),
                        |_, _| Ok(()),
                        || Ok(()),
                        &state,
                    )
                    .unwrap();
                assert_eq!(state.get(), CaptureWriteState::Committed);
            }
            _ => unreachable!(),
        }
        assert_eq!(
            metadata.generation("key").unwrap(),
            Some(3),
            "creator {creator}"
        );
        assert_eq!(tombstone(&metadata, "key"), Some(2));
        // Ordinary idempotence remains byte-exact; strict creation still fails.
        metadata
            .put_controlled("key", &true, None, &control)
            .unwrap();
        assert_eq!(metadata.generation("key").unwrap(), Some(3));
        assert_eq!(
            metadata.create("key", &true).unwrap_err().code,
            "vault.generation_conflict"
        );
        assert_eq!(
            metadata.put("key", &false, Some(2)).unwrap_err().code,
            "vault.generation_conflict"
        );
        metadata.close().unwrap();
        Metadata::with_owned_write(&fixture.0, &control, |writer| {
            assert_eq!(
                writer
                    .commit(&[change("key", 2, b"false")])
                    .unwrap_err()
                    .code,
                "vault.generation_conflict"
            );
            assert_eq!(
                writer.delete(&[("key".into(), 2)]).unwrap_err().code,
                "vault.generation_conflict"
            );
            assert_eq!(writer.document("key")?.generation, 3);
            writer.delete(&[("key".into(), 3)])?;
            writer.commit(&[change("key", 0, b"true")])?;
            assert_eq!(writer.document("key")?.generation, 4);
            Ok(())
        })
        .unwrap();
    }
}

fn remove(metadata: &mut Metadata, kind: usize, control: &OperationControl) -> Result<()> {
    match kind {
        0 => {
            assert!(metadata.delete("key")?);
            Ok(())
        }
        1 => metadata.consume::<bool, _>("key", |_| Ok(())),
        2 => metadata.consume_controlled::<bool, _>("key", control, |_| Ok(())),
        3 => metadata.consume_generation_controlled::<bool, _>("key", 2, control, |_| Ok(())),
        _ => unreachable!(),
    }
}

#[test]
fn every_default_removal_preserves_history_for_historical_and_default_recreation() {
    for kind in 0..4 {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let mut metadata = fixture.open(kind % 2 == 0, true);
        metadata.put("key", &true, None).unwrap();
        metadata.put("key", &true, Some(1)).unwrap();
        remove(&mut metadata, kind, &control).unwrap();
        assert_eq!(metadata.generation("key").unwrap(), None);
        assert_eq!(tombstone(&metadata, "key"), Some(2), "removal {kind}");
        assert!(!metadata.delete("key").unwrap());
        assert_eq!(tombstone(&metadata, "key"), Some(2));
        metadata.close().unwrap();
        Metadata::with_owned_write(&fixture.0, &control, |writer| {
            writer.commit(&[change("key", 0, b"true")])?;
            assert_eq!(writer.document("key")?.generation, 3);
            Ok(())
        })
        .unwrap();
        let mut metadata = fixture.open(false, true);
        assert_eq!(
            metadata
                .consume_generation_controlled::<bool, ()>("key", 2, &control, |_| panic!(
                    "stale validator ran"
                ))
                .unwrap_err()
                .code,
            "vault.generation_conflict"
        );
        metadata
            .consume_generation_controlled::<bool, _>("key", 3, &control, |_| Ok(()))
            .unwrap();
        metadata.create_controlled("key", &true, &control).unwrap();
        assert_eq!(metadata.generation("key").unwrap(), Some(4));
        assert_eq!(tombstone(&metadata, "key"), Some(3));
        metadata.close().unwrap();
    }
}

#[test]
fn failed_delete_rolls_back_new_tombstone_and_preserves_older_history() {
    for kind in 0..4 {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let mut metadata = fixture.open(true, true);
        metadata.put("key", &true, None).unwrap();
        metadata.delete("key").unwrap();
        metadata.create("key", &true).unwrap();
        assert_eq!(tombstone(&metadata, "key"), Some(1));
        metadata.connection.execute_batch("CREATE TRIGGER reject_delete BEFORE DELETE ON documents WHEN OLD.key='key' BEGIN SELECT RAISE(ABORT,'injected delete failure'); END;").unwrap();
        let error = remove(&mut metadata, kind, &control).unwrap_err();
        assert_eq!(error.code, "vault.metadata");
        assert!(metadata.connection.is_autocommit());
        assert_eq!(metadata.generation("key").unwrap(), Some(2));
        assert_eq!(tombstone(&metadata, "key"), Some(1));
        metadata
            .connection
            .execute_batch("DROP TRIGGER reject_delete")
            .unwrap();
        remove(&mut metadata, kind, &control).unwrap();
        assert_eq!(tombstone(&metadata, "key"), Some(2));
        metadata.close().unwrap();
    }
}


#[test]
fn stopped_recreation_preserves_deleted_generation_and_independent_budget() {
    for supplemental in [false, true] {
        let fixture = Fixture::new();
        let mut metadata = fixture.open(true, true);
        metadata.put("key", &true, None).unwrap();
        metadata.delete("key").unwrap();
        let blocker = Connection::open(fixture.0.join("state/toolkit.sqlite")).unwrap();
        blocker
            .execute_batch("BEGIN; SELECT * FROM documents")
            .unwrap();
        let token = TrackedCancellation::new();
        let stopped =
            OperationControl::tracked(Instant::now() + Duration::from_secs(10), token.clone());
        let invocation = OperationControl::default();
        let mut busy = false;
        let control = if supplemental { &invocation } else { &stopped };
        let error = metadata
            .put_controlled_policy_with_check(
                "key",
                &true,
                None,
                control,
                true,
                || {
                    if supplemental {
                        stopped.check()
                    } else {
                        Ok(())
                    }
                },
                || {
                    busy = true;
                    token.cancel();
                },
            )
            .unwrap_err();
        assert!(busy);
        assert_eq!(error.code, "command.cancelled");
        if supplemental {
            invocation.check().unwrap();
        }
        assert!(metadata.connection.is_autocommit());
        blocker.execute_batch("ROLLBACK").unwrap();
        blocker.close().unwrap();
        assert_eq!(metadata.generation("key").unwrap(), None);
        assert_eq!(tombstone(&metadata, "key"), Some(1));
        metadata
            .create_controlled("key", &true, &OperationControl::default())
            .unwrap();
        assert_eq!(metadata.generation("key").unwrap(), Some(2));
        metadata.close().unwrap();
    }
}

#[test]
fn legacy_tombstone_migration_contention_observes_independent_stop_and_closes() {
    let fixture = Fixture::new();
    fixture.legacy_schema();
    let blocker = Connection::open(fixture.0.join("state/toolkit.sqlite")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let invocation = OperationControl::default();
    let token = TrackedCancellation::new();
    let budget = OperationControl::tracked(Instant::now() + Duration::from_secs(10), token.clone());
    let mut busy = false;
    let error = match Metadata::open_controlled_observed_with_check(
        &fixture.0,
        true,
        &invocation,
        || budget.check(),
        || {
            busy = true;
            token.cancel();
        },
    ) {
        Err(error) => error,
        Ok(metadata) => {
            metadata.close().unwrap();
            panic!("migration escaped contention");
        }
    };
    assert!(busy);
    assert_eq!(error.code, "command.cancelled");
    invocation.check().unwrap();
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    let metadata = fixture.open(false, false);
    assert_eq!(tables(&metadata), ["documents"]);
    assert_eq!(metadata.generation("retained").unwrap(), Some(7));
    metadata.close().unwrap();
    fixture.open(true, true).close().unwrap();
}
