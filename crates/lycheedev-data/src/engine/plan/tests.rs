//! Task-native semantic witnesses. Execution status is recorded by the test
//! runner; merely declaring these tests is not validation evidence.
use super::*;
use crate::{
    query_value::Scalar,
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
        11
    );
    assert_eq!(
        schema["$defs"]["Expression"]["oneOf"]
            .as_array()
            .unwrap()
            .len(),
        18
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

// Fixed expected scalars/errors; no second executor computes these values.
fn expected_scalar(case: &str) -> lycheedev_core::Result<Scalar> {
    let type_error = || crate::query_value::type_error();
    let numeric = || crate::query_value::numeric_error();
    Ok(match case {
        "CAST(18446744073709551615 AS TEXT)" => Scalar::String(u64::MAX.to_string()),
        "CAST('-2.75' AS INTEGER)" => Scalar::Int(-2),
        "CAST('9223372036854775807' AS BIGINT)" => Scalar::Int(i64::MAX),
        "CAST('-9223372036854775808' AS BIGINT)" => Scalar::Int(i64::MIN),
        "CAST(1e-5 AS TEXT)" => Scalar::String("1e-05".into()),
        "CAST(-0.0 AS TEXT)" => Scalar::String("-0".into()),
        "CAST(NULL AS INTEGER)" | "NULL LIKE 1" => Scalar::Null,
        "CAST('true' AS BOOLEAN)" => Scalar::Bool(true),
        "CAST(2 AS BOOL)" | "CAST(TRUE AS INTEGER)" | "CAST(' 1' AS FLOAT)" | "1 LIKE '%'" => {
            return Err(type_error());
        }
        "CAST('1e999' AS FLOAT)" | "CAST(18446744073709551615 AS BIGINT)" => return Err(numeric()),
        "'猫犬' LIKE '_犬'" => Scalar::Bool(true),
        "'abc' LIKE 'a\\%'" => Scalar::Bool(false),
        "UPPER('ßσ猫')" => Scalar::String("ßΣ猫".into()),
        "LOWER('İΣ犬')" => Scalar::String("iσ犬".into()),
        "LENGTH('猫é')" => Scalar::Int(2),
        "'猫' || '犬'" => Scalar::String("猫犬".into()),
        "BIT_AND(-1, 7)" => Scalar::UInt(7),
        "HAS_FLAG(-1, 7)" => Scalar::Bool(true),
        "BIT_AND(NULL, CAST('bad' AS INTEGER))" => Scalar::Null,
        "BIT_AND('bad', NULL)" => return Err(type_error()),
        _ => panic!("missing fixed expectation: {case}"),
    })
}
async fn new_scalar(expression: Value) -> lycheedev_core::Result<Scalar> {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![("ID", Arc::new(UInt64Array::from(vec![1])))],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"result","expression":expression}]}),
    );
    let frame = lower(&context, &plan, owner)
        .await
        .map_err(super::super::error)?;
    let batches = frame.collect().await.map_err(super::super::error)?;
    super::super::values::read(
        ScalarValue::try_from_array(batches[0].column(0), 0).map_err(super::super::error)?,
    )
    .map_err(super::super::error)
}
fn lit(value: Value) -> Value {
    json!({"kind":"literal","value":value})
}
#[tokio::test(flavor = "current_thread")]
async fn casts_preserve_fixed_independent_expected_scalars() {
    for (sql, input, target) in [
        (
            "CAST(18446744073709551615 AS TEXT)",
            json!(u64::MAX),
            "string",
        ),
        ("CAST('-2.75' AS INTEGER)", json!("-2.75"), "integer"),
        (
            "CAST('9223372036854775807' AS BIGINT)",
            json!("9223372036854775807"),
            "integer",
        ),
        (
            "CAST('-9223372036854775808' AS BIGINT)",
            json!("-9223372036854775808"),
            "integer",
        ),
        ("CAST(1e-5 AS TEXT)", json!(0.00001), "string"),
        ("CAST(-0.0 AS TEXT)", json!(-0.0), "string"),
        ("CAST(NULL AS INTEGER)", Value::Null, "integer"),
        ("CAST('true' AS BOOLEAN)", json!("true"), "boolean"),
        ("CAST(2 AS BOOL)", json!(2), "boolean"),
        ("CAST(TRUE AS INTEGER)", json!(true), "integer"),
        ("CAST(' 1' AS FLOAT)", json!(" 1"), "float"),
        ("CAST('1e999' AS FLOAT)", json!("1e999"), "float"),
        (
            "CAST(18446744073709551615 AS BIGINT)",
            json!(u64::MAX),
            "integer",
        ),
    ] {
        let actual = new_scalar(json!({"kind":"cast","value":lit(input),"target":target})).await;
        match expected_scalar(sql) {
            Ok(expected) => assert_eq!(actual.unwrap(), expected, "{sql}"),
            Err(expected) => assert_eq!(actual.unwrap_err().code, expected.code, "{sql}"),
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn unicode_like_case_mapping_length_concat_match_fixed_oracle() {
    for (sql, expression) in [
        (
            "'猫犬' LIKE '_犬'",
            json!({"kind":"like","value":lit(json!("猫犬")),"pattern":lit(json!("_犬")),"negated":false}),
        ),
        (
            "'abc' LIKE 'a\\%'",
            json!({"kind":"like","value":lit(json!("abc")),"pattern":lit(json!("a\\%")),"negated":false}),
        ),
        (
            "NULL LIKE 1",
            json!({"kind":"like","value":lit(Value::Null),"pattern":lit(json!(1)),"negated":false}),
        ),
        (
            "1 LIKE '%'",
            json!({"kind":"like","value":lit(json!(1)),"pattern":lit(json!("%")),"negated":false}),
        ),
        (
            "UPPER('ßσ猫')",
            json!({"kind":"call","function":"upper","arguments":[lit(json!("ßσ猫"))]}),
        ),
        (
            "LOWER('İΣ犬')",
            json!({"kind":"call","function":"lower","arguments":[lit(json!("İΣ犬"))]}),
        ),
        (
            "LENGTH('猫é')",
            json!({"kind":"call","function":"length","arguments":[lit(json!("猫é"))]}),
        ),
        (
            "'猫' || '犬'",
            json!({"kind":"call","function":"concat","arguments":[lit(json!("猫")),lit(json!("犬"))]}),
        ),
    ] {
        let actual = new_scalar(expression).await;
        match expected_scalar(sql) {
            Ok(expected) => assert_eq!(actual.unwrap(), expected, "{sql}"),
            Err(expected) => assert_eq!(actual.unwrap_err().code, expected.code, "{sql}"),
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn conditional_selection_is_lazy_and_does_not_round_wide_integers() {
    let failing = json!({"kind":"cast","target":"integer","value":lit(json!("bad"))});
    let case = json!({"kind":"case","branches":[{"when":lit(json!(false)),"then":failing}],"otherwise":lit(json!(u64::MAX))});
    assert_eq!(new_scalar(case).await.unwrap(), Scalar::UInt(u64::MAX));
    let coalesce = json!({"kind":"coalesce","values":[lit(json!(42)),json!({"kind":"cast","target":"integer","value":lit(json!("bad"))})]});
    assert_eq!(new_scalar(coalesce).await.unwrap(), Scalar::Int(42));
    let null_if = json!({"kind":"null_if","left":lit(json!(9007199254740993u64)),"right":lit(json!(9007199254740992.0))});
    assert_eq!(
        new_scalar(null_if).await.unwrap(),
        Scalar::Int(9007199254740993)
    );
    let between = json!({"kind":"between","value":lit(json!(9007199254740993u64)),"low":lit(json!(9007199254740992.0)),"high":lit(json!(9007199254740994u64)),"negated":false});
    assert_eq!(new_scalar(between).await.unwrap(), Scalar::Bool(true));
}
#[tokio::test(flavor = "current_thread")]
async fn unsafe_heterogeneous_conditional_type_is_rejected_not_rounded() {
    let expression = json!({"kind":"case","branches":[{"when":{"kind":"binary","operator":"equal","left":{"kind":"column","name":"ID"},"right":lit(json!(1))},"then":lit(json!(9007199254740993u64))}],"otherwise":lit(json!(1.0))});
    assert_eq!(
        new_scalar(expression).await.unwrap_err().code,
        "relational.unsupported"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn nested_array_index_preserves_wide_integer_and_null_and_checks_bounds() {
    use arrow_array::{FixedSizeListArray, types::UInt64Type};
    let (context, owner) = setup();
    let list = FixedSizeListArray::from_iter_primitive::<UInt64Type, _, _>(
        vec![Some(vec![Some(7), Some(u64::MAX)]), None],
        2,
    );
    register(&context, &request("T"), vec![("A", Arc::new(list))]);
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"value","expression":{"kind":"index","value":{"kind":"column","name":"A"},"index":1}}]}),
    );
    assert_eq!(
        run(&context, owner.clone(), &plan).await,
        vec![vec![Scalar::UInt(u64::MAX)], vec![Scalar::Null]]
    );
    let invalid = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"value","expression":{"kind":"index","value":{"kind":"column","name":"A"},"index":2}}]}),
    );
    assert!(
        lower(&context, &invalid, owner)
            .await
            .unwrap()
            .collect()
            .await
            .is_err()
    );
}
#[test]
fn extended_expression_wire_and_schema_agree() {
    let example = json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"n","expression":{"kind":"call","function":"length","arguments":[{"kind":"literal","value":"猫"}]}}]});
    let plan: QueryPlan = serde_json::from_value(example.clone()).unwrap();
    assert_eq!(serde_json::to_value(plan).unwrap(), example);
    let schema = super::super::api::query_schema();
    let kinds = schema["$defs"]["Expression"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["properties"]["kind"]["const"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    for name in [
        "cast", "like", "between", "case", "coalesce", "null_if", "call", "index",
    ] {
        assert!(kinds.contains(name));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn upper_rejects_expanded_utf8_output_above_original_limit() {
    // U+023F maps to U+2C7E: the input fits, but the mapped output does not.
    let expression =
        json!({"kind":"call","function":"upper","arguments":[lit(json!("ȿ".repeat(400_000)))]});
    assert!(new_scalar(expression).await.is_err());
    let expression =
        json!({"kind":"call","function":"upper","arguments":[lit(json!("ȿ".repeat(100)))]});
    assert_eq!(
        new_scalar(expression).await.unwrap(),
        Scalar::String("Ȿ".repeat(100))
    );
}

#[test]
fn nested_bit_left_subtrees_are_charged_for_actual_lowering_expansion() {
    let mut expression = lit(json!(1));
    for _ in 0..20 {
        expression =
            json!({"kind":"call","function":"bit_and","arguments":[expression,lit(json!(1))]});
    }
    let expression: Expression = serde_json::from_value(expression).unwrap();
    assert!(expression_expansion(&expression).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn between_evaluates_high_error_even_when_low_comparison_is_false() {
    let expression = json!({"kind":"between","value":lit(json!(0)),"low":lit(json!(1)),"high":{"kind":"cast","target":"integer","value":lit(json!("bad"))},"negated":false});
    assert_eq!(
        new_scalar(expression).await.unwrap_err().code,
        "relational.type_mismatch"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn bit_functions_preserve_left_null_error_order_and_exact_signed_bits() {
    for (sql, expression) in [
        (
            "BIT_AND(-1, 7)",
            json!({"kind":"call","function":"bit_and","arguments":[lit(json!(-1)),lit(json!(7))]}),
        ),
        (
            "HAS_FLAG(-1, 7)",
            json!({"kind":"call","function":"has_flag","arguments":[lit(json!(-1)),lit(json!(7))]}),
        ),
        (
            "BIT_AND(NULL, CAST('bad' AS INTEGER))",
            json!({"kind":"call","function":"bit_and","arguments":[lit(Value::Null),{"kind":"cast","target":"integer","value":lit(json!("bad"))}]}),
        ),
        (
            "BIT_AND('bad', NULL)",
            json!({"kind":"call","function":"bit_and","arguments":[lit(json!("bad")),lit(Value::Null)]}),
        ),
    ] {
        let actual = new_scalar(expression).await;
        match expected_scalar(sql) {
            Ok(expected) => assert_eq!(actual.unwrap(), expected, "{sql}"),
            Err(expected) => assert_eq!(actual.unwrap_err().code, expected.code, "{sql}"),
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn eager_boolean_operators_keep_right_errors_after_false_or_true_left() {
    for (operator, left) in [("and", false), ("or", true)] {
        let (context, owner) = setup();
        register(
            &context,
            &request("T"),
            vec![("ID", Arc::new(UInt64Array::from(vec![1])))],
        );
        let right = json!({"kind":"binary","operator":"equal","left":{"kind":"literal","value":"1"},"right":{"kind":"literal","value":1}});
        let plan = decode(
            json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"result","expression":{"kind":"binary","operator":operator,"left":{"kind":"literal","value":left},"right":right}}]}),
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
async fn eager_boolean_keeps_errors_inside_otherwise_native_column_equalities() {
    for (operator, left_column) in [("and", "R"), ("or", "L")] {
        let (context, owner) = setup();
        register(
            &context,
            &request("T"),
            vec![
                ("L", Arc::new(Int64Array::from(vec![1]))),
                ("R", Arc::new(Int64Array::from(vec![2]))),
            ],
        );
        let plan = decode(json!({
            "op":"project", "input":{"op":"scan","table":{"name":"T","mode":"static"}},
            "columns":[{"name":"result","expression":{"kind":"binary","operator":operator,
                "left":{"kind":"binary","operator":"equal",
                    "left":{"kind":"column","name":"L"},
                    "right":{"kind":"column","name":left_column}},
                "right":{"kind":"binary","operator":"equal",
                    "left":{"kind":"column","name":"L"},
                    "right":{"kind":"cast","target":"integer","value":lit(json!("bad"))}}}}]
        }));
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
async fn eager_boolean_three_valued_truth_table_matches_oracle() {
    for and in [true, false] {
        for left in [Some(false), Some(true), None] {
            for right in [Some(false), Some(true), None] {
                let (context, owner) = setup();
                register(
                    &context,
                    &request("T"),
                    vec![("ID", Arc::new(UInt64Array::from(vec![1])))],
                );
                let expected = match (and, left, right) {
                    (true, Some(false), _) | (true, _, Some(false)) => Scalar::Bool(false),
                    (false, Some(true), _) | (false, _, Some(true)) => Scalar::Bool(true),
                    (_, Some(a), Some(b)) => Scalar::Bool(if and { a && b } else { a || b }),
                    _ => Scalar::Null,
                };
                let plan = decode(
                    json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"result","expression":{"kind":"binary","operator":if and{"and"}else{"or"},"left":{"kind":"literal","value":left},"right":{"kind":"literal","value":right}}}]}),
                );
                assert_eq!(run(&context, owner, &plan).await, vec![vec![expected]]);
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn out_of_bounds_fixed_index_is_null_for_null_array_and_lazy_when_unselected() {
    use arrow_array::{FixedSizeListArray, types::UInt64Type};
    let (context, owner) = setup();
    let list = FixedSizeListArray::from_iter_primitive::<UInt64Type, _, _>(
        vec![None::<Vec<Option<u64>>>],
        2,
    );
    register(&context, &request("T"), vec![("A", Arc::new(list))]);
    let expression = json!({"kind":"index","value":{"kind":"column","name":"A"},"index":2});
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"value","expression":expression}]}),
    );
    assert_eq!(run(&context, owner, &plan).await, vec![vec![Scalar::Null]]);

    let (context, owner) = setup();
    let list = FixedSizeListArray::from_iter_primitive::<UInt64Type, _, _>(
        vec![Some(vec![Some(1), Some(2)])],
        2,
    );
    register(&context, &request("T"), vec![("A", Arc::new(list))]);
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"value","expression":{"kind":"case","branches":[{"when":lit(json!(false)),"then":expression}],"otherwise":lit(json!(7))}}]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![Scalar::Int(7)]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn between_finishes_argument_expressions_before_comparison_type_errors() {
    let high = json!({"kind":"binary","left":lit(json!(1e308)),"operator":"multiply","right":lit(json!(1e308))});
    let expression = json!({"kind":"between","value":lit(json!("bad")),"low":lit(json!(1)),"high":high,"negated":false});
    assert_eq!(
        new_scalar(expression).await.unwrap_err().code,
        "relational.numeric_range"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn typed_in_does_not_hide_later_incompatible_value_after_match() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![("ID", Arc::new(UInt64Array::from(vec![1])))],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"result","expression":{"kind":"in","value":{"kind":"column","name":"ID"},"values":[1,"bad"]}}]}),
    );
    let error = lower(&context, &plan, owner)
        .await
        .unwrap()
        .collect()
        .await
        .unwrap_err();
    assert_eq!(super::super::error(error).code, "relational.type_mismatch");
}

#[tokio::test(flavor = "current_thread")]
async fn empty_typed_in_still_evaluates_failing_input() {
    let (context, owner) = setup();
    register(
        &context,
        &request("T"),
        vec![("ID", Arc::new(UInt64Array::from(vec![1])))],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"T","mode":"static"}},"columns":[{"name":"result","expression":{"kind":"in","value":{"kind":"binary","left":{"kind":"column","name":"ID"},"operator":"add","right":{"kind":"literal","value":"bad"}},"values":[]}}]}),
    );
    let error = lower(&context, &plan, owner)
        .await
        .unwrap()
        .collect()
        .await
        .unwrap_err();
    assert_eq!(super::super::error(error).code, "relational.type_mismatch");
}

