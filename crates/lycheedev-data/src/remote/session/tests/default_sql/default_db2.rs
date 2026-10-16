//! Real default DB2 profiles over the same authenticated offline fixture.
use super::*;
use crate::{db2, remote::provider::OpenedScope};

fn invocation(f: &SqlFixture, command: &str, flags: &[(&str, &str)]) -> Invocation {
    let mut inv = f.invocation("");
    inv.command = command.into();
    inv.flags.remove("--sql");
    if command == "data db2" {
        inv.flags.insert("--table".into(), "RemoteSample".into());
    } else {
        inv.positional = Some("RemoteSample".into());
    }
    inv.flags.extend(
        flags
            .iter()
            .map(|(key, value)| ((*key).into(), (*value).into())),
    );
    inv
}

#[test]
fn default_remote_db2_record_page_schema_search_and_foreign_key_capture_exact_bytes() {
    let f = SqlFixture::new(false, true);
    let record = crate::dispatch(&invocation(&f, "data db2", &[("--id", "7")])).unwrap();
    assert_eq!(
        record.data["row"],
        json!({"ID":7,"Name":"verified remote row"})
    );
    assert_eq!(record.data["file"]["source"], "cdn");
    f.verify(&record);
    let page = crate::dispatch(&invocation(&f, "data db2", &[("--limit", "1")])).unwrap();
    assert_eq!(page.data["page"]["ids"], json!([7]));
    assert_eq!(page.data["page"]["more"], false);
    assert_eq!(page.data["file"]["source"], "cdn");
    f.verify(&page);
    for (command, flags) in [
        ("data db2 schema", vec![]),
        (
            "data db2 search",
            vec![
                ("--field", "Name"),
                ("--query", "verified"),
                ("--limit", "1"),
            ],
        ),
        (
            "data db2 foreign-key",
            vec![("--field", "ID"), ("--value", "7"), ("--limit", "1")],
        ),
    ] {
        let out = crate::dispatch(&invocation(&f, command, &flags)).unwrap();
        assert_eq!(out.data["source"], "cdn");
        assert_eq!(out.data["tables"][0]["contentVerified"], true);
        if command != "data db2 schema" {
            assert_eq!(
                out.data["rows"],
                json!([{"ID":7,"Name":"verified remote row"}])
            );
        }
        f.verify(&out);
    }
    f.close();
}

#[test]
fn default_remote_db2_schema_does_not_decode_invalid_string_cells() {
    let f = SqlFixture::new(true, true);
    let schema = crate::dispatch(&invocation(&f, "data db2 schema", &[])).unwrap();
    assert_eq!(schema.data["source"], "cdn");
    f.verify(&schema);
    let before = f.captures().len();
    assert_eq!(
        crate::dispatch(&invocation(&f, "data db2", &[("--id", "7")]))
            .unwrap_err()
            .code,
        "table.invalid_format"
    );
    assert_eq!(f.captures().len(), before);
    f.close();
}

#[test]
fn default_remote_db2_stop_after_read_keeps_admission_until_checked_teardown_and_no_capture() {
    let f = SqlFixture::new(false, true);
    let inv = invocation(&f, "data db2", &[("--id", "7")]);
    let before = f.captures().len();
    let mut read = false;
    let error = db2::dispatch_after_read(&inv, &mut || {
        read = true;
        let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
        assert!(
            lycheedev_core::workspace::data_resources::acquire(
                &f.source.home,
                Policy::Measurement,
                &short
            )
            .is_err()
        );
        inv.control.cancel_handle().store(true, Ordering::Release);
        Ok(())
    })
    .unwrap_err();
    assert!(read);
    assert_eq!(error.exit_code, 7);
    assert_eq!(f.captures().len(), before);
    let fresh = OperationControl::default();
    lycheedev_core::workspace::data_resources::acquire(&f.source.home, Policy::Measurement, &fresh)
        .unwrap()
        .close()
        .unwrap();
    f.close();
}

