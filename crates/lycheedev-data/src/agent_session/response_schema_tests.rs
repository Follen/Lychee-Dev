//! Independent fixed inputs exercise actual Session serialization. Transport
//! frames remain owned and tested by CLI, not reconstructed as fake frames here.
use super::*;
use crate::engine::{QueryPlan, SourceMode};
use std::collections::BTreeSet;

#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, Fixture};

fn table(name: &str) -> TableRequest {
    TableRequest {
        name: name.into(),
        mode: SourceMode::Static,
        hotfix_captures: vec![],
    }
}
fn query(name: &str) -> QueryRequest {
    QueryRequest::Plan {
        plan: QueryPlan::Scan { table: table(name) },
    }
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn execute(runtime: &tokio::runtime::Runtime, session: &mut Session, task: Task) -> Value {
    serde_json::to_value(
        runtime
            .block_on(session.execute(task, OperationControl::default()))
            .unwrap(),
    )
    .unwrap()
}
fn declared(schema: &Value, names: &[&str]) {
    // Check a fixed public field inventory, not equality to a generated clone.
    for name in names {
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .contains(&json!(name)),
            "required {name}"
        );
        assert!(schema["properties"].get(name).is_some(), "property {name}");
    }
}
fn handle(value: &Value) -> String {
    value["result"]["handle"].as_str().unwrap().to_owned()
}

