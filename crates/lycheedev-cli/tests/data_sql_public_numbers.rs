//! Actual binary output for fixed-Go SQL plan/row shapes and numeric tokens.
//! Reuse the packaged-Skill's authenticated synthetic catalog, not a row mock.
#[allow(dead_code)]
#[path = "../../lycheedev-data/tests/support/catalog_fixture.rs"]
mod catalog_fixture;

use catalog_fixture::{Fixture, decor_rows, decors};
use lycheedev_core::{evidence, metadata::Metadata};
use serde_json::{Value, json, value::RawValue};
use std::{collections::BTreeMap, fs, process::Command};

const CATALOG_SQL: &str = "SELECT ID, Name_lang, ModelFileDataID FROM HouseDecor WHERE ID IN (:third,:first,:duplicate,:second) ORDER BY ID";
const CATALOG_PARAMETERS: &str = r#"{"third":30,"first":10,"duplicate":10,"second":20}"#;

fn captures(fx: &Fixture) -> Vec<Value> {
    Metadata::open(&fx.home, false)
        .unwrap()
        .list("capture/", 100)
        .unwrap()
}

fn raw_field(raw: &str, field: &str) -> String {
    let object: BTreeMap<String, Box<RawValue>> = serde_json::from_str(raw).unwrap();
    object.get(field).unwrap().get().to_owned()
}

fn parameter_tokens(raw: &str) -> BTreeMap<String, String> {
    let query = raw_field(raw, "query");
    let parameters = raw_field(&query, "parameters");
    let object: BTreeMap<String, Box<RawValue>> = serde_json::from_str(&parameters).unwrap();
    object
        .into_iter()
        .map(|(key, value)| (key, value.get().to_owned()))
        .collect()
}

// Public binary64 formatting can turn an archived 1.0 into 1. Compare every
// field structurally, keeping integer tokens exact. The tests separately check
// original parameter and typed row tokens, including integers beyond 2^53.
fn same_value(public: &Value, archived: &Value) {
    match (public, archived) {
        (Value::Number(a), Value::Number(b)) if a != b => {
            assert!(
                b.as_str().contains(['.', 'e', 'E']),
                "an archived integer must not be rounded: {a} != {b}"
            );
            let a = a.as_f64().unwrap();
            let b = b.as_f64().unwrap();
            assert!(a.is_finite() && b.is_finite());
            // Value may normalize public -0 to integer 0. Signed zero is
            // asserted on both public and archived raw row tokens below.
            assert_eq!(a, b);
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b) {
                same_value(a, b);
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (key, a) in a {
                same_value(a, &b[key]);
            }
        }
        _ => assert_eq!(public, archived),
    }
}

fn successful_envelope(value: &Value) {
    assert_eq!(value["schema"], "lycheedev.result.v1");
    assert_eq!(value["ok"], true);
    assert_eq!(value["error"], Value::Null);
    assert_eq!(value["warnings"], json!([]));
}

struct Reading {
    value: Value,
    raw: Vec<String>,
    archived: String,
}

