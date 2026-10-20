//! Task-native semantic witnesses. Execution status is recorded by the test
//! runner; merely declaring these tests is not validation evidence.
use super::*;
use crate::{
    relational::Scalar,
    resource::{Budget, Cost},
};
use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch, UInt64Array};
use arrow_schema::{Field, Schema};
use datafusion::{execution::session_state::SessionStateBuilder, prelude::SessionConfig};
use serde_json::json;
use std::time::{Duration, Instant};

fn setup() -> (SessionContext, Arc<QueryOwner>) {
    let owner = QueryOwner::new(Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    )));
    let state = SessionStateBuilder::new()
        .with_config(SessionConfig::new().with_target_partitions(1))
        .build();
    (SessionContext::new_with_state(state), owner)
}
fn request(name: &str) -> TableRequest {
    TableRequest {
        name: name.into(),
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    }
}
fn scan(name: &str) -> QueryPlan {
    QueryPlan::Scan {
        table: request(name),
    }
}
fn register(context: &SessionContext, table: &TableRequest, columns: Vec<(&str, ArrayRef)>) {
    let schema = Arc::new(Schema::new(
        columns
            .iter()
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), true))
            .collect::<Vec<_>>(),
    ));
    let batch = RecordBatch::try_new(
        schema,
        columns.into_iter().map(|(_, array)| array).collect(),
    )
    .unwrap();
    context
        .register_batch(TableReference::bare(table_name(table).unwrap()), batch)
        .unwrap();
}
fn decode(value: Value) -> QueryPlan {
    serde_json::from_value(value).unwrap()
}
async fn run(
    context: &SessionContext,
    owner: Arc<QueryOwner>,
    plan: &QueryPlan,
) -> Vec<Vec<Scalar>> {
    let batches = lower(context, plan, owner)
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    batches
        .iter()
        .flat_map(|batch| {
            (0..batch.num_rows()).map(move |row| {
                batch
                    .columns()
                    .iter()
                    .map(|array| {
                        super::super::values::read(ScalarValue::try_from_array(array, row).unwrap())
                            .unwrap()
                    })
                    .collect()
            })
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_scan_filter_project_sort_and_limit_execute_in_datafusion() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![
            ("ID", Arc::new(Int64Array::from(vec![1, 2, 3]))),
            ("Value", Arc::new(Int64Array::from(vec![10, 30, 20]))),
        ],
    );
    let plan = decode(
        json!({"op":"limit","offset":1,"count":1,"input":{"op":"sort","keys":[{"expression":{"kind":"column","name":"answer"},"descending":true,"nulls_first":false}],"input":{"op":"project","columns":[{"name":"answer","expression":{"kind":"binary","left":{"kind":"column","name":"T.Value"},"operator":"add","right":{"kind":"literal","value":1}}}],"input":{"op":"filter","predicate":{"kind":"binary","left":{"kind":"column","name":"ID"},"operator":"greater","right":{"kind":"literal","value":1}},"input":{"op":"scan","table":{"name":"T","mode":"static"}}}}}}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![Scalar::Int(21)]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_lookup_is_membership_and_empty_ids_stays_empty() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![("ID", Arc::new(Int64Array::from(vec![1, 2, 3])))],
    );
    assert_eq!(
        run(
            &context,
            owner.clone(),
            &QueryPlan::Lookup {
                table: request("T"),
                ids: vec![2, 2, 99]
            }
        )
        .await,
        vec![vec![Scalar::Int(2)]]
    );
    assert!(
        run(
            &context,
            owner,
            &QueryPlan::Lookup {
                table: request("T"),
                ids: vec![]
            }
        )
        .await
        .is_empty()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_exact_integer_arithmetic_crosses_signed_boundary() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![("value", Arc::new(Int64Array::from(vec![i64::MAX])))],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"answer","expression":{"kind":"binary","left":{"kind":"column","name":"value"},"operator":"add","right":{"kind":"literal","value":1}}}]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![Scalar::UInt(1u64 << 63)]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_exact_arithmetic_rejects_unsigned_overflow() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![("value", Arc::new(UInt64Array::from(vec![u64::MAX])))],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"answer","expression":{"kind":"binary","left":{"kind":"column","name":"value"},"operator":"add","right":{"kind":"literal","value":1}}}]}),
    );
    let error = lower(&context, &plan, owner)
        .await
        .unwrap()
        .collect()
        .await
        .unwrap_err();
    assert_eq!(super::super::error(error).code, "relational.numeric_range");
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_exact_mixed_comparison_and_membership_do_not_round_integer() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![(
            "value",
            Arc::new(UInt64Array::from(vec![9_007_199_254_740_993])),
        )],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[
            {"name":"equal","expression":{"kind":"binary","left":{"kind":"column","name":"value"},"operator":"equal","right":{"kind":"literal","value":9_007_199_254_740_992.0}}},
            {"name":"member","expression":{"kind":"in","value":{"kind":"column","name":"value"},"values":[9_007_199_254_740_992.0]}},
            {"name":"precise","expression":{"kind":"in","value":{"kind":"column","name":"value"},"values":[9_007_199_254_740_992.0,9_007_199_254_740_993u64]}}
        ]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![
            Scalar::Bool(false),
            Scalar::Bool(false),
            Scalar::Bool(true)
        ]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_membership_not_and_null_semantics_are_preserved() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![(
            "value",
            Arc::new(Int64Array::from(vec![None, Some(2), Some(3)])),
        )],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[
            {"name":"member","expression":{"kind":"in","value":{"kind":"column","name":"value"},"values":[2,null]}},
            {"name":"absent","expression":{"kind":"not","value":{"kind":"is_null","value":{"kind":"column","name":"value"}}}},
            {"name":"empty","expression":{"kind":"in","value":{"kind":"column","name":"value"},"values":[]}}
        ]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![
            vec![Scalar::Null, Scalar::Bool(false), Scalar::Bool(false)],
            vec![Scalar::Bool(true), Scalar::Bool(true), Scalar::Bool(false)],
            vec![Scalar::Null, Scalar::Bool(true), Scalar::Bool(false)]
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_all_join_kinds_and_self_join_have_real_semantics() {
    let (context, owner) = setup();
    register(
        &context,
        &request("L"),
        vec![("ID", Arc::new(Int64Array::from(vec![1, 2])))],
    );
    register(
        &context,
        &request("R"),
        vec![("ID", Arc::new(Int64Array::from(vec![2, 3])))],
    );
    for (kind, expected_rows, expected_columns) in [
        (JoinKind::Inner, 1, 2),
        (JoinKind::Left, 2, 2),
        (JoinKind::Full, 3, 2),
        (JoinKind::Semi, 1, 1),
        (JoinKind::Anti, 1, 1),
    ] {
        let plan = QueryPlan::Join {
            left: Box::new(scan("L")),
            right: Box::new(scan("R")),
            kind,
            on: vec![super::super::api::JoinKey {
                left: "ID".into(),
                right: "ID".into(),
            }],
        };
        let rows = run(&context, owner.clone(), &plan).await;
        assert_eq!(rows.len(), expected_rows);
        assert!(rows.iter().all(|row| row.len() == expected_columns));
    }
    let plan = decode(
        json!({"op":"project","input":{"op":"join","kind":"inner","left":{"op":"scan","table":{"name":"L","mode":"static"}},"right":{"op":"scan","table":{"name":"L","mode":"static"}},"on":[{"left":"L.ID","right":"L.ID"}]},"columns":[{"name":"first","expression":{"kind":"column","name":"left.ID"}},{"name":"second","expression":{"kind":"column","name":"right.ID"}}]}),
    );
    let rows = run(&context, owner, &plan).await;
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row[0] == row[1]));
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_mixed_numeric_join_does_not_create_rounded_match() {
    let (context, owner) = setup();
    register(
        &context,
        &request("L"),
        vec![(
            "ID",
            Arc::new(UInt64Array::from(vec![9_007_199_254_740_993])),
        )],
    );
    register(
        &context,
        &request("R"),
        vec![(
            "ID",
            Arc::new(Float64Array::from(vec![9_007_199_254_740_992.0])),
        )],
    );
    let plan = QueryPlan::Join {
        left: Box::new(scan("L")),
        right: Box::new(scan("R")),
        kind: JoinKind::Inner,
        on: vec![super::super::api::JoinKey {
            left: "ID".into(),
            right: "ID".into(),
        }],
    };
    assert!(run(&context, owner, &plan).await.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_aggregate_uses_exact_sum_average_and_native_count_min_max() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![(
            "value",
            Arc::new(Int64Array::from(vec![
                Some(9_007_199_254_740_993),
                Some(-9_007_199_254_740_992),
                None,
            ])),
        )],
    );
    let plan = decode(
        json!({"op":"aggregate","input":{"op":"scan","table":{"name":"T","mode":"static"}},"groups":[],"metrics":[
            {"name":"sum","function":"sum","expression":{"kind":"column","name":"value"}},
            {"name":"avg","function":"average","expression":{"kind":"column","name":"value"}},
            {"name":"all","function":"count"},{"name":"nonnull","function":"count","expression":{"kind":"column","name":"value"}},
            {"name":"min","function":"minimum","expression":{"kind":"column","name":"value"}},
            {"name":"max","function":"maximum","expression":{"kind":"column","name":"value"}}
        ]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![
            Scalar::Int(1),
            Scalar::Float(0.5),
            Scalar::Int(3),
            Scalar::Int(2),
            Scalar::Int(-9_007_199_254_740_992),
            Scalar::Int(9_007_199_254_740_993)
        ]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn task_plan_static_and_effective_sources_never_alias_each_other() {
    let (context, owner) = setup();
    let static_request = request("T");
    let effective_request = TableRequest {
        mode: SourceMode::Effective,
        hotfix_captures: vec!["CAP-example".into()],
        ..request("T")
    };
    register(
        &context,
        &static_request,
        vec![("ID", Arc::new(Int64Array::from(vec![1])))],
    );
    register(
        &context,
        &effective_request,
        vec![("ID", Arc::new(Int64Array::from(vec![2])))],
    );
    assert_eq!(
        run(
            &context,
            owner.clone(),
            &QueryPlan::Scan {
                table: static_request
            }
        )
        .await,
        vec![vec![Scalar::Int(1)]]
    );
    assert_eq!(
        run(
            &context,
            owner,
            &QueryPlan::Scan {
                table: effective_request
            }
        )
        .await,
        vec![vec![Scalar::Int(2)]]
    );
}

#[test]
fn task_plan_preflight_rejects_depth_size_shapes_and_ambiguous_bindings() {
    let mut plan = scan("T");
    for _ in 0..MAX_DEPTH {
        plan = QueryPlan::Limit {
            input: Box::new(plan),
            offset: 0,
            count: 1,
        };
    }
    assert!(tables(&plan).is_err());
    assert!(
        tables(&QueryPlan::Lookup {
            table: request("T"),
            ids: vec![0; MAX_VALUES + 1]
        })
        .is_err()
    );
    assert!(tables(&decode(json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"a","expression":{"kind":"literal","value":{}}}]}))).is_err());
    assert!(tables(&decode(json!({"op":"aggregate","input":{"op":"scan","table":{"name":"T","mode":"static"}},"groups":[],"metrics":[{"name":"total","function":"sum"}]}))).is_err());
    assert!(tables(&decode(json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"same","expression":{"kind":"literal","value":1}},{"name":"same","expression":{"kind":"literal","value":2}}]}))).is_err());
    let conflicting = decode(
        json!({"op":"join","kind":"inner","left":{"op":"scan","table":{"name":"T","mode":"effective","hotfix_captures":["CAP-a"]}},"right":{"op":"scan","table":{"name":"T","mode":"effective","hotfix_captures":["CAP-b"]}},"on":[{"left":"ID","right":"ID"}]}),
    );
    assert!(tables(&conflicting).is_err());
    assert!(
        tables(&QueryPlan::Limit {
            input: Box::new(scan("T")),
            offset: usize::MAX,
            count: 1
        })
        .is_err()
    );
}