#[test]
fn response_schema_tasks_and_all_local_references_resolve() {
    let public = schema();
    let request_tasks: BTreeSet<_> = public["requestSchema"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| {
            let task = variant["properties"]["task"]["const"].as_str().unwrap();
            let pointer = variant["x-resultSchema"].as_str().unwrap();
            assert!(
                public.pointer(pointer.strip_prefix('#').unwrap()).is_some(),
                "{pointer}"
            );
            task
        })
        .collect();
    let result_tasks: BTreeSet<_> = public["resultSchemas"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(request_tasks, result_tasks);
    fn references(value: &Value, root: &Value) {
        match value {
            Value::Object(fields) => {
                if let Some(reference) = fields.get("$ref") {
                    let reference = reference.as_str().unwrap();
                    assert!(
                        root.pointer(reference.strip_prefix('#').unwrap()).is_some(),
                        "{reference}"
                    );
                }
                for child in fields.values() {
                    references(child, root);
                }
            }
            Value::Array(values) => {
                for child in values {
                    references(child, root);
                }
            }
            _ => {}
        }
    }
    references(&public["responseSchema"], &public["responseSchema"]);
    for result in public["resultSchemas"].as_object().unwrap().values() {
        references(result, result);
    }
    let definitions = &public["responseSchema"]["$defs"];
    declared(
        &definitions["SuccessFrame"],
        &["protocol", "id", "ok", "result"],
    );
    declared(
        &definitions["ErrorFrame"],
        &["protocol", "id", "ok", "error"],
    );
    declared(
        &definitions["TerminalDiagnostic"],
        &["protocol", "ok", "error"],
    );
    assert_eq!(definitions["SuccessFrame"]["x-stream"], "stdout");
    assert_eq!(definitions["TerminalDiagnostic"]["x-stream"], "stderr");
}

#[test]
fn response_schema_real_session_exact_pages_csv_capture_and_release() {
    let fixture = Fixture::new(&[fixture::table(
        "SchemaRows",
        921,
        &[("ID", "uint"), ("value", "signed64"), ("label", "text")],
        &[
            vec![
                Cell::U(1),
                Cell::I64(9_007_199_254_740_993),
                Cell::Text("a,\"b".into()),
            ],
            vec![Cell::U(2), Cell::I64(i64::MIN), Cell::Text("中文".into())],
        ],
    )]);
    let mut session = Session::new(fixture.context(), Default::default()).unwrap();
    let runtime = runtime();
    let schemas = schema()["resultSchemas"].clone();
    let result = execute(
        &runtime,
        &mut session,
        Task::Query {
            query: query("SchemaRows"),
        },
    );
    declared(&schemas["query"], &["result", "complete", "provenance"]);
    declared(
        &schemas["query"]["properties"]["result"],
        &["handle", "rows", "retainedBytes", "expiresAfterMs"],
    );
    assert_eq!(result["result"]["rows"], 2);
    assert!(result["result"]["retainedBytes"].as_u64().unwrap() > 0);
    assert!(result["result"]["expiresAfterMs"].as_u64().unwrap() > 0);
    assert!(result["provenance"].is_object());
    assert_eq!(result["complete"], true);
    let handle = handle(&result);
    let page_response = runtime
        .block_on(session.execute(
            Task::Page {
                handle: handle.clone(),
                offset: 0,
                limit: 1,
            },
            OperationControl::default(),
        ))
        .unwrap();
    let raw = serde_json::to_string(&page_response).unwrap();
    assert!(
        raw.contains("9007199254740993"),
        "exact number lexeme must survive serialization"
    );
    let page = serde_json::to_value(page_response).unwrap();
    declared(
        &schemas["page"],
        &["columns", "rows", "offset", "totalRows", "complete"],
    );
    assert_eq!(page["columns"], json!(["ID", "value", "label"]));
    assert_eq!(
        page["rows"],
        json!([[1, 9_007_199_254_740_993u64, "a,\"b"]])
    );
    assert_eq!(page["offset"], 0);
    assert_eq!(page["totalRows"], 2);
    assert_eq!(
        page["complete"], true,
        "bounded page is not incomplete source coverage"
    );
    let csv = execute(
        &runtime,
        &mut session,
        Task::Export {
            handle: handle.clone(),
            offset: 1,
            limit: 1,
            format: ExportFormat::Csv,
        },
    );
    declared(&schemas["export"]["oneOf"][1], &["format", "text", "page"]);
    assert_eq!(csv["format"], "csv");
    assert_eq!(
        csv["text"],
        "\"ID\",\"value\",\"label\"\r\n\"2\",\"-9223372036854775808\",\"中文\"\r\n"
    );
    assert_eq!(csv["page"]["rows"], json!([[2, i64::MIN, "中文"]]));
    assert_eq!(csv["page"]["offset"], 1);
    let json_export = execute(
        &runtime,
        &mut session,
        Task::Export {
            handle: handle.clone(),
            offset: 0,
            limit: 1,
            format: ExportFormat::Json,
        },
    );
    declared(&schemas["export"]["oneOf"][0], &["format", "page"]);
    assert_eq!(json_export["format"], "json");
    assert_eq!(json_export["page"], page);
    let capture = execute(
        &runtime,
        &mut session,
        Task::Capture {
            handle: handle.clone(),
        },
    );
    declared(
        &schemas["capture"],
        &[
            "schema",
            "id",
            "blob",
            "mediaType",
            "provenance",
            "complete",
            "truncated",
            "capturedAt",
        ],
    );
    assert_eq!(capture["schema"], "lycheedev.capture.v1");
    assert!(capture["id"].as_str().unwrap().starts_with("CAP-"));
    assert_eq!(capture["blob"]["sha256"].as_str().unwrap().len(), 64);
    assert!(capture["blob"]["bytes"].as_u64().unwrap() > 0);
    assert_eq!(capture["complete"], true);
    assert_eq!(capture["truncated"], false);
    let bytes =
        lycheedev_core::evidence::read_blob(&fixture.home, &capture["blob"], 16 << 20).unwrap();
    let captured: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        captured["rows"].as_array().unwrap().len(),
        2,
        "CAP retains the whole handle, not one exported page"
    );
    let released = execute(
        &runtime,
        &mut session,
        Task::Release {
            handle: handle.clone(),
        },
    );
    declared(&schemas["release"], &["released"]);
    assert_eq!(released["released"], handle);
    let error = runtime
        .block_on(session.execute(
            Task::Page {
                handle,
                offset: 0,
                limit: 1,
            },
            OperationControl::default(),
        ))
        .err()
        .unwrap();
    let error = serde_json::to_value(error).unwrap();
    declared(
        &schema()["responseSchema"]["$defs"]["ToolkitError"],
        &["code", "message", "exit_code", "details"],
    );
    assert_eq!(error["code"], "data.session.result_missing");
    assert!(error["message"].is_string());
    assert_eq!(error["exit_code"], 4);
    assert!(error.get("details").is_some());
    let explained = execute(
        &runtime,
        &mut session,
        Task::Explain {
            query: query("SchemaRows"),
            verbose: false,
            analyze: false,
        },
    );
    declared(&schemas["explain"], &["result", "complete", "provenance"]);
    assert!(
        explained["result"]["handle"]
            .as_str()
            .unwrap()
            .starts_with("RES-")
    );
    let plan = execute(
        &runtime,
        &mut session,
        Task::Page {
            handle: self::handle(&explained),
            offset: 0,
            limit: 1000,
        },
    );
    assert!(!plan["columns"].as_array().unwrap().is_empty());
    assert!(!plan["rows"].as_array().unwrap().is_empty());
    session.close().unwrap();
}

