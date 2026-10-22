use lycheedev_emmy_adapter::{
    ByteRange, CallableInput, DeclarationFailure, DeclarationModel, OriginInput, TypeProjection,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const SOURCE: &str = "---@param value number\nfunction C_Test.Test(value) end\n";

#[derive(Debug, PartialEq, Eq)]
struct Stop(&'static str);

fn target() -> ByteRange {
    let start = SOURCE.find("Test(value)").unwrap() as u32;
    ByteRange {
        start,
        end: start + 4,
    }
}

fn record(type_name: &str) -> Value {
    json!({"kind":"function","name":"C_Test.Test","parameters":[
        {"index":1,"name":"value","type":type_name,"nilable":false,"line":7,"raw":{"Original":"unchanged"}}
    ],"returns":[],"raw":{}})
}

fn input<'a>(record: &'a Value, aliases: &'a BTreeMap<String, String>) -> CallableInput<'a> {
    CallableInput {
        origin: OriginInput {
            material: "api",
            material_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            pointer: "/bindings/0/descriptor",
            identity: "authenticated-test",
        },
        declaration: record,
        catalog_identity: "catalog",
        aliases,
    }
}

fn query(
    model: &DeclarationModel,
    record: &Value,
    aliases: &BTreeMap<String, String>,
) -> TypeProjection {
    model
        .type_at(
            target(),
            "query",
            input(record, aliases),
            Arc::new(|| Ok::<(), Stop>(())),
            |error| error.0.into(),
            || Ok(()),
        )
        .unwrap()
        .unwrap()
}

#[test]
fn declaration_type_uses_selected_evidence_and_returns_owned_coverage() {
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let projected = query(&model, &record("string"), &BTreeMap::new());
    assert!(projected.known);
    assert!(
        projected.rendered.contains("string"),
        "{}",
        projected.rendered
    );
    assert!(
        projected
            .coverage
            .iter()
            .any(|row| row.state == "bound" && row.proof == "query")
    );
    let serialized = serde_json::to_value(&projected.coverage).unwrap();
    assert!(serialized[0].get("reason").is_some());
    assert!(serialized[0].get("site").is_none());
    drop(model);
    assert!(!projected.rendered.is_empty());
}

#[test]
fn unsupported_environment_type_never_falls_back_to_raw_number() {
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let projected = query(&model, &record("UnsupportedNominal"), &BTreeMap::new());
    assert!(!projected.known);
    assert!(
        projected
            .coverage
            .iter()
            .any(|row| row.state == "unavailable")
    );
}

#[test]
fn borrowed_aliases_are_projected_without_mutating_input_records() {
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let record = record("Alias");
    let before = record.clone();
    let projected = query(
        &model,
        &record,
        &BTreeMap::from([("Alias".into(), "string".into())]),
    );
    assert!(projected.known);
    assert!(projected.rendered.contains("string"));
    assert_eq!(record, before);
}

#[test]
fn cancellation_and_step_errors_keep_the_original_payload() {
    assert!(matches!(
        DeclarationModel::new(Arc::from(SOURCE), || Err(Stop("admission"))),
        Err(DeclarationFailure::Interrupted(Stop("admission")))
    ));
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let declaration = record("number");
    let aliases = BTreeMap::new();
    let result = model.type_at(
        target(),
        "query",
        input(&declaration, &aliases),
        Arc::new(|| Err(Stop("original-cancellation"))),
        |error| error.0.into(),
        || Ok(()),
    );
    assert!(matches!(
        result,
        Err(DeclarationFailure::Interrupted(Stop(
            "original-cancellation"
        )))
    ));
    let result = model.type_at(
        target(),
        "query",
        input(&declaration, &aliases),
        Arc::new(|| Ok(())),
        |error: &Stop| error.0.into(),
        || Err(Stop("original-budget")),
    );
    assert!(matches!(
        result,
        Err(DeclarationFailure::Interrupted(Stop("original-budget")))
    ));
}

#[test]
fn cancellation_during_selection_does_not_return_a_partial_projection() {
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let query_cancelled = cancelled.clone();
    let check = Arc::new(move || {
        if query_cancelled.load(Ordering::Acquire) {
            Err(Stop("cancelled"))
        } else {
            Ok(())
        }
    });
    let declaration = record("number");
    let aliases = BTreeMap::new();
    let result = model.type_at(
        target(),
        "query",
        input(&declaration, &aliases),
        check,
        |error| error.0.into(),
        || {
            cancelled.store(true, Ordering::Release);
            Ok(())
        },
    );
    assert!(matches!(
        result,
        Err(DeclarationFailure::Interrupted(Stop("cancelled")))
    ));
}

#[test]
fn malformed_generated_definitions_preserve_parse_kind_and_byte_range() {
    let source = "---@return number in\nfunction Test() end\n";
    let error = DeclarationModel::new(Arc::from(source), || Ok::<(), Stop>(()))
        .err()
        .unwrap();
    let DeclarationFailure::Parse(errors) = error else {
        panic!("expected parse evidence")
    };
    assert!(errors.count > 0);
    assert_eq!(errors.first.kind, "DocError");
    assert!(errors.first.range.start <= errors.first.range.end);
    assert!(errors.first.range.end as usize <= source.len());
}

#[test]
fn invalid_target_and_absent_syntax_are_distinct() {
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let declaration = record("number");
    let aliases = BTreeMap::new();
    let result = model.type_at(
        ByteRange { start: 4, end: 3 },
        "query",
        input(&declaration, &aliases),
        Arc::new(|| Ok::<(), Stop>(())),
        |error| error.0.into(),
        || Ok(()),
    );
    assert!(matches!(result, Err(DeclarationFailure::InvalidInput(_))));
    let absent = model
        .type_at(
            ByteRange { start: 0, end: 3 },
            "query",
            input(&declaration, &aliases),
            Arc::new(|| Ok::<(), Stop>(())),
            |error| error.0.into(),
            || Ok(()),
        )
        .unwrap();
    assert!(absent.is_none());
}

#[test]
fn repeated_queries_do_not_share_contextual_selected_types() {
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let aliases = BTreeMap::new();
    let first = query(&model, &record("string"), &aliases);
    let second = query(&model, &record("boolean"), &aliases);
    let again = query(&model, &record("string"), &aliases);
    assert!(first.known && second.known && again.known);
    assert_ne!(first.rendered, second.rendered);
    assert_eq!(first.rendered, again.rendered);
    assert_eq!(first.coverage, again.coverage);
}

#[test]
fn definition_byte_limit_accepts_eight_mib_and_rejects_the_next_byte() {
    let limit = 8 << 20;
    let definitions: Arc<str> = format!("--{}", "x".repeat(limit - 2)).into();
    let model = DeclarationModel::new(definitions, || Ok::<(), Stop>(())).unwrap();
    drop(model);
    let oversized: Arc<str> = format!("--{}", "x".repeat(limit - 1)).into();
    let error = DeclarationModel::new(oversized, || Ok::<(), Stop>(()))
        .err()
        .unwrap();
    assert!(matches!(error, DeclarationFailure::InvalidInput(message)
        if message == "API definitions exceed the admitted environment byte limit"));
}

#[test]
fn admission_cancellation_precedes_the_definition_byte_limit() {
    let oversized: Arc<str> = "x".repeat((8 << 20) + 1).into();
    let error = DeclarationModel::new(oversized, || Err(Stop("original-admission-cancelled")))
        .err()
        .unwrap();
    assert!(matches!(
        error,
        DeclarationFailure::Interrupted(Stop("original-admission-cancelled"))
    ));
}

#[test]
fn cancellation_on_the_last_scanned_node_precedes_absent_syntax() {
    let model = DeclarationModel::new(Arc::from(SOURCE), || Ok::<(), Stop>(())).unwrap();
    let declaration = record("number");
    let aliases = BTreeMap::new();
    let target = ByteRange { start: 0, end: 3 };
    let mut total = 0;
    let result = model
        .type_at(
            target,
            "query",
            input(&declaration, &aliases),
            Arc::new(|| Ok::<(), Stop>(())),
            |error| error.0.into(),
            || {
                total += 1;
                Ok(())
            },
        )
        .unwrap();
    assert!(result.is_none());
    assert!(total > 0);
    let cancelled = Arc::new(AtomicBool::new(false));
    let query_cancelled = cancelled.clone();
    let check = Arc::new(move || {
        if query_cancelled.load(Ordering::Acquire) {
            Err(Stop("last-node-cancelled"))
        } else {
            Ok(())
        }
    });
    let mut visited = 0;
    let result = model.type_at(
        target,
        "query",
        input(&declaration, &aliases),
        check,
        |error| error.0.into(),
        || {
            visited += 1;
            if visited == total {
                cancelled.store(true, Ordering::Release);
            }
            Ok(())
        },
    );
    assert!(matches!(
        result,
        Err(DeclarationFailure::Interrupted(Stop("last-node-cancelled")))
    ));
}
