//! Keep the former double-Value projection as an independent byte oracle.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell, Fixture, table};
use lycheedev_core::{metadata::Metadata, storage};
use relational::{Explanation, QueryResult, Scalar};
use std::fs;

// Deliberately do not call QueryResult::serialize: this is the removed public
// method's projection, including the outer Value serialization at its caller.
fn legacy_reading(result: &QueryResult) -> Value {
    let public = if let Some(plan) = &result.plan {
        json!({"columns":null,"rows":null,"plan":plan})
    } else {
        json!({"columns":result.columns,"rows":result.rows})
    };
    json!({"result":public})
}

fn assert_legacy_projection(result: &QueryResult) -> Value {
    let actual = json!({"result":result});
    let expected = legacy_reading(result);
    assert_eq!(actual, expected);
    assert_eq!(
        serde_json::to_vec(&actual).unwrap(),
        serde_json::to_vec(&expected).unwrap(),
        "The archive's complete compact JSON bytes must remain identical"
    );
    let archive = |value: &Value| {
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let raw = capture::json_bytes(value, &budget, 1 << 20, || {
            fault("relational.resource_limit", 3, "projection test byte limit")
        })
        .unwrap();
        (raw, budget.snapshot().unwrap())
    };
    assert_eq!(archive(&actual), archive(&expected));
    actual
}

#[test]
fn typed_result_projection_matches_legacy_scalar_and_empty_bytes() {
    let floats = [
        -0.0,
        0.0,
        1.0,
        1e-7,
        1e-6,
        1e20,
        1e21,
        f64::from_bits(1),
        f64::MIN_POSITIVE,
        f64::MAX,
    ];
    let mut row = vec![
        Scalar::Null,
        Scalar::Bool(false),
        Scalar::Bool(true),
        Scalar::Int(i64::MIN),
        Scalar::Int(i64::MAX),
        Scalar::UInt(9_007_199_254_740_993),
        Scalar::UInt(u64::MAX),
        Scalar::String("<>&\u{2028}\u{2029}\n\r\t\"\\界🙂".repeat(1024)),
    ];
    row.extend(floats.into_iter().map(Scalar::Float));
    row.extend([f64::NAN, f64::INFINITY, f64::NEG_INFINITY].map(Scalar::Float));
    let mut result = QueryResult {
        plan: None,
        columns: (0..row.len()).map(|index| format!("c{index}")).collect(),
        rows: vec![row],
        complete: false,
    };
    let actual = assert_legacy_projection(&result);
    let row = actual["result"]["rows"][0].as_array().unwrap();
    assert_eq!(row[3].to_string(), "-9223372036854775808");
    assert_eq!(row[4].to_string(), "9223372036854775807");
    assert_eq!(row[5].to_string(), "9007199254740993");
    assert_eq!(row[6].to_string(), "18446744073709551615");
    for (actual, expected) in row[8..8 + floats.len()].iter().zip(floats) {
        assert_eq!(actual.as_f64().unwrap().to_bits(), expected.to_bits());
    }
    assert!(row[8 + floats.len()..].iter().all(Value::is_null));
    assert!(actual["result"].get("complete").is_none());
    assert!(actual["result"].get("plan").is_none());
    // These cases distinguish empty arrays from the EXPLAIN null projection.
    result.rows.clear();
    assert_eq!(
        assert_legacy_projection(&result)["result"]["rows"],
        json!([])
    );
    result.columns.clear();
    assert_eq!(
        assert_legacy_projection(&result),
        json!({"result":{"columns":[],"rows":[]}})
    );
}

#[test]
fn typed_result_projection_matches_legacy_plan_and_measurement_bytes() {
    let mut result = QueryResult {
        plan: Some(Explanation {
            output_columns: vec!["ID".into()],
            steps: vec![relational::PlanStep {
                id: 1,
                parent: 0,
                operation: "select",
                detail: String::new(),
            }],
            measured: None,
        }),
        // A plan must hide these even if an internal caller retains rows.
        columns: vec!["hidden".into()],
        rows: vec![vec![Scalar::Float(f64::NAN)]],
        complete: false,
    };
    for measured in [
        None,
        Some(relational::ExecutionMeasurement {
            scans: vec![relational::ScanMeasurement {
                table: relational::MeasuredTable {
                    catalog: "static".into(),
                    name: "sample".into(),
                },
                calls: 1,
                rows: 2,
                lookups: 0,
            }],
            work_units: 9_007_199_254_740_993,
            charged_bytes: 42,
            output_rows: 2,
        }),
    ] {
        result.plan.as_mut().unwrap().measured = measured;
        let actual = assert_legacy_projection(&result);
        assert!(actual["result"]["columns"].is_null());
        assert!(actual["result"]["rows"].is_null());
        assert!(actual["result"].get("complete").is_none());
        let plan = &actual["result"]["plan"];
        assert!(plan["steps"][0].get("detail").is_none());
        if result.plan.as_ref().unwrap().measured.is_some() {
            assert_eq!(
                plan["measured"]["workUnits"].to_string(),
                "9007199254740993"
            );
            assert!(plan["measured"]["scans"][0].get("lookups").is_none());
        } else {
            assert!(plan.get("measured").is_none());
        }
    }
}

