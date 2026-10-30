//! Discover requests through the public schema, then use the production serde
//! and Session paths. These tests do not treat metadata equality as execution.
use super::*;
use std::collections::BTreeSet;

#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;

fn variant(task: &str) -> Value {
    schema()["requestSchema"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["properties"]["task"]["const"] == task)
        .unwrap()
        .clone()
}
fn example(task: &str) -> Value {
    variant(task)["examples"][0].clone()
}
fn name(task: &Task) -> &'static str {
    // Exhaustive over the production enum: adding a Task requires extending
    // this independent inventory and its public schema admission checks.
    match task {
        Task::Navigate { .. } => "navigate",
        Task::ItemGet { .. } => "item_get",
        Task::Semantics { .. } => "semantics",
        Task::Explain { .. } => "explain",
        Task::Capture { .. } => "capture",
        Task::Catalog {} => "catalog",
        Task::Fields { .. } => "fields",
        Task::Business { .. } => "business",
        Task::Discover { .. } => "discover",
        Task::Query { .. } => "query",
        Task::Page { .. } => "page",
        Task::Export { .. } => "export",
        Task::Release { .. } => "release",
        Task::Cancel { .. } => "cancel",
        Task::Close {} => "close",
    }
}

#[test]
fn public_wire_schema_covers_every_typed_task_and_required_field() {
    let fields: &[(&str, &[&str], &[&str])] = &[
        ("navigate", &["request"], &[]),
        ("item_get", &["item_id", "mode"], &["hotfix_captures"]),
        ("semantics", &["table"], &[]),
        ("explain", &["query"], &["verbose", "analyze"]),
        ("capture", &["handle"], &[]),
        ("catalog", &[], &[]),
        ("discover", &["table"], &[]),
        ("fields", &["table"], &[]),
        ("business", &["request"], &[]),
        ("query", &["query"], &[]),
        ("page", &["handle", "limit"], &["offset"]),
        ("export", &["handle", "limit", "format"], &["offset"]),
        ("release", &["handle"], &[]),
        ("cancel", &["request_id"], &[]),
        ("close", &[], &[]),
    ];
    let public = schema();
    assert_eq!(
        public["requestSchema"]["oneOf"].as_array().unwrap().len(),
        fields.len()
    );
    for (task, required, optional) in fields {
        let contract = variant(task);
        let expected_required: BTreeSet<_> = ["protocol", "id", "task"]
            .into_iter()
            .chain(required.iter().copied())
            .collect();
        let expected_properties: BTreeSet<_> = expected_required
            .iter()
            .copied()
            .chain(["timeout_ms"])
            .chain(optional.iter().copied())
            .collect();
        let actual_required: BTreeSet<_> = contract["required"]
            .as_array()
            .unwrap()
            .iter()
            .map(|field| field.as_str().unwrap())
            .collect();
        let actual_properties: BTreeSet<_> = contract["properties"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(actual_required, expected_required, "{task}");
        assert_eq!(actual_properties, expected_properties, "{task}");
        assert_eq!(contract["additionalProperties"], false, "{task}");
        assert!(contract["x-result"].as_str().unwrap().len() > 10);
        assert!(contract["x-recovery"].as_str().unwrap().len() > 10);
        let input = example(task);
        let parsed: Request = serde_json::from_value(input.clone()).unwrap();
        assert_eq!(name(&parsed.task), *task);
        assert_eq!(parsed.protocol, PROTOCOL);
        assert_eq!(parsed.timeout_ms, 30_000);
        for field in actual_required {
            let mut missing = input.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<Request>(missing).is_err(),
                "{task} must require {field}"
            );
        }
        let mut unknown = input;
        unknown["invented"] = json!(true);
        assert!(
            serde_json::from_value::<Request>(unknown).is_err(),
            "{task} accepted an unknown field"
        );
    }
}

#[test]
fn published_wire_bounds_and_defaults_match_typed_request_fields() {
    let page = variant("page");
    assert_eq!(page["properties"]["id"]["minimum"], 1);
    assert_eq!(page["properties"]["id"]["maximum"], json!(u64::MAX));
    assert_eq!(page["properties"]["timeout_ms"]["minimum"], 1);
    assert_eq!(page["properties"]["timeout_ms"]["maximum"], 3_600_000);
    assert_eq!(page["properties"]["timeout_ms"]["default"], 30_000);
    assert_eq!(page["properties"]["limit"]["minimum"], 1);
    assert_eq!(page["properties"]["limit"]["maximum"], 1000);
    assert_eq!(page["properties"]["offset"]["default"], 0);
    let parsed: Request = serde_json::from_value(example("page")).unwrap();
    assert!(matches!(
        parsed.task,
        Task::Page {
            offset: 0,
            limit: 10,
            ..
        }
    ));
    let parsed: Request = serde_json::from_value(example("explain")).unwrap();
    assert!(matches!(
        parsed.task,
        Task::Explain {
            verbose: false,
            analyze: false,
            ..
        }
    ));
    let parsed: Request = serde_json::from_value(example("item_get")).unwrap();
    assert!(
        matches!(parsed.task, Task::ItemGet { hotfix_captures, .. } if hotfix_captures.is_empty())
    );
    let export = variant("export");
    assert_eq!(
        export["properties"]["format"]["enum"],
        json!(["json", "csv"])
    );
    let mut invalid_format = example("export");
    invalid_format["format"] = json!("CSV");
    assert!(serde_json::from_value::<Request>(invalid_format).is_err());
    let mut invalid_id = example("catalog");
    invalid_id["id"] = json!(-1);
    assert!(serde_json::from_value::<Request>(invalid_id.clone()).is_err());
    invalid_id["id"] = serde_json::from_str::<Value>("18446744073709551616").unwrap();
    assert!(serde_json::from_value::<Request>(invalid_id).is_err());
}