#[test]
fn default_remote_db2_pin_and_admission_error_order_follows_each_fixed_go_wrapper() {
    let f = SqlFixture::new(false, false);
    let control = OperationControl::default();
    let held = lycheedev_core::workspace::data_resources::acquire(
        &f.source.home,
        Policy::Measurement,
        &control,
    )
    .unwrap();
    let mut record = invocation(&f, "data db2", &[("--id", "7")]);
    record
        .flags
        .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
    assert_eq!(
        crate::dispatch(&record).unwrap_err().code,
        "vault.record_missing",
        "inspectData loads the pin before attempting admission"
    );
    let mut schema = invocation(&f, "data db2 schema", &[]);
    schema
        .flags
        .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
    schema.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(50));
    let error = crate::dispatch(&schema).unwrap_err();
    assert_eq!(
        error.code,
        schema.control.check().unwrap_err().code,
        "withNavigator first waits for admission"
    );
    held.close().unwrap();
    schema.control = OperationControl::default();
    assert_eq!(
        crate::dispatch(&schema).unwrap_err().code,
        "vault.record_missing"
    );
    assert!(f.captures().is_empty());
    f.close();
}

#[test]
fn explicit_db2_scope_rejects_a_different_invocation_or_budget_before_definition_reads() {
    let f = SqlFixture::new(false, false);
    let inv = invocation(&f, "data db2 schema", &[]);
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let mut scope = OpenedScope::open(&inv, &budget).unwrap().admit().unwrap();
    let other = invocation(&f, "data db2 schema", &[]);
    let result = db2::prepare_scoped(&other, "RemoteSample", &budget, Some(&mut scope), None);
    assert_eq!(result.err().unwrap().code, "records.file_query");
    let other_budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        inv.control.deadline(),
        inv.control.clone(),
    ));
    let result = db2::prepare_scoped(&inv, "RemoteSample", &other_budget, Some(&mut scope), None);
    assert_eq!(result.err().unwrap().code, "records.file_query");
    scope.finish(Ok(())).unwrap();
    assert!(f.captures().is_empty());
    f.close();
}

#[test]
fn default_remote_sql_and_db2_open_corrupt_metadata_before_pin_or_busy_lease() {
    let f = SqlFixture::new(false, false);
    let control = OperationControl::default();
    let held = lycheedev_core::workspace::data_resources::acquire(
        &f.source.home,
        Policy::Measurement,
        &control,
    )
    .unwrap();
    std::fs::write(
        f.source.home.join("state/toolkit.sqlite"),
        b"not a sqlite database",
    )
    .unwrap();
    let expected = Metadata::open_controlled(&f.source.home, true, &control)
        .err()
        .expect("corrupt metadata");
    for mut inv in [
        f.invocation("SELECT ID FROM RemoteSample"),
        invocation(&f, "data db2", &[("--id", "7")]),
        invocation(&f, "data db2 schema", &[]),
    ] {
        inv.flags
            .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
        // A regression that incorrectly waits on admission remains bounded.
        inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
        let error = crate::dispatch(&inv).unwrap_err();
        assert_eq!(error.code, expected.code);
        inv.control
            .check()
            .expect("must reject SQLite without waiting for a resource slot");
    }
    held.close().unwrap();
    f.close();
}

#[test]
fn default_remote_db2_original_stop_precedes_corrupt_metadata_and_missing_pin() {
    let f = SqlFixture::new(false, false);
    std::fs::write(f.source.home.join("state/toolkit.sqlite"), b"not sqlite").unwrap();
    let mut inv = invocation(&f, "data db2 schema", &[]);
    inv.flags
        .insert("--snapshot".into(), format!("PIN-{}", "0".repeat(64)));
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(crate::dispatch(&inv).unwrap_err().code, "command.cancelled");
    f.close();
}

