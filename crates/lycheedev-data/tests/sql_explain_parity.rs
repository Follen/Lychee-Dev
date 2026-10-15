//! Authenticated synthetic CLI data sources; no client or live-game claim.
//! Fixed Go 3.2 EXPLAIN query/capture/CSV contracts. Execution: NOT_RUN.
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell, Fixture, table};
use lycheedev_core::{Outcome, evidence, metadata::Metadata};
use serde_json::{Value, json};
use std::fs;
fn fixture(bad: bool) -> Fixture {
    Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![
            Cell::U(1),
            if bad {
                Cell::BadString
            } else {
                Cell::Text("valid".into())
            },
        ]],
    )])
}
fn count(fx: &Fixture) -> usize {
    Metadata::open(&fx.home, false)
        .unwrap()
        .list::<Value>("capture/", 1000)
        .unwrap()
        .len()
}
fn run(fx: &Fixture, sql: &str) -> lycheedev_core::Result<Outcome> {
    lycheedev_data::dispatch(&fx.invocation("data sql", &[("--sql", sql)]))
}
fn verify(fx: &Fixture, out: &Outcome) {
    assert_eq!(out.provenance.len(), 1);
    assert_eq!(out.data["result"]["columns"], Value::Null);
    assert_eq!(out.data["result"]["rows"], Value::Null);
    assert!(out.data["result"]["plan"].get("measured").is_none());
    assert_eq!(out.provenance[0]["provenance"]["kind"], "data-query");
    assert_eq!(
        serde_json::from_slice::<Value>(
            &evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap()
        )
        .unwrap(),
        out.data
    );
}
#[test]
fn explain_prepares_authentic_wdc_schema_without_decoding_bad_non_identity_field() {
    let fx = fixture(true);
    let out = run(&fx, "EXPLAIN SELECT Name FROM Sample").unwrap();
    verify(&fx, &out);
    assert_eq!(out.data["result"]["plan"]["outputColumns"], json!(["Name"]));
    assert_eq!(out.data["complete"], true);
    assert_eq!(out.data["sources"].as_array().unwrap().len(), 1);
    let before = count(&fx);
    assert!(run(&fx, "SELECT Name FROM Sample").is_err());
    assert_eq!(count(&fx), before);
}
#[test]
fn explain_still_rejects_duplicate_identity_indices() {
    let fx = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint")],
        &[vec![Cell::U(1)], vec![Cell::U(1)]],
    )]);
    let before = count(&fx);
    assert!(run(&fx, "EXPLAIN SELECT ID FROM Sample").is_err());
    assert_eq!(count(&fx), before);
}
#[test]
fn explain_failure_is_no_capture_and_analyze_is_explicitly_incomplete() {
    let fx = fixture(false);
    let before = count(&fx);
    for sql in [
        "EXPLAIN SELECT Missing FROM Sample",
        "EXPLAIN SELECT 1",
        "EXPLAIN SELECT ID FROM Missing",
    ] {
        assert!(run(&fx, sql).is_err(), "{sql}");
        assert_eq!(count(&fx), before);
    }
    let error = run(&fx, "EXPLAIN ANALYZE SELECT ID FROM Sample").unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("query.unsupported_expression", 3)
    );
    assert_eq!(count(&fx), before);
    verify(&fx, &run(&fx, "EXPLAIN SELECT ID FROM Sample").unwrap());
}
#[test]
fn explain_csv_publishes_one_newline_null_columns_and_two_captures() {
    let fx = fixture(true);
    let path = fx.root.join("explain.csv");
    let out = lycheedev_data::dispatch(&fx.invocation(
        "data sql",
        &[
            ("--sql", "EXPLAIN SELECT ID,Name FROM Sample"),
            ("--encoding", "csv"),
            ("--output", path.to_str().unwrap()),
        ],
    ))
    .unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"\n");
    assert_eq!(out.data["columns"], Value::Null);
    assert_eq!(out.data["rows"], 0);
    assert_eq!(out.data["bytes"], 1);
    assert_eq!(out.provenance.len(), 2);
    assert_eq!(
        out.provenance[1]["dependencies"][0],
        out.provenance[0]["id"]
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &fs::read(format!("{}.manifest.json", path.display())).unwrap()
        )
        .unwrap(),
        out.data
    );
    let archived = serde_json::from_slice::<Value>(
        &evidence::read_blob(&fx.home, &out.provenance[0]["blob"], 16 << 20).unwrap(),
    )
    .unwrap();
    assert_eq!(
        archived["result"]["plan"]["outputColumns"],
        json!(["ID", "Name"])
    );
    assert_eq!(
        evidence::read_blob(&fx.home, &out.provenance[1]["blob"], 32 << 20).unwrap(),
        b"\n"
    );
}
#[test]
fn explain_csv_conflict_preserves_pair_but_retains_successful_query_capture() {
    let fx = fixture(false);
    let path = fx.root.join("explain.csv");
    let sidecar = format!("{}.manifest.json", path.display());
    fs::write(&path, b"old").unwrap();
    fs::write(&sidecar, b"manifest").unwrap();
    let before = count(&fx);
    let error = lycheedev_data::dispatch(&fx.invocation(
        "data sql",
        &[
            ("--sql", "EXPLAIN SELECT ID FROM Sample"),
            ("--encoding", "csv"),
            ("--output", path.to_str().unwrap()),
        ],
    ))
    .unwrap_err();
    assert_eq!(error.code, "records.export_conflict");
    assert_eq!(count(&fx), before + 1);
    assert_eq!(fs::read(path).unwrap(), b"old");
    assert_eq!(fs::read(sidecar).unwrap(), b"manifest");
}
