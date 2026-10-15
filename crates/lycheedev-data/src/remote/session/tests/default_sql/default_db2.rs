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
