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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
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
    let config = crate::request::SessionConfig { max_handles: 1, ..Default::default() };
    let mut session = Session::new(fx.context(), config).unwrap();
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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
    let runtime = runtime();
    let catalog = response(
        runtime
            .block_on(session.execute(Task::Catalog {}, control()))
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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
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
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
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
    assert_eq!(
        lycheedev_core::evidence::dispatch(&fx.core_context(), lycheedev_core::evidence::EvidenceRequest::Verify { id: capture_id.into() }).unwrap().data["verified"],
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

fn compound_handle_witness(query: QueryRequest, expected_rows: Value, expected_csv: &str) {
    use lycheedev_core::evidence;
    let fx = Fixture::new(&[fixture::table(
        "Sequence",
        926,
        &[("ID", "uint"), ("Exact", "signed64")],
        &[
            vec![Cell::U(1), Cell::I64(9_007_199_254_740_993)],
            vec![Cell::U(2), Cell::I64(-7)],
            vec![Cell::U(3), Cell::I64(i64::MAX)],
        ],
    )]);
    let mut context = fx.context();
    let casc_root = fx.client.parent().unwrap();
    context.source.installation = Some(casc_root.to_owned());
    let mut session = Session::new(context, Default::default()).unwrap();
    let runtime = runtime();
    let submitted = response(
        runtime
            .block_on(session.execute(
                Task::Query {
                    query: query.clone(),
                },
                control(),
            ))
            .unwrap(),
    );
    let handle = submitted["result"]["handle"].as_str().unwrap().to_owned();
    assert_eq!(submitted["result"]["rows"], 3);
    assert_eq!(submitted["complete"], true);
    assert_eq!(submitted["provenance"]["engine"], "datafusion");
    assert_eq!(
        submitted["provenance"]["sources"].as_array().unwrap().len(),
        1
    );

    // ResultOwner explicitly survives Engine::close. Evict the source snapshot
    // so even a native Arrow cache cannot make a rerun silently succeed, then
    // invalidate only this owned fixture's external installation catalog.
    session.engine.close().unwrap();
    assert_eq!(session.engine.cached_source_count(), 0);
    std::fs::write(casc_root.join(".build.info"), b"not a build catalog").unwrap();
    let fresh = runtime
        .block_on(session.execute(
            Task::Query {
                query: query.clone(),
            },
            control(),
        ))
        .err()
        .expect("a new execution must fail against the invalidated fixture");
    assert_eq!(fresh.code, "records.invalid_metadata");

    let first = response(
        runtime
            .block_on(session.execute(
                Task::Page {
                    handle: handle.clone(),
                    offset: 0,
                    limit: 2,
                },
                control(),
            ))
            .unwrap(),
    );
    let tail = response(
        runtime
            .block_on(session.execute(
                Task::Page {
                    handle: handle.clone(),
                    offset: 2,
                    limit: 2,
                },
                control(),
            ))
            .unwrap(),
    );
    assert_eq!(first["columns"], json!(["ID", "Exact"]));
    assert_eq!(tail["columns"], first["columns"]);
    assert_eq!(first["totalRows"], 3);
    assert_eq!(tail["totalRows"], 3);
    assert_eq!(first["complete"], true);
    assert_eq!(tail["complete"], true);
    let rows = first["rows"]
        .as_array()
        .unwrap()
        .iter()
        .chain(tail["rows"].as_array().unwrap())
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(Value::Array(rows), expected_rows);

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
    assert_eq!(csv["text"], expected_csv);
    assert_eq!(csv["page"]["rows"], expected_rows);
    assert_eq!(csv["page"]["complete"], true);
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
    assert_eq!(capture["complete"], true);
    assert_eq!(capture["truncated"], false);
    assert_eq!(
        evidence::read_capture(&fx.home, capture["id"].as_str().unwrap()).unwrap(),
        capture
    );
    let bytes = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["schema"], "lycheedev.data-agent-result.v1");
    assert_eq!(body["columns"], json!(["ID", "Exact"]));
    assert_eq!(body["rowCount"], 3);
    assert_eq!(body["rows"], expected_rows);
    assert_eq!(body["complete"], true);
    assert_eq!(body["provenance"], submitted["provenance"]);
    assert_eq!(
        body["provenance"]["request"],
        serde_json::to_value(query).unwrap()
    );
    let after_capture = response(
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
    assert_eq!(after_capture["rows"], expected_rows);
    session.close().unwrap();
}

#[test]
fn union_all_session_handle_preserves_order_duplicates_and_exact_values_without_rerun() {
    // The three branch bags are [row 1], [row 2], [row 1]. Neither sorting nor
    // deduplication is part of UnionAll; the wide integer must survive intact.
    let query = serde_json::from_value(json!({"kind":"plan","plan":{
        "op":"union_all","inputs":[
            {"op":"lookup","table":{"name":"Sequence","mode":"static"},"ids":[1]},
            {"op":"lookup","table":{"name":"Sequence","mode":"static"},"ids":[2]},
            {"op":"lookup","table":{"name":"Sequence","mode":"static"},"ids":[1]},
        ]
    }}))
    .unwrap();
    compound_handle_witness(
        query,
        json!([
            [1, 9_007_199_254_740_993u64],
            [2, -7],
            [1, 9_007_199_254_740_993u64]
        ]),
        "\"ID\",\"Exact\"\r\n\"1\",\"9007199254740993\"\r\n\"2\",\"-7\"\r\n\"1\",\"9007199254740993\"\r\n",
    );
}

#[test]
fn iterate_session_handle_preserves_previous_delta_order_and_exact_values_without_rerun() {
    // Starting at ID 1, only ID 2 matches ID-1 IN previous delta, then only ID
    // 3, then no row. Re-reading the accumulated bag would repeat prior rows.
    let query = serde_json::from_value(json!({"kind":"plan","plan":{
        "op":"iterate","binding":"r",
        "seed":{"op":"lookup","table":{"name":"Sequence","mode":"static"},"ids":[1]},
        "step":{"op":"filter","input":{"op":"scan","table":{"name":"Sequence","mode":"static"}},
            "predicate":{"kind":"in_query","value":{"kind":"binary","operator":"subtract",
                "left":{"kind":"column","name":"Sequence.ID"},"right":{"kind":"literal","value":1}},
                "input":{"op":"project","input":{"op":"delta","binding":"r"},
                    "columns":[{"name":"ID","expression":{"kind":"column","name":"ID"}}]}}}
    }}))
    .unwrap();
    compound_handle_witness(
        query,
        json!([[1, 9_007_199_254_740_993u64], [2, -7], [3, i64::MAX]]),
        "\"ID\",\"Exact\"\r\n\"1\",\"9007199254740993\"\r\n\"2\",\"-7\"\r\n\"3\",\"9223372036854775807\"\r\n",
    );
}

#[test]
fn local_session_rebuilds_missing_selected_body_without_changing_alias_owner_or_capture() {
    use lycheedev_core::{evidence, metadata::{DataCacheMetadata, Metadata}, storage,
        workspace::data_disk::DataCachePaths};

    let table = fixture::table("AgentRebuild", 942,
        &[("ID", "uint"), ("value", "signed64")],
        &[vec![Cell::U(7), Cell::I64(9_007_199_254_740_993)]]);
    let expected = table.raw.clone();
    let fx = Fixture::new(&[table]);
    let query = || Task::Query { query: QueryRequest::Plan { plan: QueryPlan::Lookup {
        table: TableRequest { name: "AgentRebuild".into(), mode: SourceMode::Static, hotfix_captures: vec![] },
        ids: vec![7],
    } } };
    let runtime = runtime();
    let mut session = Session::new(fx.context(), Default::default()).unwrap();
    let cold = response(runtime.block_on(session.execute(query(), control())).unwrap());
    assert_eq!(cold["complete"], true);
    let source = cold["provenance"]["sources"][0]["sourceIdentity"]["source"].clone();
    assert_eq!(source["origin"], "installation");
    assert_eq!(source["content"]["sha256"], storage::sha256(&expected));
    assert_eq!(source["content"]["bytes"], expected.len());
    let page = |session: &mut Session, handle: &Value| response(runtime.block_on(session.execute(
        Task::Page { handle: handle.as_str().unwrap().into(), offset: 0, limit: 10 }, control(),
    )).unwrap());
    let cold_page = page(&mut session, &cold["result"]["handle"]);
    assert_eq!(cold_page["rows"], json!([[7, 9_007_199_254_740_993_u64]]));
    let capture = response(runtime.block_on(session.execute(Task::Capture {
        handle: cold["result"]["handle"].as_str().unwrap().into(),
    }, control())).unwrap());
    let capture_id = capture["id"].as_str().unwrap();
    let capture_key = format!("capture/{capture_id}");
    let capture_document = || {
        let document = Metadata::read_document_raw_controlled(&fx.home, &capture_key, &Default::default()).unwrap();
        (document.generation, document.bytes)
    };
    let capture_before = capture_document();
    let capture_body = evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap();
    let captured: Value = serde_json::from_slice(&capture_body).unwrap();
    assert_eq!(captured["rows"], cold_page["rows"]);
    // The local alias binds the decoded CKey, key context and encoded object.
    let mut identity = format!("decoded-v1\0{}\0{}\0{}\0", source["contentKey"].as_str().unwrap(),
        source["keySource"]["kind"].as_str().unwrap(), source["keySource"]["sha256"].as_str().unwrap()).into_bytes();
    identity.extend_from_slice(b"BLTE\0\0\0\0N");
    identity.extend_from_slice(&expected);
    let alias_key = format!("decoded-v1/{}", storage::sha256(&identity));
    let alias_document = || {
        let document = DataCacheMetadata::read_document_raw_controlled_with_check(
            &fx.home, &alias_key, &Default::default(), || Ok(()),
        ).unwrap();
        (document.generation, document.bytes)
    };
    let alias_before = alias_document();
    let alias: Value = serde_json::from_slice(&alias_before.1).unwrap();
    assert_eq!(alias["blob"], source["content"]);
    let digest = source["content"]["sha256"].as_str().unwrap();
    let paths = DataCachePaths::new(&fx.home);
    let pair = || (
        std::fs::read(paths.shared_blobs().join(format!("{digest}.intent"))).unwrap(),
        std::fs::read(paths.shared_blobs().join(format!("{digest}.complete"))).unwrap(),
    );
    let owner_before = pair();
    let repair = paths.shared_blobs().join(format!("{digest}.repair.intent"));
    let body = evidence::blob_path(&fx.home, digest).unwrap();
    assert_eq!(std::fs::read(&body).unwrap(), expected);
    session.close().unwrap();
    // Only the reconstructible canonical selected body is removed, after all
    // cold-session readers are closed. Its alias and Data attribution remain.
    std::fs::remove_file(&body).unwrap();
    let mut rebuilt_session = Session::new(fx.context(), Default::default()).unwrap();
    let rebuilt = response(runtime.block_on(rebuilt_session.execute(query(), control())).unwrap());
    assert_eq!(rebuilt["complete"], true);
    assert_eq!(rebuilt["provenance"]["sources"][0]["sourceIdentity"]["source"], source);
    assert_eq!(page(&mut rebuilt_session, &rebuilt["result"]["handle"]), cold_page);
    assert_eq!(std::fs::read(&body).unwrap(), expected);
    assert_eq!(evidence::read_blob(&fx.home, &source["content"], 1 << 20).unwrap(), expected);
    assert_eq!(alias_document(), alias_before);
    assert_eq!(pair(), owner_before);
    assert_eq!(capture_document(), capture_before);
    assert_eq!(evidence::read_capture(&fx.home, capture_id).unwrap(), capture);
    assert_eq!(evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap(), capture_body);
    assert!(!repair.exists());
    rebuilt_session.close().unwrap();

    // Existing bytes with a wrong digest remain corruption, never a cache miss
    // that could be repaired or cause an automatic replacement capture.
    let mut corrupt = expected;
    corrupt[0] ^= 1;
    std::fs::write(&body, &corrupt).unwrap();
    let mut corrupt_session = Session::new(fx.context(), Default::default()).unwrap();
    let error = runtime.block_on(corrupt_session.execute(query(), control())).err().unwrap();
    assert_eq!(error.code, "vault.blob_integrity");
    corrupt_session.close().unwrap();
    assert_eq!(std::fs::read(&body).unwrap(), corrupt);
    assert_eq!(alias_document(), alias_before);
    assert_eq!(pair(), owner_before);
    assert_eq!(capture_document(), capture_before);
    assert_eq!(evidence::read_blob(&fx.home, &capture["blob"], 16 << 20).unwrap(), capture_body);
    assert!(!repair.exists());
    let metadata = Metadata::open(&fx.home, false).unwrap();
    assert_eq!(metadata.list::<Value>("capture/", 100).unwrap().len(), 1);
    metadata.close().unwrap();
    assert_eq!(std::fs::read_dir(paths.staging).unwrap().count(), 0);
}
