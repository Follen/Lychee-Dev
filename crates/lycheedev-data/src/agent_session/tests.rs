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

#[test]
fn fields_and_batch_tasks_use_real_pinned_source() {
    let fx = fixture();
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let table = TableRequest {
        name: "AgentSample".into(),
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    };
    let fields = response(
        runtime
            .block_on(session.execute(
                Task::Fields {
                    table: table.clone(),
                },
                control(),
            ))
            .unwrap(),
    );
    assert!(
        fields["columns"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["name"] == "label")
    );
    let batch = response(
        runtime
            .block_on(session.execute(
                Task::Business {
                    request: tasks::BusinessTask::BatchIds {
                        table,
                        ids: vec![9, 9, 404],
                    },
                },
                control(),
            ))
            .unwrap(),
    );
    assert_eq!(batch["components"][0]["result"]["rows"], 1);
    assert!(batch["components"][0]["provenance"].is_object());
    session.close().unwrap();
}

#[test]
fn item_navigation_publishes_separate_guarded_components_without_invented_rows() {
    let fx = Fixture::new(&[
        fixture::table(
            "Item",
            931,
            &[("ID", "uint"), ("ClassID", "uint")],
            &[vec![Cell::U(7), Cell::U(2)]],
        ),
        fixture::table(
            "ItemSparse",
            932,
            &[("ID", "uint"), ("Display_lang", "text")],
            &[vec![Cell::U(7), Cell::Text("Exact item".into())]],
        ),
    ]);
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let value = response(
        runtime
            .block_on(session.execute(
                Task::Business {
                    request: tasks::BusinessTask::Navigate {
                        template: tasks::NavigationTemplate::ItemRecords,
                        ids: vec![7, 404],
                        mode: SourceMode::Static,
                        hotfix_captures: vec![],
                    },
                },
                control(),
            ))
            .unwrap(),
    );
    assert_eq!(value["components"].as_array().unwrap().len(), 2);
    for component in value["components"].as_array().unwrap() {
        assert_eq!(component["result"]["rows"], 1);
        let page = response(
            runtime
                .block_on(session.execute(
                    Task::Page {
                        handle: component["result"]["handle"].as_str().unwrap().into(),
                        offset: 0,
                        limit: 10,
                    },
                    control(),
                ))
                .unwrap(),
        );
        assert_eq!(page["rows"].as_array().unwrap().len(), 1);
    }
    session.close().unwrap();
}

#[test]
fn related_task_filters_exact_foreign_ids_and_rejects_missing_field() {
    let fx = Fixture::new(&[fixture::table(
        "SpellEffect",
        933,
        &[("ID", "uint"), ("SpellID", "uint")],
        &[
            vec![Cell::U(1), Cell::U(7)],
            vec![Cell::U(2), Cell::U(8)],
            vec![Cell::U(3), Cell::U(7)],
        ],
    )]);
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let table = TableRequest {
        name: "SpellEffect".into(),
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    };
    let value = response(
        runtime
            .block_on(session.execute(
                Task::Business {
                    request: tasks::BusinessTask::Related {
                        table: table.clone(),
                        field: "SpellID".into(),
                        ids: vec![7],
                    },
                },
                control(),
            ))
            .unwrap(),
    );
    assert_eq!(value["components"][0]["result"]["rows"], 2);
    assert!(
        runtime
            .block_on(session.execute(
                Task::Business {
                    request: tasks::BusinessTask::Related {
                        table,
                        field: "Invented".into(),
                        ids: vec![7]
                    }
                },
                control()
            ))
            .is_err()
    );
    session.close().unwrap();
}

#[test]
fn multi_component_capacity_failure_rolls_back_unpublished_handles() {
    let fx = Fixture::new(&[
        fixture::table("Item", 934, &[("ID", "uint")], &[vec![Cell::U(7)]]),
        fixture::table("ItemSparse", 935, &[("ID", "uint")], &[vec![Cell::U(7)]]),
    ]);
    let mut invocation = fx.invocation("data agent serve", &[]);
    invocation.flags.insert("--max-objects".into(), "1".into());
    let mut session = Session::new(invocation).unwrap();
    let runtime = runtime();
    assert!(
        runtime
            .block_on(session.execute(
                Task::Business {
                    request: tasks::BusinessTask::Navigate {
                        template: tasks::NavigationTemplate::ItemRecords,
                        ids: vec![7],
                        mode: SourceMode::Static,
                        hotfix_captures: vec![]
                    }
                },
                control()
            ))
            .is_err()
    );
    let value = response(
        runtime
            .block_on(session.execute(
                Task::Business {
                    request: tasks::BusinessTask::BatchIds {
                        table: TableRequest {
                            name: "Item".into(),
                            mode: SourceMode::Static,
                            hotfix_captures: vec![],
                        },
                        ids: vec![7],
                    },
                },
                control(),
            ))
            .unwrap(),
    );
    assert_eq!(value["components"][0]["result"]["rows"], 1);
    session.close().unwrap();
}