#[test]
fn response_schema_real_catalog_fields_business_and_domain_result_paths() {
    let fixture = Fixture::new(&fixture::items());
    let mut session = Session::new(fixture.context(), Default::default()).unwrap();
    let runtime = runtime();
    let schemas = schema()["resultSchemas"].clone();
    let catalog = execute(&runtime, &mut session, Task::Catalog {});
    declared(
        &schemas["catalog"],
        &[
            "snapshotId",
            "definitionCommit",
            "scope",
            "complete",
            "provenance",
            "tables",
        ],
    );
    assert_eq!(catalog["snapshotId"], fixture.pin);
    assert_eq!(
        catalog["scope"],
        "exact_definition_manifest_not_installed_tables"
    );
    assert!(
        catalog["tables"]
            .as_array()
            .unwrap()
            .iter()
            .any(|table| table["name"] == "Item" && table["availability"] == "not_checked")
    );
    for task in [
        Task::Fields {
            table: table("Item"),
        },
        Task::Discover {
            table: table("Item"),
        },
    ] {
        let fields = execute(&runtime, &mut session, task);
        declared(
            &schemas["fields"],
            &["table", "columns", "rowCount", "provenance"],
        );
        assert_eq!(fields["table"]["name"], "Item");
        assert!(fields["rowCount"].as_u64().unwrap() > 0);
        let first = &fields["columns"][0];
        assert!(first["name"].is_string());
        assert!(first["dataType"].is_string());
        assert!(first["nullable"].is_boolean());
    }
    let business = execute(
        &runtime,
        &mut session,
        Task::Business {
            request: tasks::BusinessTask::BatchIds {
                table: table("Item"),
                ids: vec![7],
            },
        },
    );
    declared(&schemas["business"], &["components"]);
    declared(
        &schemas["business"]["properties"]["components"]["items"],
        &["name", "result", "complete", "provenance"],
    );
    assert_eq!(business["components"][0]["name"], "records");
    assert_eq!(business["components"][0]["result"]["rows"], 1);
    assert!(
        business["components"][0]["result"]["handle"]
            .as_str()
            .unwrap()
            .starts_with("RES-")
    );
    let domain = execute(
        &runtime,
        &mut session,
        Task::Navigate {
            request: crate::engine::DomainRequest::ItemGet {
                item_id: 7,
                mode: SourceMode::Static,
                hotfix_captures: vec![],
            },
        },
    );
    declared(&schemas["navigate"], &["report", "sources", "capture"]);
    assert_eq!(domain["report"]["item"]["id"], 7);
    assert!(!domain["sources"].as_array().unwrap().is_empty());
    assert!(
        domain["capture"]["id"]
            .as_str()
            .unwrap()
            .starts_with("CAP-")
    );
    let alias = execute(
        &runtime,
        &mut session,
        Task::ItemGet {
            item_id: 7,
            mode: SourceMode::Static,
            hotfix_captures: vec![],
        },
    );
    declared(&schemas["item_get"], &["report", "sources", "capture"]);
    assert_eq!(alias["report"]["item"]["id"], 7);
    session.close().unwrap();
}

#[test]
fn response_schema_real_partial_page_and_capture_keep_coverage_false() {
    let spec = fixture::encrypted_sections(929);
    let (_, encoded) = fixture::encrypted_frame(&spec.raw);
    let fixture = Fixture::with_encoded_table(spec, encoded);
    let mut session = Session::new(fixture.context(), Default::default()).unwrap();
    let runtime = runtime();
    let result = execute(
        &runtime,
        &mut session,
        Task::Query {
            query: query("Sample"),
        },
    );
    assert_eq!(result["complete"], false);
    let handle = handle(&result);
    let page = execute(
        &runtime,
        &mut session,
        Task::Page {
            handle: handle.clone(),
            offset: 0,
            limit: 1000,
        },
    );
    assert_eq!(page["complete"], false);
    assert_eq!(page["totalRows"], 1);
    assert_eq!(page["rows"].as_array().unwrap().len(), 1);
    let capture = execute(&runtime, &mut session, Task::Capture { handle });
    assert_eq!(capture["complete"], false);
    assert_eq!(
        capture["truncated"], false,
        "unread encrypted source is partial coverage, not output truncation"
    );
    let schemas = schema();
    assert_eq!(
        schemas["resultSchemas"]["page"]["properties"]["complete"]["type"],
        "boolean"
    );
    assert_eq!(
        schemas["resultSchemas"]["capture"]["properties"]["complete"]["type"],
        "boolean"
    );
    session.close().unwrap();
}
