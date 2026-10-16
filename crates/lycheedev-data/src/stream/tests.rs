use super::*;
use crate::resource::Budget;
use lycheedev_core::{
    ToolkitError,
    control::OperationControl,
    public_json::{self, NumberLayout},
};
use serde_json::json;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Instant,
};

#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod catalog_fixture;
use catalog_fixture::Cell;

fn invocation() -> Invocation {
    Invocation {
        command: "data db2 stream".into(),
        positional: Some("Sample".into()),
        flags: BTreeMap::from([
            ("--format".into(), "jsonl".into()),
            ("--limit".into(), "1".into()),
        ]),
        home: Default::default(),
        cwd: Default::default(),
        control: Default::default(),
    }
}
fn request_error(inv: &Invocation) -> ToolkitError {
    StreamRequest::from_invocation(inv)
        .err()
        .expect("invalid request")
}
fn definition(table: &catalog_fixture::Table) -> schema::Definition {
    schema::Document::parse(table.dbd.as_bytes())
        .unwrap()
        .select(catalog_fixture::BUILD, &format!("{:08X}", table.layout))
        .unwrap()
}
fn row_fixture(second: Cell) -> catalog_fixture::Table {
    catalog_fixture::table(
        "Sample",
        7,
        &[("ID", "uint"), ("Name", "text")],
        &[
            vec![Cell::U(7), Cell::Text("first".into())],
            vec![Cell::U(8), second],
        ],
    )
}
fn budget(control: &OperationControl) -> Arc<Budget> {
    Arc::new(Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        control.clone(),
    ))
}
fn begin() -> StreamBegin {
    StreamBegin {
        proof: TableProof {
            key_source: json!("fixture"),
            requested_name: "Sample".into(),
            identity: TableIdentity {
                name: "Sample".into(),
                hash: 42,
                db2_file_data_id: 7,
                dbc_file_data_id: 0,
            },
            layout_hash: "AABBCCDD".into(),
            definition_sha256: "sha".into(),
            content_key: json!("key"),
            logical_rows: 2,
        },
        snapshot: "PIN-fixture".into(),
        source: json!("cdn"),
        projection: Projection {
            fields: vec!["ID".into(), "ID".into(), "Name".into()],
        },
        filter: " ID = +07 ".into(),
        limit: Limit(1),
    }
}

#[test]
fn typed_filter_retains_exact_signed_unsigned_and_array_rendering() {
    use execute::matches;
    assert!(!matches(
        &table::Value::UInt(u64::MAX),
        "+18446744073709551615"
    ));
    assert!(matches(
        &table::Value::UInt(u64::MAX),
        "18446744073709551615"
    ));
    assert!(!matches(&table::Value::UInt(8), "+08"));
    assert!(matches(&table::Value::UInt(8), "08"));
    assert!(matches(&table::Value::Int(8), "+08"));
    assert!(!matches(
        &table::Value::UInt(u64::MAX),
        "18446744073709551616"
    ));
    assert!(matches(&table::Value::Int(-2), "-02"));
    assert!(matches(
        &table::Value::Array(vec![table::Value::Int(1), table::Value::Int(2)]),
        "[1 2]"
    ));
    assert!(!matches(&table::Value::Null, ""));
    assert_eq!(
        Filter::parse(" ID = 01 ").unwrap(),
        Some(Filter {
            field: "ID",
            value: "01"
        })
    );
    assert!(Filter::parse("=1").is_err());
    assert!(Filter::parse("x").is_err());
}

#[test]
fn typed_request_preserves_validation_order_and_original_borrows() {
    let mut inv = invocation();
    inv.flags.insert("--format".into(), "wrong".into());
    inv.positional = None;
    inv.flags.remove("--limit");
    inv.flags.insert("--filter".into(), "invalid".into());
    inv.flags
        .insert("--timeout-seconds".into(), "invalid".into());
    inv.control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(request_error(&inv).exit_code, 7);
    inv.control = Default::default();
    assert_eq!(
        request_error(&inv).message,
        "data db2 stream requires --format jsonl"
    );
    inv.flags.insert("--format".into(), "jsonl".into());
    assert_eq!(
        request_error(&inv).message,
        "data db2 stream requires an argument"
    );
    inv.positional = Some("Sample".into());
    assert_eq!(
        request_error(&inv).message,
        "data db2 stream requires --limit"
    );
    inv.flags.insert("--limit".into(), "invalid".into());
    assert_eq!(request_error(&inv).message, "--limit requires an integer");
    inv.flags.insert("--limit".into(), "1".into());
    assert_eq!(request_error(&inv).code, "records.field_type_mismatch");
    inv.flags.insert("--filter".into(), " ID = +07 ".into());
    assert_eq!(
        request_error(&inv).message,
        "--timeout-seconds requires an integer"
    );
    inv.flags.remove("--timeout-seconds");
    inv.flags.insert(
        "--fields".into(),
        std::iter::repeat_n("Missing", 1025)
            .collect::<Vec<_>>()
            .join(","),
    );
    let request = StreamRequest::from_invocation(&inv).unwrap();
    assert!(std::ptr::eq(request.invocation, &inv));
    assert_eq!(request.name.as_ptr(), inv.argument().unwrap().as_ptr());
    assert_eq!(request.filter_text, " ID = +07 ");
    assert_eq!(
        request.fields.unwrap().as_ptr(),
        inv.flag("--fields").unwrap().as_ptr()
    );
    assert_eq!(request.limit.get(), 1);
    assert_eq!(request.timeout, Duration::from_secs(300));
    // Neither unknown fields nor the projection count is checked before IO.
}

