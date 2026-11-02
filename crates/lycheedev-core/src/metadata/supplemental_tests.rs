use super::*;
use crate::{
    control::{OperationControl, TrackedCancellation},
    storage::ExclusiveLock,
};
use std::{
    cell::Cell,
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-metadata-supplemental-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        crate::workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn database(&self) -> PathBuf {
        self.0.join("state/toolkit.sqlite")
    }
    fn seed(&self) -> Metadata {
        let mut metadata = Metadata::open(&self.0, true).unwrap();
        metadata.put("seed", &true, None).unwrap();
        metadata
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
fn alive() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(5))
}
fn open_error(result: Result<Metadata>) -> ToolkitError {
    match result {
        Ok(metadata) => {
            metadata.close().unwrap();
            panic!("expected stopped open")
        }
        Err(error) => error,
    }
}
fn independent() -> (TrackedCancellation, OperationControl) {
    let token = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    (token, control)
}
fn check_stopped(error: &ToolkitError, invocation: &OperationControl) {
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    invocation.check().unwrap();
}
fn release(blocker: Connection) {
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
}

#[test]
fn supplemental_pre_stop_has_no_open_read_or_publication_side_effects() {
    let fixture = Fixture::new();
    let invocation = alive();
    let (token, budget) = independent();
    token.cancel();
    let absent = fixture.0.join("must-not-create");
    check_stopped(
        &open_error(Metadata::open_controlled_with_check(
            &absent,
            true,
            &invocation,
            || budget.check(),
        )),
        &invocation,
    );
    check_stopped(
        &Metadata::read_document_raw_controlled_with_check(&absent, "x", &invocation, || {
            budget.check()
        })
        .unwrap_err(),
        &invocation,
    );
    assert!(!absent.exists());
    let mut metadata = fixture.seed();
    check_stopped(
        &metadata
            .put_controlled_with_check("seed", &false, Some(1), &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    check_stopped(
        &metadata
            .create_controlled_with_check("new", &true, &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    assert_eq!(metadata.generation("seed").unwrap(), Some(1));
    assert!(metadata.read::<bool>("seed").unwrap());
    assert_eq!(metadata.generation("new").unwrap(), None);
    metadata.close().unwrap();
}

#[test]
fn supplemental_schema_lease_stop_observes_real_contention_and_retains_owner() {
    let fixture = Fixture::new();
    let scope = fixture.0.join("locks");
    let held = ExclusiveLock::resource(&scope, "metadata-schema").unwrap();
    let invocation = alive();
    let (token, budget) = independent();
    let mut busy = 0;
    let result = owned_write::owned_lease_observed_with_check(
        &fixture.0,
        "metadata-schema",
        &invocation,
        &mut || budget.check(),
        &mut || {
            busy += 1;
            token.cancel();
        },
    );
    let error = match result {
        Err(error) => error,
        Ok(lease) => {
            lease.close().unwrap();
            panic!("held schema lease was acquired")
        }
    };
    assert_eq!(busy, 1);
    check_stopped(&error, &invocation);
    assert!(!fixture.database().exists());
    assert!(ExclusiveLock::resource(&scope, "metadata-schema").is_err());
    held.close().unwrap();
    ExclusiveLock::resource(&scope, "metadata-schema")
        .unwrap()
        .close()
        .unwrap();
    Metadata::open_controlled(&fixture.0, true, &invocation)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn supplemental_open_stops_at_real_schema_read_begin_and_commit() {
    for sql in [
        "BEGIN EXCLUSIVE",
        "BEGIN IMMEDIATE",
        "BEGIN; SELECT * FROM sqlite_master",
    ] {
        let fixture = Fixture::new();
        let blocker = Connection::open(fixture.database()).unwrap();
        blocker.busy_timeout(Duration::ZERO).unwrap();
        blocker.execute_batch(sql).unwrap();
        let invocation = alive();
        let (token, budget) = independent();
        let mut busy = 0;
        let error = open_error(Metadata::open_controlled_observed_with_check(
            &fixture.0,
            true,
            &invocation,
            || budget.check(),
            || {
                busy += 1;
                token.cancel();
            },
        ));
        assert_eq!(busy, 1, "{sql}");
        check_stopped(&error, &invocation);
        assert_eq!(error.details["sqliteError"]["code"], "vault.metadata");
        release(blocker);
        let inspect = Connection::open(fixture.database()).unwrap();
        let version: i32 = inspect
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        let tables: i64 = inspect
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!((version, tables), (0, 0));
        inspect.close().unwrap();
        Metadata::open_controlled(&fixture.0, true, &invocation)
            .unwrap()
            .close()
            .unwrap();
        // On Windows removal also proves no internally acquired SQLite handle escaped.
        std::fs::remove_file(fixture.database()).unwrap();
    }
}

#[test]
fn supplemental_put_and_create_rollback_real_begin_and_commit_contention() {
    for strict in [false, true] {
        for commit in [false, true] {
            let fixture = Fixture::new();
            let mut metadata = fixture.seed();
            let blocker = Connection::open(fixture.database()).unwrap();
            blocker.busy_timeout(Duration::ZERO).unwrap();
            blocker
                .execute_batch(if commit {
                    "BEGIN; SELECT * FROM documents"
                } else {
                    "BEGIN IMMEDIATE"
                })
                .unwrap();
            let invocation = alive();
            let (token, budget) = independent();
            let mut busy = 0;
            let error = metadata
                .put_controlled_policy_with_check(
                    if strict { "new" } else { "seed" },
                    &false,
                    if strict { None } else { Some(1) },
                    &invocation,
                    strict,
                    || budget.check(),
                    || {
                        busy += 1;
                        token.cancel();
                    },
                )
                .unwrap_err();
            assert_eq!(busy, 1);
            check_stopped(&error, &invocation);
            assert!(metadata.connection.is_autocommit());
            let timeout: i32 = metadata
                .connection
                .pragma_query_value(None, "busy_timeout", |r| r.get(0))
                .unwrap();
            assert_eq!(timeout, 5000);
            release(blocker);
            assert_eq!(metadata.generation("new").unwrap(), None);
            assert_eq!(metadata.generation("seed").unwrap(), Some(1));
            assert!(metadata.read::<bool>("seed").unwrap());
            metadata
                .create_controlled_with_check("new", &true, &invocation, || Ok(()))
                .unwrap();
            metadata
                .put_controlled_with_check("seed", &false, Some(1), &invocation, || Ok(()))
                .unwrap();
            metadata.close().unwrap();
            std::fs::remove_file(fixture.database()).unwrap();
        }
    }
}

#[test]
fn supplemental_raw_read_stops_during_real_schema_and_document_reads() {
    for document in [false, true] {
        let fixture = Fixture::new();
        fixture.seed().close().unwrap();
        let blocker = Connection::open(fixture.database()).unwrap();
        blocker.busy_timeout(Duration::ZERO).unwrap();
        if !document {
            blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
        }
        let invocation = alive();
        let (token, budget) = independent();
        let visits = Cell::new(0);
        let mut busy = 0;
        let error = Metadata::read_raw_controlled_observed_with_check(
            &fixture.0,
            "seed",
            &invocation,
            || {
                visits.set(visits.get() + 1);
                // This exact checkpoint is immediately after the successful
                // user_version query and before SELECT. A real exclusive lock
                // therefore blocks the document read, not its schema preflight.
                if document && visits.get() == 6 {
                    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
                }
                budget.check()
            },
            || {
                busy += 1;
                token.cancel();
            },
        )
        .unwrap_err();
        assert_eq!(busy, 1);
        assert_eq!(visits.get(), if document { 9 } else { 7 });
        check_stopped(&error, &invocation);
        release(blocker);
        let document = Metadata::read_document_raw_controlled_with_check(
            &fixture.0,
            "seed",
            &invocation,
            || Ok(()),
        )
        .unwrap();
        assert_eq!(document.bytes, b"true");
        assert_eq!(document.generation, 1);
        std::fs::remove_file(fixture.database()).unwrap();
    }
}

#[test]
fn supplemental_deadline_stops_held_sqlite_without_shortening_invocation() {
    let fixture = Fixture::new();
    let mut metadata = fixture.seed();
    let blocker = Connection::open(fixture.database()).unwrap();
    blocker
        .execute_batch("BEGIN; SELECT * FROM documents")
        .unwrap();
    let invocation = alive();
    let deadline = invocation.deadline();
    let budget_deadline = Cell::new(None::<Instant>);
    let mut busy = 0;
    let mut deadline_set = 0;
    let error = metadata
        .put_controlled_policy_with_check(
            "new",
            &true,
            None,
            &invocation,
            true,
            || {
                if budget_deadline
                    .get()
                    .is_some_and(|until| Instant::now() >= until)
                {
                    Err(ToolkitError::new(
                        "records.query_resource_budget",
                        "Data query deadline exceeded",
                        3,
                    ))
                } else {
                    Ok(())
                }
            },
            || {
                busy += 1;
                // Establish one absolute budget only after genuine COMMIT
                // contention. Later busy observations must never refresh it.
                if budget_deadline.get().is_none() {
                    budget_deadline.set(Some(Instant::now() + Duration::from_millis(40)));
                    deadline_set += 1;
                }
            },
        )
        .unwrap_err();
    assert!(busy > 0);
    assert_eq!(deadline_set, 1);
    let budget_deadline = budget_deadline.get().expect("observed real contention");
    assert!(Instant::now() >= budget_deadline);
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    invocation.check().unwrap();
    assert_eq!(invocation.deadline(), deadline);
    assert!(Instant::now() < budget_deadline + Duration::from_secs(1));
    release(blocker);
    assert_eq!(metadata.generation("new").unwrap(), None);
    metadata.close().unwrap();
}

#[test]
fn supplemental_sql_failure_keeps_both_typed_causes_and_original_details() {
    let connection = Connection::open_in_memory().unwrap();
    let invocation = alive();
    let fail = Cell::new(false);
    let original = ToolkitError::new("records.query_resource_budget", "specific limit", 3)
        .with_details(serde_json::json!({"component":"independent","nested":{"limit":17}}));
    let error = checked_sql_with_check(
        &invocation,
        invocation.deadline(),
        &mut || {
            if fail.get() {
                Err(original.clone())
            } else {
                Ok(())
            }
        },
        &mut || panic!("nonbusy SQL failure"),
        || {
            fail.set(true);
            connection.execute_batch("SELECT missing_column FROM missing_table")
        },
    )
    .unwrap_err();
    assert_eq!(
        (&error.code, error.exit_code, &error.message),
        (&original.code, original.exit_code, &original.message)
    );
    assert_eq!(
        error.details["originalSupplemental"],
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(error.details["sqliteError"]["code"], "vault.metadata");
    assert!(
        error.details["sqliteError"]["message"]
            .as_str()
            .unwrap()
            .contains("missing_table")
    );
    assert!(error.details.get("cleanup").is_none());
    invocation.check().unwrap();
    connection.close().unwrap();
}

#[test]
fn successful_sql_open_transfers_owner_before_supplemental_stop_check() {
    let invocation = alive();
    let calls = Cell::new(0);
    let stopped = Cell::new(false);
    let mut check = || {
        calls.set(calls.get() + 1);
        if stopped.get() {
            Err(ToolkitError::new("supplemental.stop", "stopped", 7))
        } else {
            Ok(())
        }
    };
    let connection = checked_sql_with_check(
        &invocation,
        invocation.deadline(),
        &mut check,
        &mut || {},
        || {
            let connection = Connection::open_in_memory()?;
            stopped.set(true);
            Ok(connection)
        },
    )
    .unwrap();
    assert_eq!(
        calls.get(),
        1,
        "helper must not drop an acquired owner after a late check"
    );
    let result = metadata_check(&invocation, &mut check);
    let mut close_calls = 0;
    let closed = {
        close_calls += 1;
        connection
            .close()
            .map_err(|(_, error)| ToolkitError::from(error))
    };
    let error = owned_write::finish_owned(result, closed).unwrap_err();
    assert_eq!(error.code, "supplemental.stop");
    assert_eq!(close_calls, 1);
    invocation.check().unwrap();
}

#[test]
fn legacy_noop_sql_check_keeps_original_control_priority() {
    let invocation = alive();
    let connection = Connection::open_in_memory().unwrap();
    let (token, controlled) = independent();
    let error = checked_sql(&controlled, invocation.deadline(), &mut || {}, || {
        token.cancel();
        connection.execute_batch("SELECT missing FROM missing")
    })
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert!(error.details.is_null());
    connection.close().unwrap();
}

#[test]
fn supplemental_open_schema_wait_stops_inside_held_lease_retry() {
    let fixture = Fixture::new();
    let held = ExclusiveLock::resource(&fixture.0.join("locks"), "metadata-schema").unwrap();
    let invocation = alive();
    let (token, budget) = independent();
    let busy = Cell::new(0);
    let error = open_error(Metadata::open_controlled_with_events(
        &fixture.0,
        true,
        &invocation,
        || budget.check(),
        || {
            busy.set(busy.get() + 1);
            token.cancel();
        },
        || panic!("schema contention must stop before opening SQLite"),
    ));
    assert_eq!(busy.get(), 1);
    check_stopped(&error, &invocation);
    assert!(!fixture.database().exists());
    assert!(ExclusiveLock::resource(&fixture.0.join("locks"), "metadata-schema").is_err());
    held.close().unwrap();
    Metadata::open_controlled(&fixture.0, true, &invocation)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn supplemental_post_open_stop_closes_owned_handle_before_return() {
    let fixture = Fixture::new();
    fixture.seed().close().unwrap();
    let invocation = alive();
    let (token, budget) = independent();
    let opened = Cell::new(0);
    let error = open_error(Metadata::open_controlled_with_events(
        &fixture.0,
        false,
        &invocation,
        || budget.check(),
        || panic!("uncontended read-only fixture must not wait"),
        || {
            opened.set(opened.get() + 1);
            token.cancel();
        },
    ));
    assert_eq!(opened.get(), 1);
    check_stopped(&error, &invocation);
    std::fs::remove_file(fixture.database()).unwrap();
}
