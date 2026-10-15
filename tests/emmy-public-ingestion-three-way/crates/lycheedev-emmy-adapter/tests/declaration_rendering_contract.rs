//! Full-result/cancellation traces for the fixed legacy/public-API differential.
//! The runner executes these exact bytes against both frozen engine recipes.
use lycheedev_emmy_adapter::{
    ByteRange, CallableInput, DeclarationFailure, DeclarationLookup, DeclarationModel, OriginInput,
    TypeProjection,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Debug)]
struct Stop {
    axis: &'static str,
    ordinal: usize,
}
#[derive(Default)]
struct Trace {
    checks: usize,
    steps: usize,
    events: Vec<String>,
}
#[derive(Clone, Copy, Default)]
struct Fault {
    axis: &'static str,
    ordinal: usize,
    persistent: bool,
}
impl Fault {
    fn hits(self, axis: &str, ordinal: usize) -> bool {
        self.axis == axis
            && (ordinal == self.ordinal || (self.persistent && ordinal > self.ordinal))
    }
}
struct Case {
    name: String,
    source: String,
    needle: String,
    record: Value,
    aliases: BTreeMap<String, String>,
    declarations: Vec<Value>,
}
fn slot(index: usize, name: &str, ty: &str) -> Value {
    json!({"index":index,"name":name,"type":ty})
}
fn case(name: &str, ty: &str) -> Case {
    Case {
        name: name.into(),
        source: "---@param value number\nfunction C_Test.Test(value) return 901 end\n".into(),
        needle: "C_Test.Test".into(),
        record: json!({"kind":"function","name":"C_Test.Test","parameters":[slot(1,"value",ty)],"returns":[],"raw":{}}),
        aliases: BTreeMap::new(),
        declarations: vec![],
    }
}
fn callback(name: &str, params: Vec<Value>, returns: Vec<Value>) -> Value {
    json!({"kind":"callbacktype","name":name,"parameters":params,"returns":returns,
        "path":"Interface/ExampleDocumentation.lua","line":41})
}
fn projection(result: Result<Option<TypeProjection>, DeclarationFailure<Stop>>) -> Value {
    match result {
        Ok(Some(value)) => json!({"known":value.known,"rendered":value.rendered,
            "coverage":value.coverage,"declarations":value.declarations}),
        Ok(None) => Value::Null,
        Err(DeclarationFailure::Interrupted(error)) => {
            json!({"interrupted":{"axis":error.axis,"ordinal":error.ordinal}})
        }
        Err(DeclarationFailure::InvalidInput(error)) => json!({"invalid":error}),
        Err(DeclarationFailure::Parse(error)) => {
            json!({"parse":{"count":error.count,"kind":error.first.kind,
            "start":error.first.range.start,"end":error.first.range.end}})
        }
    }
}
fn run(model: &DeclarationModel, case: &Case, fault: Fault) -> Value {
    let trace = Arc::new(Mutex::new(Trace::default()));
    let checked = trace.clone();
    let start = case.source.find(&case.needle).unwrap() as u32;
    let result = model.type_at_with_declarations(
        ByteRange {
            start,
            end: start + case.needle.len() as u32,
        },
        "query/精确-identity",
        CallableInput {
            origin: OriginInput {
                material: "api",
                material_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                pointer: "/binding/descriptor",
                identity: "admitted",
            },
            declaration: &case.record,
            catalog_identity: "catalog/known",
            aliases: &case.aliases,
        },
        Arc::new(move || {
            let mut trace = checked.lock().unwrap();
            trace.checks += 1;
            let ordinal = trace.checks;
            trace.events.push(format!("check:{ordinal}"));
            if fault.hits("check", ordinal) {
                Err(Stop {
                    axis: "check",
                    ordinal,
                })
            } else {
                Ok(())
            }
        }),
        |error| format!("original_{}_{}", error.axis, error.ordinal),
        || {
            let mut trace = trace.lock().unwrap();
            trace.steps += 1;
            let ordinal = trace.steps;
            trace.events.push(format!("step:{ordinal}"));
            if fault.hits("step", ordinal) {
                Err(Stop {
                    axis: "step",
                    ordinal,
                })
            } else {
                Ok(())
            }
        },
        |name| {
            let mut trace = trace.lock().unwrap();
            trace.events.push(format!("resolve:{name}"));
            if fault.hits("resolve", 1) {
                return Err(Stop {
                    axis: "resolve",
                    ordinal: 1,
                });
            }
            let matches = case
                .declarations
                .iter()
                .filter(|value| value["name"] == name)
                .collect::<Vec<_>>();
            Ok(match matches.as_slice() {
                [] => DeclarationLookup::Missing,
                [value] => DeclarationLookup::Unique((*value).clone()),
                _ => DeclarationLookup::Ambiguous,
            })
        },
    );
    let trace = trace.lock().unwrap();
    json!({"case":case.name,"fault":{"axis":fault.axis,"ordinal":fault.ordinal,"persistent":fault.persistent},
        "result":projection(result),"checks":trace.checks,"steps":trace.steps,"events":trace.events})
}
fn emit(value: Value) {
    println!("\nDECLARATION_PARITY {}", value);
}

