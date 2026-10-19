//! Real authenticated CASC/DBD/WDC task witnesses. These declarations are not
//! Windows execution evidence until the root runs the fixed candidate.
use super::*;
use crate::{
    engine::{error, values},
    relational::Scalar,
    resource::Cost,
};
use arrow_array::{Array, FixedSizeListArray, Float32Array, Int64Array, StringArray, UInt64Array};
use datafusion::{
    common::ScalarValue,
    execution::{
        context::{SQLOptions, SessionContext},
        memory_pool::MemoryConsumer,
    },
    logical_expr::lit,
    physical_plan::common::collect,
};
use futures_util::StreamExt;
use serde_json::json;
use std::{
    fs,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};
#[allow(dead_code)]
#[path = "../../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};

fn budget() -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        None,
    ))
}
fn limits() -> SnapshotLimits {
    SnapshotLimits {
        rows: 16,
        batches: 16,
        arrow_bytes: 1 << 20,
        batch: ImportLimits {
            rows: 2,
            value_bytes: 1024,
            text_bytes: 1 << 20,
        },
    }
}
fn spec() -> fixture::Table {
    fixture::table(
        "Sample",
        921,
        &[("ID", "uint"), ("value", "signed64"), ("label", "text")],
        &[
            vec![Cell::U(9), Cell::I64(i64::MIN), Cell::Text("low".into())],
            vec![
                Cell::U(1),
                Cell::I64(9_007_199_254_740_993),
                Cell::Text("中文🙂".into()),
            ],
            vec![
                Cell::U(2),
                Cell::I64(9_007_199_254_740_992),
                Cell::Text("neighbor".into()),
            ],
        ],
    )
}
fn prepare(fx: &Fixture, budget: &Arc<Budget>) -> Arc<FixedSnapshot> {
    FixedSnapshot::prepare(
        &fx.invocation("data sql", &[]),
        "Sample",
        SourceMode::Static,
        limits(),
        budget.clone(),
    )
    .unwrap()
}
fn register(
    snapshot: &Arc<FixedSnapshot>,
    budget: Arc<Budget>,
    batch_rows: usize,
) -> (SessionContext, Arc<QueryOwner>) {
    let (context, owner) = runtime::context(budget, 16 << 20, batch_rows).unwrap();
    context
        .register_table(
            "t",
            Arc::new(FixedTableProvider::new(snapshot.clone(), owner.clone()).unwrap()),
        )
        .unwrap();
    (context, owner)
}
async fn sql(
    context: &SessionContext,
    snapshot: &Arc<FixedSnapshot>,
    owner: &Arc<QueryOwner>,
    query: &str,
) -> QualifiedResult {
    let stream = context
        .sql_with_options(
            query,
            SQLOptions::new()
                .with_allow_ddl(false)
                .with_allow_dml(false)
                .with_allow_statements(false),
        )
        .await
        .unwrap_or_else(|error| panic!("SQL planning failed for {query:?}: {error}"))
        .execute_stream()
        .await
        .unwrap_or_else(|error| panic!("SQL execute_stream failed for {query:?}: {error}"));
    snapshot
        .collect_result(
            stream,
            owner.clone(),
            OutputLimits {
                rows: 16,
                batches: 16,
                bytes: 65536,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("SQL collect_result failed for {query:?}: {error}"))
}
fn scalar_rows(output: &QualifiedResult) -> Vec<Vec<Scalar>> {
    output
        .records()
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

#[tokio::test(flavor = "current_thread")]
async fn authenticated_provider_filters_exact_large_integer_on_nonprojected_column() {
    let fx = Fixture::new(&[spec()]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget, 1);
    let output = sql(
        &context,
        &snapshot,
        &owner,
        "SELECT label FROM t WHERE value = 9007199254740993",
    )
    .await;
    assert_eq!(
        scalar_rows(&output),
        vec![vec![Scalar::String("中文🙂".into())]]
    );
    assert!(output.complete());
    assert!(output.observed_bytes() > 0);
    let evidence = serde_json::to_value(snapshot.identity().unwrap()).unwrap();
    assert_eq!(evidence["sourceKind"], "static");
    assert_eq!(evidence["data"]["language"], "zhCN");
    assert_eq!(
        evidence["definition"]["sha256"],
        snapshot.source.definition.sha256
    );
    assert_eq!(
        snapshot.source_sha256(),
        snapshot.identity().unwrap().digest(&owner.budget).unwrap()
    );
    drop((output, context, owner, snapshot));
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn admitted_scan_preserves_u64_float32_fixed_arrays_and_null_relationships() {
    let mut spec = fixture::table(
        "Sample",
        922,
        &[
            ("ID", "uint"),
            ("unsigned", "signed64"),
            ("ratio", "float"),
            ("items", "array3"),
        ],
        &[vec![
            Cell::U(1),
            Cell::I64(-1),
            Cell::F(-0.0),
            Cell::Array(vec![Cell::U(7), Cell::U(0), Cell::U(u32::MAX)]),
        ]],
    );
    spec.dbd = spec
        .dbd
        .replace("unsigned<64>", "unsigned<u64>")
        .replace("\n\nBUILD", "\nint Parent\n\nBUILD");
    spec.dbd.push_str("$noninline,relation$Parent\n");
    let fx = Fixture::new(&[spec]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget, 4);
    let provider = FixedTableProvider::new(snapshot.clone(), owner).unwrap();
    let plan = provider
        .scan(&context.state(), None, &[], None)
        .await
        .unwrap();
    let records = collect(plan.execute(0, context.task_ctx()).unwrap())
        .await
        .unwrap();
    let batch = &records[0];
    assert_eq!(
        batch
            .column(1)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0),
        u64::MAX
    );
    assert_eq!(
        batch
            .column(2)
            .as_any()
            .downcast_ref::<Float32Array>()
            .unwrap()
            .value(0)
            .to_bits(),
        (-0.0f32).to_bits()
    );
    let child = batch
        .column(3)
        .as_any()
        .downcast_ref::<FixedSizeListArray>()
        .unwrap()
        .value(0);
    assert_eq!(
        child
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .values()
            .as_ref(),
        &[7, 0, u32::MAX as u64]
    );
    assert!(batch.column(4).is_null(0));
    assert_eq!(batch.column(4).data_type(), &arrow_schema::DataType::UInt64);
    drop((records, plan, provider, context, snapshot));
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn projection_order_duplicates_zero_columns_limits_and_batch_rows_are_preserved() {
    let fx = Fixture::new(&[spec()]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget, 1);
    let provider = FixedTableProvider::new(snapshot.clone(), owner).unwrap();
    let plan = provider
        .scan(&context.state(), Some(&vec![2, 0, 2]), &[], Some(2))
        .await
        .unwrap();
    let records = collect(plan.execute(0, context.task_ctx()).unwrap())
        .await
        .unwrap();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|batch| batch.num_rows() == 1));
    assert_eq!(
        records[0]
            .schema()
            .fields()
            .iter()
            .map(|field| field.name().as_str())
            .collect::<Vec<_>>(),
        vec!["label", "ID", "label"]
    );
    assert_eq!(
        records[1]
            .column(1)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap()
            .value(0),
        2
    );
    let empty = provider
        .scan(&context.state(), Some(&vec![]), &[], None)
        .await
        .unwrap();
    let zero_columns = collect(empty.execute(0, context.task_ctx()).unwrap())
        .await
        .unwrap();
    assert_eq!(
        zero_columns
            .iter()
            .map(RecordBatch::num_rows)
            .sum::<usize>(),
        3
    );
    assert!(zero_columns.iter().all(|batch| batch.num_columns() == 0));
    let zero_limit = provider
        .scan(&context.state(), None, &[], Some(0))
        .await
        .unwrap();
    assert!(
        collect(zero_limit.execute(0, context.task_ctx()).unwrap())
            .await
            .unwrap()
            .is_empty()
    );
    drop((
        records,
        zero_columns,
        zero_limit,
        empty,
        plan,
        provider,
        context,
        snapshot,
    ));
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn partial_source_empty_lookup_and_count_never_establish_absence_or_completeness() {
    let spec = fixture::encrypted_sections(923);
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let fx = Fixture::with_encoded_table(spec, encoded);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget, 4);
    let absent = sql(
        &context,
        &snapshot,
        &owner,
        "SELECT * FROM t WHERE \"ID\" = 2",
    )
    .await;
    assert!(scalar_rows(&absent).is_empty());
    assert!(!absent.complete());
    let count = sql(&context, &snapshot, &owner, "SELECT count(*) FROM t").await;
    assert_eq!(scalar_rows(&count), vec![vec![Scalar::Int(1)]]);
    assert!(!count.complete());
    let column_count = sql(&context, &snapshot, &owner, "SELECT count(\"ID\") FROM t").await;
    assert_eq!(scalar_rows(&column_count), vec![vec![Scalar::Int(1)]]);
    assert!(!column_count.complete());
    let empty_count = sql(
        &context,
        &snapshot,
        &owner,
        "SELECT count(*), count(\"ID\") FROM t WHERE \"ID\" = 2",
    )
    .await;
    assert_eq!(
        scalar_rows(&empty_count),
        vec![vec![Scalar::Int(0), Scalar::Int(0)]]
    );
    assert!(!empty_count.complete());
    let identity = serde_json::to_value(snapshot.identity().unwrap()).unwrap();
    assert_eq!(identity["complete"], false);
    assert_eq!(identity["missing"][0]["keyID"], "1234567890abcdef");
    assert_eq!(identity["unavailablePartitions"], json!([1]));
    assert_eq!(identity["source"]["content"]["sha256"], "");
    assert_eq!(
        identity["source"]["partialContent"]["sha256"]
            .as_str()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(snapshot.batches[0].ids.values().as_ref(), &[1]);
    drop((
        absent,
        count,
        column_count,
        empty_count,
        context,
        owner,
        snapshot,
    ));
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_provider_sql_count_preserves_null_empty_and_owner_contracts() {
    let mut spec = spec();
    spec.dbd = spec.dbd.replace("\n\nBUILD", "\nint Parent\n\nBUILD");
    spec.dbd.push_str("$noninline,relation$Parent\n");
    let fx = Fixture::new(&[spec]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget.clone(), 1);
    let counts = sql(
        &context,
        &snapshot,
        &owner,
        "SELECT count(*), count(value), count(\"Parent\") FROM t",
    )
    .await;
    assert_eq!(
        scalar_rows(&counts),
        vec![vec![Scalar::Int(3), Scalar::Int(3), Scalar::Int(0)]]
    );
    assert!(counts.complete());
    assert_eq!(counts.records()[0].schema().field(0).name(), "count(*)");
    let empty = sql(
        &context,
        &snapshot,
        &owner,
        "SELECT count(*), count(value), count(\"Parent\") FROM t WHERE \"ID\" = 99",
    )
    .await;
    assert_eq!(
        scalar_rows(&empty),
        vec![vec![Scalar::Int(0), Scalar::Int(0), Scalar::Int(0)]]
    );
    assert!(empty.complete());
    drop((empty, context, owner, snapshot));
    assert!(budget.refundable_retained_bytes() > 0);
    assert_eq!(scalar_rows(&counts)[0][0], Scalar::Int(3));
    drop(counts);
    assert_eq!(budget.refundable_retained_bytes(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn authenticated_provider_sql_count_still_enforces_output_byte_bound() {
    let fx = Fixture::new(&[spec()]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget.clone(), 1);
    let stream = context
        .sql_with_options(
            "SELECT count(*) FROM t",
            SQLOptions::new()
                .with_allow_ddl(false)
                .with_allow_dml(false)
                .with_allow_statements(false),
        )
        .await
        .unwrap()
        .execute_stream()
        .await
        .unwrap();
    let result = snapshot
        .collect_result(
            stream,
            owner.clone(),
            OutputLimits {
                rows: 1,
                batches: 1,
                bytes: 1,
            },
        )
        .await;
    let failure = match result {
        Ok(_) => panic!("COUNT output must not bypass the admitted byte bound"),
        Err(failure) => failure,
    };
    assert_eq!(error(failure).code, "records.query_resource_budget");
    drop((context, owner, snapshot));
    assert_eq!(budget.refundable_retained_bytes(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn explicit_copy_rows_keep_logical_ids_and_values_in_provider() {
    let mut spec = fixture::table(
        "Sample",
        924,
        &[("ID", "uint"), ("value", "uint")],
        &[vec![Cell::U(1), Cell::U(42)]],
    );
    spec.raw[108..112].copy_from_slice(&1u32.to_le_bytes());
    spec.raw.extend(9u32.to_le_bytes());
    spec.raw.extend(1u32.to_le_bytes());
    let fx = Fixture::new(&[spec]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    assert_eq!(snapshot.rows, 2);
    assert_eq!(snapshot.batches[0].ids.values().as_ref(), &[1, 9]);
    let (context, owner) = register(&snapshot, budget, 4);
    let result = sql(
        &context,
        &snapshot,
        &owner,
        "SELECT \"ID\", value FROM t WHERE \"ID\" = 9",
    )
    .await;
    assert_eq!(
        scalar_rows(&result),
        vec![vec![Scalar::UInt(9), Scalar::UInt(42)]]
    );
    drop((result, context, owner, snapshot));
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn unselected_malformed_column_is_not_hidden_by_later_projection_or_limit() {
    let fx = Fixture::new(&[fixture::table(
        "Sample",
        925,
        &[("ID", "uint"), ("label", "text")],
        &[
            vec![Cell::U(1), Cell::Text("valid".into())],
            vec![Cell::U(2), Cell::BadString],
        ],
    )]);
    let budget = budget();
    let result = FixedSnapshot::prepare(
        &fx.invocation("data sql", &[]),
        "Sample",
        SourceMode::Static,
        limits(),
        budget.clone(),
    );
    assert!(result.is_err());
    assert_eq!(budget.refundable_retained_bytes(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn effective_hotfix_request_is_explicitly_rejected_before_source_access() {
    let fx = Fixture::new(&[spec()]);
    let mut inv = fx.invocation("data sql", &[]);
    inv.home = fx.root.join("does-not-exist");
    let err = FixedSnapshot::prepare(&inv, "Sample", SourceMode::Effective, limits(), budget())
        .unwrap_err();
    assert_eq!(err.code, "relational.unsupported");
    assert!(err.message.contains("Hotfix"));
    assert!(!inv.home.exists());
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn total_rows_batches_and_arrow_admission_caps_reject_and_refund_owned_buffers() {
    let fx = Fixture::new(&[spec()]);
    for cap in [
        SnapshotLimits {
            rows: 2,
            ..limits()
        },
        SnapshotLimits {
            batches: 1,
            ..limits()
        },
        SnapshotLimits {
            arrow_bytes: 1,
            ..limits()
        },
    ] {
        let budget = budget();
        let error = FixedSnapshot::prepare(
            &fx.invocation("data sql", &[]),
            "Sample",
            SourceMode::Static,
            cap,
            budget.clone(),
        )
        .unwrap_err();
        assert_eq!(error.code, "records.query_resource_budget");
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn snapshot_identity_stays_fixed_and_distinguishes_changed_source_bytes() {
    let first = Fixture::new(&[spec()]);
    let mut changed = spec();
    let raw_offset = u32::from_le_bytes(changed.raw[80..84].try_into().unwrap()) as usize;
    changed.raw[raw_offset + 4..raw_offset + 12].copy_from_slice(&42i64.to_le_bytes());
    let second = Fixture::new(&[changed]);
    let first_budget = budget();
    let second_budget = budget();
    let a = prepare(&first, &first_budget);
    let b = prepare(&second, &second_budget);
    assert_ne!(a.source_sha256(), b.source_sha256());
    assert_ne!(
        a.source.file.reading["content"]["sha256"],
        b.source.file.reading["content"]["sha256"]
    );
    let value = a
        .batches
        .last()
        .unwrap()
        .records
        .column(1)
        .as_any()
        .downcast_ref::<Int64Array>()
        .unwrap()
        .value(0);
    assert_eq!(value, i64::MIN);
    drop((a, b));
    fs::remove_dir_all(first.root).unwrap();
    fs::remove_dir_all(second.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn scan_rejects_invalid_projection_filters_partition_children_and_foreign_owner() {
    let fx = Fixture::new(&[spec()]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget, 4);
    let provider = FixedTableProvider::new(snapshot.clone(), owner).unwrap();
    assert!(
        provider
            .scan(&context.state(), Some(&vec![3]), &[], None)
            .await
            .is_err()
    );
    assert!(
        provider
            .scan(&context.state(), None, &[lit(true)], Some(1))
            .await
            .is_err()
    );
    let plan = provider
        .scan(&context.state(), None, &[], None)
        .await
        .unwrap();
    assert!(plan.execute(1, context.task_ctx()).is_err());
    assert!(plan.clone().with_new_children(vec![plan.clone()]).is_err());
    assert!(
        FixedTableProvider::new(snapshot.clone(), QueryOwner::new(super::tests::budget())).is_err()
    );
    drop((plan, provider, context, snapshot));
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_during_scan_returns_original_error_once_and_cannot_resume() {
    let fx = Fixture::new(&[spec()]);
    let cancelled = Arc::new(AtomicBool::new(false));
    let budget = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        Some(cancelled.clone()),
    ));
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget.clone(), 1);
    let provider = FixedTableProvider::new(snapshot.clone(), owner).unwrap();
    let plan = provider
        .scan(&context.state(), None, &[], None)
        .await
        .unwrap();
    let mut stream = plan.execute(0, context.task_ctx()).unwrap();
    assert!(stream.next().await.unwrap().is_ok());
    cancelled.store(true, Ordering::Release);
    assert_eq!(
        error(stream.next().await.unwrap().unwrap_err()).code,
        "operation.cancelled"
    );
    cancelled.store(false, Ordering::Release);
    assert!(stream.next().await.is_none());
    drop((stream, plan, provider, context, snapshot));
    assert_eq!(budget.refundable_retained_bytes(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cloned_string_buffers_keep_import_admission_after_plan_context_and_snapshot_drop() {
    let fx = Fixture::new(&[spec()]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget.clone(), 1);
    let provider = FixedTableProvider::new(snapshot.clone(), owner).unwrap();
    let plan = provider
        .scan(&context.state(), Some(&vec![2]), &[], Some(1))
        .await
        .unwrap();
    let mut stream = plan.execute(0, context.task_ctx()).unwrap();
    let batch = stream.next().await.unwrap().unwrap();
    let survivor = batch.column(0).clone();
    drop((batch, stream, plan, provider, context, snapshot));
    assert!(budget.refundable_retained_bytes() > 0);
    assert_eq!(
        survivor
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap()
            .value(0),
        "中文🙂"
    );
    drop(survivor);
    assert_eq!(
        budget.refundable_retained_bytes(),
        0,
        "buffer owner has no Arc cycle"
    );
    fs::remove_dir_all(fx.root).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn zero_column_output_keeps_explicit_result_owner_until_consumed() {
    let fx = Fixture::new(&[spec()]);
    let budget = budget();
    let snapshot = prepare(&fx, &budget);
    let (context, owner) = register(&snapshot, budget.clone(), 1);
    let provider = FixedTableProvider::new(snapshot.clone(), owner.clone()).unwrap();
    let plan = provider
        .scan(&context.state(), Some(&vec![]), &[], None)
        .await
        .unwrap();
    let result = snapshot
        .collect_result(
            plan.execute(0, context.task_ctx()).unwrap(),
            owner.clone(),
            OutputLimits {
                rows: 16,
                batches: 16,
                bytes: 65536,
            },
        )
        .await
        .unwrap();
    drop((plan, provider, context, owner, snapshot));
    assert!(budget.refundable_retained_bytes() > 0);
    assert_eq!(
        result
            .records()
            .iter()
            .map(RecordBatch::num_rows)
            .sum::<usize>(),
        3
    );
    drop(result);
    assert_eq!(budget.refundable_retained_bytes(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn checked_close_failure_withholds_snapshot_and_releases_all_refundable_owners() {
    let fx = Fixture::new(&[spec()]);
    let inv = fx.invocation("data sql", &[]);
    let budget = budget();
    let mut reached = false;
    let err = FixedSnapshot::prepare_finishing(
        &inv,
        "Sample",
        SourceMode::Static,
        limits(),
        budget.clone(),
        |scope, result| {
            assert!(result.is_ok());
            scope.finish_after_metadata_close(result, &mut || {
                reached = true;
                Err(fault(
                    "test.checked_close",
                    5,
                    "Injected final close failure",
                ))
            })
        },
    )
    .unwrap_err();
    assert!(reached);
    assert_eq!(err.code, "test.checked_close");
    assert_eq!(budget.refundable_retained_bytes(), 0);
    fs::remove_dir_all(fx.root).unwrap();
}

#[test]
fn original_operator_pool_enforces_try_grow_releases_and_disables_spill() {
    let budget = budget();
    let (context, owner) = runtime::context(budget.clone(), 4096, 1).unwrap();
    let runtime = context.runtime_env();
    assert!(!runtime.disk_manager.tmp_files_enabled());
    let mut reservation =
        MemoryConsumer::new("provider-qualification").register(&runtime.memory_pool);
    reservation.try_grow(4096).unwrap();
    assert!(reservation.try_grow(1).is_err());
    assert_eq!(runtime.memory_pool.reserved(), 4096);
    drop((context, owner));
    assert!(budget.refundable_retained_bytes() >= 4096);
    drop(reservation);
    assert_eq!(runtime.memory_pool.reserved(), 0);
    drop(runtime);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn cancelled_or_unadmitted_operator_pool_does_not_grant_new_reservations() {
    let token = Arc::new(AtomicBool::new(false));
    let budget = Arc::new(Budget::new(
        Cost {
            retained_bytes: 32768,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(60),
        Some(token.clone()),
    ));
    assert!(runtime::context(budget.clone(), 65536, 1).is_err());
    assert_eq!(budget.refundable_retained_bytes(), 0);
    let (context, owner) = runtime::context(budget.clone(), 4096, 1).unwrap();
    let mut reservation =
        MemoryConsumer::new("cancelled-qualification").register(&context.runtime_env().memory_pool);
    token.store(true, Ordering::Release);
    assert_eq!(
        error(reservation.try_grow(1).unwrap_err()).code,
        "operation.cancelled"
    );
    assert_eq!(reservation.size(), 0);
    drop((reservation, context, owner));
    assert_eq!(budget.refundable_retained_bytes(), 0);
}