#[test]
fn default_remote_db2_typed_project_selection_matches_snapshot_result_and_capture() {
    let f = SqlFixture::new(false, true);
    let pin = selection::load_pinned(&f.source.home, &f.snapshot).unwrap();
    let project = f.source.home.join("pin-request-project");
    std::fs::create_dir_all(project.join("nested")).unwrap();
    std::fs::write(
        project.join("lycheedev.json"),
        serde_json::to_vec(&json!({
            "schema":"lycheedev.project.v1", "product":f.source.pin.product,
        }))
        .unwrap(),
    )
    .unwrap();
    std::fs::write(
        project.join("lycheedev.lock.json"),
        serde_json::to_vec(&json!({
            "schema":"lycheedev.project-lock.v1", "selection":pin,
        }))
        .unwrap(),
    )
    .unwrap();
    // The default CDN provider must ignore a bad project when a snapshot wins.
    let mut snapshot = invocation(&f, "data db2", &[("--id", "7"), ("--project", "absent")]);
    snapshot.cwd = project.join("nested");
    let expected = crate::dispatch(&snapshot).unwrap();
    f.verify(&expected);
    for explicit in [false, true] {
        let mut project_inv = snapshot.clone();
        project_inv.flags.insert("--snapshot".into(), String::new());
        if explicit {
            project_inv
                .flags
                .insert("--project".into(), "absent/../../".into());
        } else {
            project_inv.flags.insert("--project".into(), String::new());
        }
        let actual = crate::dispatch(&project_inv).unwrap();
        assert_eq!(actual.data, expected.data);
        assert_eq!(actual.selection, expected.selection);
        assert_eq!(actual.complete, expected.complete);
        assert_eq!(actual.warnings, expected.warnings);
        assert_eq!(actual.coverage, expected.coverage);
        assert_eq!(actual.operation, expected.operation);
        // Captures have separate publication identities; each authenticates its
        // whole actual result and the same immutable pin, without normalizing data.
        f.verify(&actual);
    }
    let metadata = Metadata::open(&f.source.home, false).unwrap();
    assert_eq!(
        metadata.generation(&format!("pin/{}", f.snapshot)).unwrap(),
        Some(1)
    );
    metadata.close().unwrap();
    f.close();
}

#[test]
fn default_provider_typed_pin_resolution_retains_invocation_and_budget_owner_identity() {
    let f = SqlFixture::new(false, false);
    let inv = invocation(&f, "data db2 schema", &[]);
    let stopped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let budget = Arc::new(Budget::new(
        Cost::defaults(),
        inv.control.deadline(),
        Some(stopped.clone()),
    ));
    let scope = OpenedScope::open(&inv, &budget).unwrap().admit().unwrap();
    let pin = scope.resolve_pin().unwrap();
    assert_eq!(
        serde_json::to_value(&pin).unwrap(),
        serde_json::to_value(selection::load_pinned(&inv.home, &f.snapshot).unwrap()).unwrap(),
    );
    scope.check_consumer(&inv, &Arc::clone(&budget)).unwrap();
    let other_inv = inv.clone();
    let other_budget = Arc::new(Budget::new(Cost::defaults(), inv.control.deadline(), None));
    for error in [
        scope.check_consumer(&other_inv, &budget).unwrap_err(),
        scope.check_consumer(&inv, &other_budget).unwrap_err(),
    ] {
        assert_eq!(error.code, "records.file_query");
        assert_eq!(
            error.message,
            "Consumer differs from admitted invocation/budget"
        );
        assert_eq!(error.exit_code, 4);
    }
    stopped.store(true, Ordering::Release);
    let error = scope.resolve_pin().unwrap_err();
    assert_eq!(
        serde_json::to_value(&error).unwrap(),
        serde_json::to_value(budget.check().unwrap_err()).unwrap(),
    );
    inv.control.check().unwrap();
    assert!(scope.finish::<()>(Err(error)).is_err());
    lycheedev_core::workspace::data_resources::acquire(
        &f.source.home,
        Policy::Measurement,
        &inv.control,
    )
    .unwrap()
    .close()
    .unwrap();
    assert!(f.captures().is_empty());
    f.close();
}
