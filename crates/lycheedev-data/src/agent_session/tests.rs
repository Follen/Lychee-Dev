//! Actual session -> authenticated source -> unified engine witnesses. These
//! are declarations, not execution evidence until the exact candidate runs.
use super::*;
use crate::engine::{QueryPlan, SourceMode};
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};

fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn request() -> QueryRequest {
    QueryRequest::Plan {
        plan: QueryPlan::Lookup {
            table: TableRequest {
                name: "AgentSample".into(),
                mode: SourceMode::Static,
                hotfix_captures: vec![],
            },
            ids: vec![9, 1],
        },
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn fixture() -> Fixture {
    Fixture::new(&[fixture::table(
        "AgentSample",
        921,
        &[("ID", "uint"), ("value", "signed64"), ("label", "text")],
        &[
            vec![
                Cell::U(9),
                Cell::I64(i64::MIN),
                Cell::Text("low,\"quoted\"".into()),
            ],
            vec![
                Cell::U(1),
                Cell::I64(9_007_199_254_740_993),
                Cell::Text("中文🙂".into()),
            ],
        ],
    )])
}
fn response(value: Response) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn wire_parses_typed_tasks_and_rejects_unknown_fields_without_sql() {
    let valid = json!({"protocol":PROTOCOL,"id":1,"task":"query","query":{"kind":"plan","plan":{"op":"lookup","table":{"name":"AgentSample","mode":"static"},"ids":[9,1]}}});
    assert!(serde_json::from_value::<Request>(valid.clone()).is_ok());
    let mut unknown = valid;
    unknown["invented"] = json!(true);
    assert!(serde_json::from_value::<Request>(unknown).is_err());
    assert!(
        serde_json::from_value::<Request>(
            json!({"protocol":PROTOCOL,"id":2,"task":"cancel","request_id":1})
        )
        .is_ok()
    );
}
#[test]
fn real_session_reuses_engine_keeps_exact_rows_and_invalidates_released_handles() {
    let fx = fixture();
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let first = response(
        runtime
            .block_on(session.execute(Task::Query { query: request() }, control()))
            .unwrap(),
    );
    let handle = first["result"]["handle"].as_str().unwrap().to_owned();
    assert_eq!(first["result"]["rows"], 2);
    let page = response(
        runtime
            .block_on(session.execute(
                Task::Page {
                    handle: handle.clone(),
                    offset: 0,
                    limit: 10,
                },
                control(),
            ))
            .unwrap(),
    );
    let rows = page["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .any(|row| row[1].to_string() == "9007199254740993")
    );
    assert!(
        rows.iter()
            .any(|row| row[1].to_string() == "-9223372036854775808")
    );
    let csv = response(
        runtime
            .block_on(session.execute(
                Task::Export {
                    handle: handle.clone(),
                    offset: 0,
                    limit: 10,
                    format: ExportFormat::Csv,
                },
                control(),
            ))
            .unwrap(),
    );
    assert!(
        csv["text"]
            .as_str()
            .unwrap()
            .contains("\"low,\"\"quoted\"\"\"")
    );
    assert!(csv["text"].as_str().unwrap().contains("9007199254740993"));
    let second = response(
        runtime
            .block_on(session.execute(Task::Query { query: request() }, control()))
            .unwrap(),
    );
    assert_ne!(first["result"]["handle"], second["result"]["handle"]);
    runtime
        .block_on(session.execute(
            Task::Release {
                handle: handle.clone(),
            },
            control(),
        ))
        .unwrap();
    let error = runtime
        .block_on(session.execute(
            Task::Page {
                handle,
                offset: 0,
                limit: 1,
            },
            control(),
        ))
        .err()
        .unwrap();
    assert_eq!(error.code, "data.session.result_missing");
    session.close().unwrap();
}
#[test]
fn pre_admission_cancelled_library_call_does_not_execute_or_poison_engine() {
    let fx = fixture();
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let cancelled = control();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    let error = runtime
        .block_on(session.execute(Task::Query { query: request() }, cancelled))
        .err()
        .unwrap();
    assert_eq!(error.code, "command.cancelled");
    let complete = response(
        runtime
            .block_on(session.execute(Task::Query { query: request() }, control()))
            .unwrap(),
    );
    assert_eq!(complete["result"]["rows"], 2);
    session.close().unwrap();
}