#[test]
fn task_plan_literals_reject_integer_rounding_and_nonfinite_float() {
    for text in ["18446744073709551616", "-9223372036854775809", "1e999"] {
        let literal: Value = serde_json::from_str(text).unwrap();
        assert!(scalar(&literal).is_err(), "{text}");
    }
    assert_eq!(
        scalar(&json!(u64::MAX)).unwrap(),
        ScalarValue::UInt64(Some(u64::MAX))
    );
}

#[test]
fn task_plan_computed_membership_expansion_is_bounded_before_cloning() {
    let mut expr = Expression::Column { name: "ID".into() };
    for _ in 0..5 {
        expr = Expression::In {
            value: Box::new(expr),
            values: vec![json!(1); 32],
        };
    }
    let plan = QueryPlan::Filter {
        input: Box::new(scan("T")),
        predicate: expr,
    };
    assert!(tables(&plan).is_err());
}

#[test]
fn task_schema_has_exact_wire_tags_and_unknown_fields_are_rejected() {
    let schema = super::super::api::query_schema();
    assert_eq!(
        schema["$defs"]["QueryPlan"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        8
    );
    assert_eq!(
        schema["$defs"]["Expression"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        schema["$defs"]["SortKey"]["required"],
        json!(["expression", "descending", "nulls_first"])
    );
    assert_eq!(
        schema["$defs"]["TableRequest"]["properties"]["hotfix_captures"]["default"],
        json!([])
    );
    let value = json!({"kind":"plan","plan":{"op":"scan","table":{"name":"T","mode":"static"}}});
    let request: super::super::api::QueryRequest = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(request).unwrap(), value);
    assert!(serde_json::from_value::<super::super::api::QueryRequest>(json!({"kind":"plan","plan":{"op":"scan","table":{"name":"T","mode":"static","typo":true}}})).is_err());
    assert!(
        table_name(&TableRequest {
            name: "catalog.table".into(),
            ..self::request("T")
        })
        .is_err()
    );
    assert!(column("table.ID + 1").is_ok()); // A literal field name, never SQL.
}

#[test]
fn sql_is_not_an_advertised_or_deserializable_task_capability() {
    assert!(
        serde_json::from_value::<super::super::api::QueryRequest>(
            serde_json::json!({"kind":"sql","sql":"SELECT 1","tables":[]})
        )
        .is_err()
    );
    let schema = super::super::api::query_schema();
    assert_eq!(schema["oneOf"].as_array().unwrap().len(), 1);
    assert_eq!(schema["oneOf"][0]["properties"]["kind"]["const"], "plan");
}

#[tokio::test(flavor = "current_thread")]
async fn mixed_primitive_domains_do_not_use_native_coercion() {
    for (left, right, operator) in [
        (json!("1"), json!(1), "equal"),
        (json!(true), json!(1), "equal"),
        (json!("2"), json!(3), "add"),
    ] {
        let (context, owner) = setup();
        register(
            &context,
            &request("T"),
            vec![("ID", Arc::new(UInt64Array::from(vec![1])))],
        );
        let plan = decode(
            json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"result","expression":{"kind":"binary","left":{"kind":"literal","value":left},"operator":operator,"right":{"kind":"literal","value":right}}}]}),
        );
        let result = match lower(&context, &plan, owner).await {
            Ok(frame) => frame.collect().await.map(|_| ()),
            Err(error) => Err(error),
        };
        assert_eq!(
            super::super::error(result.unwrap_err()).code,
            "relational.type_mismatch"
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn mixed_domain_null_still_short_circuits_to_null() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![("ID", Arc::new(UInt64Array::from(vec![1])))],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"result","expression":{"kind":"binary","left":{"kind":"literal","value":"1"},"operator":"equal","right":{"kind":"literal","value":null}}}]}),
    );
    assert_eq!(run(&context, owner, &plan).await, vec![vec![Scalar::Null]]);
}