#[test]
fn full_projection_primitive_alias_callback_and_unavailable_matrix() {
    let mut cases = vec![];
    for ty in [
        "number",
        "integer",
        "string",
        "bool",
        "boolean",
        "nil",
        "any",
        "function",
        "LuaFunction",
        "table",
        "UnknownNominal",
        "T",
    ] {
        cases.push(case(ty, ty));
    }
    let mut aliased = case("borrowed-alias", "Completion");
    aliased.aliases.insert("Completion".into(), "string".into());
    cases.push(aliased);
    let mut cycle = case("alias-cycle", "A");
    cycle.aliases = BTreeMap::from([("A".into(), "B".into()), ("B".into(), "A".into())]);
    cases.push(cycle);
    let mut rich = case("nested-callback-union-returns-names", "Completion");
    rich.record["parameters"][0]["name"] = json!("callback_精确");
    rich.record["parameters"][0]["nilable"] = json!(true);
    rich.record["returns"] = json!([slot(1, "first", "nil"), slot(2, "second", "boolean")]);
    let mut optional = slot(1, "label_精确", "string");
    optional["nilable"] = json!(true);
    rich.declarations = vec![
        callback(
            "Completion",
            vec![optional, slot(2, "next", "Nested")],
            vec![slot(1, "count", "integer"), slot(2, "ok", "boolean")],
        ),
        callback("Nested", vec![], vec![]),
    ];
    cases.push(rich);
    let mut ambiguous = case("ambiguous-callback", "Duplicate");
    let declaration = callback("Duplicate", vec![], vec![]);
    ambiguous.declarations = vec![declaration.clone(), declaration];
    cases.push(ambiguous);
    let mut unknown_nested = case("unknown-nested-is-not-generic-function", "Outer");
    unknown_nested.declarations = vec![callback("Outer", vec![slot(1, "unknown", "T")], vec![])];
    cases.push(unknown_nested);
    let mut maybe_empty = case("uncertain-return-count", "number");
    maybe_empty.record["returns"] = json!([slot(1, "result", "string")]);
    maybe_empty.record["raw"] = json!({"MayReturnNothing":true});
    cases.push(maybe_empty);
    let mut receiver = case("unsupported-receiver", "number");
    receiver.record["owner"] = json!("Frame");
    cases.push(receiver);
    let mut inner = case("unsupported-table-inner", "table");
    inner.record["parameters"][0]["innerType"] = json!("string");
    cases.push(inner);
    for case in cases {
        let model =
            DeclarationModel::new(Arc::from(case.source.as_str()), || Ok::<_, Stop>(())).unwrap();
        let before = case.record.clone();
        let value = run(&model, &case, Fault::default());
        let result = &value["result"];
        assert_eq!(result["coverage"][0]["operation"], "resolve");
        assert_eq!(result["coverage"][0]["proof"], "query/精确-identity");
        assert!(result["coverage"][0].get("reason").is_some());
        if result["known"] == false {
            assert_eq!(result["rendered"], "unknown");
        }
        assert_eq!(before, case.record);
        assert_eq!(
            value,
            run(&model, &case, Fault::default()),
            "query-local repeat"
        );
        emit(value);
    }
}

#[test]
fn every_check_and_step_keeps_complete_result_order_and_original_fault() {
    // Two domains: a successful callback with provenance and a failed projection.
    // Fault every check/step, including the boundaries before/after projection,
    // selected-value resolution and rendering. Single-shot checks distinguish
    // the old unavailable coverage path from persistent cancellation.
    for mut case in [
        case("callback-checkpoints", "Callback"),
        case("unavailable-checkpoints", "Missing"),
    ] {
        if case.name == "callback-checkpoints" {
            case.declarations = vec![callback("Callback", vec![slot(1, "arg", "nil")], vec![])];
        }
        let model =
            DeclarationModel::new(Arc::from(case.source.as_str()), || Ok::<_, Stop>(())).unwrap();
        let success = run(&model, &case, Fault::default());
        for axis in ["check", "step"] {
            let total = success[if axis == "check" { "checks" } else { "steps" }]
                .as_u64()
                .unwrap() as usize;
            assert!(total > 3);
            for ordinal in 1..=total {
                for persistent in [false, true] {
                    let value = run(
                        &model,
                        &case,
                        Fault {
                            axis,
                            ordinal,
                            persistent,
                        },
                    );
                    let result = &value["result"];
                    assert!(result.get("interrupted").is_some() || result["known"] == false);
                    if let Some(error) = result.get("interrupted") {
                        assert_eq!(error["axis"], axis);
                        assert!(error["ordinal"].as_u64().unwrap() >= ordinal as u64);
                    }
                    emit(value);
                }
            }
        }
        emit(success);
        emit(run(
            &model,
            &case,
            Fault {
                axis: "resolve",
                ordinal: 1,
                persistent: false,
            },
        ));
    }
}

#[test]
fn selected_expression_shapes_keep_full_owned_projection() {
    for (name, source, needle) in [
        ("name", "return value\n", "value"),
        ("index", "local value = C_Test.Test\n", "C_Test.Test"),
        (
            "scalar-call",
            "local value = C_Test.Test()\n",
            "C_Test.Test()",
        ),
        ("tail-call", "return C_Test.Test()\n", "C_Test.Test()"),
        ("constant", "local value = 901\n", "901"),
        (
            "utf8-crlf",
            "-- 精确\r\nfunction C_Test.Test(value) end\r\n",
            "Test",
        ),
    ] {
        let mut case = case(name, "string");
        case.source = source.into();
        case.needle = needle.into();
        let model = DeclarationModel::new(Arc::from(source), || Ok::<_, Stop>(())).unwrap();
        let result = run(&model, &case, Fault::default());
        assert_eq!(result["result"]["known"], true);
        emit(result);
    }
}
