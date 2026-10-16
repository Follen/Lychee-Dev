//! Exercise both public entries against real files, SQLite and operation owners.
use super::*;
use std::cell::Cell;

fn resolve(
    inv: &Invocation,
    typed: bool,
    supplemental: impl FnMut() -> Result<()>,
) -> Result<PinnedSet> {
    if typed {
        resolve_pin_request_controlled_with_check(
            PinSelectionRequest {
                home: &inv.home,
                cwd: &inv.cwd,
                snapshot: inv.flag("--snapshot"),
                project: inv.flag("--project"),
            },
            &inv.control,
            supplemental,
        )
    } else {
        resolve_pinned_controlled_with_check(inv, supplemental)
    }
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
fn pin_request_and_invocation_preserve_full_outputs_and_immutable_generations() {
    for (snapshot, project) in [
        (None, None),
        (Some(""), None),
        (None, Some("")),
        (Some(""), Some("")),
        (Some(""), Some("..")),
    ] {
        let mut f = Fixture::new();
        for (flag, value) in [("--snapshot", snapshot), ("--project", project)] {
            if let Some(value) = value {
                f.invocation.flags.insert(flag.into(), value.into());
            }
        }
        let expected = json(&f.pin);
        // The first request creates; the second must authenticate the CAS winner.
        assert_eq!(
            json(resolve(&f.invocation, true, || Ok(())).unwrap()),
            expected
        );
        assert_eq!(
            json(resolve(&f.invocation, false, || Ok(())).unwrap()),
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
    f.invocation
        .flags
        .insert("--snapshot".into(), pin.id.clone());
    f.invocation
        .flags
        .insert("--project".into(), "absent".into());
    // No selected project directory needs to exist when a snapshot is present.
    f.invocation.cwd = f.root.join("also-absent");
    for typed in [true, false] {
        assert_eq!(
            json(resolve(&f.invocation, typed, || Ok(())).unwrap()),
            json(&pin)
        );
    }
    f.invocation.cwd = f.project().join("nested");
    f.invocation.flags.remove("--project");
    f.invocation
        .flags
        .insert("--snapshot".into(), "PIN-missing".into());
    let typed = resolve(&f.invocation, true, || Ok(())).unwrap_err();
    let compatible = resolve(&f.invocation, false, || Ok(())).unwrap_err();
    assert_eq!(json(&typed), json(compatible));
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
        if let Some(path) = explicit {
            f.invocation.flags.insert("--project".into(), path.into());
        }
        for typed in [true, false] {
            assert_eq!(
                json(resolve(&f.invocation, typed, || Ok(())).unwrap()),
                json(&f.pin)
            );
        }
    }
    assert_eq!(persisted(&f), (1, json(&f.pin)));
    f.cleanup();
}

#[test]
fn pin_request_and_invocation_preserve_complete_nearest_project_errors() {
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
        let typed = resolve(&f.invocation, true, || Ok(())).unwrap_err();
        let compatible = resolve(&f.invocation, false, || Ok(())).unwrap_err();
        assert_eq!(json(&typed), json(compatible));
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
    let typed = resolve(&f.invocation, true, || Ok(())).unwrap_err();
    let compatible = resolve(&f.invocation, false, || Ok(())).unwrap_err();
    assert_eq!(json(&typed), json(compatible));
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
    let expected: Result<PinnedSet> = crate::metadata::historical_write::finish_historical(
        Err(f.invocation.control.check().unwrap_err()),
        Err(independent_stop()),
    );
    for typed in [true, false] {
        let calls = Cell::new(0);
        let failure = resolve(&f.invocation, typed, || {
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
    }
    f.cleanup();
}

#[test]
fn pin_request_independent_stop_during_real_sqlite_wait_matches_compatibility() {
    let f = Fixture::new();
    Metadata::open(&f.invocation.home, true)
        .unwrap()
        .close()
        .unwrap();
    let blocker =
        rusqlite::Connection::open(f.invocation.home.join("state/toolkit.sqlite")).unwrap();
    blocker.busy_timeout(Duration::ZERO).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let mut failures = Vec::new();
    for typed in [true, false] {
        let started = Instant::now();
        let deadline = started + Duration::from_millis(80);
        failures.push(
            resolve(&f.invocation, typed, || {
                if Instant::now() >= deadline {
                    Err(independent_stop())
                } else {
                    Ok(())
                }
            })
            .unwrap_err(),
        );
        assert!(started.elapsed() < Duration::from_secs(3));
        f.invocation.control.check().unwrap();
    }
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert_eq!(json(&failures[0]), json(&failures[1]));
    assert_eq!(failures[0].code, "records.query_resource_budget");
    let metadata = Metadata::open(&f.invocation.home, false).unwrap();
    assert!(metadata.list::<Value>("pin/", 10).unwrap().is_empty());
    metadata.close().unwrap();
    // Checked teardown leaves the same invocation usable for a successful retry.
    assert_eq!(
        json(resolve(&f.invocation, true, || Ok(())).unwrap()),
        json(&f.pin)
    );
    f.cleanup();
}

#[test]
fn pin_request_stop_after_immutable_publication_retains_the_exact_committed_winner() {
    for cancelled in [false, true] {
        let mut failures = Vec::new();
        for typed in [true, false] {
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
            let failure = resolve(&f.invocation, typed, || {
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
            failures.push(json(failure));
            observer.close().unwrap();
            assert_eq!(persisted(&f), (1, json(&f.pin)));
            f.cleanup();
        }
        assert_eq!(failures[0], failures[1]);
    }
}