#[test]
fn catalog_is_pinned_manifest_inventory_not_table_readability() {
    let fx = fixture();
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let catalog = response(
        runtime
            .block_on(session.execute(Task::Catalog, control()))
            .unwrap(),
    );
    assert_eq!(catalog["complete"], true);
    let tables = catalog["tables"].as_array().unwrap();
    let table = tables
        .iter()
        .find(|table| table["name"] == "AgentSample")
        .unwrap();
    assert_eq!(table["availability"], "not_checked");
    assert!(
        catalog["snapshotId"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert!(
        catalog["definitionCommit"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    session.close().unwrap();
}

#[test]
fn second_component_source_failure_preserves_preexisting_handle() {
    let fx = Fixture::new(&[fixture::table(
        "Item",
        936,
        &[("ID", "uint")],
        &[vec![Cell::U(7)]],
    )]);
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let first = response(
        runtime
            .block_on(session.execute(
                Task::Business {
                    request: tasks::BusinessTask::BatchIds {
                        table: TableRequest {
                            name: "Item".into(),
                            mode: SourceMode::Static,
                            hotfix_captures: vec![],
                        },
                        ids: vec![7],
                    },
                },
                control(),
            ))
            .unwrap(),
    );
    let handle = first["components"][0]["result"]["handle"]
        .as_str()
        .unwrap()
        .to_owned();
    let error = runtime
        .block_on(session.execute(
            Task::Business {
                request: tasks::BusinessTask::Navigate {
                    template: tasks::NavigationTemplate::ItemRecords,
                    ids: vec![7],
                    mode: SourceMode::Static,
                    hotfix_captures: vec![],
                },
            },
            control(),
        ))
        .err()
        .expect("the second ItemSparse component must fail");
    assert_eq!(error.code, "schema.definition_not_found");
    let page = response(
        runtime
            .block_on(session.execute(
                Task::Page {
                    handle,
                    offset: 0,
                    limit: 10,
                },
                control(),
            ))
            .unwrap(),
    );
    assert_eq!(page["rows"].as_array().unwrap().len(), 1);
    session.close().unwrap();
}

#[test]
fn explicit_capture_persists_result_without_consuming_handle() {
    let fx = fixture();
    let mut session = Session::new(fx.invocation("data agent serve", &[])).unwrap();
    let runtime = runtime();
    let result = response(
        runtime
            .block_on(session.execute(Task::Query { query: request() }, control()))
            .unwrap(),
    );
    let handle = result["result"]["handle"].as_str().unwrap().to_owned();
    let cancelled = control();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    let cancelled_error = runtime
        .block_on(session.execute(
            Task::Capture {
                handle: handle.clone(),
            },
            cancelled,
        ))
        .err()
        .expect("pre-admission capture cancellation");
    assert_eq!(cancelled_error.code, "command.cancelled");
    let capture = response(
        runtime
            .block_on(session.execute(
                Task::Capture {
                    handle: handle.clone(),
                },
                control(),
            ))
            .unwrap(),
    );
    assert!(
        capture["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("CAP-"))
    );
    let capture_id = capture["id"].as_str().unwrap();
    assert_eq!(
        lycheedev_core::evidence::read_capture(&fx.home, capture_id).unwrap(),
        capture
    );
    let bytes = lycheedev_core::evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["schema"], "lycheedev.data-agent-result.v1");
    assert_eq!(body["rowCount"], 2);
    assert_eq!(body["rows"].as_array().unwrap().len(), 2);
    assert!(
        body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row[1].to_string() == "9007199254740993")
    );
    assert!(
        body["rows"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row[1].to_string() == "-9223372036854775808")
    );
    assert_eq!(capture["complete"], body["complete"]);
    assert_eq!(capture["truncated"], false);
    let mut verify = fx.invocation("evidence verify", &[]);
    verify.flags.clear();
    verify.positional = Some(capture_id.into());
    assert_eq!(
        lycheedev_core::evidence::dispatch(&verify).unwrap().data["verified"],
        true
    );
    let page = response(
        runtime
            .block_on(session.execute(
                Task::Page {
                    handle,
                    offset: 0,
                    limit: 10,
                },
                control(),
            ))
            .unwrap(),
    );
    assert_eq!(page["rows"].as_array().unwrap().len(), 2);
    session.close().unwrap();
}
