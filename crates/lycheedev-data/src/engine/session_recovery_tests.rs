//! Declaration-only recovery witnesses. No test/compile execution was authorized.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell,Fixture};
fn setup()->(Fixture,Engine){
    let fx=Fixture::new(&[fixture::table("Sample",921,&[("ID","uint"),("Value","signed64")],&[
        vec![Cell::U(1),Cell::I64(9_007_199_254_740_993)],vec![Cell::U(2),Cell::I64(-7)],vec![Cell::U(3),Cell::I64(i64::MAX)]])]);
    let engine=Engine::new(fx.invocation("data agent serve",&[]),EngineLimits::default()).unwrap();
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
