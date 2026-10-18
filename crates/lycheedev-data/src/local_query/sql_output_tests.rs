//! Actual SQL consumers distinguish CSV manifests from ordinary numeric output.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell::U, Fixture, table};
use lycheedev_core::{metadata::Metadata, public_json, storage};
use serde_json::value::RawValue;
use std::{cell::Cell, fs};

thread_local! {
    static NUMBER_LAYOUT_CALLS: Cell<Option<usize>> = const { Cell::new(None) };
}

// Observation only: no replacement result, changed budget or injected failure.
// Other tests and unobserved calls never acquire a persistent counter.
pub(super) fn observe_number_layout() {
    NUMBER_LAYOUT_CALLS.with(|calls| {
        if let Some(count) = calls.get() {
            calls.set(Some(count + 1));
        }
    });
}

fn with_layout_count<T>(operation: impl FnOnce() -> T) -> (T, usize) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            NUMBER_LAYOUT_CALLS.with(|calls| calls.set(None));
        }
    }
    NUMBER_LAYOUT_CALLS.with(|calls| {
        assert!(
            calls.get().is_none(),
            "numeric layout observer already active"
        );
        calls.set(Some(0));
    });
    let _reset = Reset;
    let result = operation();
    let count = NUMBER_LAYOUT_CALLS.with(|calls| calls.get().unwrap());
    (result, count)
}

const SQL: &str = "SELECT :zero AS c0,:negative AS c1,:small AS c2,:edge AS c3,:wide AS c4,:huge AS c5,:unit AS c6,:big AS c7,:max AS c8 FROM Sample";
const PARAMETERS: &str = r#"{"zero":-0,"negative":-0.0,"small":1E-07,"edge":1e-6,"wide":1.0e20,"huge":1e21,"unit":1.00,"big":9007199254740993,"max":18446744073709551615}"#;
const CSV: &[u8] = b"c0,c1,c2,c3,c4,c5,c6,c7,c8\n0,-0,1e-07,1e-06,1e+20,1e+21,1,9007199254740993,18446744073709551615\n";

fn fixture() -> Fixture {
    Fixture::new(&[table("Sample", 201, &[("ID", "uint")], &[vec![U(1)]])])
}

fn invocation(fx: &Fixture, csv: bool) -> Invocation {
    let request = fx.root.join("numeric-query.json");
    fs::write(
        &request,
        format!(
            "{{\"sql\":{},\"parameters\":{PARAMETERS}}}\n",
            serde_json::to_string(SQL).unwrap()
        ),
    )
    .unwrap();
    let mut inv = fx.invocation("data sql", &[("--file", request.to_str().unwrap())]);
    if csv {
        inv.flags.insert("--encoding".into(), "csv".into());
        inv.flags.insert(
            "--output".into(),
            fx.root.join("numeric.csv").to_string_lossy().into_owned(),
        );
    }
    inv
}

fn captures(fx: &Fixture) -> Vec<Value> {
    let metadata = Metadata::open(&fx.home, false).unwrap();
    let result = metadata.list("capture/", 1000).unwrap();
    metadata.close().unwrap();
    result
}

fn raw_field(raw: &str, key: &str) -> String {
    let object: BTreeMap<String, Box<RawValue>> = serde_json::from_str(raw).unwrap();
    object[key].get().to_owned()
}

fn parameter_tokens(raw: &str) -> BTreeMap<String, String> {
    let tokens: BTreeMap<String, Box<RawValue>> = serde_json::from_str(raw).unwrap();
    tokens
        .into_iter()
        .map(|(key, value)| (key, value.get().to_owned()))
        .collect()
}

fn verified_body(fx: &Fixture, capture: &Value) -> Vec<u8> {
    let id = capture["id"].as_str().unwrap();
    assert_eq!(evidence::read_capture(&fx.home, id).unwrap(), *capture);
    let bytes = evidence::read_blob(&fx.home, &capture["blob"], 32 << 20).unwrap();
    assert_eq!(capture["blob"]["sha256"], storage::sha256(&bytes));
    assert_eq!(capture["blob"]["bytes"], bytes.len());
    assert_eq!(capture["complete"], true);
    assert_eq!(capture["truncated"], false);
    let mut inv = fx.invocation("evidence verify", &[]);
    inv.flags.clear();
    inv.positional = Some(id.into());
    assert_eq!(evidence::dispatch(&inv).unwrap().data["verified"], true);
    bytes
}

