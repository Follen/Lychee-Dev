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
fn frozen_go_cte_scopes_dependencies_and_exact_rows() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/cte-cases.json"
    ))
    .unwrap();
    let expected: Vec<Value> =
        serde_json::from_str(include_str!("../../../tests/rust-source-data/cte-go.json")).unwrap();
    assert_eq!(cases.len(), 14);
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
fn ctes_share_original_budgets_borrow_physical_rows_and_propagate_partial_inputs() {
    use std::sync::atomic::AtomicBool;
    let input = BTreeMap::from([(
        "a".into(),
        Source {
            columns: vec!["ID".into(), "unused".into()],
            rows: vec![vec![Scalar::Int(1), Scalar::String("界".repeat(300_000))]],
            complete: false,
        },
    )]);
    let plan = Program::compile(
        "WITH x AS (SELECT COUNT(*) AS n FROM a),y AS (SELECT n FROM x) SELECT n FROM y",
    )
    .unwrap();
    assert_eq!(plan.source_tables(), vec!["a"]);
    let limits = Limits {
        memory_bytes: 16 << 10,
        ..Limits::default()
    };
    let result = plan
        .execute(&input, &BTreeMap::new(), limits, None)
        .unwrap();
    assert_eq!(result.rows, vec![vec![Scalar::Int(1)]]);
    assert!(!result.complete);
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
            plan.execute(&input, &BTreeMap::new(), limits, None)
                .unwrap_err()
                .code,
            "relational.resource_limit"
        );
    }
    assert_eq!(
        plan.execute(
            &input,
            &BTreeMap::new(),
            Limits::default(),
            Some(&AtomicBool::new(true))
        )
        .unwrap_err()
        .exit_code,
        7
    );
    assert!(
        Program::compile("WITH a AS (SELECT ID FROM a) SELECT ID FROM a")
            .unwrap()
            .source_tables()
            .is_empty()
    );
    assert_eq!(
        Program::compile("WITH a AS (SELECT ID FROM static.a) SELECT ID FROM a")
            .unwrap()
            .source_tables(),
        vec!["static.a"]
    );
    let mut bad = input;
    bad.get_mut("a").unwrap().rows[0][1] = Scalar::Float(f64::NAN);
    assert_eq!(
        plan.execute(&bad, &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.numeric_range"
    );
}