#[test]
fn typed_projection_keeps_duplicates_and_go_filter_error_priority() {
    let raw = row_fixture(Cell::Text("second".into()));
    let definition = definition(&raw);
    let projection = Projection::resolve(Some("Name,ID,ID"), None, &definition).unwrap();
    assert_eq!(projection.fields(), ["Name", "ID", "ID"]);
    let error = Projection::resolve(Some(" Name , ID"), None, &definition).unwrap_err();
    assert_eq!(error.message, "records.field_unknown:  Name ");

    let error = Projection::resolve(
        Some("Missing"),
        Some(Filter {
            field: "Other",
            value: "1",
        }),
        &definition,
    )
    .unwrap_err();
    assert_eq!(error.message, "records.field_unknown: Other");
    assert_eq!(error.details, json!({"stage":"query"}));
    let error = Projection::resolve(
        Some("Missing"),
        Some(Filter {
            field: "ID",
            value: "1",
        }),
        &definition,
    )
    .unwrap_err();
    assert_eq!(error.message, "records.field_unknown: Missing");
    let oversized = std::iter::repeat_n("Missing", 1025)
        .collect::<Vec<_>>()
        .join(",");
    let error = Projection::resolve(
        Some(&oversized),
        Some(Filter {
            field: "Other",
            value: "1",
        }),
        &definition,
    )
    .unwrap_err();
    assert_eq!(error.message, "records.field_unknown: Other");
    let error = Projection::resolve(Some("ID,"), None, &definition).unwrap_err();
    assert_eq!(error.message, "records.field_unknown");
}

