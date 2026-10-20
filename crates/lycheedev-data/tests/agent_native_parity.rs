//! New-architecture adapter witnesses. Historical Node fixtures and reports are
//! deliberately unchanged. This fixture seeds private Data metadata through
//! the production API, then compares task-native output with the migration
//! oracle on the same authenticated CASC/DBD/WDC input. Not performance evidence.
//! Cooperative library cancellation is distinct from production transport hard
//! cancellation, which retires the entire worker epoch and all its handles.
#[path = "support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::{Cell, Fixture, table};
use lycheedev_core::{
    control::{OperationControl, TrackedCancellation},
    metadata::Metadata,
};
use lycheedev_data::agent_session::{Request, Session};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(60))
}
fn fixture() -> Fixture {
    Fixture::new(&[table(
        "NativeSample",
        937,
        &[("ID", "uint"), ("Value", "signed64"), ("A", "array3")],
        &[
            vec![
                Cell::U(2),
                Cell::I64(i64::MIN),
                Cell::Array(vec![Cell::U(4), Cell::U(5), Cell::U(6)]),
            ],
            vec![
                Cell::U(1),
                Cell::I64(9_007_199_254_740_993),
                Cell::Array(vec![Cell::U(1), Cell::U(2), Cell::U(3)]),
            ],
        ],
    )])
}
fn query() -> Value {
    json!({"task":"query", "query":{"kind":"plan", "plan":{"op":"sort", "keys":[{"expression":{"kind":"column","name":"ID"},"descending":false,"nulls_first":false}],"input":{"op":"scan","table":{"name":"NativeSample","mode":"static"}}}}})
}
async fn execute(
    session: &mut Session,
    mut task: Value,
    control: OperationControl,
) -> lycheedev_core::Result<Value> {
    task["protocol"] = json!(lycheedev_data::agent_session::PROTOCOL);
    task["id"] = json!(1);
    let request: Request = serde_json::from_value(task).unwrap();
    // Serialize the guarded response directly; to_value here is a test-only
    // inspection after serialization, never the production response builder.
    let response = session.execute(request.task, control).await?;
    let wire = serde_json::to_vec(&response).unwrap();
    Ok(serde_json::from_slice(&wire).unwrap())
}

#[tokio::test(flavor = "current_thread")]
async fn private_metadata_fixture_drives_typed_session_with_oracle_exact_rows() {
    let fx = fixture();
    assert!(fx.home.join("state/data/cache-v1.sqlite").is_file());
    let metadata = Metadata::open(&fx.home, false).unwrap();
    assert!(
        metadata
            .list::<Value>("definition/", 100)
            .unwrap()
            .is_empty()
    );
    drop(metadata);
    let oracle = lycheedev_data::migration_oracle::dispatch(&fx.invocation(
        "data sql",
        &[(
            "--sql",
            r#"SELECT ID, Value, "A[0]", "A[1]", "A[2]" FROM NativeSample ORDER BY ID"#,
        )],
    ))
    .unwrap();
    assert_eq!(oracle.data["complete"], true);
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let receipt = execute(&mut session, query(), control()).await.unwrap();
    assert_eq!(receipt["complete"], true);
    let handle = receipt["result"]["handle"].as_str().unwrap();
    let page = execute(
        &mut session,
        json!({"task":"page","handle":handle,"offset":0,"limit":10}),
        control(),
    )
    .await
    .unwrap();
    // The migration SQL interface exposes physical array slots as columns;
    // the typed interface preserves the array. Authenticate the exact legacy
    // shape before rebuilding only that declared fixed-width array for parity.
    assert_eq!(
        oracle.data["result"]["columns"],
        json!(["ID", "Value", "A[0]", "A[1]", "A[2]"])
    );
    assert_eq!(page["columns"], json!(["ID", "Value", "A"]));
    let canonical_oracle_rows: Vec<Value> = oracle.data["result"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let cells = row.as_array().unwrap();
            assert_eq!(cells.len(), 5);
            json!([cells[0], cells[1], [cells[2], cells[3], cells[4]]])
        })
        .collect();
    assert_eq!(page["rows"], json!(canonical_oracle_rows));
    assert_eq!(page["rows"][0][1].to_string(), "9007199254740993");
    assert_eq!(page["rows"][1][1].to_string(), "-9223372036854775808");
    assert_eq!(page["rows"][0][2], json!([1, 2, 3]));
    assert_eq!(page["rows"][1][2], json!([4, 5, 6]));
    execute(
        &mut session,
        json!({"task":"release","handle":handle}),
        control(),
    )
    .await
    .unwrap();
    assert!(
        execute(
            &mut session,
            json!({"task":"page","handle":handle,"offset":0,"limit":1}),
            control()
        )
        .await
        .is_err()
    );
    session.close().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cooperative_library_cancel_does_not_poison_next_request() {
    let fx = fixture();
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let token = TrackedCancellation::new();
    let cancelled =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), token.clone());
    token.cancel();
    assert!(execute(&mut session, query(), cancelled).await.is_err());
    let first = execute(&mut session, query(), control()).await.unwrap();
    execute(
        &mut session,
        json!({"task":"release","handle":first["result"]["handle"]}),
        control(),
    )
    .await
    .unwrap();
    let second = execute(&mut session, query(), control()).await.unwrap();
    assert_eq!(second["result"]["rows"], 2);
    assert_ne!(first["result"]["handle"], second["result"]["handle"]);
    session.close().unwrap();
}