fn query(fx: &Fixture, case: &str, sql: &str, parameters: &str, format: &str) -> Reading {
    let before = captures(fx);
    let path = fx.root.join(format!("{case}.query.json"));
    fs::write(
        &path,
        format!(
            "{{\"sql\":{},\"parameters\":{parameters}}}\n",
            serde_json::to_string(sql).unwrap()
        ),
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
        .current_dir(&fx.root)
        .args(["data", "sql", "--file"])
        .arg(&path)
        .args(["--snapshot", &fx.pin, "--installation"])
        .arg(&fx.client)
        .args(["--offline", "--home"])
        .arg(&fx.home)
        .args(["--format", format])
        .output()
        .unwrap();
    // Preserve original public bytes and inputs in the caller-owned fixture,
    // including on assertion failure. No fixture Drop removes the evidence.
    fs::write(fx.root.join(format!("{case}.stdout.bin")), &output.stdout).unwrap();
    fs::write(fx.root.join(format!("{case}.stderr.bin")), &output.stderr).unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{case}: {} / {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty(), "{case}: {:?}", output.stderr);
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.ends_with('\n'));
    assert!(!stdout.contains("public_numbers"));
    let (raw, envelope): (Vec<String>, Option<Value>) = match format {
        "json" => {
            let envelope: Value = serde_json::from_str(&stdout).unwrap();
            (vec![raw_field(&stdout, "result")], Some(envelope))
        }
        "jsonl" => {
            let lines: Vec<_> = stdout.lines().collect();
            assert_eq!(lines.len(), 3, "one complete begin/record/end sequence");
            let frames: Vec<Value> = lines
                .iter()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(
                frames[0],
                json!({"type":"begin", "schema":"lycheedev.result.v1"})
            );
            assert_eq!(frames[1]["type"], "record");
            assert_eq!(frames[2]["type"], "end");
            let envelope = raw_field(lines[2], "envelope");
            (
                vec![
                    raw_field(lines[1], "result"),
                    raw_field(&envelope, "result"),
                ],
                Some(serde_json::from_str(&envelope).unwrap()),
            )
        }
        "text" => (vec![stdout], None),
        _ => panic!("unexpected format"),
    };
    let value: Value = serde_json::from_str(&raw[0]).unwrap();
    assert_eq!(
        value["query"],
        json!({"sql":sql,"parameters":serde_json::from_str::<Value>(parameters).unwrap()})
    );
    assert_eq!(value["complete"], true);
    assert_eq!(value["partial"], false);
    assert_eq!(value["sources"].as_array().unwrap().len(), 1);
    let file = &value["sources"][0]["file"];
    assert_eq!(file["source"], "installation");
    assert_eq!(file["entry"]["fileDataID"], 403);
    assert_eq!(file["contentVerified"], true);
    assert_eq!(file["verification"]["rootWholeDecodedCKey"], true);
    assert_eq!(file["verification"]["contentWholeDecodedCKey"], true);
    for key in ["definitionsMs", "preparationMs", "executionMs"] {
        let time = value["timings"][key].as_f64().unwrap();
        assert!(time.is_finite() && time >= 0.0);
    }
    let after = captures(fx);
    assert_eq!(after.len(), before.len() + 1, "exactly one query capture");
    let fresh: Vec<_> = after
        .iter()
        .filter(|capture| !before.iter().any(|old| old["id"] == capture["id"]))
        .collect();
    assert_eq!(fresh.len(), 1);
    let capture = fresh[0];
    let id = capture["id"].as_str().unwrap();
    assert_eq!(evidence::read_capture(&fx.home, id).unwrap(), *capture);
    assert_eq!(capture["provenance"]["kind"], "data-query");
    assert_eq!(capture["provenance"]["snapshot"], fx.pin);
    assert_eq!(capture["complete"], true);
    assert_eq!(capture["truncated"], false);
    let archived =
        String::from_utf8(evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap())
            .unwrap();
    let archived_value: Value = serde_json::from_str(&archived).unwrap();
    for reading in &raw {
        same_value(
            &serde_json::from_str::<Value>(reading).unwrap(),
            &archived_value,
        );
    }
    if let Some(envelope) = envelope {
        successful_envelope(&envelope);
        assert_eq!(envelope["captures"], json!([capture]));
        assert_eq!(envelope["result"], value);
    }
    let verified = Command::new(env!("CARGO_BIN_EXE_lycheedev"))
        .current_dir(&fx.root)
        .args(["evidence", "verify", id, "--home"])
        .arg(&fx.home)
        .args(["--format", "json"])
        .output()
        .unwrap();
    fs::write(
        fx.root.join(format!("{case}.verify.stdout.bin")),
        &verified.stdout,
    )
    .unwrap();
    fs::write(
        fx.root.join(format!("{case}.verify.stderr.bin")),
        &verified.stderr,
    )
    .unwrap();
    assert_eq!(
        verified.status.code(),
        Some(0),
        "{case}: {:?}",
        verified.stderr
    );
    assert!(verified.stderr.is_empty());
    let verified: Value = serde_json::from_slice(&verified.stdout).unwrap();
    successful_envelope(&verified);
    assert_eq!(verified["result"], *capture);
    assert_eq!(verified["captures"], json!([]));
    assert_eq!(
        captures(fx),
        after,
        "verification must not create another capture"
    );
    Reading {
        value,
        raw,
        archived,
    }
}

#[test]
fn actual_catalog_explain_and_analyze_cli_preserve_null_rows_and_captures() {
    let fx = Fixture::new(&[decors(&decor_rows())]);
    for format in ["json", "jsonl", "text"] {
        for (mode, prefix) in [("explain", "EXPLAIN "), ("analyze", "EXPLAIN ANALYZE ")] {
            let out = query(
                &fx,
                &format!("{mode}-{format}"),
                &format!("{prefix}{CATALOG_SQL}"),
                CATALOG_PARAMETERS,
                format,
            );
            let result = &out.value["result"];
            assert_eq!(result["columns"], Value::Null);
            assert_eq!(result["rows"], Value::Null);
            assert_eq!(
                result["plan"]["outputColumns"],
                json!(["ID", "Name_lang", "ModelFileDataID"])
            );
            assert!(!result["plan"]["steps"].as_array().unwrap().is_empty());
            if mode == "analyze" {
                let measured = &result["plan"]["measured"];
                assert_eq!(measured["outputRows"], 3);
                assert_eq!(
                    measured["scans"],
                    json!([{"table":{"catalog":"static","name":"housedecor"},"calls":3,"rows":3,"lookups":3}])
                );
                assert!(measured["workUnits"].as_u64().unwrap() > 0);
                assert!(measured["chargedBytes"].as_u64().unwrap() > 0);
            } else {
                assert!(result["plan"].get("measured").is_none());
            }
        }
    }
}

