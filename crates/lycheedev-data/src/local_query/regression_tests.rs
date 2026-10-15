//! Review-driven boundaries for fixed Go sql.go admission and CheckSource.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, Fixture, table};
use lycheedev_core::metadata::Metadata;
use std::fs;

#[test]
fn sql_go_admission_orders_table_count_names_size_before_numeric_conversion() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let file = fx.root.join("oversize-and-overflow.json");
    let raw = format!(
        r#"{{"sql":"SELECT :n,:text FROM Sample","parameters":{{"n":1e999,"text":"{}"}}}}"#,
        "<".repeat(200_000)
    );
    assert!(raw.len() < 1 << 20);
    fs::write(&file, raw).unwrap();
    let mut inv = fx.invocation("data sql", &[("--file", file.to_str().unwrap())]);
    inv.home = fx.root.join("must-not-create");
    assert_eq!(sql(&inv).unwrap_err().code, "query.budget_exceeded");
    assert!(!inv.home.exists());
    let sql_text = (0..33)
        .map(|index| format!("SELECT :missing FROM T{index}"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    assert_eq!(
        relational::Program::compile(&sql_text)
            .unwrap()
            .source_tables()
            .len(),
        33
    );
    let mut inv = fx.invocation("data sql", &[("--sql", &sql_text)]);
    inv.home = fx.root.join("must-not-create");
    assert_eq!(
        sql(&inv).unwrap_err().code,
        "query.budget_exceeded",
        "Physical source count precedes the missing parameter error"
    );
    assert!(!inv.home.exists());
}
#[test]
fn sql_checks_changed_casc_metadata_after_execution_before_capture_and_recovers() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let inv = fx.invocation("data sql", &[("--sql", "SELECT ID FROM Sample")]);
    let catalog = fx.client.parent().unwrap().join(".build.info");
    let original = fs::read(&catalog).unwrap();
    let before = Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 100)
        .unwrap()
        .len();
    let mut executed = false;
    let error = sql_inner_after_execute(&inv, &mut || {
        executed = true;
        let mut changed = original.clone();
        changed.push(b'\n');
        fs::write(&catalog, changed)?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        executed,
        "Mutation happens only after SQL has returned its result"
    );
    assert_eq!(error.code, "records.pinned_build_changed");
    assert_eq!(
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        before,
        "No query capture may precede final source verification"
    );
    fs::write(&catalog, original).unwrap();
    let out = sql(&inv).unwrap();
    assert_eq!(out.data["result"]["rows"], json!([[1]]));
    assert_eq!(out.provenance.len(), 1);
}

#[test]
fn sql_normalizes_numeric_parameters_after_pin_before_any_source_resolution() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let file = fx.root.join("numeric-source-conflict.json");
    let missing_installation = fx.root.join("missing-installation");
    assert!(!missing_installation.exists());
    let before = Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 100)
        .unwrap()
        .len();
    for missing_definition in [false, true] {
        for missing_casc in [false, true] {
            let table = if missing_definition {
                "Missing"
            } else {
                "Sample"
            };
            let sql_text = format!("SELECT :n FROM effective.{table}");
            let mut inv = fx.invocation("data sql", &[("--file", file.to_str().unwrap())]);
            if missing_casc {
                inv.flags.insert(
                    "--installation".into(),
                    missing_installation.to_string_lossy().into_owned(),
                );
            }
            assert!(selection::resolve_pinned(&inv).unwrap().data.is_some());
            // With a canonical scalar, prove the later provider conflict exists.
            fs::write(
                &file,
                format!(r#"{{"sql":"{sql_text}","parameters":{{"n":1}}}}"#),
            )
            .unwrap();
            let source_error = sql(&inv).unwrap_err();
            if missing_definition {
                assert_eq!(source_error.code, "schema.definition_not_found");
            } else if missing_casc {
                assert_ne!(source_error.code, "records.hotfix_identity");
                assert_ne!(source_error.code, "query.numeric_range");
            } else {
                assert_eq!(source_error.code, "records.hotfix_identity");
            }
            fs::write(
                &file,
                format!(r#"{{"sql":"{sql_text}","parameters":{{"n":1e999}}}}"#),
            )
            .unwrap();
            assert_eq!(
                sql(&inv).unwrap_err().code,
                "query.numeric_range",
                "Numeric normalization precedes the provider error {} (missing definition={missing_definition}, missing CASC={missing_casc})",
                source_error.code
            );
        }
    }
    assert_eq!(
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        before
    );
}

#[test]
fn explain_rechecks_source_after_binding_before_capture() {
    let fx = Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])]);
    let inv = fx.invocation("data sql", &[("--sql", "EXPLAIN SELECT ID FROM Sample")]);
    let catalog = fx.client.parent().unwrap().join(".build.info");
    let original = fs::read(&catalog).unwrap();
    let count = || {
        Metadata::open(&fx.home, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len()
    };
    let before = count();
    let error = sql_inner_after_execute(&inv, &mut || {
        let mut changed = original.clone();
        changed.push(b'\n');
        fs::write(&catalog, changed)?;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "records.pinned_build_changed");
    assert_eq!(count(), before);
    fs::write(catalog, original).unwrap();
    assert!(sql(&inv).unwrap().data["result"]["plan"].is_object());
}

#[test]
fn sql_checked_close_error_preserves_actual_committed_capture_identities() {
    let primary = fault("storage.close_failed", 5, "real close failure");
    let ids = vec!["CAP-query".to_owned(), "CAP-csv".to_owned()];
    let error = sql_committed_error(primary.clone(), &ids, true);
    assert_eq!(error.code, primary.code);
    assert_eq!(error.exit_code, primary.exit_code);
    assert_eq!(
        error.details["primary"],
        serde_json::to_value(primary).unwrap()
    );
    assert_eq!(error.details["completedCaptures"], json!(ids));
    assert_eq!(error.details["outputPublished"], true);
    assert!(error.message.contains("CAP-query, CAP-csv"));
    let early = fault("schema.definition_not_found", 3, "missing table");
    let unchanged = sql_committed_error(early.clone(), &[], false);
    assert_eq!(
        serde_json::to_value(unchanged).unwrap(),
        serde_json::to_value(early).unwrap()
    );
}
