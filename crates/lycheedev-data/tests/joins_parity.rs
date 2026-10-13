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
#[test]
fn frozen_go_join_results_errors_order_and_exact_integer_keys() {
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/joins-cases.json"
    ))
    .unwrap();
    let expected: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/joins-go.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 14);
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
        assert_eq!(actual, expected["result"], "{}", case["name"]);
    }
}
#[test]
fn joins_share_work_memory_cancel_and_partial_source_coverage() {
    use std::sync::atomic::AtomicBool;
    let case: Value = serde_json::from_str(include_str!(
        "../../../tests/rust-source-data/joins-cases.json"
    ))
    .unwrap();
    let mut input = sources(&case[0]);
    let program = Program::compile(case[0]["sql"].as_str().unwrap()).unwrap();
    input.get_mut("b").unwrap().complete = false;
    assert!(
        !program
            .execute(&input, &BTreeMap::new(), Limits::default(), None)
            .unwrap()
            .complete
    );
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
                .execute(&input, &BTreeMap::new(), limits, None)
                .unwrap_err()
                .code,
            "relational.resource_limit"
        );
    }
    assert_eq!(
        program
            .execute(
                &input,
                &BTreeMap::new(),
                Limits::default(),
                Some(&AtomicBool::new(true))
            )
            .unwrap_err()
            .code,
        "operation.cancelled"
    );
}

#[test]
fn cross_join_filters_without_retaining_product_and_validates_unused_cells() {
    let rows = (0..200)
        .map(|id| vec![Scalar::Int(id), Scalar::String("x".repeat(32))])
        .collect::<Vec<_>>();
    let mut input = BTreeMap::from([
        (
            "a".into(),
            Source {
                columns: vec!["ID".into(), "Text".into()],
                rows: rows.clone(),
                complete: true,
            },
        ),
        (
            "b".into(),
            Source {
                columns: vec!["ID".into(), "Text".into()],
                rows,
                complete: true,
            },
        ),
    ]);
    let query = Program::compile("SELECT a.ID,b.ID FROM a CROSS JOIN b WHERE FALSE").unwrap();
    let result = query
        .execute(
            &input,
            &BTreeMap::new(),
            Limits {
                memory_bytes: 16384,
                ..Limits::default()
            },
            None,
        )
        .unwrap();
    assert!(result.rows.is_empty());
    // An unselected invalid provider value cannot disappear through filtering.
    input.get_mut("b").unwrap().rows[199][1] = Scalar::Float(f64::INFINITY);
    assert_eq!(
        query
            .execute(&input, &BTreeMap::new(), Limits::default(), None)
            .unwrap_err()
            .code,
        "relational.numeric_range"
    );
}