#[test]
fn nested_request_schemas_keep_resolvable_production_references() {
    let public = schema();
    let root = &public["requestSchema"];
    fn visit(value: &Value, root: &Value) {
        match value {
            Value::Object(fields) => {
                if let Some(reference) = fields.get("$ref").and_then(Value::as_str) {
                    assert!(reference.starts_with("#/$defs/"), "{reference}");
                    assert!(
                        root.pointer(&reference[1..]).is_some(),
                        "unresolved {reference}"
                    );
                }
                for value in fields.values() {
                    visit(value, root);
                }
            }
            Value::Array(values) => {
                for value in values {
                    visit(value, root);
                }
            }
            _ => {}
        }
    }
    visit(root, root);
    assert_eq!(
        root["$defs"]["TableRequest"],
        public["query"]["$defs"]["TableRequest"]
    );
    assert_eq!(
        variant("item_get")["allOf"],
        public["navigate"]["oneOf"][0]["allOf"]
    );
}

#[test]
fn schema_examples_drive_real_query_page_export_capture_and_release() {
    use fixture::{Cell, Fixture};
    let fixture = Fixture::new(&[fixture::table(
        "AgentSample",
        921,
        &[("ID", "uint"), ("value", "signed64"), ("label", "text")],
        &[
            vec![
                Cell::U(9),
                Cell::I64(i64::MIN),
                Cell::Text("schema,quote".into()),
            ],
            vec![
                Cell::U(1),
                Cell::I64(9_007_199_254_740_993),
                Cell::Text("中文".into()),
            ],
        ],
    )]);
    let mut session = Session::new(fixture.context(), Default::default()).unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let control = || OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
    let execute = |session: &mut Session, input: Value| {
        let request: Request = serde_json::from_value(input).unwrap();
        serde_json::to_value(
            runtime
                .block_on(session.execute(request.task, control()))
                .unwrap(),
        )
        .unwrap()
    };
    let mut input = example("query");
    input["query"]["plan"]["table"]["name"] = json!("AgentSample");
    input["query"]["plan"]["ids"] = json!([9, 1]);
    let query = execute(&mut session, input);
    assert_eq!(query["complete"], true);
    assert_eq!(query["result"]["rows"], 2);
    let handle = query["result"]["handle"].as_str().unwrap();
    let with_handle = |task| {
        let mut input = example(task);
        input["handle"] = json!(handle);
        input
    };
    let page = execute(&mut session, with_handle("page"));
    assert_eq!(page["rows"].as_array().unwrap().len(), 2);
    let values: BTreeSet<_> = page["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row[1].to_string())
        .collect();
    assert_eq!(
        values,
        BTreeSet::from(["-9223372036854775808".into(), "9007199254740993".into()])
    );
    for task in ["page", "export"] {
        for (offset, limit) in [(0, 0), (0, 1001), (usize::MAX, 1)] {
            let mut input = with_handle(task);
            input["offset"] = json!(offset);
            input["limit"] = json!(limit);
            let request: Request = serde_json::from_value(input).unwrap();
            assert_eq!(
                runtime
                    .block_on(session.execute(request.task, control()))
                    .err()
                    .unwrap()
                    .code,
                "data.session.page_limit",
                "{task}"
            );
        }
    }
    let mut first_page = with_handle("page");
    first_page["limit"] = json!(1);
    assert_eq!(
        execute(&mut session, first_page)["rows"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut maximum_page = with_handle("page");
    maximum_page["limit"] = json!(1000);
    assert_eq!(
        execute(&mut session, maximum_page)["rows"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let csv = execute(&mut session, with_handle("export"));
    assert_eq!(csv["format"], "csv");
    assert!(csv["text"].as_str().unwrap().contains("\"schema,quote\""));
    assert!(csv["text"].as_str().unwrap().contains("9007199254740993"));
    let capture = execute(&mut session, with_handle("capture"));
    let capture_id = capture["id"].as_str().unwrap();
    assert_eq!(
        lycheedev_core::evidence::read_capture(&fixture.home, capture_id).unwrap(),
        capture
    );
    let released = execute(&mut session, with_handle("release"));
    assert_eq!(released["released"], handle);
    let input: Request = serde_json::from_value(with_handle("page")).unwrap();
    assert_eq!(
        runtime
            .block_on(session.execute(input.task, control()))
            .err()
            .unwrap()
            .code,
        "data.session.result_missing"
    );
    assert_eq!(
        lycheedev_core::evidence::read_capture(&fixture.home, capture_id).unwrap(),
        capture
    );
    session.close().unwrap();
}
