use super::*;
use crate::OriginInput;
use serde_json::json;

#[derive(Debug, PartialEq, Eq)]
struct Stop(&'static str);

fn slot(index: usize, name: &str, kind: &str) -> Value {
    json!({"index":index,"name":name,"type":kind})
}
fn callback(name: &str, parameters: Vec<Value>, returns: Vec<Value>) -> Value {
    json!({"kind":"callbacktype","name":name,"parameters":parameters,"returns":returns,
        "path":"Interface/AddOns/Blizzard_APIDocumentationGenerated/UITimerDocumentation.lua","line":42})
}
fn root(name: &str) -> Value {
    json!({"kind":"function","name":"C_Test.After","parameters":[slot(1,"callback",name)],"returns":[]})
}
fn input<'a>(record: &'a Value, aliases: &'a BTreeMap<String, String>) -> CallableInput<'a> {
    CallableInput {
        origin: OriginInput {
            material: "api",
            material_sha256: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            pointer: "/bindings/0/descriptor",
            identity: "authenticated",
        },
        declaration: record,
        catalog_identity: "catalog-sha256",
        aliases,
    }
}
fn projected(record: &Value, aliases: &BTreeMap<String, String>, records: &[Value]) -> Projection {
    project(
        &input(record, aliases),
        &|| Ok::<(), Stop>(()),
        &mut || Ok(()),
        &mut |name| {
            let matches: Vec<_> = records
                .iter()
                .filter(|record| record["name"] == name)
                .collect();
            Ok(match matches.as_slice() {
                [] => DeclarationLookup::Missing,
                [record] => DeclarationLookup::Unique((*record).clone()),
                _ => DeclarationLookup::Ambiguous,
            })
        },
    )
    .unwrap()
}
fn reason(result: Projection, expected: &str) {
    assert_eq!(result.signature.unwrap_err(), expected);
}
fn parameter(signature: &LuaFunctionType) -> &LuaType {
    signature.get_params()[0].1.as_ref().unwrap()
}

#[test]
fn callback_preserves_parameter_return_nilability_cardinality_and_provenance() {
    let mut optional = slot(1, "label", "string");
    optional["nilable"] = json!(true);
    let cb = callback(
        "Completion",
        vec![optional],
        vec![slot(1, "count", "integer"), slot(2, "ok", "boolean")],
    );
    let result = projected(&root("Completion"), &BTreeMap::new(), &[cb]);
    assert_eq!(result.declarations.len(), 1);
    assert_eq!(result.declarations[0].name, "Completion");
    assert_eq!(result.declarations[0].catalog_identity, "catalog-sha256");
    assert!(
        result.declarations[0]
            .path
            .ends_with("UITimerDocumentation.lua")
    );
    assert_eq!(result.declarations[0].line, 42);
    let signature = result.signature.unwrap();
    let LuaType::DocFunction(cb) = parameter(&signature) else {
        panic!("callback signature")
    };
    assert_eq!(cb.get_params()[0].0, "label");
    assert_eq!(
        parameter(cb),
        &LuaType::Union(Arc::new(LuaUnionType::from_vec(vec![
            LuaType::String,
            LuaType::Nil
        ])))
    );
    assert_eq!(
        cb.get_ret(),
        &LuaType::Variadic(Arc::new(VariadicType::Multi(vec![
            LuaType::Integer,
            LuaType::Boolean
        ])))
    );
    assert_eq!(
        signature.get_ret(),
        &LuaType::Variadic(Arc::new(VariadicType::Multi(vec![])))
    );
}

#[test]
fn empty_callback_is_exactly_zero_arguments_and_returns() {
    let result = projected(
        &root("EmptyCallback"),
        &BTreeMap::new(),
        &[callback("EmptyCallback", vec![], vec![])],
    );
    let signature = result.signature.unwrap();
    let LuaType::DocFunction(cb) = parameter(&signature) else {
        panic!("callback")
    };
    assert!(cb.get_params().is_empty());
    assert_eq!(
        cb.get_ret(),
        &LuaType::Variadic(Arc::new(VariadicType::Multi(vec![])))
    );
}

#[test]
fn optional_callback_and_uncertain_return_count_are_not_widened() {
    let mut record = root("Optional");
    record["parameters"][0]["nilable"] = json!(true);
    let mut cb = callback("Optional", vec![], vec![slot(1, "result", "string")]);
    cb["raw"] = json!({"MayReturnNothing":true});
    let result = projected(&record, &BTreeMap::new(), &[cb]);
    let signature = result.signature.unwrap();
    let LuaType::Union(union) = parameter(&signature) else {
        panic!("nilable callback")
    };
    let expected = LuaType::DocFunction(Arc::new(LuaFunctionType::new(
        AsyncState::None,
        false,
        false,
        vec![],
        LuaType::Variadic(Arc::new(VariadicType::Base(LuaType::Unknown))),
        None,
    )));
    assert_eq!(
        union.as_ref(),
        &LuaUnionType::from_vec(vec![expected, LuaType::Nil])
    );
}

#[test]
fn missing_ambiguous_wrong_name_and_noncallback_stay_unknown() {
    reason(
        projected(&root("Missing"), &BTreeMap::new(), &[]),
        "environment_type_unavailable",
    );
    let cb = callback("Duplicate", vec![], vec![]);
    reason(
        projected(&root("Duplicate"), &BTreeMap::new(), &[cb.clone(), cb]),
        "environment_declaration_ambiguous",
    );
    reason(
        projected(
            &root("Object"),
            &BTreeMap::new(),
            &[json!({"kind":"structure","name":"Object"})],
        ),
        "environment_type_unavailable",
    );
    let result = project(
        &input(&root("Expected"), &BTreeMap::new()),
        &|| Ok::<(), Stop>(()),
        &mut || Ok(()),
        &mut |_| Ok(DeclarationLookup::Unique(callback("Wrong", vec![], vec![]))),
    )
    .unwrap();
    reason(result, "environment_declaration_identity_invalid");
}

#[test]
fn unknown_nested_callback_parameter_is_not_generic_function() {
    let cb = callback(
        "KnownCallback",
        vec![slot(1, "value", "UnknownObject")],
        vec![],
    );
    reason(
        projected(&root("KnownCallback"), &BTreeMap::new(), &[cb]),
        "environment_type_unavailable",
    );
}

#[test]
fn callback_alias_and_cross_callback_cycles_fail_closed() {
    reason(
        projected(
            &root("A"),
            &BTreeMap::new(),
            &[
                callback("A", vec![slot(1, "b", "B")], vec![]),
                callback("B", vec![], vec![slot(1, "a", "A")]),
            ],
        ),
        "environment_type_cycle",
    );
    reason(
        projected(
            &root("A"),
            &BTreeMap::from([("A".into(), "B".into()), ("B".into(), "A".into())]),
            &[],
        ),
        "environment_type_cycle",
    );
    reason(
        projected(
            &root("Alias"),
            &BTreeMap::from([("Alias".into(), "Callback".into())]),
            &[callback(
                "Callback",
                vec![slot(1, "alias", "Alias")],
                vec![],
            )],
        ),
        "environment_type_cycle",
    );
}

#[test]
fn depth_limit_keeps_last_admitted_alias_and_rejects_next() {
    let aliases: BTreeMap<_, _> = (0..31)
        .map(|i| {
            (
                format!("A{i}"),
                if i == 30 {
                    "number".into()
                } else {
                    format!("A{}", i + 1)
                },
            )
        })
        .collect();
    assert!(projected(&root("A0"), &aliases, &[]).signature.is_ok());
    let mut deeper = aliases;
    deeper.insert("number".into(), "integer".into());
    reason(
        projected(&root("A0"), &deeper, &[]),
        "environment_type_depth",
    );
    let records: Vec<_> = (0..32)
        .map(|i| {
            callback(
                &format!("C{i}"),
                if i == 31 {
                    vec![]
                } else {
                    vec![slot(1, "next", &format!("C{}", i + 1))]
                },
                vec![],
            )
        })
        .collect();
    reason(
        projected(&root("C0"), &BTreeMap::new(), &records),
        "environment_type_depth",
    );
}

#[test]
fn repeated_names_resolve_once_but_expanded_dag_has_a_total_work_limit() {
    let mut calls = BTreeMap::<String, usize>::new();
    let records: BTreeMap<_, _> = (0..12)
        .map(|i| {
            let name = format!("C{i}");
            let args = if i == 11 {
                vec![]
            } else {
                vec![
                    slot(1, "left", &format!("C{}", i + 1)),
                    slot(2, "right", &format!("C{}", i + 1)),
                ]
            };
            (name.clone(), callback(&name, args, vec![]))
        })
        .collect();
    let result = project(
        &input(&root("C0"), &BTreeMap::new()),
        &|| Ok::<(), Stop>(()),
        &mut || Ok(()),
        &mut |name| {
            *calls.entry(name.into()).or_default() += 1;
            Ok(DeclarationLookup::Unique(records[name].clone()))
        },
    )
    .unwrap();
    reason(result, "environment_projection_budget");
    assert!(calls.values().all(|count| *count == 1));
}

#[test]
fn signature_slot_indices_and_limits_are_preserved() {
    let mut bad = callback("Bad", vec![slot(2, "x", "number")], vec![]);
    reason(
        projected(&root("Bad"), &BTreeMap::new(), &[bad.clone()]),
        "environment_parameter_index_invalid",
    );
    bad["parameters"] = json!([]);
    bad["returns"] = json!([slot(2, "x", "number")]);
    reason(
        projected(&root("Bad"), &BTreeMap::new(), &[bad]),
        "environment_return_index_invalid",
    );
    let slots: Vec<_> = (1..=32)
        .map(|i| slot(i, &format!("x{i}"), "number"))
        .collect();
    assert!(
        projected(
            &root("Bound"),
            &BTreeMap::new(),
            &[callback("Bound", slots.clone(), slots.clone())]
        )
        .signature
        .is_ok()
    );
    let mut extra = slots;
    extra.push(slot(33, "extra", "number"));
    reason(
        projected(
            &root("Bound"),
            &BTreeMap::new(),
            &[callback("Bound", extra, vec![])],
        ),
        "environment_signature_budget",
    );
}

#[test]
fn metadata_byte_and_node_limits_reject_before_unbounded_projection() {
    let mut cb = callback("Large", vec![], vec![]);
    cb["raw"] = json!({"text":"x".repeat(MAX_METADATA_BYTES+1)});
    reason(
        projected(&root("Large"), &BTreeMap::new(), &[cb]),
        "environment_declaration_budget",
    );
    let mut cb = callback("Many", vec![], vec![]);
    cb["raw"] = json!({"nodes":vec![Value::Null;MAX_METADATA_NODES]});
    reason(
        projected(&root("Many"), &BTreeMap::new(), &[cb]),
        "environment_declaration_budget",
    );
}

#[test]
fn resolver_error_and_last_step_cancellation_keep_original_payload() {
    let result = project(
        &input(&root("Callback"), &BTreeMap::new()),
        &|| Ok::<(), Stop>(()),
        &mut || Ok(()),
        &mut |_| Err(Stop("resolver")),
    );
    assert!(matches!(
        result,
        Err(DeclarationFailure::Interrupted(Stop("resolver")))
    ));
    let cancelled = std::cell::Cell::new(false);
    let check = || {
        if cancelled.get() {
            Err(Stop("last-resolver"))
        } else {
            Ok(())
        }
    };
    let result = project(
        &input(&root("Callback"), &BTreeMap::new()),
        &check,
        &mut || Ok(()),
        &mut |_| {
            cancelled.set(true);
            Ok(DeclarationLookup::Missing)
        },
    );
    assert!(matches!(
        result,
        Err(DeclarationFailure::Interrupted(Stop("last-resolver")))
    ));
    cancelled.set(false);
    let result = project(
        &input(&root("number"), &BTreeMap::new()),
        &check,
        &mut || {
            cancelled.set(true);
            Ok(())
        },
        &mut |_| Ok(DeclarationLookup::Missing),
    );
    assert!(matches!(
        result,
        Err(DeclarationFailure::Interrupted(Stop("last-resolver")))
    ));
}
