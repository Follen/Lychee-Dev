//! Independent expected-value witnesses for pinned DataFusion suitability.
//! These declarations are NOT_RUN until the coordinator executes them.
use super::*;
use crate::{relational::Scalar, resource::Cost};
use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use datafusion::{
    common::ScalarValue,
    datasource::MemTable,
    execution::{
        context::{SQLOptions, SessionContext},
        session_state::SessionStateBuilder,
    },
    logical_expr::{Expr, ExprSchemable, LogicalPlan},
    physical_plan::{
        ExecutionPlan,
        aggregates::{AggregateExec, AggregateMode},
        collect,
        joins::HashJoinExec,
    },
    prelude::SessionConfig,
};
use std::time::{Duration, Instant};

fn budget() -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    ))
}
fn configured() -> (SessionContext, Arc<QueryOwner>) {
    configured_partitions(1)
}
fn configured_partitions(partitions: usize) -> (SessionContext, Arc<QueryOwner>) {
    let owner = QueryOwner::new(budget());
    // Empty state has no file formats, external-table factories, table
    // functions, or ordinary SUM/AVG implementations to fall back to.
    let mut functions = aggregate::functions(&owner);
    functions.push(datafusion::functions_aggregate::count::count_udaf());
    let state = SessionStateBuilder::new()
        .with_config(
            SessionConfig::new()
                .with_target_partitions(partitions)
                .with_batch_size(64),
        )
        // Registering COUNT's UDAF alone does not lower SQL COUNT(*) in
        // DataFusion 54. Keep its upstream aggregate planner without enabling
        // unrelated default functions, file formats, or table factories.
        .with_expr_planners(vec![
            Arc::new(numeric::NumericPlanner {
                owner: owner.clone(),
            }),
            Arc::new(datafusion::functions_aggregate::planner::AggregateFunctionPlanner),
        ])
        .with_aggregate_functions(functions)
        .build();
    (SessionContext::new_with_state(state), owner)
}
fn register(context: &SessionContext, name: &str, columns: Vec<(&str, ArrayRef)>) {
    let schema = Arc::new(Schema::new(
        columns
            .iter()
            .map(|(name, array)| Field::new(*name, array.data_type().clone(), true))
            .collect::<Vec<_>>(),
    ));
    context
        .register_batch(
            name,
            RecordBatch::try_new(
                schema,
                columns.into_iter().map(|(_, array)| array).collect(),
            )
            .unwrap(),
        )
        .unwrap();
}
async fn rows(context: &SessionContext, sql: &str) -> Vec<Vec<Scalar>> {
    let batches = context
        .sql_with_options(
            sql,
            SQLOptions::new()
                .with_allow_ddl(false)
                .with_allow_dml(false)
                .with_allow_statements(false),
        )
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    scalar_rows(&batches)
}
fn scalar_rows(batches: &[RecordBatch]) -> Vec<Vec<Scalar>> {
    batches
        .iter()
        .flat_map(|batch| {
            (0..batch.num_rows()).map(move |row| {
                batch
                    .columns()
                    .iter()
                    .map(|array| {
                        values::read(ScalarValue::try_from_array(array, row).unwrap()).unwrap()
                    })
                    .collect()
            })
        })
        .collect()
}
fn contains_plan<T: ExecutionPlan>(plan: &dyn ExecutionPlan) -> bool {
    plan.is::<T>()
        || plan
            .children()
            .iter()
            .any(|child| contains_plan::<T>(child.as_ref()))
}
fn aggregate_modes(plan: &dyn ExecutionPlan, modes: &mut Vec<AggregateMode>) {
    if let Some(aggregate) = plan.downcast_ref::<AggregateExec>() {
        modes.push(*aggregate.mode());
    }
    for child in plan.children() {
        aggregate_modes(child.as_ref(), modes);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn compatible_integer_addition_crosses_i64_without_float_or_wrap() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![
            ("a", Arc::new(Int64Array::from(vec![i64::MAX]))),
            ("b", Arc::new(Int64Array::from(vec![1]))),
        ],
    );
    assert_eq!(
        rows(&context, "SELECT a + b AS answer FROM t").await,
        vec![vec![Scalar::UInt(1u64 << 63)]]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn compatible_integer_overflow_remains_original_typed_error() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![
            ("a", Arc::new(UInt64Array::from(vec![u64::MAX]))),
            ("b", Arc::new(UInt64Array::from(vec![1]))),
        ],
    );
    let failure = context
        .sql("SELECT a + b AS answer FROM t")
        .await
        .unwrap()
        .collect()
        .await
        .unwrap_err();
    assert_eq!(error(failure).code, "relational.numeric_range");
}
#[tokio::test(flavor = "current_thread")]
async fn compatible_integer_sum_crosses_i64_and_rejects_u64_overflow() {
    let (context, _owner) = configured();
    register(
        &context,
        "signed",
        vec![("value", Arc::new(Int64Array::from(vec![i64::MAX, 1])))],
    );
    assert_eq!(
        rows(&context, "SELECT SUM(value) AS answer FROM signed").await,
        vec![vec![Scalar::UInt(1u64 << 63)]]
    );
    register(
        &context,
        "unsigned",
        vec![("value", Arc::new(UInt64Array::from(vec![u64::MAX, 1])))],
    );
    let failure = context
        .sql("SELECT SUM(value) AS answer FROM unsigned")
        .await
        .unwrap()
        .collect()
        .await
        .unwrap_err();
    assert_eq!(error(failure).code, "relational.numeric_range");
}
#[tokio::test(flavor = "current_thread")]
async fn compatible_average_preserves_small_residue_beyond_binary64_integer_precision() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![(
            "value",
            Arc::new(Int64Array::from(vec![
                9_007_199_254_740_993,
                -9_007_199_254_740_992,
            ])),
        )],
    );
    assert_eq!(
        rows(&context, "SELECT AVG(value) AS answer FROM t").await,
        vec![vec![Scalar::Float(0.5)]]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn compatible_average_does_not_overflow_finite_final_answer() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![(
            "value",
            Arc::new(Float64Array::from(vec![f64::MAX, f64::MAX])),
        )],
    );
    assert_eq!(
        rows(&context, "SELECT AVG(value) AS answer FROM t").await,
        vec![vec![Scalar::Float(f64::MAX)]]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn compatible_mixed_comparison_and_zero_comparison_preserve_domain_contract() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![
            (
                "integer_value",
                Arc::new(UInt64Array::from(vec![9_007_199_254_740_993])),
            ),
            (
                "float_value",
                Arc::new(Float64Array::from(vec![9_007_199_254_740_992.0])),
            ),
            ("negative_zero", Arc::new(Float64Array::from(vec![-0.0]))),
            ("positive_zero", Arc::new(Float64Array::from(vec![0.0]))),
        ],
    );
    assert_eq!(rows(&context,"SELECT integer_value = float_value AS precise, negative_zero = positive_zero AS same_zero FROM t").await,vec![vec![Scalar::Bool(false),Scalar::Bool(true)]]);
    let projected = rows(&context, "SELECT negative_zero FROM t").await;
    let Scalar::Float(value) = &projected[0][0] else {
        panic!("float expected")
    };
    assert_eq!(value.to_bits(), (-0.0f64).to_bits());
}
#[tokio::test(flavor = "current_thread")]
async fn compatible_division_and_null_zero_behavior_use_existing_exact_primitive() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![
            ("a", Arc::new(Int64Array::from(vec![1]))),
            ("b", Arc::new(Int64Array::from(vec![3]))),
            ("z", Arc::new(Int64Array::from(vec![0]))),
        ],
    );
    assert_eq!(
        rows(
            &context,
            "SELECT a / b AS fraction, a / z AS missing, a % z AS remainder FROM t"
        )
        .await,
        vec![vec![Scalar::Float(1.0 / 3.0), Scalar::Null, Scalar::Null]]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn compatible_grouping_uses_native_datafusion_execution_with_exact_accumulators() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![
            ("kind", Arc::new(Int64Array::from(vec![1, 2, 1, 2]))),
            (
                "value",
                Arc::new(Int64Array::from(vec![
                    9_007_199_254_740_993,
                    7,
                    -9_007_199_254_740_992,
                    9,
                ])),
            ),
        ],
    );
    assert_eq!(
        rows(
            &context,
            "SELECT kind, AVG(value) AS answer FROM t GROUP BY kind ORDER BY kind"
        )
        .await,
        vec![
            vec![Scalar::Int(1), Scalar::Float(0.5)],
            vec![Scalar::Int(2), Scalar::Float(8.0)]
        ]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn native_integer_binary_and_sum_wrap_boundaries_are_recorded_separately() {
    let context = SessionContext::new();
    register(
        &context,
        "t",
        vec![("value", Arc::new(Int64Array::from(vec![i64::MAX, 1])))],
    );
    // Source-derived expectation, deliberately isolated from compatibility
    // acceptance. An unexpected runtime result requires revising the matrix.
    assert_eq!(
        rows(&context, "SELECT SUM(value) FROM t").await,
        vec![vec![Scalar::Int(i64::MIN)]]
    );
    register(
        &context,
        "arithmetic",
        vec![
            ("signed_value", Arc::new(Int64Array::from(vec![i64::MAX]))),
            (
                "unsigned_value",
                Arc::new(UInt64Array::from(vec![u64::MAX])),
            ),
            ("unsigned_one", Arc::new(UInt64Array::from(vec![1]))),
        ],
    );
    assert_eq!(
        rows(
            &context,
            "SELECT signed_value + 1, unsigned_value + unsigned_one FROM arithmetic"
        )
        .await,
        vec![vec![Scalar::Int(i64::MIN), Scalar::UInt(0)]]
    );
}
#[tokio::test(flavor = "current_thread")]
async fn native_average_precision_boundary_witness_is_recorded_separately() {
    let context = SessionContext::new();
    register(
        &context,
        "t",
        vec![(
            "value",
            Arc::new(Int64Array::from(vec![
                9_007_199_254_740_993,
                -9_007_199_254_740_992,
            ])),
        )],
    );
    assert_eq!(
        rows(&context, "SELECT AVG(value) FROM t").await,
        vec![vec![Scalar::Float(0.0)]]
    );
    register(
        &context,
        "large",
        vec![(
            "value",
            Arc::new(Float64Array::from(vec![f64::MAX, f64::MAX])),
        )],
    );
    let output = context
        .sql("SELECT AVG(value) FROM large")
        .await
        .unwrap()
        .collect()
        .await
        .unwrap();
    assert_eq!(
        ScalarValue::try_from_array(output[0].column(0), 0).unwrap(),
        ScalarValue::Float64(Some(f64::INFINITY))
    );
}
#[test]
fn public_decimal_boundary_does_not_coerce_through_float() {
    assert_eq!(
        values::read(ScalarValue::Decimal128(Some(u64::MAX as i128), 20, 0)).unwrap(),
        Scalar::UInt(u64::MAX)
    );
    assert_eq!(
        error(
            values::read(ScalarValue::Decimal128(Some(u64::MAX as i128 + 1), 20, 0)).unwrap_err()
        )
        .code,
        "relational.numeric_range"
    );
    assert_eq!(
        values::write(Scalar::Null, &DataType::Float64).unwrap(),
        ScalarValue::Float64(None)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn exact_state_merges_across_two_real_input_partitions() {
    let (context, _owner) = configured_partitions(2);
    let schema = Arc::new(Schema::new(vec![Field::new(
        "value",
        DataType::Int64,
        false,
    )]));
    let partitions = [
        vec![9_007_199_254_740_993, 0],
        vec![-9_007_199_254_740_992, 0],
    ]
    .into_iter()
    .map(|values| {
        vec![
            RecordBatch::try_new(schema.clone(), vec![Arc::new(Int64Array::from(values))]).unwrap(),
        ]
    })
    .collect();
    context
        .register_table(
            "t",
            Arc::new(MemTable::try_new(schema, partitions).unwrap()),
        )
        .unwrap();
    let plan = context
        .sql("SELECT SUM(value) AS total, AVG(value) AS mean FROM t")
        .await
        .unwrap()
        .create_physical_plan()
        .await
        .unwrap();
    let mut modes = Vec::new();
    aggregate_modes(plan.as_ref(), &mut modes);
    assert!(modes.contains(&AggregateMode::Partial), "{modes:?}");
    assert!(
        modes.contains(&AggregateMode::Final) || modes.contains(&AggregateMode::FinalPartitioned),
        "{modes:?}"
    );
    let batches = collect(plan, context.task_ctx()).await.unwrap();
    assert_eq!(
        scalar_rows(&batches),
        vec![vec![Scalar::Int(1), Scalar::Float(0.25)]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn nested_and_constant_arithmetic_keep_exact_type_and_schema() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![("a", Arc::new(Int64Array::from(vec![i64::MAX])))],
    );
    let query = context
        .sql("SELECT (a + 1) - a AS nested, (9223372036854775807 + 1) - 9223372036854775807 AS folded FROM t")
        .await
        .unwrap();
    let optimized = query.clone().into_optimized_plan().unwrap();
    let LogicalPlan::Projection(projection) = optimized else {
        panic!("expected a native projection plan")
    };
    let Expr::Alias(folded) = &projection.expr[1] else {
        panic!("expected the public folded alias")
    };
    // Exact arithmetic is request-metered and Volatile: DF must preserve its
    // exact type, but is not required to evaluate constants during planning.
    assert_eq!(
        folded.expr.get_type(projection.input.schema()).unwrap(),
        DataType::Decimal128(20, 0)
    );
    let plan = query.create_physical_plan().await.unwrap();
    for (field, name) in plan.schema().fields().iter().zip(["nested", "folded"]) {
        assert_eq!(field.name(), name);
        assert_eq!(field.data_type(), &DataType::Decimal128(20, 0));
    }
    let schema = plan.schema();
    let batches = collect(plan, context.task_ctx()).await.unwrap();
    for batch in &batches {
        assert_eq!(batch.schema(), schema);
        for column in batch.columns() {
            assert_eq!(
                ScalarValue::try_from_array(column, 0).unwrap(),
                ScalarValue::Decimal128(Some(1), 20, 0)
            );
        }
    }
    assert_eq!(
        scalar_rows(&batches),
        vec![vec![Scalar::Int(1), Scalar::Int(1)]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn exact_mixed_integer_equality_retains_native_hash_join_plan() {
    let (context, _owner) = configured_partitions(2);
    register(
        &context,
        "l",
        vec![(
            "key",
            Arc::new(UInt64Array::from(vec![u64::MAX, i64::MAX as u64])),
        )],
    );
    register(
        &context,
        "r",
        vec![("key", Arc::new(Int64Array::from(vec![i64::MAX, -1])))],
    );
    let plan = context
        .sql("SELECT l.key AS matched FROM l JOIN r ON l.key = r.key ORDER BY l.key")
        .await
        .unwrap()
        .create_physical_plan()
        .await
        .unwrap();
    assert!(contains_plan::<HashJoinExec>(plan.as_ref()));
    let batches = collect(plan, context.task_ctx()).await.unwrap();
    assert_eq!(
        scalar_rows(&batches),
        vec![vec![Scalar::UInt(i64::MAX as u64)]]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn upstream_sql_count_preserves_wildcard_nullable_and_empty_inputs() {
    let (context, _owner) = configured();
    register(
        &context,
        "t",
        vec![(
            "value",
            Arc::new(Int64Array::from(vec![Some(i64::MIN), None, Some(i64::MAX)])),
        )],
    );
    assert_eq!(
        rows(
            &context,
            "SELECT count(*), count(value), count(DISTINCT value) FROM t"
        )
        .await,
        vec![vec![Scalar::Int(3), Scalar::Int(2), Scalar::Int(2)]]
    );
    register(
        &context,
        "empty",
        vec![(
            "value",
            Arc::new(Int64Array::from(Vec::<Option<i64>>::new())),
        )],
    );
    assert_eq!(
        rows(&context, "SELECT count(*), count(value) FROM empty").await,
        vec![vec![Scalar::Int(0), Scalar::Int(0)]]
    );
    assert_eq!(
        rows(
            &context,
            "SELECT count(*), count(value) FROM t WHERE value IS NULL"
        )
        .await,
        vec![vec![Scalar::Int(1), Scalar::Int(0)]]
    );
}
