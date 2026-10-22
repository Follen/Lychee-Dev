//! Real files, SQLite and original operation owners through typed selection requests.
use super::*;
use std::cell::Cell;

fn resolve(f: &Fixture, supplemental: impl FnMut() -> Result<()>) -> Result<PinnedSet> {
    resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, supplemental)
}
fn json<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}
fn independent_stop() -> ToolkitError {
    ToolkitError::new("records.query_resource_budget", "independent deadline", 3)
}
fn persisted(f: &Fixture) -> (i64, Value) {
    let metadata = Metadata::open(&f.invocation.home, false).unwrap();
    let key = format!("pin/{}", f.pin.id);
    let value = (
        metadata.generation(&key).unwrap().unwrap(),
        metadata.read::<Value>(&key).unwrap(),
    );
    metadata.close().unwrap();
    value
}

#[test]
fn pin_request_preserves_full_outputs_and_immutable_generations() {
    for (snapshot, project) in [
        (None, None),
        (Some(""), None),
        (None, Some("")),
        (Some(""), Some("")),
        (Some(""), Some("..")),
    ] {
        let mut f = Fixture::new();
        f.snapshot = snapshot.map(str::to_owned);
        f.selected_project = project.map(str::to_owned);
        let expected = json(&f.pin);
        // The first request creates; the second must authenticate the CAS winner.
        assert_eq!(
            json(resolve(&f, || Ok(())).unwrap()),
            expected
        );
        assert_eq!(
            json(resolve(&f, || Ok(())).unwrap()),
            expected
        );
        assert_eq!(persisted(&f), (1, expected));
        f.cleanup();
    }
}

#[test]
fn pin_request_snapshot_precedes_project_and_never_falls_back_on_missing_snapshot() {
    let mut f = Fixture::new();
    let pin = pin_selection(&f.invocation.home, f.pin.spec.clone()).unwrap();
    f.snapshot = Some(pin.id.clone());
    f.selected_project = Some("absent".into());
    // No selected project directory needs to exist when a snapshot is present.
    f.invocation.cwd = f.root.join("also-absent");
    assert_eq!(
        json(resolve(&f, || Ok(())).unwrap()),
        json(&pin)
    );
    f.invocation.cwd = f.project().join("nested");
    f.selected_project = None;
    f.snapshot = Some("PIN-missing".into());
    let typed = resolve(&f, || Ok(())).unwrap_err();
    assert_eq!(
        json(typed),
        json(ToolkitError::new(
            "vault.record_missing",
            "document pin/PIN-missing does not exist",
            3,
        ))
    );
    f.cleanup();
}

#[test]
fn pin_request_paths_are_lexically_absolute_without_canonicalizing_missing_components() {
    let mut f = Fixture::new();
    f.invocation.cwd = f.project().join("absent/../nested");
    assert!(!f.project().join("absent").exists());
    for explicit in [None, Some("absent/../../")] {
        f.selected_project = explicit.map(str::to_owned);
        assert_eq!(
            json(resolve(&f, || Ok(())).unwrap()),
            json(&f.pin)
        );
    }
    assert_eq!(persisted(&f), (1, json(&f.pin)));
    f.cleanup();
}

#[test]
fn pin_request_preserves_complete_nearest_project_errors() {
    let f = Fixture::new();
    Metadata::open(&f.invocation.home, true)
        .unwrap()
        .close()
        .unwrap();
    let project = f.invocation.cwd.join("lycheedev.json");
    let lock = f.invocation.cwd.join("lycheedev.lock.json");
    for (declaration, locked, expected) in [
        (b"{".as_slice(), false, "selection.project_format"),
        (
            br#"{"schema":"lycheedev.project.v1","product":"retail"}"#.as_slice(),
            false,
            "selection.project_unlocked",
        ),
        (
            br#"{"schema":"lycheedev.project.v1","product":"classic"}"#.as_slice(),
            true,
            "selection.project_conflict",
        ),
    ] {
        fs::write(&project, declaration).unwrap();
        if locked {
            fs::copy(f.project().join("lycheedev.lock.json"), &lock).unwrap();
        }
        let typed = resolve(&f, || Ok(())).unwrap_err();
        assert_eq!(json(typed), json(error(expected)));
    }
    let metadata = Metadata::open(&f.invocation.home, false).unwrap();
    assert!(metadata.list::<Value>("pin/", 10).unwrap().is_empty());
    metadata.close().unwrap();
    f.cleanup();
}

#[test]
fn pin_request_authenticates_existing_cas_winner_without_overwriting_corruption() {
    let f = Fixture::new();
    let mut forged = f.pin.clone();
    forged.spec.source.as_mut().unwrap().exact_commit = "b".repeat(40);
    let mut metadata = Metadata::open(&f.invocation.home, true).unwrap();
    metadata
        .create(&format!("pin/{}", f.pin.id), &forged)
        .unwrap();
    metadata.close().unwrap();
    let typed = resolve(&f, || Ok(())).unwrap_err();
    assert_eq!(
        json(typed),
        json(ToolkitError::new(
            "command.failed",
            "selection.pin_integrity",
            5
        ))
    );
    assert_eq!(persisted(&f), (1, json(forged)));
    f.cleanup();
}