#[test]
fn native_row_executor_projects_one_row_and_filters_before_limit_probe() {
    let raw = row_fixture(Cell::Text("second".into()));
    let definition = definition(&raw);
    for (filter, expected_id, truncated) in [
        (None, 7, true),
        (
            Some(Filter {
                field: "ID",
                value: "08",
            }),
            8,
            false,
        ),
    ] {
        let control = OperationControl::default();
        let budget = budget(&control);
        let view = table::open_budgeted(
            &raw.raw,
            &definition,
            Default::default(),
            &Default::default(),
            &[],
            budget.clone(),
        )
        .unwrap();
        let projection = Projection::resolve(Some("ID,ID"), filter, &definition).unwrap();
        let mut count = 0;
        let summary = execute::execute_rows(
            &view,
            &projection,
            filter,
            Limit(1),
            &budget,
            &mut |event| {
                let StreamEvent::Record(record) = event else {
                    panic!("row executor emitted non-row")
                };
                assert_eq!(record.index, count);
                assert_eq!(record.record_id, expected_id);
                assert_eq!(record.row.len(), 1);
                assert_eq!(record.row["ID"], table::Value::UInt(expected_id as u64));
                count += 1;
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(count, 1);
        assert_eq!(
            summary,
            WalkSummary {
                count: 1,
                truncated
            }
        );
    }
}

#[test]
fn native_row_executor_backpressure_stops_decode_on_cancel_or_failed_sink() {
    let raw = row_fixture(Cell::BadString);
    let definition = definition(&raw);
    for cancel in [false, true] {
        let control = OperationControl::default();
        let budget = budget(&control);
        let view = table::open_budgeted(
            &raw.raw,
            &definition,
            Default::default(),
            &Default::default(),
            &[],
            budget.clone(),
        )
        .unwrap();
        let projection = Projection::resolve(None, None, &definition).unwrap();
        let mut calls = 0;
        let error =
            execute::execute_rows(&view, &projection, None, Limit(1), &budget, &mut |event| {
                let StreamEvent::Record(record) = event else {
                    panic!("expected record")
                };
                assert_eq!(record.record_id, 7);
                calls += 1;
                if cancel {
                    control.cancel_handle().store(true, Ordering::Release);
                    Ok(())
                } else {
                    Err(fault("external.io", 5, "sink failed"))
                }
            })
            .unwrap_err();
        assert_eq!(calls, 1);
        assert_eq!(error.exit_code, if cancel { 7 } else { 5 });
        assert_ne!(
            error.code, "table.invalid_format",
            "the next row must not be decoded ahead of the sink"
        );
    }
}

#[test]
fn native_row_executor_still_decodes_limit_probe_before_marking_truncated() {
    let raw = row_fixture(Cell::BadString);
    let definition = definition(&raw);
    let budget = budget(&OperationControl::default());
    let view = table::open_budgeted(
        &raw.raw,
        &definition,
        Default::default(),
        &Default::default(),
        &[],
        budget.clone(),
    )
    .unwrap();
    let projection = Projection::resolve(None, None, &definition).unwrap();
    let mut calls = 0;
    let error = execute::execute_rows(&view, &projection, None, Limit(1), &budget, &mut |_| {
        calls += 1;
        Ok(())
    })
    .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.code, "table.invalid_format");
}

#[test]
fn typed_wire_keeps_exact_record_bytes_numeric_types_empty_maps_and_escaping() {
    let row = BTreeMap::from([
        (
            "array".into(),
            table::Value::Array(vec![table::Value::Int(-2), table::Value::UInt(u64::MAX)]),
        ),
        ("signed".into(), table::Value::Int(i64::MIN)),
        ("text".into(), table::Value::String("<>&\u{2028}".into())),
        ("unsigned".into(), table::Value::UInt(u64::MAX)),
        ("zero".into(), table::Value::Float(-0.0)),
    ]);
    let frame = wire::event(StreamEvent::Record(StreamRecord {
        index: 0,
        record_id: 7,
        row: &row,
    }));
    let mut bytes = Vec::new();
    public_json::write_line(&mut bytes, &frame, &NumberLayout::Verbatim).unwrap();
    assert_eq!(
        String::from_utf8(bytes).unwrap(),
        "{\"frame\":\"record\",\"record\":{\"index\":0,\"recordID\":7,\"row\":{\"array\":[-2,18446744073709551615],\"signed\":-9223372036854775808,\"text\":\"<>&\\u2028\",\"unsigned\":18446744073709551615,\"zero\":-0.0}}}\n"
    );
    assert_eq!(
        wire::event(StreamEvent::Record(StreamRecord {
            index: 1,
            record_id: 8,
            row: &BTreeMap::new()
        })),
        json!({"frame":"record","record":{"index":1,"recordID":8,"row":{}}})
    );
}

#[test]
fn typed_wire_manifest_and_summary_share_exact_fixed_shapes() {
    let begin = begin();
    let walk = WalkSummary {
        count: 1,
        truncated: true,
    };
    let expected_proof = json!({"contentVerified":true,"keySource":"fixture","name":"Sample","identity":{"name":"Sample","hash":42,"db2FileDataId":7,"dbcFileDataId":0},"layoutHash":"AABBCCDD","definitionSHA256":"sha","fileDataID":7,"contentKey":"key","logicalRows":2});
    let expected_begin = json!({"tables":[expected_proof.clone()],"snapshot":"PIN-fixture","source":"cdn","table":"Sample","identity":{"name":"Sample","hash":42,"db2FileDataId":7,"dbcFileDataId":0},"layoutHash":"AABBCCDD","fields":["ID","ID","Name"],"filter":" ID = +07 ","limit":1});
    let expected_end = json!({"partial":false,"partialReasons":null,"count":1,"complete":false,"truncated":true,"truncationReason":"limit_reached"});
    assert_eq!(
        wire::event(StreamEvent::Begin(&begin)),
        json!({"frame":"begin","begin":expected_begin})
    );
    assert_eq!(
        wire::event(StreamEvent::End(walk)),
        json!({"frame":"end","end":expected_end})
    );
    assert_eq!(
        wire::manifest(&begin, walk),
        json!({"begin":expected_begin,"end":expected_end})
    );
    let memory_budget = MemoryBudget {
        mode: "fixed",
        concurrent_queries: 0,
        ceiling_bytes: 0,
        retained_bytes: 3,
        metadata_bytes: 2,
        peak_scratch_bytes: 1,
        growths: 0,
    };
    let outcome = wire::into_outcome(StreamSummary {
        begin,
        pin: DataPin::default(),
        resources: Cost::default(),
        memory_budget: memory_budget.clone(),
        resource_usage: Cost::default(),
        walk,
        captures: vec![json!({"id":"CAP-fixture"})],
    });
    assert_eq!(
        outcome.data,
        json!({"resources":Cost::default(),"memoryBudget":memory_budget,"snapshot":"PIN-fixture","pin":DataPin::default(),"source":"cdn","tables":[expected_proof],"relations":[],"complete":false,"truncated":true,"truncationReason":"limit_reached","partial":false,"partialReasons":null,"table":"Sample","mode":"stream","limit":1,"fields":["ID","ID","Name"],"filter":" ID = +07 ","count":1,"resourceAccounting":{"wholeQueryAccountingComplete":false,"used":Cost::default()}})
    );
    assert!(!outcome.complete);
    assert_eq!(outcome.provenance, [json!({"id":"CAP-fixture"})]);
    assert_eq!(
        wire::event(StreamEvent::Error {
            code: "records.cancelled",
            message: "cancelled"
        }),
        json!({"frame":"error","error":{"code":"records.cancelled","message":"cancelled"}})
    );
}