#[test]
fn lookup_requirements_union_ids_and_full_scan_dominates_without_alias_confusion() {
    let (_, owner) = setup();
    let lookup = |ids: Vec<u32>| QueryPlan::Lookup {
        table: request("T"),
        ids,
    };
    let join = |left, right| QueryPlan::Join {
        left: Box::new(left),
        right: Box::new(right),
        kind: JoinKind::Inner,
        on: vec![super::super::api::JoinKey {
            left: "ID".into(),
            right: "ID".into(),
        }],
    };
    let plan = join(lookup(vec![3, 1, 3]), lookup(vec![4, 2]));
    let required = requirements(&plan, &owner).unwrap();
    assert_eq!(required.len(), 1);
    assert_eq!(required[0].ids.as_deref(), Some([1, 2, 3, 4].as_slice()));
    let plan = join(plan, scan("T"));
    let required = requirements(&plan, &owner).unwrap();
    assert_eq!(required.len(), 1);
    assert!(required[0].ids.is_none());
}

#[test]
fn conflicting_effective_histories_are_rejected_before_source_preparation() {
    let (_, owner) = setup();
    let table = |id: &str| TableRequest {
        name: "T".into(),
        mode: SourceMode::Effective,
        hotfix_captures: vec![id.into()],
    };
    let plan = QueryPlan::Join {
        left: Box::new(QueryPlan::Lookup {
            table: table("CAP-one"),
            ids: vec![1],
        }),
        right: Box::new(QueryPlan::Lookup {
            table: table("CAP-two"),
            ids: vec![1],
        }),
        kind: JoinKind::Inner,
        on: vec![super::super::api::JoinKey {
            left: "ID".into(),
            right: "ID".into(),
        }],
    };
    assert!(requirements(&plan, &owner).is_err());
}