#[test]
fn pin_request_keeps_original_cancellation_first_and_supplemental_failure_after_it() {
    let mut f = Fixture::new();
    let token = TrackedCancellation::new();
    f.invocation.control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    token.cancel();
    let expected: Result<PinnedSet> = crate::metadata::owned_write::finish_owned(
        Err(f.invocation.control.check().unwrap_err()),
        Err(independent_stop()),
    );
    let calls = Cell::new(0);
    let failure = resolve(&f, || {
        calls.set(calls.get() + 1);
        Err(independent_stop())
    })
    .unwrap_err();
    assert_eq!(json(failure), json(expected.as_ref().unwrap_err()));
    assert_eq!(
        calls.get(),
        1,
        "only the final supplemental check follows a pre-stop"
    );
    f.cleanup();
}

#[test]
fn pin_request_independent_stop_during_real_sqlite_wait_retains_exact_cause_tree() {
    let f = Fixture::new();
    Metadata::open(&f.invocation.home, true)
        .unwrap()
        .close()
        .unwrap();
    let blocker =
        rusqlite::Connection::open(f.invocation.home.join("state/toolkit.sqlite")).unwrap();
    blocker.busy_timeout(Duration::ZERO).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    // Observe the same SQLite statement outside the controlled path to retain
    // the exact platform's real BUSY error in the expected complete cause tree.
    let probe = rusqlite::Connection::open(f.invocation.home.join("state/toolkit.sqlite")).unwrap();
    probe.busy_timeout(Duration::ZERO).unwrap();
    let sqlite_error = probe
        .pragma_query_value::<i32, _>(None, "user_version", |row| row.get(0))
        .unwrap_err();
    assert_eq!(
        sqlite_error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    probe.close().unwrap();
    let stop = independent_stop();
    let primary = independent_stop().with_details(json!({
        "originalSupplemental": stop,
        "sqliteError": ToolkitError::from(sqlite_error),
    }));
    let expected = json!({
        "code": "records.query_resource_budget",
        "message": "independent deadline\nindependent deadline",
        "exit_code": 3,
        "details": {
            "schema": storage::FILE_CLEANUP_SCHEMA,
            "primary": primary,
            "cleanup": stop,
            "control": null,
        },
    });
    let original_deadline = f.invocation.control.deadline();
    let started = Instant::now();
    let busy_before = crate::metadata::sql_busy_observations();
    let failure = resolve(&f, || {
            // An arbitrary elapsed cutoff can stop before a retry or after
            // SQL has failed, which legitimately produces different details.
            // Stop immediately after the actual BUSY observation.
            if crate::metadata::sql_busy_observations() > busy_before {
                Err(independent_stop())
            } else {
                Ok(())
            }
        }).unwrap_err();
    assert_eq!(crate::metadata::sql_busy_observations() - busy_before, 1);
    assert_eq!(json(&failure), expected);
    // This is only the fixture's existing completion bound, not the trigger.
    assert!(started.elapsed() < Duration::from_secs(3));
    assert_eq!(f.invocation.control.deadline(), original_deadline);
    f.invocation.control.check().unwrap();
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    let metadata = Metadata::open(&f.invocation.home, false).unwrap();
    assert!(metadata.list::<Value>("pin/", 10).unwrap().is_empty());
    metadata.close().unwrap();
    // Checked teardown leaves the same request/control usable for a successful retry.
    assert_eq!(
        json(resolve(&f, || Ok(())).unwrap()),
        json(&f.pin)
    );
    f.cleanup();
}

#[test]
fn pin_request_stop_after_immutable_publication_retains_the_exact_committed_winner() {
    for cancelled in [false, true] {
        let mut f = Fixture::new();
        let token = TrackedCancellation::new();
        f.invocation.control =
            OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
        Metadata::open(&f.invocation.home, true)
            .unwrap()
            .close()
            .unwrap();
        let observer =
            rusqlite::Connection::open(f.invocation.home.join("state/toolkit.sqlite")).unwrap();
        observer.busy_timeout(Duration::ZERO).unwrap();
        let key = format!("pin/{}", f.pin.id);
        let published = Cell::new(false);
        let failure = resolve(&f, || {
            let count: i64 = observer
                .query_row(
                    "SELECT count(*) FROM documents WHERE key=?1",
                    [&key],
                    |row| row.get(0),
                )
                .unwrap();
            if count != 0 {
                published.set(true);
                if cancelled {
                    token.cancel();
                } else {
                    return Err(independent_stop());
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert!(published.get());
        assert_eq!(
            failure.code,
            if cancelled {
                "command.cancelled"
            } else {
                "records.query_resource_budget"
            }
        );
        observer.close().unwrap();
        assert_eq!(persisted(&f), (1, json(&f.pin)));
        f.cleanup();
    }
}