#[test]
fn actual_sql_cli_preserves_exact_parameters_and_canonical_float_rows() {
    let fx = Fixture::new(&[decors(&decor_rows())]);
    let parameters = r#"{"zero":-0,"negative":-0.0,"small":1E-07,"edge":1e-6,"wide":1.0e20,"huge":1e21,"unit":1.00,"big":9007199254740993,"max":18446744073709551615}"#;
    let expected_parameters: BTreeMap<String, Box<RawValue>> =
        serde_json::from_str(parameters).unwrap();
    let expected_parameters: BTreeMap<String, String> = expected_parameters
        .into_iter()
        .map(|(key, value)| (key, value.get().to_owned()))
        .collect();
    let sql = "SELECT :zero,:negative,:small,:edge,:wide,:huge,:unit,:big,:max FROM HouseDecor WHERE ID=10";
    for format in ["json", "jsonl", "text"] {
        for (mode, prefix) in [
            ("rows", ""),
            ("explain", "EXPLAIN "),
            ("analyze", "EXPLAIN ANALYZE "),
        ] {
            let out = query(
                &fx,
                &format!("numbers-{mode}-{format}"),
                &format!("{prefix}{sql}"),
                parameters,
                format,
            );
            for raw in out.raw.iter().chain([&out.archived]) {
                assert_eq!(
                    parameter_tokens(raw),
                    expected_parameters,
                    "{mode}/{format}"
                );
            }
            let archived_result = raw_field(&out.archived, "result");
            let archived_rows = raw_field(&archived_result, "rows");
            if mode == "rows" {
                let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_str(&archived_rows).unwrap();
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].len(), 9);
                for (index, exact) in [
                    (0, "0"),
                    (7, "9007199254740993"),
                    (8, "18446744073709551615"),
                ] {
                    assert_eq!(rows[0][index].get(), exact, "archived integer token");
                }
                // Archive spelling is its own contract, distinct from public
                // formatting. Preserve the stored float type, full binary64
                // value and negative-zero sign without rounding any integer.
                for (index, expected) in [(-0.0_f64), 1e-7, 1e-6, 1e20, 1e21, 1.0]
                    .into_iter()
                    .enumerate()
                {
                    let token = rows[0][index + 1].get();
                    assert!(
                        token.contains(['.', 'e', 'E']),
                        "archived float token: {token}"
                    );
                    assert_eq!(token.parse::<f64>().unwrap().to_bits(), expected.to_bits());
                }
            } else {
                assert_eq!(archived_rows, "null");
            }
            for raw in &out.raw {
                let result = raw_field(raw, "result");
                let rows = raw_field(&result, "rows");
                if mode == "rows" {
                    let rows: Vec<Vec<Box<RawValue>>> = serde_json::from_str(&rows).unwrap();
                    assert_eq!(rows.len(), 1);
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
                    assert!(out.value["result"].get("plan").is_none());
                } else {
                    assert_eq!(rows, "null");
                    assert_eq!(out.value["result"]["columns"], Value::Null);
                    assert!(out.value["result"]["plan"].is_object());
                }
            }
        }
    }
}

#[test]
fn actual_sql_cli_keeps_normal_empty_and_nonempty_rows_as_arrays() {
    let fx = Fixture::new(&[decors(&decor_rows())]);
    for format in ["json", "jsonl", "text"] {
        let full = query(
            &fx,
            &format!("full-{format}"),
            CATALOG_SQL,
            CATALOG_PARAMETERS,
            format,
        );
        assert_eq!(
            full.value["result"],
            json!({"columns":["ID","Name_lang","ModelFileDataID"],"rows":[[10,"无模型🙂",0],[20,"第二",501],[30,"第三",501]]})
        );
        let empty = query(
            &fx,
            &format!("empty-{format}"),
            "SELECT ID, Name_lang, ModelFileDataID FROM HouseDecor WHERE ID=:id",
            r#"{"id":999}"#,
            format,
        );
        assert_eq!(
            empty.value["result"],
            json!({"columns":["ID","Name_lang","ModelFileDataID"],"rows":[]})
        );
    }
}
