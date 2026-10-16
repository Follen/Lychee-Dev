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
fn frozen_go_aggregate_results_errors_and_exact_numeric_keys() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/aggregates-cases.json"
    ))
    .unwrap();
    let expected: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/aggregates-go.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 28);
    assert_eq!(expected.len(), cases.len());
    for (case, expected) in cases.iter().zip(expected) {
        assert_eq!(case["name"], expected["name"]);
        let result = Program::compile(case["sql"].as_str().unwrap())
            .and_then(|p| p.execute(&sources(case), &BTreeMap::new(), Limits::default(), None));
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
fn groups_aggregates_inherit_partial_work_memory_and_cancellation() {
    use std::sync::atomic::AtomicBool;
    let mut sources = BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["G".into(), "V".into()],
            rows: vec![
                vec![Scalar::String("a".into()), Scalar::Int(1)],
                vec![Scalar::String("b".into()), Scalar::UInt(u64::MAX)],
            ],
            complete: false,
        },
    )]);
    let program = Program::compile("SELECT G,COUNT(*) FROM t GROUP BY G ORDER BY G").unwrap();
    let result = program
        .execute(&sources, &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert!(!result.complete);
    assert_eq!(result.rows.len(), 2);
    for limits in [
        Limits {
            work: 1,
            ..Limits::default()
        },
        Limits {
            memory_bytes: 1,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            program
                .execute(&sources, &BTreeMap::new(), limits, None)
                .unwrap_err()
                .code,
            "relational.resource_limit"
        );
    }
    let cancelled = AtomicBool::new(true);
    assert_eq!(
        program
            .execute(
                &sources,
                &BTreeMap::new(),
                Limits::default(),
                Some(&cancelled)
            )
            .unwrap_err()
            .exit_code,
        7
    );
    sources.get_mut("t").unwrap().rows.push(vec![
        Scalar::String("unused".into()),
        Scalar::Float(f64::NAN),
    ]);
    // Fixed Go group.go:grouping.add evaluates group keys and non-star
    // aggregate arguments only. COUNT(*) must not read the unused V column.
    let result = program
        .execute(&sources, &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert!(!result.complete);
    assert_eq!(
        result.rows,
        vec![
            vec![Scalar::String("a".into()), Scalar::Int(1)],
            vec![Scalar::String("b".into()), Scalar::Int(1)],
            vec![Scalar::String("unused".into()), Scalar::Int(1)],
        ]
    );
    // Reading that same NaN as an aggregate argument or group key must still
    // fail canonicalScalar, rather than exposing a partial result.
    for sql in [
        "SELECT G,COUNT(V) FROM t GROUP BY G ORDER BY G",
        "SELECT V,COUNT(*) FROM t GROUP BY V",
    ] {
        assert_eq!(
            Program::compile(sql)
                .unwrap()
                .execute(&sources, &BTreeMap::new(), Limits::default(), None)
                .unwrap_err()
                .code,
            "relational.numeric_range",
            "{sql}"
        );
    }
}

#[test]
fn many_count_groups_fit_the_original_per_group_allowance() {
    let source = BTreeMap::from([(
        "t".into(),
        Source {
            columns: vec!["ID".into()],
            rows: (0..40000).map(|id| vec![Scalar::Int(id)]).collect(),
            complete: true,
        },
    )]);
    let result = Program::compile("SELECT ID,COUNT(*) FROM t GROUP BY ID")
        .unwrap()
        .execute(&source, &BTreeMap::new(), Limits::default(), None)
        .unwrap();
    assert_eq!(result.rows.len(), 40000);
    assert_eq!(result.rows[39999], vec![Scalar::Int(39999), Scalar::Int(1)]);
}