fn assert_query_capture(fx: &Fixture, capture: &Value) -> Value {
    assert_eq!(capture["provenance"]["kind"], "data-query");
    assert_eq!(capture["provenance"]["snapshot"], fx.pin);
    let raw = String::from_utf8(verified_body(fx, capture)).unwrap();
    let body: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(body["complete"], true);
    assert_eq!(body["partial"], false);
    assert_eq!(body["query"]["sql"], SQL);
    assert_eq!(
        body["result"]["columns"],
        json!(["c0", "c1", "c2", "c3", "c4", "c5", "c6", "c7", "c8"])
    );
    assert_eq!(body["sources"].as_array().unwrap().len(), 1);
    assert_eq!(body["sources"][0]["file"]["entry"]["fileDataID"], 201);
    assert_eq!(body["sources"][0]["file"]["contentVerified"], true);
    let query = raw_field(&raw, "query");
    assert_eq!(
        parameter_tokens(&raw_field(&query, "parameters")),
        parameter_tokens(PARAMETERS)
    );
    let result = raw_field(&raw, "result");
    let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_str(&raw_field(&result, "rows")).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 9);
    for (index, token) in [
        (0, "0"),
        (7, "9007199254740993"),
        (8, "18446744073709551615"),
    ] {
        assert_eq!(rows[0][index].get(), token);
    }
    for (index, expected) in [-0.0_f64, 1e-7, 1e-6, 1e20, 1e21, 1.0]
        .into_iter()
        .enumerate()
    {
        let token = rows[0][index + 1].get();
        assert!(token.contains(['.', 'e', 'E']), "archived float {token}");
        assert_eq!(token.parse::<f64>().unwrap().to_bits(), expected.to_bits());
    }
    body
}

#[test]
fn csv_numeric_consumer_skips_layout_and_preserves_exact_output_and_both_captures() {
    let fx = fixture();
    let inv = invocation(&fx, true);
    let before = captures(&fx).len();
    let (out, calls) = with_layout_count(|| crate::dispatch(&inv));
    let out = out.unwrap();
    assert_eq!(calls, 0);
    assert!(matches!(
        &out.public_numbers,
        public_json::NumberLayout::Verbatim
    ));
    assert!(out.complete);
    assert_eq!(out.selection["id"], fx.pin);
    assert_eq!(out.provenance.len(), 2);
    assert_query_capture(&fx, &out.provenance[0]);
    let csv = &out.provenance[1];
    assert_eq!(verified_body(&fx, csv), CSV);
    assert_eq!(fs::read(fx.root.join("numeric.csv")).unwrap(), CSV);
    assert_eq!(csv["provenance"]["kind"], "data-query-csv");
    assert_eq!(csv["dependencies"], json!([out.provenance[0]["id"]]));
    let manifest: Value =
        serde_json::from_slice(&fs::read(fx.root.join("numeric.csv.manifest.json")).unwrap())
            .unwrap();
    assert_eq!(manifest, out.data);
    assert_eq!(manifest["schema"], "lycheedev.query-csv.v1");
    assert_eq!(manifest["queryCapture"], out.provenance[0]["id"]);
    assert_eq!(manifest["csvCapture"], csv["id"]);
    assert_eq!(manifest["sha256"], storage::sha256(CSV));
    assert_eq!(manifest["bytes"], CSV.len());
    assert_eq!(manifest["rows"], 1);
    assert_eq!(captures(&fx).len(), before + 2);
}

