use lycheedev_data::relational::{Limits, Program, Scalar, Source};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn scalar(value: &Value) -> Scalar {
    match value {
        Value::Null => Scalar::Null,
        Value::Bool(v) => Scalar::Bool(*v),
        Value::String(v) => Scalar::String(v.clone()),
        Value::Number(v) => {
            if let Some(v) = v.as_i64() {
                Scalar::Int(v)
            } else if let Some(v) = v.as_u64() {
                Scalar::UInt(v)
            } else {
                Scalar::Float(v.as_f64().unwrap())
            }
        }
        _ => panic!("fixture scalar"),
    }
}
fn sources(case: &Value) -> BTreeMap<String, Source> {
    case["sources"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(name, value)| {
            (
                name.clone(),
                Source {
                    columns: value["columns"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|v| v.as_str().unwrap().into())
                        .collect(),
                    rows: value["rows"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|r| r.as_array().unwrap().iter().map(scalar).collect())
                        .collect(),
                    complete: true,
                },
            )
        })
        .collect()
}
// JSON distinguishes numeric value, not notation (2, 2.0, 2e0). Canonicalize
// decimal digits/exponent exactly, without f64 conversion, epsilon or lost u64s.
fn numeric_json(value: Value) -> Value {
    match value {
        Value::Number(n) => {
            let text = n.to_string();
            let (mantissa, exponent) = text
                .split_once(['e', 'E'])
                .map_or((text.as_str(), 0i32), |(m, e)| (m, e.parse().unwrap()));
            let negative = mantissa.starts_with('-');
            let mantissa = mantissa.trim_start_matches('-');
            let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len()) as i32;
            let mut digits = mantissa
                .replace('.', "")
                .trim_start_matches('0')
                .to_string();
            let mut power = exponent - fraction;
            while digits.ends_with('0') {
                digits.pop();
                power += 1;
            }
            if digits.is_empty() {
                digits = "0".into();
                power = 0;
            }
            json!({"exactNumber":format!("{}{}e{}",if negative{"-"}else{""},digits,power)})
        }
        Value::Array(items) => Value::Array(items.into_iter().map(numeric_json).collect()),
        Value::Object(items) => Value::Object(
            items
                .into_iter()
                .map(|(k, v)| (k, numeric_json(v)))
                .collect(),
        ),
        v => v,
    }
}

#[test]
fn frozen_go_union_all_order_scope_and_exact_rows() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/compound-cases.json"
    ))
    .unwrap();
    let expected: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/compound-go.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 16);
    assert_eq!(expected.len(), cases.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let result = Program::compile(case["sql"].as_str().unwrap()).and_then(|p| {
            p.execute(
                &sources(case),
                &case["parameters"]
                    .as_object()
                    .map(|m| m.iter().map(|(k, v)| (k.clone(), scalar(v))).collect())
                    .unwrap_or_default(),
                Limits::default(),
                None,
            )
        });
        let actual = match result {
            Ok(result) => json!({"columns": result.columns, "rows": result.rows}),
            Err(error) => {
                let legacy = match error.code.as_str() {
                    "relational.invalid_binding" => "relational.unresolved_binding",
                    "relational.unsupported" => "relational.unsupported_expression",
                    other => other,
                };
                assert_eq!(
                    legacy,
                    expected["error"].as_str().unwrap(),
                    "{}",
                    case["name"]
                );
                continue;
            }
        };
        assert_eq!(
            numeric_json(actual),
            numeric_json(expected["result"].clone()),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn union_keeps_partial_and_original_work_budget_and_validates_tail() {
    use std::sync::atomic::AtomicBool;
    let mut inputs = BTreeMap::from([
        (
            "a".into(),
            Source {
                columns: vec!["ID".into()],
                rows: vec![vec![Scalar::UInt(u64::MAX)]],
                complete: true,
            },
        ),
        (
            "b".into(),
            Source {
                columns: vec!["ID".into()],
                rows: vec![vec![Scalar::Int(1)]],
                complete: false,
            },
        ),
    ]);
    let p = Program::compile("SELECT ID FROM a UNION ALL SELECT ID FROM b ORDER BY ID LIMIT 1")
        .unwrap();
    let result = p
        .execute(&inputs, &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert!(!result.complete);
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    let limits = Limits {
        work: 1,
        ..Limits::default()
    };
    assert_eq!(
        p.execute(&inputs, &BTreeMap::new(), limits, None)
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
    let limits = Limits {
        memory_bytes: 64,
        ..Limits::default()
    };
    assert_eq!(
        p.execute(&inputs, &BTreeMap::new(), limits, None)
            .unwrap_err()
            .code,
        "relational.resource_limit"
    );
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        p.execute(
            &inputs,
            &BTreeMap::new(),
            Limits::default(),
            Some(&cancelled)
        )
        .unwrap_err()
        .exit_code,
        7
    );
    // A top-level LIMIT cannot hide a bad value in a later physical branch.
    inputs.get_mut("b").unwrap().rows[0][0] = Scalar::Float(f64::INFINITY);
    assert_eq!(
        p.execute(&inputs, &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.numeric_range"
    );
    assert_eq!(p.source_tables(), ["a", "b"]);
}

#[test]
fn union_all_diagnostic_preserves_unicode_and_comment_source_locations() {
    for (sql, offset, line, column) in [
        (
            "SELECT ID FROM Map UNION SELECT ID FROM ChrClasses",
            25,
            1,
            26,
        ),
        (
            "SELECT '界面' FROM a\nUNION -- ignored ALL\nSELECT ID FROM b",
            44,
            3,
            1,
        ),
        ("SELECT ID FROM a UNION", 22, 1, 23),
    ] {
        let error = Program::compile(sql)
            .err()
            .expect("non-ALL union is invalid");
        assert_eq!(error.code, "relational.invalid_syntax");
        assert_eq!(
            error.details,
            json!({"location":{"offset":offset,"line":line,"column":column}})
        );
        assert_eq!(
            error.message,
            format!("relational.invalid_syntax at {line}:{column}: expected ALL")
        );
    }
    // Comments and string/quoted identifier content cannot masquerade as a
    // UNION keyword or supply the ALL token after a real UNION.
    assert!(
        Program::compile("SELECT 'UNION' AS text FROM a UNION /* ALL */ ALL SELECT 'x' FROM b")
            .is_ok()
    );
}