fn verified_body(fx: &Fixture, capture: &Value) -> Vec<u8> {
    let id = capture["id"].as_str().unwrap();
    assert_eq!(evidence::read_capture(&fx.home, id).unwrap(), *capture);
    let raw = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    assert_eq!(capture["blob"]["sha256"], storage::sha256(&raw));
    assert_eq!(capture["blob"]["bytes"], raw.len());
    assert_eq!(capture["complete"], true);
    assert_eq!(capture["truncated"], false);
    let mut inv = fx.invocation("evidence verify", &[]);
    inv.flags.clear();
    inv.positional = Some(id.into());
    assert_eq!(evidence::dispatch(&inv).unwrap().data["verified"], true);
    raw
}

#[test]
fn actual_sql_projection_keeps_plain_explain_analyze_and_csv_captures() {
    let fx = Fixture::new(&[table(
        "Sample",
        201,
        &[("ID", "uint"), ("Value", "text")],
        &[
            vec![Cell::U(1), Cell::Text("<>&".into())],
            vec![Cell::U(2), Cell::Text("two".into())],
        ],
    )]);
    let metadata = Metadata::open(&fx.home, false).unwrap();
    let before = metadata.list::<Value>("capture/", 100).unwrap().len();
    metadata.close().unwrap();
    for (prefix, csv) in [
        ("", false),
        ("EXPLAIN ", false),
        ("EXPLAIN ANALYZE ", false),
        ("", true),
    ] {
        let sql = format!("{prefix}SELECT ID,Value FROM Sample ORDER BY ID");
        let path = fx.root.join("projection.csv");
        let mut inv = fx.invocation("data sql", &[("--sql", &sql)]);
        if csv {
            inv.flags.insert("--encoding".into(), "csv".into());
            inv.flags
                .insert("--output".into(), path.to_string_lossy().into_owned());
        }
        let out = crate::migration_oracle::dispatch(&inv).unwrap();
        assert!(out.complete);
        assert_eq!(out.selection["id"], fx.pin);
        assert_eq!(out.provenance.len(), if csv { 2 } else { 1 });
        let query = &out.provenance[0];
        assert_eq!(query["provenance"]["kind"], "data-query");
        assert_eq!(query["provenance"]["snapshot"], fx.pin);
        let body: Value = serde_json::from_slice(&verified_body(&fx, query)).unwrap();
        assert_eq!(body["query"]["sql"], sql);
        assert_eq!(body["complete"], true);
        assert_eq!(body["partial"], false);
        assert_eq!(body["sources"].as_array().unwrap().len(), 1);
        if prefix.is_empty() {
            assert_eq!(
                body["result"],
                json!({"columns":["ID","Value"],"rows":[[1,"<>&"],[2,"two"]]})
            );
        } else {
            assert!(body["result"]["columns"].is_null());
            assert!(body["result"]["rows"].is_null());
            let plan = &body["result"]["plan"];
            assert_eq!(plan["outputColumns"], json!(["ID", "Value"]));
            assert!(!plan["steps"].as_array().unwrap().is_empty());
            if prefix == "EXPLAIN ANALYZE " {
                assert_eq!(plan["measured"]["outputRows"], 2);
            } else {
                assert!(plan.get("measured").is_none());
            }
        }
        if csv {
            let csv_capture = &out.provenance[1];
            let raw = b"ID,Value\n1,<>&\n2,two\n";
            assert_eq!(verified_body(&fx, csv_capture), raw);
            assert_eq!(fs::read(&path).unwrap(), raw);
            assert_eq!(csv_capture["dependencies"], json!([query["id"]]));
            let manifest: Value = serde_json::from_slice(
                &fs::read(path.with_extension("csv.manifest.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(manifest, out.data);
            assert_eq!(manifest["queryCapture"], query["id"]);
            assert_eq!(manifest["csvCapture"], csv_capture["id"]);
            assert_eq!(manifest["columns"], json!(["ID", "Value"]));
            assert_eq!(manifest["rows"], 2);
            assert_eq!(manifest["sha256"], storage::sha256(raw));
            assert_eq!(manifest["bytes"], raw.len());
        } else {
            assert_eq!(body, out.data);
        }
    }
    let metadata = Metadata::open(&fx.home, false).unwrap();
    assert_eq!(
        metadata.list::<Value>("capture/", 100).unwrap().len(),
        before + 5
    );
    metadata.close().unwrap();
}