#[test]
fn ordinary_numeric_consumer_keeps_layout_and_full_public_and_archive_tokens() {
    let fx = fixture();
    let inv = invocation(&fx, false);
    let before = captures(&fx).len();
    let (out, calls) = with_layout_count(|| crate::dispatch(&inv));
    let out = out.unwrap();
    assert_eq!(calls, 1);
    assert!(matches!(
        &out.public_numbers,
        public_json::NumberLayout::Object(_)
    ));
    assert_eq!(out.provenance.len(), 1);
    assert_eq!(assert_query_capture(&fx, &out.provenance[0]), out.data);
    let mut public = Vec::new();
    public_json::write_line(&mut public, &out.data, &out.public_numbers).unwrap();
    let public = String::from_utf8(public).unwrap();
    let query = raw_field(&public, "query");
    assert_eq!(
        parameter_tokens(&raw_field(&query, "parameters")),
        parameter_tokens(PARAMETERS)
    );
    let result = raw_field(&public, "result");
    let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_str(&raw_field(&result, "rows")).unwrap();
    let tokens: Vec<_> = rows[0].iter().map(|cell| cell.get()).collect();
    assert_eq!(
        tokens,
        [
            "0",
            "-0",
            "1e-7",
            "0.000001",
            "100000000000000000000",
            "1e+21",
            "1",
            "9007199254740993",
            "18446744073709551615"
        ]
    );
    assert_eq!(captures(&fx).len(), before + 1);
}

#[test]
fn csv_conflict_still_follows_query_capture_without_constructing_layout() {
    let fx = fixture();
    let inv = invocation(&fx, true);
    let path = fx.root.join("numeric.csv");
    let sidecar = fx.root.join("numeric.csv.manifest.json");
    fs::write(&path, b"original data").unwrap();
    fs::write(&sidecar, b"original manifest").unwrap();
    let before = captures(&fx);
    let (error, calls) = with_layout_count(|| crate::dispatch(&inv));
    let error = error.unwrap_err();
    assert_eq!(calls, 0);
    assert_eq!(error.code, "records.export_conflict");
    assert_eq!(fs::read(path).unwrap(), b"original data");
    assert_eq!(fs::read(sidecar).unwrap(), b"original manifest");
    let after = captures(&fx);
    let fresh: Vec<_> = after
        .iter()
        .filter(|capture| !before.iter().any(|old| old["id"] == capture["id"]))
        .collect();
    assert_eq!(fresh.len(), 1);
    assert_query_capture(&fx, fresh[0]);
    assert_eq!(error.details["completedCaptures"], json!([fresh[0]["id"]]));
    assert_eq!(error.details["outputPublished"], false);
}

#[test]
fn csv_post_execution_cancel_still_precedes_capture_and_output() {
    let fx = fixture();
    let inv = invocation(&fx, true);
    let before = captures(&fx);
    let cancel = inv.control.tracked_cancel_handle().unwrap();
    let mut executions = 0;
    let (result, calls) = with_layout_count(|| {
        sql_inner_after_execute(&inv, &mut || {
            executions += 1;
            cancel.cancel();
            Ok(())
        })
    });
    let error = result.unwrap_err();
    assert_eq!(executions, 1);
    assert_eq!(calls, 0);
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert_eq!(captures(&fx), before);
    assert!(!fx.root.join("numeric.csv").exists());
    assert!(!fx.root.join("numeric.csv.manifest.json").exists());
}

#[test]
fn numeric_layout_observer_is_thread_local_and_resets_on_unwind() {
    let (_, calls) = with_layout_count(|| {
        observe_number_layout();
        std::thread::spawn(|| {
            NUMBER_LAYOUT_CALLS.with(|calls| assert_eq!(calls.get(), None));
            observe_number_layout();
        })
        .join()
        .unwrap();
        assert!(std::panic::catch_unwind(|| with_layout_count(|| ())).is_err());
        observe_number_layout();
    });
    assert_eq!(calls, 2);
    assert!(std::panic::catch_unwind(|| with_layout_count(|| panic!("fixture unwind"))).is_err());
    NUMBER_LAYOUT_CALLS.with(|calls| assert_eq!(calls.get(), None));
    assert_eq!(with_layout_count(|| ()).1, 0);
}