// Recovery-authored witnesses; none has been executed or compiler-qualified.
#[tokio::test(flavor = "current_thread")]
async fn recovered_union_preserves_branch_bags_labels_and_exact_u64() {
    let (context, owner) = setup();
    register(
        &context,
        &request("U"),
        vec![(
            "high",
            Arc::new(UInt64Array::from(vec![u64::MAX, u64::MAX - 1])),
        )],
    );
    register(
        &context,
        &request("I"),
        vec![("low", Arc::new(Int64Array::from(vec![-1, 2])))],
    );
    let plan = decode(
        json!({"op":"union_all","inputs":[{"op":"scan","table":{"name":"U","mode":"static"}},{"op":"scan","table":{"name":"I","mode":"static"}},{"op":"scan","table":{"name":"U","mode":"static"}}]}),
    );
    let prepared = prepare(&context, &plan, owner.clone()).await.unwrap();
    assert_eq!(
        prepared
            .explain_frame(&context)
            .unwrap()
            .schema()
            .field(0)
            .name(),
        "high"
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![
            vec![Scalar::UInt(u64::MAX)],
            vec![Scalar::UInt(u64::MAX - 1)],
            vec![Scalar::Int(-1)],
            vec![Scalar::Int(2)],
            vec![Scalar::UInt(u64::MAX)],
            vec![Scalar::UInt(u64::MAX - 1)]
        ]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_membership_keeps_null_empty_and_duplicate_truth_tables() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![(
            "ID",
            Arc::new(Int64Array::from(vec![Some(1), Some(2), None, Some(1)])),
        )],
    );
    register(
        &context,
        &request("B"),
        vec![(
            "ID",
            Arc::new(Int64Array::from(vec![Some(1), None, Some(1)])),
        )],
    );
    for (empty, negated, expected) in [
        (
            false,
            false,
            vec![
                Scalar::Bool(true),
                Scalar::Null,
                Scalar::Null,
                Scalar::Bool(true),
            ],
        ),
        (
            false,
            true,
            vec![
                Scalar::Bool(false),
                Scalar::Null,
                Scalar::Null,
                Scalar::Bool(false),
            ],
        ),
        (true, false, vec![Scalar::Bool(false); 4]),
        (true, true, vec![Scalar::Bool(true); 4]),
    ] {
        let rhs = if empty {
            json!({"op":"filter","input":{"op":"scan","table":{"name":"B","mode":"static"}},"predicate":lit(json!(false))})
        } else {
            json!({"op":"scan","table":{"name":"B","mode":"static"}})
        };
        let plan = decode(
            json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"hit","expression":{"kind":"in_query","value":{"kind":"column","name":"ID"},"input":rhs,"negated":negated}}]}),
        );
        assert_eq!(
            run(&context, owner.clone(), &plan).await,
            expected.into_iter().map(|v| vec![v]).collect::<Vec<_>>()
        );
    }
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_membership_does_not_round_u64_into_float_matches() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![(
            "ID",
            Arc::new(UInt64Array::from(vec![u64::MAX, 9_007_199_254_740_993])),
        )],
    );
    register(
        &context,
        &request("B"),
        vec![(
            "ID",
            Arc::new(Float64Array::from(vec![
                18_446_744_073_709_551_616.0,
                9_007_199_254_740_992.0,
            ])),
        )],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"hit","expression":{"kind":"in_query","value":{"kind":"column","name":"ID"},"input":{"op":"scan","table":{"name":"B","mode":"static"}}}}]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![Scalar::Bool(false)], vec![Scalar::Bool(false)]]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_scalar_zero_one_many_and_unreachable_width() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![("ID", Arc::new(Int64Array::from(vec![1])))],
    );
    register(
        &context,
        &request("B"),
        vec![("ID", Arc::new(Int64Array::from(vec![7, 8])))],
    );
    let project = |rhs: Value| {
        decode(
            json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"v","expression":{"kind":"scalar_query","input":rhs}}]}),
        )
    };
    for (count, expected) in [(0, Scalar::Null), (1, Scalar::Int(7))] {
        let plan = project(
            json!({"op":"limit","input":{"op":"scan","table":{"name":"B","mode":"static"}},"offset":0,"count":count}),
        );
        assert_eq!(
            run(&context, owner.clone(), &plan).await,
            vec![vec![expected]]
        );
    }
    let many = project(json!({"op":"scan","table":{"name":"B","mode":"static"}}));
    let err = lower(&context, &many, owner.clone())
        .await
        .unwrap()
        .collect()
        .await
        .unwrap_err();
    assert_eq!(
        super::super::error(err).code,
        "relational.invalid_cardinality"
    );
    let invalid = project(
        json!({"op":"project","input":{"op":"scan","table":{"name":"B","mode":"static"}},"columns":[{"name":"a","expression":{"kind":"column","name":"ID"}},{"name":"b","expression":{"kind":"column","name":"ID"}}]}),
    );
    assert!(
        prepare(
            &context,
            &QueryPlan::Limit {
                input: Box::new(invalid),
                offset: 0,
                count: 0
            },
            owner
        )
        .await
        .is_err()
    );
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_correlated_global_max_membership_keeps_empty_null() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![(
            "ID",
            Arc::new(Int64Array::from(vec![Some(1), Some(2), None])),
        )],
    );
    register(
        &context,
        &request("B"),
        vec![("ID", Arc::new(Int64Array::from(vec![Some(1), None])))],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"hit","expression":{"kind":"in_query","value":{"kind":"column","name":"ID"},"input":{"op":"aggregate","groups":[],"metrics":[{"name":"max","function":"maximum","expression":{"kind":"column","name":"B.ID"}}],"input":{"op":"filter","input":{"op":"scan","table":{"name":"B","mode":"static"}},"predicate":{"kind":"binary","operator":"equal","left":{"kind":"column","name":"B.ID"},"right":{"kind":"outer_column","name":"A.ID"}}}}}}]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![
            vec![Scalar::Bool(true)],
            vec![Scalar::Null],
            vec![Scalar::Null]
        ]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_nested_membership_keeps_grandparent_and_immediate_scopes() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![(
            "ID",
            Arc::new(Int64Array::from(vec![Some(1), Some(2), Some(3), None])),
        )],
    );
    register(
        &context,
        &request("B"),
        vec![(
            "ID",
            Arc::new(Int64Array::from(vec![Some(1), Some(3), None])),
        )],
    );
    let inner = json!({"kind":"in_query","value":{"kind":"column","name":"B.ID"},"input":{"op":"filter","input":{"op":"scan","table":{"name":"B","mode":"static"}},"predicate":{"kind":"binary","operator":"and","left":{"kind":"binary","operator":"equal","left":{"kind":"column","name":"B.ID"},"right":{"kind":"outer_column","name":"A.ID","depth":2}},"right":{"kind":"binary","operator":"equal","left":{"kind":"column","name":"B.ID"},"right":{"kind":"outer_column","name":"B.ID","depth":1}}}}});
    let plan = decode(
        json!({"op":"filter","input":{"op":"scan","table":{"name":"A","mode":"static"}},"predicate":{"kind":"in_query","value":{"kind":"column","name":"A.ID"},"input":{"op":"filter","input":{"op":"scan","table":{"name":"B","mode":"static"}},"predicate":inner}}}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![Scalar::Int(1)], vec![Scalar::Int(3)]]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_recursive_membership_uses_one_previous_delta() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![("ID", Arc::new(Int64Array::from(vec![1, 2, 3, 4])))],
    );
    let plan = decode(
        json!({"op":"iterate","binding":"r","seed":{"op":"lookup","table":{"name":"A","mode":"static"},"ids":[1]},"step":{"op":"filter","input":{"op":"scan","table":{"name":"A","mode":"static"}},"predicate":{"kind":"in_query","value":{"kind":"binary","operator":"subtract","left":{"kind":"column","name":"A.ID"},"right":lit(json!(1))},"input":{"op":"delta","binding":"r"}}}}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![
            vec![Scalar::Int(1)],
            vec![Scalar::Int(2)],
            vec![Scalar::Int(3)],
            vec![Scalar::Int(4)]
        ]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_cycle_cannot_be_masked_by_outer_zero_limit() {
    let (context, _) = setup();
    register(
        &context,
        &request("A"),
        vec![("ID", Arc::new(Int64Array::from(vec![1])))],
    );
    let owner = QueryOwner::new(Arc::new(Budget::new(
        Cost {
            decode_work: 10_000,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    )));
    let plan = decode(
        json!({"op":"limit","offset":0,"count":0,"input":{"op":"iterate","binding":"r","seed":{"op":"scan","table":{"name":"A","mode":"static"}},"step":{"op":"delta","binding":"r"}}}),
    );
    let prepared = prepare(&context, &plan, owner.clone()).await.unwrap();
    assert!(
        prepared
            .explain_frame(&context)
            .unwrap()
            .logical_plan()
            .display_indent()
            .to_string()
            .contains("RecursiveQuery")
    );
    assert!(prepared.complete(&context, owner, 1 << 20).await.is_err());
}
#[test]
fn recovered_requirements_include_all_query_branches_and_reject_unbound_delta() {
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"n","expression":{"kind":"scalar_query","input":{"op":"union_all","inputs":[{"op":"lookup","table":{"name":"B","mode":"static"},"ids":[1]},{"op":"scan","table":{"name":"C","mode":"static"}}]}}}]}),
    );
    assert_eq!(
        tables(&plan)
            .unwrap()
            .iter()
            .map(|v| v.name.as_str())
            .collect::<Vec<_>>(),
        vec!["A", "B", "C"]
    );
    assert!(
        tables(&QueryPlan::Delta {
            binding: "missing".into()
        })
        .is_err()
    );
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_static_conditionals_bind_but_do_not_execute_dead_queries() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![("ID", Arc::new(Int64Array::from(vec![1, 2])))],
    );
    register(
        &context,
        &request("B"),
        vec![("ID", Arc::new(Int64Array::from(vec![7, 8])))],
    );
    let project = |expression: Value| {
        decode(
            json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"value","expression":expression}]}),
        )
    };
    let cycle = json!({"kind":"scalar_query","input":{"op":"iterate","binding":"dead","seed":{"op":"scan","table":{"name":"B","mode":"static"}},"step":{"op":"delta","binding":"dead"}}});
    let plan = project(
        json!({"kind":"case","branches":[{"when":lit(json!(true)),"then":{"kind":"column","name":"ID"}}],"otherwise":cycle}),
    );
    assert!(
        !prepare(&context, &plan, owner.clone())
            .await
            .unwrap()
            .has_prerequisites()
    );
    assert_eq!(tables(&plan).unwrap().len(), 2);
    assert_eq!(
        run(&context, owner.clone(), &plan).await,
        vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]]
    );
    let many =
        json!({"kind":"scalar_query","input":{"op":"scan","table":{"name":"B","mode":"static"}}});
    let plan = project(json!({"kind":"coalesce","values":[lit(json!(null)),lit(json!(9)),many]}));
    assert_eq!(
        run(&context, owner.clone(), &plan).await,
        vec![vec![Scalar::Int(9)], vec![Scalar::Int(9)]]
    );
    let plan = project(
        json!({"kind":"case","branches":[{"when":lit(json!(true)),"then":{"kind":"column","name":"ID"}}],"otherwise":{"kind":"in_query","value":lit(json!(1)),"input":{"op":"scan","table":{"name":"B","mode":"static"}}}}),
    );
    assert_eq!(
        run(&context, owner.clone(), &plan).await,
        vec![vec![Scalar::Int(1)], vec![Scalar::Int(2)]]
    );
    let invalid = project(
        json!({"kind":"case","branches":[{"when":lit(json!(true)),"then":lit(json!(1))}],"otherwise":{"kind":"scalar_query","input":{"op":"project","input":{"op":"scan","table":{"name":"B","mode":"static"}},"columns":[{"name":"bad","expression":{"kind":"column","name":"missing"}}]}}}),
    );
    assert!(prepare(&context, &invalid, owner).await.is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_first_case_query_condition_and_dynamic_result_boundary() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![("ID", Arc::new(Int64Array::from(vec![1, 2])))],
    );
    register(
        &context,
        &request("B"),
        vec![("ID", Arc::new(Int64Array::from(vec![1])))],
    );
    let query = json!({"kind":"in_query","value":{"kind":"column","name":"ID"},"input":{"op":"scan","table":{"name":"B","mode":"static"}}});
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"value","expression":{"kind":"case","branches":[{"when":query,"then":{"kind":"column","name":"ID"}}],"otherwise":lit(json!(null))}}]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![vec![Scalar::Int(1)], vec![Scalar::Null]]
    );
    let invalid = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"value","expression":{"kind":"case","branches":[{"when":{"kind":"is_null","value":{"kind":"column","name":"ID"}},"then":query}],"otherwise":lit(json!(false))}}]}),
    );
    assert!(tables(&invalid).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn recovered_projected_away_correlation_key_survives_native_pullup() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![("key", Arc::new(Int64Array::from(vec![1, 2, 3])))],
    );
    register(
        &context,
        &request("B"),
        vec![
            ("ID", Arc::new(Int64Array::from(vec![10, 11, 20]))),
            ("key", Arc::new(Int64Array::from(vec![1, 1, 2]))),
        ],
    );
    let plan = decode(
        json!({"op":"project","input":{"op":"scan","table":{"name":"A","mode":"static"}},"columns":[{"name":"value","expression":{"kind":"scalar_query","input":{"op":"aggregate","groups":[],"metrics":[{"name":"maximum","function":"maximum","expression":{"kind":"column","name":"ID"}}],"input":{"op":"project","columns":[{"name":"ID","expression":{"kind":"column","name":"B.ID"}}],"input":{"op":"filter","input":{"op":"scan","table":{"name":"B","mode":"static"}},"predicate":{"kind":"binary","operator":"equal","left":{"kind":"column","name":"B.key"},"right":{"kind":"outer_column","name":"A.key"}}}}}}}]}),
    );
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![
            vec![Scalar::Int(11)],
            vec![Scalar::Int(20)],
            vec![Scalar::Null]
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn recovered_static_wide_integer_branch_does_not_coerce_to_dead_float() {
    let expression = json!({"kind":"case","branches":[{"when":lit(json!(true)),"then":lit(json!(u64::MAX))}],"otherwise":lit(json!(1.0))});
    assert_eq!(
        new_scalar(expression).await.unwrap(),
        Scalar::UInt(u64::MAX)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn correlated_scalar_max_preserves_multiple_equality_keys() {
    let (context, owner) = setup();
    register(
        &context,
        &request("A"),
        vec![
            ("key", Arc::new(Int64Array::from(vec![1, 1, 2, 3]))),
            ("group", Arc::new(Int64Array::from(vec![10, 20, 10, 10]))),
        ],
    );
    register(
        &context,
        &request("B"),
        vec![
            ("ID", Arc::new(Int64Array::from(vec![7, 9, 8, 5]))),
            ("key", Arc::new(Int64Array::from(vec![1, 1, 1, 2]))),
            ("group", Arc::new(Int64Array::from(vec![10, 20, 10, 10]))),
        ],
    );
    let plan = decode(json!({
        "op":"project", "input":{"op":"scan","table":{"name":"A","mode":"static"}},
        "columns":[{"name":"value","expression":{"kind":"scalar_query","input":{
            "op":"aggregate", "groups":[],
            "metrics":[{"name":"maximum","function":"maximum","expression":{"kind":"column","name":"B.ID"}}],
            "input":{"op":"filter","input":{"op":"scan","table":{"name":"B","mode":"static"}},
                "predicate":{"kind":"binary","operator":"and",
                    "left":{"kind":"binary","operator":"equal",
                        "left":{"kind":"column","name":"B.key"},"right":{"kind":"outer_column","name":"A.key"}},
                    "right":{"kind":"binary","operator":"equal",
                        "left":{"kind":"column","name":"B.group"},"right":{"kind":"outer_column","name":"A.group"}}}}
        }}}]
    }));
    assert_eq!(
        run(&context, owner, &plan).await,
        vec![
            vec![Scalar::Int(8)],
            vec![Scalar::Int(9)],
            vec![Scalar::Int(5)],
            vec![Scalar::Null]
        ]
    );
}
