//! Session recovery and exact native result witnesses using owned WDC inputs.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell,Fixture};
fn setup()->(Fixture,Engine){
    let fx=Fixture::new(&[fixture::table("Sample",921,&[("ID","uint"),("Value","signed64")],&[
        vec![Cell::U(1),Cell::I64(9_007_199_254_740_993)],vec![Cell::U(2),Cell::I64(-7)],vec![Cell::U(3),Cell::I64(i64::MAX)]])]);
    let engine=Engine::new(fx.context(),EngineLimits::default()).unwrap();
    (fx,engine)
}
fn table()->TableRequest{TableRequest{name:"Sample".into(),mode:SourceMode::Static,hotfix_captures:vec![]}}
fn query(ids:Vec<u32>)->QueryRequest{QueryRequest::Plan{plan:QueryPlan::Lookup{table:table(),ids}}}
#[tokio::test(flavor="current_thread")]
async fn partial_lookup_shard_reuses_native_rows_but_never_claims_unvisited_ids(){
    let(_fx,mut engine)=setup();
    let first=engine.execute(query(vec![1]),OperationControl::default()).await.unwrap();
    assert_eq!(first.row_count(),1);drop(first);
    let hit=engine.execute(query(vec![1]),OperationControl::default()).await.unwrap();
    assert_eq!(hit.provenance()["sources"][0]["sessionSnapshot"]["reused"],true);
    assert_eq!(hit.provenance()["sources"][0]["sessionSnapshot"]["coverage"],"resolved_physical_id_set");
    let page=serde_json::to_value(hit.page(0,10).unwrap()).unwrap();
    assert_eq!(page["rows"][0][1].as_i64(),Some(9_007_199_254_740_993));drop(hit);
    let other=engine.execute(query(vec![3]),OperationControl::default()).await.unwrap();
    assert_eq!(other.row_count(),1);
    assert_eq!(other.provenance()["sources"][0]["sessionSnapshot"]["reused"],false);
}
#[tokio::test(flavor="current_thread")]
async fn full_source_request_cannot_reuse_partial_lookup_coverage(){
    let(_fx,mut engine)=setup();drop(engine.execute(query(vec![1]),OperationControl::default()).await.unwrap());
    let all=engine.execute(QueryRequest::Plan{plan:QueryPlan::Scan{table:table()}},OperationControl::default()).await.unwrap();
    assert_eq!(all.row_count(),3);assert_eq!(all.provenance()["sources"][0]["sessionSnapshot"]["reused"],false);drop(all);
    let hit=engine.execute(QueryRequest::Plan{plan:QueryPlan::Scan{table:table()}},OperationControl::default()).await.unwrap();
    assert_eq!(hit.row_count(),3);assert_eq!(hit.provenance()["sources"][0]["sessionSnapshot"]["reused"],true);
}
#[tokio::test(flavor="current_thread")]
async fn finite_domain_scan_cache_binds_filter_and_limit_and_keeps_extra(){
    let(_fx,mut engine)=setup();
    let first=engine.scan_domain_records(table(),None,1,OperationControl::default()).await.unwrap();
    assert_eq!(first.row_count(),1);assert!(first.extra());drop(first);
    let hit=engine.scan_domain_records(table(),None,1,OperationControl::default()).await.unwrap();
    assert!(hit.extra());assert_eq!(hit.provenance()["sources"][0]["sessionSnapshot"]["reused"],true);drop(hit);
    let wider=engine.scan_domain_records(table(),None,3,OperationControl::default()).await.unwrap();
    assert_eq!(wider.row_count(),3);assert!(!wider.extra());
    assert_eq!(wider.provenance()["sources"][0]["sessionSnapshot"]["reused"],false);
}
#[tokio::test(flavor="current_thread")]
async fn cache_and_result_owners_release_real_admission_on_close(){
    let(_fx,mut engine)=setup();
    let result=engine.execute(query(vec![1]),OperationControl::default()).await.unwrap();
    assert!(engine.memory.budget.snapshot().unwrap().retained_bytes>0);
    engine.close().unwrap();assert!(result.page(0,1).is_ok());drop(result);
    assert_eq!(engine.memory.budget.snapshot().unwrap().retained_bytes,0);
}
#[tokio::test(flavor="current_thread")]
async fn nonanalyzing_explain_resources_observe_post_source_release_ledger(){
    let(_fx,mut engine)=setup();
    let result=engine.explain(QueryRequest::Plan{plan:QueryPlan::Scan{table:table()}},false,false,OperationControl::default()).await.unwrap();
    let current=serde_json::to_value(result._lifetimes[0].budget.snapshot().unwrap()).unwrap();
    assert_eq!(result.provenance()["resources"]["requestSafetyLedger"],current);
    assert_eq!(result.provenance()["resources"]["sessionRetainedBytes"].as_u64(),Some(engine.memory.budget.snapshot().unwrap().retained_bytes));
}

#[tokio::test(flavor = "current_thread")]
async fn analyze_executes_cold_lookup_once_and_preserves_reusable_exact_source() {
    let (_fx, mut engine) = setup();
    let explain = engine
        .explain(query(vec![1]), false, false, OperationControl::default())
        .await
        .unwrap();
    assert_eq!(
        explain.provenance()["analysis"]["analysisScope"],
        "expanded_native_plan_without_execution"
    );
    assert_eq!(engine.cached_source_count(), 0);
    drop(explain);

    let analyzed = engine
        .explain(query(vec![1]), false, true, OperationControl::default())
        .await
        .unwrap();
    assert_eq!(analyzed.provenance()["analysis"]["analysisScope"], "complete_native_root");
    assert_eq!(
        analyzed.provenance()["analysis"]["nativePreflightAndPrerequisites"]["hasPrerequisites"],
        false
    );
    assert_eq!(analyzed.provenance()["sources"][0]["sessionSnapshot"]["reused"], false);
    assert!(analyzed.row_count() > 0);
    let plan = serde_json::to_value(analyzed.page(0, 10).unwrap()).unwrap();
    assert_eq!(plan["columns"], serde_json::json!(["plan_type", "plan"]));
    // The cold provider records consumed physical IDs. A second execution of
    // this provider would record [1, 1], which cannot promote an exact shard.
    assert_eq!(engine.cached_source_count(), 1);
    drop(analyzed);

    let result = engine.execute(query(vec![1]), OperationControl::default()).await.unwrap();
    assert_eq!(result.provenance()["sources"][0]["sessionSnapshot"]["reused"], true);
    let page = serde_json::to_value(result.page(0, 10).unwrap()).unwrap();
    assert_eq!(page["rows"], serde_json::json!([[1, 9_007_199_254_740_993u64]]));
    drop(result);
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn execute_admits_optimized_output_schema_without_rounding_wide_values() {
    let (_fx, mut engine) = setup();
    let request: QueryRequest = serde_json::from_value(serde_json::json!({
        "kind":"plan",
        "plan":{"op":"project","input":{"op":"scan","table":{"name":"Sample","mode":"static"}},
            "columns":[{"name":"exact","expression":{"kind":"null_if",
                "left":{"kind":"literal","value":9_007_199_254_740_993u64},
                "right":{"kind":"literal","value":9_007_199_254_740_992.0}}}]}
    })).unwrap();
    let result = engine.execute(request, OperationControl::default()).await.unwrap();
    assert_eq!(result.row_count(), 3);
    result.visit_batches(&OperationControl::default(), |schema, batches| {
        assert_eq!(schema.field(0).name(), "exact");
        assert_eq!(schema.field(0).data_type(), &arrow_schema::DataType::Int64);
        for batch in batches {
            assert_eq!(&batch.schema(), schema);
        }
        Ok(())
    }).unwrap();
    let page = serde_json::to_value(result.page(0, 10).unwrap()).unwrap();
    assert_eq!(page["columns"], serde_json::json!(["exact"]));
    assert_eq!(page["rows"], serde_json::json!([
        [9_007_199_254_740_993u64], [9_007_199_254_740_993u64], [9_007_199_254_740_993u64]
    ]));
}

#[tokio::test(flavor = "current_thread")]
async fn execute_eager_boolean_preserves_right_type_errors() {
    let (_fx, mut engine) = setup();
    for (operator, left) in [("and", false), ("or", true)] {
        let request: QueryRequest = serde_json::from_value(serde_json::json!({
            "kind":"plan",
            "plan":{"op":"project","input":{"op":"scan","table":{"name":"Sample","mode":"static"}},
                "columns":[{"name":"value","expression":{"kind":"binary","operator":operator,
                    "left":{"kind":"literal","value":left},
                    "right":{"kind":"binary","operator":"equal",
                        "left":{"kind":"literal","value":"1"},
                        "right":{"kind":"literal","value":1}}}}]}
        })).unwrap();
        let error = engine.execute(request, OperationControl::default()).await.err().unwrap();
        assert_eq!(error.code, "relational.type_mismatch");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_arrow_sort_counts_each_native_buffer_once() {
    // One modest IPC batch, deliberately large enough that accounting for its
    // full backing string buffer for every single-row slice exceeds the real
    // production operator pool. The actual table and sorted output are ~1 MiB.
    let rows = (1..=512u32)
        .map(|id| {
            vec![
                Cell::U(id),
                Cell::I64(9_007_199_254_740_993),
                Cell::Text(format!("{id:04}:{}", "x".repeat(2048))),
            ]
        })
        .collect::<Vec<_>>();
    let fx = Fixture::new(&[fixture::table(
        "PreparedSort",
        923,
        &[("ID", "uint"), ("Exact", "signed64"), ("Label", "text")],
        &rows,
    )]);
    let context = fx.context();
    let prepare = crate::request::CachePrepareRequest { table: "PreparedSort".into(), timeout: std::time::Duration::from_secs(300) };
    assert!(crate::prepare_cache(&context, &prepare).unwrap().complete);
    let mut engine = Engine::new(
        fx.context(),
        EngineLimits::default(),
    )
    .unwrap();
    let request = QueryRequest::Plan {
        plan: QueryPlan::Sort {
            input: Box::new(QueryPlan::Scan {
                table: TableRequest {
                    name: "PreparedSort".into(),
                    mode: SourceMode::Static,
                    hotfix_captures: vec![],
                },
            }),
            keys: vec![SortKey {
                expression: Expression::Column { name: "ID".into() },
                descending: true,
                nulls_first: false,
            }],
        },
    };
    let result = engine
        .execute(request, OperationControl::default())
        .await
        .unwrap();
    assert_eq!(
        result.provenance()["sources"][0]["typedGenerationCache"]["representationSelection"],
        "authenticated_arrow_session_snapshot"
    );
    assert_eq!(result.row_count(), rows.len());
    let page = serde_json::to_value(result.page(0, 512).unwrap()).unwrap();
    for (offset, row) in page["rows"].as_array().unwrap().iter().enumerate() {
        let id = 512 - offset;
        assert_eq!(
            row,
            &serde_json::json!([
                id,
                9_007_199_254_740_993u64,
                format!("{id:04}:{}", "x".repeat(2048))
            ])
        );
    }
    drop(page);
    drop(result);
    // A sparse lookup must gather native arrays, rather than returning 256
    // separate slices of that same 1 MiB input buffer to the sort operator.
    let selected = (1..=512u32).step_by(2).chain([999]).collect::<Vec<_>>();
    let lookup: QueryRequest = serde_json::from_value(serde_json::json!({"kind":"plan","plan":{
        "op":"sort","input":{"op":"lookup","table":{"name":"PreparedSort","mode":"static"},"ids":selected},
        "keys":[{"expression":{"kind":"column","name":"ID"},"descending":true,"nulls_first":false}]}})).unwrap();
    let result = engine
        .execute(lookup, OperationControl::default())
        .await
        .unwrap();
    assert_eq!(result.row_count(), 256);
    assert_eq!(
        result.provenance()["lookupRequirements"][0]["missingIds"],
        serde_json::json!([999])
    );
    let page = serde_json::to_value(result.page(0, 512).unwrap()).unwrap();
    for (offset, row) in page["rows"].as_array().unwrap().iter().enumerate() {
        let id = 511 - offset * 2;
        assert_eq!(
            row,
            &serde_json::json!([
                id,
                9_007_199_254_740_993u64,
                format!("{id:04}:{}", "x".repeat(2048))
            ])
        );
    }
    drop(page);
    drop(result);
    let projected: QueryRequest = serde_json::from_value(serde_json::json!({"kind":"plan","plan":{
        "op":"limit","offset":0,"count":3,"input":{"op":"project","input":{"op":"scan","table":{"name":"PreparedSort","mode":"static"}},
        "columns":[{"name":"exact","expression":{"kind":"column","name":"Exact"}}]}}})).unwrap();
    let result = engine
        .execute(projected, OperationControl::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(result.page(0, 10).unwrap()).unwrap()["rows"],
        serde_json::json!([
            [9_007_199_254_740_993u64],
            [9_007_199_254_740_993u64],
            [9_007_199_254_740_993u64]
        ])
    );
    drop(result);
    let table = TableRequest {
        name: "PreparedSort".into(),
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    };
    let domain = engine
        .scan_domain_records(table.clone(), None, 2, OperationControl::default())
        .await
        .unwrap();
    assert_eq!(domain.row_count(), 2);
    assert!(domain.extra());
    domain
        .visit_records(&OperationControl::default(), |_, _, ids| {
            assert_eq!(
                ids.iter()
                    .flat_map(|a| a.values().iter().copied())
                    .collect::<Vec<_>>(),
                vec![1, 2]
            );
            Ok(())
        })
        .unwrap();
    drop(domain);
    let filtered = engine
        .scan_domain_records(
            table,
            Some(("ID".into(), 512)),
            1,
            OperationControl::default(),
        )
        .await
        .unwrap();
    assert_eq!(filtered.row_count(), 1);
    assert!(!filtered.extra());
    filtered
        .visit_records(&OperationControl::default(), |_, _, ids| {
            assert_eq!(
                ids.iter()
                    .flat_map(|a| a.values().iter().copied())
                    .collect::<Vec<_>>(),
                vec![512]
            );
            Ok(())
        })
        .unwrap();
    drop(filtered);
    engine.close().unwrap();
    assert_eq!(engine.memory.budget.snapshot().unwrap().retained_bytes, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn prepared_arrow_public_lookup_observes_every_batch_row() {
    let mut spec = fixture::table(
        "PublicIds",
        924,
        &[("Locator", "uint"), ("ID", "uint"), ("Label", "text")],
        &[
            vec![Cell::U(1), Cell::U(80), Cell::Text("first".into())],
            vec![Cell::U(2), Cell::U(90), Cell::Text("second".into())],
            vec![Cell::U(3), Cell::U(100), Cell::Text("third".into())],
        ],
    );
    spec.dbd = spec
        .dbd
        .replace("$id$ID<u32>", "ID<u32>")
        .replace("\nLocator<u32>", "\n$id$Locator<u32>");
    let fx = Fixture::new(&[spec]);
    let context = fx.context();
    let prepare = crate::request::CachePrepareRequest { table: "PublicIds".into(), timeout: std::time::Duration::from_secs(300) };
    crate::prepare_cache(&context, &prepare).unwrap();
    let mut engine = Engine::new(
        fx.context(),
        EngineLimits::default(),
    )
    .unwrap();
    let table = TableRequest {
        name: "PublicIds".into(),
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    };
    // First acquire the complete authenticated Arrow snapshot. A direct Lookup
    // alone deliberately retains the original WDC demand boundary.
    drop(
        engine
            .execute(
                QueryRequest::Plan {
                    plan: QueryPlan::Scan {
                        table: table.clone(),
                    },
                },
                OperationControl::default(),
            )
            .await
            .unwrap(),
    );
    let result = engine
        .execute(
            QueryRequest::Plan {
                plan: QueryPlan::Lookup {
                    table,
                    ids: vec![90, 100, 999],
                },
            },
            OperationControl::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.row_count(), 2);
    assert_eq!(
        serde_json::to_value(result.page(0, 10).unwrap()).unwrap()["rows"],
        serde_json::json!([[2, 90, "second"], [3, 100, "third"]])
    );
    let proof = &result.provenance()["lookupRequirements"][0];
    assert_eq!(proof["identityDomain"], "public_id_not_physical_locator");
    assert_eq!(proof["missingIds"], serde_json::json!([999]));
    assert_eq!(proof["unresolvedIds"], serde_json::json!([]));
    engine.close().unwrap();
    drop(result);
    assert_eq!(engine.memory.budget.snapshot().unwrap().retained_bytes, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn wdc_filter_project_limit_does_not_visit_the_next_bad_row() {
    let fx = Fixture::new(&[fixture::table(
        "LazyRows",
        925,
        &[("ID", "uint"), ("Label", "text")],
        &[
            vec![Cell::U(1), Cell::Text("skip".into())],
            vec![Cell::U(2), Cell::Text("keep".into())],
            vec![Cell::U(3), Cell::BadString],
        ],
    )]);
    let mut engine = Engine::new(
        fx.context(),
        EngineLimits::default(),
    )
    .unwrap();
    let limited: QueryRequest = serde_json::from_value(serde_json::json!({"kind":"plan","plan":{
        "op":"limit","offset":0,"count":1,"input":{"op":"project","columns":[{"name":"ID","expression":{"kind":"column","name":"ID"}}],
        "input":{"op":"filter","input":{"op":"scan","table":{"name":"LazyRows","mode":"static"}},
            "predicate":{"kind":"binary","operator":"equal","left":{"kind":"column","name":"ID"},"right":{"kind":"literal","value":2}}}}}})).unwrap();
    let result = engine
        .execute(limited, OperationControl::default())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(result.page(0, 10).unwrap()).unwrap()["rows"],
        serde_json::json!([[2]])
    );
    drop(result);
    let error = engine
        .execute(
            QueryRequest::Plan {
                plan: QueryPlan::Scan {
                    table: TableRequest {
                        name: "LazyRows".into(),
                        mode: SourceMode::Static,
                        hotfix_captures: vec![],
                    },
                },
            },
            OperationControl::default(),
        )
        .await
        .err()
        .unwrap();
    assert_eq!(error.code, "table.invalid_format");
    engine.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn full_numeric_sort_keeps_all_16384_rows_with_default_memory() {
    let input: Vec<_> = (0..16_384u32).map(|index| {
        let value = index as f32 + 1.5;
        vec![Cell::U(index + 1), Cell::F(value), Cell::F(-value), Cell::F(0.25), Cell::F(-0.25)]
    }).collect();
    let fx = Fixture::new(&[fixture::table("NumericSample", 926,
        &[("ID", "uint"), ("F0", "float"), ("F1", "float"), ("F2", "float"), ("F3", "float")], &input)]);
    let mut engine = Engine::new(fx.context(), EngineLimits::default()).unwrap();
    let request: QueryRequest = serde_json::from_value(serde_json::json!({"kind":"plan","plan":{
        "op":"sort","keys":[{"expression":{"kind":"column","name":"ID"},"descending":true,"nulls_first":false}],
        "input":{"op":"scan","table":{"name":"NumericSample","mode":"static"}}}})).unwrap();
    for reused in [false, true] {
        let result = engine.execute(request.clone(), OperationControl::default()).await.unwrap();
        assert_eq!(result.row_count(), input.len());
        assert!(result.complete());
        assert_eq!(result.provenance()["sources"][0]["sessionSnapshot"]["reused"], reused);
        for offset in (0..input.len()).step_by(1000) {
            let page = serde_json::to_value(result.page(offset, 1000).unwrap()).unwrap();
            for (position, row) in page["rows"].as_array().unwrap().iter().enumerate() {
                let index = input.len() - 1 - offset - position;
                let value = index as f64 + 1.5;
                assert_eq!(*row, serde_json::json!([index + 1, value, -value, 0.25, -0.25]));
            }
        }
        drop(result);
    }
    engine.close().unwrap();
    assert_eq!(engine.memory.budget.snapshot().unwrap().retained_bytes, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn wdc_projection_error_precedes_a_later_invalid_source_row() {
    let fx = Fixture::new(&[fixture::table("ExpressionRows", 927,
        &[("ID", "uint"), ("Label", "text")], &[
            vec![Cell::U(1), Cell::Text("first".into())], vec![Cell::U(2), Cell::BadString],
        ])]);
    let mut engine = Engine::new(fx.context(), EngineLimits::default()).unwrap();
    let request: QueryRequest = serde_json::from_value(serde_json::json!({"kind":"plan","plan":{
        "op":"project","columns":[{"name":"Overflow","expression":{"kind":"binary","operator":"add",
            "left":{"kind":"column","name":"ID"},"right":{"kind":"literal","value":u64::MAX}}}],
        "input":{"op":"scan","table":{"name":"ExpressionRows","mode":"static"}}}})).unwrap();
    let error = engine.execute(request, OperationControl::default()).await.err().unwrap();
    assert_eq!(error.code, "relational.numeric_range");
    engine.close().unwrap();
    assert_eq!(engine.memory.budget.snapshot().unwrap().retained_bytes, 0);
}
