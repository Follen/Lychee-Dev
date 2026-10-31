//! Fixed semantic contracts formerly checked only by the unmaintained native probes.
use super::*;
use emmylua_code_analysis::{
    CheckConfig, DiagnosticCode, FileId, LuaMemberKey, LuaType, VariadicType,
};
use emmylua_parser::LuaCallExpr;
use lycheedev_core::ToolkitError;
use lycheedev_emmy_adapter::session::{
    DiagnosticRun, OwnedBindingSession as BindingSession, OwnedQuery, OwnedValue,
};
use std::time::{Duration, Instant};

fn source(name: &str) -> &'static str {
    match name {
        "profile.lua" => include_str!("../../../../tests/fixtures/source-native/wow/profile.lua"),
        "generic-defs.lua" => {
            include_str!("../../../../tests/fixtures/source-native/wow/native/generic-defs.lua")
        }
        "generic-calls.lua" => {
            include_str!("../../../../tests/fixtures/source-native/wow/native/generic-calls.lua")
        }
        "generic-replace.lua" => {
            include_str!("../../../../tests/fixtures/source-native/wow/native/generic-replace.lua")
        }
        "coverage-unknown-argument.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/coverage-unknown-argument.lua"
        ),
        "object-versions.lua" => {
            include_str!("../../../../tests/fixtures/source-native/wow/native/object-versions.lua")
        }
        "audit-rebind.lua" => {
            include_str!("../../../../tests/fixtures/source-native/wow/native/audit-rebind.lua")
        }
        "audit-colon.lua" => {
            include_str!("../../../../tests/fixtures/source-native/wow/native/audit-colon.lua")
        }
        "audit-field-closure.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/audit-field-closure.lua"
        ),
        "audit-expression.lua" => {
            include_str!("../../../../tests/fixtures/source-native/wow/native/audit-expression.lua")
        }
        "invocation-captures.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-captures.lua"
        ),
        "invocation-reassign.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-reassign.lua"
        ),
        "invocation-short-circuit.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-short-circuit.lua"
        ),
        "invocation-selector.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-selector.lua"
        ),
        "invocation-escape.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-escape.lua"
        ),
        "invocation-escape-closure.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-escape-closure.lua"
        ),
        "invocation-escape-rebind.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-escape-rebind.lua"
        ),
        "invocation-call-capture.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-call-capture.lua"
        ),
        "invocation-return-slots.lua" => include_str!(
            "../../../../tests/fixtures/source-native/wow/native/invocation-return-slots.lua"
        ),
        _ => panic!("unknown closed fixture {name}"),
    }
}
fn prepared(names: &[&str]) -> PreparedSnapshot {
    let control = OperationControl::uncancelable(
        Instant::now() + Duration::from_millis(SourceLimits::default().total_milliseconds),
    );
    let mut config = Emmyrc::default();
    config.runtime.version = emmylua_code_analysis::EmmyrcLuaVersion::Lua51;
    config.diagnostics.disable = DiagnosticCode::all()
        .into_iter()
        .filter(|code| {
            !matches!(
                code,
                DiagnosticCode::ParamTypeMismatch | DiagnosticCode::ReturnTypeMismatch
            )
        })
        .collect();
    config.diagnostics.enables = vec![
        DiagnosticCode::ParamTypeMismatch,
        DiagnosticCode::ReturnTypeMismatch,
    ];
    let policy = b"Lychee Dev fixed fixture host: explicit Mixin, CreateFromMixins and setmetatable shallow-copy profile v1";
    let text = source("profile.lua");
    let intrinsics = ["Mixin", "CreateFromMixins", "setmetatable"]
        .into_iter()
        .map(|name| {
            let start = text.find(&format!("function {name}")).unwrap() + "function ".len();
            IntrinsicDeclaration {
                file: "profile.lua".into(),
                source_sha256: hash(text.as_bytes()),
                start: start as u32,
                end: (start + name.len()) as u32,
                name: name.into(),
                rule: PROFILE.into(),
                host_material: "compiled-fixture-host-v1".into(),
            }
        })
        .collect();
    let host = TrustedHostEnvironment::from_host_material(
        "closed-fixture-initial-globals-v1".into(),
        true,
        vec![HostMaterialInput {
            name: "compiled-fixture-host-v1".into(),
            bytes: policy.to_vec(),
            expected_sha256: hash(policy),
        }],
        intrinsics,
        &control,
    )
    .unwrap();
    let luac: Value =
        serde_json::from_str(include_str!("../../../../release/tools/luac.json")).unwrap();
    let runtime = &luac["runtime"];
    let inputs = SourceInputs::admit(SourceInputSpec {
        files: names.iter().map(|name| { let text = source(name); SourceFileInput { path: (*name).into(), original_utf8: text.into(), content_sha256: hash(text.as_bytes()), analysis_utf8: text.into(), map: SourceMap::Identity } }).collect(),
        workspace_root: std::env::current_dir().unwrap().join("owned-context-contract"),
        source_identity: json!({"kind":"compiled-fixture-inputs","schema":"lycheedev.native-fixture-admission.v1"}),
        phase: "unknown".into(), ordered_occurrences: names.iter().map(|name| (*name).into()).collect(), config, host,
        luac: LuacIdentity { version: runtime["version"].as_str().unwrap().into(), platform: runtime["platform"].as_str().unwrap().into(), executable_sha256: runtime["sha256"].as_str().unwrap().into(), executable_bytes: runtime["bytes"].as_u64().unwrap() }, limits: SourceLimits::default(),
    }, &control).unwrap();
    prepare(inputs, &control).unwrap()
}
fn site(file: &str, marker: &str, expression: &str) -> Site {
    let text = source(file);
    let marker = if text.contains(marker) {
        marker.to_owned()
    } else {
        marker.replace('\n', "\r\n")
    };
    assert_eq!(
        text.matches(&marker).count(),
        1,
        "unique fixture marker {marker}"
    );
    let start = text.find(&marker).unwrap() + marker.rfind(expression).unwrap();
    Site {
        file: file.into(),
        start: start as u32,
        end: (start + expression.len()) as u32,
    }
}
fn query<'a, 'c>(
    p: &'a PreparedSnapshot,
    session: &'a BindingSession,
    file: FileId,
    check: &'c mut dyn FnMut() -> lycheedev_core::Result<()>,
) -> OwnedQuery<'a, 'c, ToolkitError> {
    OwnedQuery::new_with_contribution_order(
        p.analysis(),
        session,
        file,
        check,
        p.contribution_order(),
    )
    .with_workspace_facts(p.workspace_facts())
}
fn label(ty: &LuaType) -> &'static str {
    match ty {
        LuaType::Number | LuaType::Integer | LuaType::IntegerConst(_) | LuaType::FloatConst(_) => {
            "number"
        }
        LuaType::String | LuaType::StringConst(_) => "string",
        LuaType::Boolean | LuaType::BooleanConst(_) => "boolean",
        LuaType::Unknown => "unknown",
        LuaType::Any => "any",
        LuaType::Nil => "nil",
        LuaType::DocFunction(f) if !f.get_generic_params().is_empty() => "generic_function",
        LuaType::DocFunction(f) if matches!(f.get_ret(), LuaType::String) => "string_function",
        LuaType::DocFunction(_) | LuaType::Function | LuaType::Signature(_) => "function",
        _ => "other",
    }
}
fn diagnostics(
    p: &PreparedSnapshot,
    session: &BindingSession,
    file: &str,
) -> Vec<(String, u32, u32)> {
    let mut check = || p.operation_control().check();
    let mut q = query(p, session, p.files()[file], &mut check);
    let mut run = DiagnosticRun::default();
    q.diagnose_into(Arc::new(CheckConfig::new(p.config())), &mut run)
        .unwrap();
    assert!(
        run.complete,
        "diagnostics must complete before their absence is evidence"
    );
    run.diagnostics
        .iter()
        .map(|d| {
            (
                d.code.get_name().to_owned(),
                u32::from(d.range.start()),
                u32::from(d.range.end()),
            )
        })
        .collect()
}

#[test]
fn generic_phase_and_load_order_contexts_remain_isolated_under_interleaved_queries() {
    let names = [
        "profile.lua",
        "generic-defs.lua",
        "generic-calls.lua",
        "generic-replace.lua",
    ];
    let reverse = [
        "profile.lua",
        "generic-replace.lua",
        "generic-defs.lua",
        "generic-calls.lua",
    ];
    let p = prepared(&names);
    // Unknown phase still has a known load order: only deferred-body value knowledge is unknown.
    let graphs = [
        p.build_plan(
            "after-a",
            "after_load",
            LoadPlan::Known(names.iter().map(|name| (*name).to_owned()).collect()),
            None,
            None,
            p.operation_control(),
        )
        .unwrap(),
        p.build_plan(
            "unknown-a",
            "unknown",
            LoadPlan::Known(names.iter().map(|name| (*name).to_owned()).collect()),
            None,
            None,
            p.operation_control(),
        )
        .unwrap(),
        p.build_plan(
            "after-b",
            "after_load",
            LoadPlan::Known(reverse.iter().map(|name| (*name).to_owned()).collect()),
            None,
            None,
            p.operation_control(),
        )
        .unwrap(),
    ];
    let providers: Vec<_> = graphs
        .iter()
        .map(|g| {
            p.freeze(
                g,
                NativeViewSelection::SourceTemplates,
                p.operation_control(),
            )
            .unwrap()
        })
        .collect();
    let sessions: Vec<_> = providers
        .iter()
        .map(|v| BindingSession::new_checked(v.clone(), || p.operation_control().check()).unwrap())
        .collect();
    let checks = [
        ("local numberResult = Identity(7)", "Identity(7)", "number"),
        (
            "local stringResult = Identity(\"seven\")",
            "Identity(\"seven\")",
            "string",
        ),
        (
            "local copiedNumber = Copied.echo(7)",
            "Copied.echo(7)",
            "number",
        ),
        (
            "local replacedString = Source.echo(\"seven\")",
            "Source.echo(\"seven\")",
            "string",
        ),
        (
            "local colonNumber = Methods:Echo(\"label\", 7)",
            "Methods:Echo(\"label\", 7)",
            "number",
        ),
        (
            "local dotString = Methods.Echo(Methods, \"label\", \"seven\")",
            "Methods.Echo(Methods, \"label\", \"seven\")",
            "string",
        ),
        (
            "function WrongCopiedReturn()\n    return Copied.echo(7)",
            "Copied.echo(7)",
            "number",
        ),
        (
            "function WrongMethodReturn()\n    return Methods:Echo(\"label\", 7)",
            "Methods:Echo(\"label\", 7)",
            "number",
        ),
        (
            "function WrongLateReturn()\n    return Identity(7)",
            "Identity(7)",
            "unknown",
        ),
    ];
    // Keep all three actual query caches alive while switching their immutable contexts.
    let control = p.operation_control();
    let mut control_checks = std::array::from_fn::<_, 3, _>(|_| move || control.check());
    let mut queries: Vec<_> = sessions
        .iter()
        .zip(control_checks.iter_mut())
        .map(|(session, check)| query(&p, session, p.files()["generic-calls.lua"], check))
        .collect();
    for index in [0usize, 1, 2, 0, 2, 1] {
        let id = p.files()["generic-calls.lua"];
        let q = &mut queries[index];
        assert_eq!(sessions[index].identity(), graphs[index].snapshot_key);
        for (n, (marker, expression, normal)) in checks.iter().enumerate() {
            let native = p
                .binding_site(&site("generic-calls.lua", marker, expression))
                .unwrap();
            let want = if index == 1 && n >= 6 {
                "unknown"
            } else if index == 2 && n == 8 {
                "number"
            } else {
                *normal
            };
            assert_eq!(
                label(q.type_of_expr(native.syntax).unwrap().structural_type()),
                want,
                "context {index} {marker}"
            );
            let model = p.analysis().semantic_model(id);
            let tree = model.syntax_tree().unwrap();
            let call = LuaCallExpr::cast(
                native
                    .syntax
                    .to_node_from_root(&tree.get_red_root())
                    .unwrap(),
            )
            .unwrap();
            let candidates = q
                .callable_candidates_for_expr(&call.get_prefix_expr().unwrap())
                .unwrap();
            // A known selected signature remains a candidate when its fixed argument is invalid.
            assert_eq!(candidates.len(), usize::from(!(index == 1 && n >= 6)));
            if index == 0 && n == 8 {
                assert_eq!(
                    candidates[0].function_parameters().unwrap(),
                    vec![Some(OwnedValue::plain(LuaType::String))]
                );
                assert_eq!(
                    candidates[0].function_return().unwrap().structural_type(),
                    &LuaType::String
                );
            }
        }
    }
    assert_ne!(sessions[0].identity(), sessions[1].identity());
    assert_ne!(sessions[0].identity(), sessions[2].identity());
    assert_ne!(sessions[1].identity(), sessions[2].identity());
    for index in 0..3 {
        let mut check = || p.operation_control().check();
        let mut q = query(
            &p,
            &sessions[index],
            p.files()["generic-defs.lua"],
            &mut check,
        );
        let native = p
            .binding_site(&site(
                "generic-defs.lua",
                "function Identity(value)\n    return value",
                "value",
            ))
            .unwrap();
        assert!(matches!(
            q.binding_decision(native.syntax).unwrap(),
            BindingDecision::Bound(BoundValue {
                value: SelectedValue::Parameter(_),
                ..
            })
        ));
        let declaration = graphs[index]
            .functions
            .iter()
            .find(|f| f.name == "WrongLateReturn" && f.declaration.file == "generic-calls.lua")
            .unwrap();
        let signature = q
            .type_of_signature_in_file(
                p.files()["generic-calls.lua"],
                p.binding_site(&declaration.closure).unwrap().syntax,
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            signature.function_return().unwrap().structural_type(),
            &LuaType::Number
        );
        // Unknown from the invalid fixed argument does not become a fabricated return mismatch.
        // The independently declared string parameter still emits its own exact argument error.
        let mut diagnostic_check = || p.operation_control().check();
        let mut diagnostic_query = query(
            &p,
            &sessions[index],
            p.files()["generic-calls.lua"],
            &mut diagnostic_check,
        );
        let mut run = DiagnosticRun::default();
        diagnostic_query
            .diagnose_into(Arc::new(CheckConfig::new(p.config())), &mut run)
            .unwrap();
        assert!(run.complete, "context {index}: diagnostics must complete");
        let actual = run
            .diagnostics
            .iter()
            .map(|d| {
                (
                    d.code.get_name().to_owned(),
                    u32::from(d.range.start()),
                    u32::from(d.range.end()),
                )
            })
            .collect::<Vec<_>>();
        // Inspect the same cold checker query after execution, without warming its caches first.
        let late_site = site("generic-calls.lua", checks[8].0, checks[8].1);
        let late_binding = p.binding_site(&late_site).unwrap();
        let checked_result = diagnostic_query
            .type_of_expr_at(late_binding.syntax, late_site.start)
            .unwrap();
        let declared_return = diagnostic_query
            .return_type(p.binding_site(&declaration.closure).unwrap().syntax)
            .unwrap()
            .unwrap();
        assert_eq!(declared_return.structural_type(), &LuaType::Number);
        if index == 2 {
            // Generic widening retains Integer; the return checker must accept it as Number.
            assert_eq!(checked_result.structural_type(), &LuaType::Integer);
        }
        let mut expected = Vec::new();
        for (marker, expression, code, enabled) in [
            (checks[6].0, checks[6].1, "return-type-mismatch", index != 1),
            (checks[7].0, checks[7].1, "return-type-mismatch", index != 1),
            (
                "local wrongLabel = Methods:Echo(99, 7)",
                "99",
                "param-type-mismatch",
                true,
            ),
            (checks[8].0, "7", "param-type-mismatch", index == 0),
        ] {
            if enabled {
                let s = site("generic-calls.lua", marker, expression);
                expected.push((code.to_owned(), s.start, s.end));
            }
        }
        let mut sorted = actual;
        sorted.sort();
        expected.sort();
        assert_eq!(
            sorted,
            expected,
            "context {index}: late checked result={:?}; declared return={:?}; diagnostics={:?}",
            checked_result.structural_type(),
            declared_return.structural_type(),
            run.diagnostics
        );
        let coverage = sessions[index]
            .coverage_checked(|| p.operation_control().check())
            .unwrap();
        let late = p
            .binding_site(&site("generic-calls.lua", checks[8].0, checks[8].1))
            .unwrap();
        assert_eq!(
            coverage
                .iter()
                .find(|r| r.site == late && r.operation == "return_check")
                .map(|r| r.state),
            // This deferred function was not invoked: even a known result type proves no arity.
            Some("unavailable")
        );
        let callee = p
            .binding_site(&site("generic-calls.lua", checks[2].0, "Copied.echo"))
            .unwrap();
        let BindingDecision::Bound(value) = providers[index].resolve(callee) else {
            panic!("copied callee must retain identity")
        };
        let BindingDecision::Bound(refs) = sessions[index]
            .references_checked(&value.identity, || p.operation_control().check())
            .unwrap()
        else {
            panic!("frozen references")
        };
        assert!(refs.iter().any(|r| r.site == callee));
    }
}

#[test]
fn object_epochs_members_foreign_queries_and_generic_returns_preserve_selected_identity() {
    let names = [
        "profile.lua",
        "generic-defs.lua",
        "generic-calls.lua",
        "generic-replace.lua",
        "object-versions.lua",
    ];
    let order = [
        "profile.lua",
        "generic-replace.lua",
        "generic-defs.lua",
        "generic-calls.lua",
        "object-versions.lua",
    ];
    let p = prepared(&names);
    let g = p
        .build_plan(
            "object-contract",
            "after_load",
            LoadPlan::Known(order.iter().map(|name| (*name).to_owned()).collect()),
            None,
            Some("object-native-api-v1"),
            p.operation_control(),
        )
        .unwrap();
    let provider = p
        .freeze(
            &g,
            NativeViewSelection::SourceTemplates,
            p.operation_control(),
        )
        .unwrap();
    let session =
        BindingSession::new_checked(provider.clone(), || p.operation_control().check()).unwrap();
    let file = "object-versions.lua";
    let mut check = || p.operation_control().check();
    let mut q = query(&p, &session, p.files()[file], &mut check);
    let mut foreign_check = || p.operation_control().check();
    let mut foreign = query(
        &p,
        &session,
        p.files()["generic-defs.lua"],
        &mut foreign_check,
    );
    let value_key = LuaMemberKey::Name("value".into());
    // The original NeedNumber annotation requires a number-valued field.
    // Compare its selected scalar value without asserting whole-record compatibility.
    let expected_value = OwnedValue::plain(LuaType::Number);
    let specs = [
        (
            "local good = NeedNumber(Source)",
            "Source",
            "sourceOld",
            "number",
            "generic_function",
            false,
        ),
        (
            "local bad = NeedNumber(Source)",
            "Source",
            "sourceNew",
            "string",
            "string_function",
            true,
        ),
        (
            "local stillGood = NeedNumber(Copied)",
            "Copied",
            "copiedOld",
            "number",
            "generic_function",
            false,
        ),
        (
            "local copiedBad = NeedNumber(Copied)",
            "Copied",
            "copiedNew",
            "string",
            "string_function",
            true,
        ),
        (
            "local twinValue = Twin.value",
            "Twin",
            "twin",
            "string",
            "string_function",
            true,
        ),
        (
            "local dynamic = NeedNumber(Source)",
            "Source",
            "dynamic",
            "unknown",
            "unknown",
            true,
        ),
    ];
    let mut values = Vec::new();
    let mut objects = Vec::new();
    for (marker, object, ..) in specs {
        let native = p.binding_site(&site(file, marker, object)).unwrap();
        values.push(q.type_of_expr(native.syntax).unwrap());
        let BindingDecision::Bound(BoundValue {
            value: SelectedValue::Object(object_ref),
            ..
        }) = provider.resolve(native)
        else {
            panic!("selected object identity required")
        };
        objects.push(object_ref);
        let mut facts = g
            .uses
            .iter()
            .filter(|f| f.site == site(file, marker, object));
        let fact = facts.next().unwrap();
        assert!(
            facts.next().is_none(),
            "object observation must have one execution context"
        );
        assert!(
            fact.object_view
                .is_some_and(|id| g.object_views.iter().any(|v| v.id == id))
        );
    }
    for index in [0usize, 1, 0, 2, 3, 2, 4, 5] {
        let (marker, object, reads, value, run, added) = specs[index];
        for current in [&mut q, &mut foreign] {
            assert_member_contract(current, &values[index], value, run, added, index == 5);
            // A structure receipt proves shape/ownership, not deep record assignability.
            assert_eq!(
                current.structure_allowed(&values[index]).unwrap(),
                index != 5
            );
            let member = current
                .member_type(&values[index], &value_key)
                .unwrap()
                .unwrap();
            if index != 5 {
                assert_eq!(
                    current.type_check(&member, &expected_value).unwrap(),
                    matches!(index, 0 | 2)
                );
            } else {
                assert_eq!(label(member.structural_type()), "unknown");
            }
        }
        for (field, suffix, want) in [
            ("value", "Value", value),
            ("run", "Run", run),
            (
                "added",
                "Added",
                if index == 5 {
                    "unknown"
                } else if added {
                    "boolean"
                } else {
                    "nil"
                },
            ),
        ] {
            let expression = format!("{object}.{field}");
            let marker = format!("local {reads}{suffix} = {expression}");
            assert_eq!(
                label(
                    q.type_of_expr(
                        p.binding_site(&site(file, &marker, &expression))
                            .unwrap()
                            .syntax
                    )
                    .unwrap()
                    .structural_type()
                ),
                want,
                "{marker}"
            );
        }
        let native = p.binding_site(&site(file, marker, object)).unwrap();
        let coverage = session
            .coverage_checked(|| p.operation_control().check())
            .unwrap();
        assert_eq!(
            coverage
                .iter()
                .find(|r| r.site == native && r.operation == "structure_check")
                .map(|r| r.state),
            Some(if index == 5 {
                "unavailable"
            } else {
                "native_checked"
            })
        );
    }
    for (left, right, same) in [
        (0, 1, true),
        (2, 3, true),
        (1, 3, false),
        (1, 4, false),
        (1, 5, true),
    ] {
        assert_eq!(objects[left].identity == objects[right].identity, same);
        if same {
            assert_ne!(objects[left].epoch, objects[right].epoch);
        }
        assert_ne!(values[left], values[right]);
    }
    let refs = |index: usize| {
        let native = p
            .binding_site(&site(file, specs[index].0, specs[index].1))
            .unwrap();
        let BindingDecision::Bound(bound) = provider.resolve(native) else {
            panic!("object")
        };
        let BindingDecision::Bound(refs) = session
            .references_checked(&bound.identity, || p.operation_control().check())
            .unwrap()
        else {
            panic!("object references")
        };
        refs.into_iter().map(|r| r.site).collect::<Vec<_>>()
    };
    let source_refs = refs(1);
    let twin_refs = refs(4);
    assert!(!source_refs.is_empty() && !twin_refs.is_empty());
    assert!(source_refs.iter().all(|r| !twin_refs.contains(r)));
    for (marker, expression, index) in [
        (
            "local oldGeneric = Identity(Source)",
            "Identity(Source)",
            0usize,
        ),
        ("local newGeneric = Identity(Source)", "Identity(Source)", 1),
        (
            "local copiedGeneric = Identity(Copied)",
            "Identity(Copied)",
            3,
        ),
        (
            "local dynamicGeneric = Identity(Source)",
            "Identity(Source)",
            5,
        ),
    ] {
        let call_site = site(file, marker, expression);
        let call = g.calls.iter().find(|c| c.site == call_site).unwrap();
        let argument = &g.uses[call.arguments[0]];
        let argument_binding = p.binding_site(&argument.site).unwrap();
        let call_binding = p.binding_site(&call_site).unwrap();
        let argument_type = q.type_of_expr(argument_binding.syntax).unwrap();
        let result = q.type_of_expr(call_binding.syntax).unwrap();
        assert_eq!(argument_type.structural_type(), result.structural_type());
        assert_eq!(argument_type.carried_fields(), result.carried_fields());
        let argument_object = argument_type.object_info().unwrap();
        let result_object = result.object_info().unwrap();
        assert_eq!(argument_object.context, session.identity());
        assert_eq!(
            (
                argument_object.context,
                argument_object.identity,
                argument_object.epoch,
                argument_object.view,
                argument_object.proof,
                argument_object.complete
            ),
            (
                result_object.context,
                result_object.identity,
                result_object.epoch,
                result_object.view,
                result_object.proof,
                result_object.complete
            ),
        );
        assert_eq!(argument_object.site, argument_binding);
        assert_eq!(result_object.site, call_binding);
        assert_eq!(q.structure_allowed(&argument_type).unwrap(), index != 5);
        for current in [&mut q, &mut foreign] {
            assert_member_contract(
                current,
                &result,
                specs[index].3,
                specs[index].4,
                specs[index].5,
                index == 5,
            );
        }
        assert_eq!(q.structure_allowed(&result).unwrap(), index != 5);
    }
}

fn assert_member_contract(
    q: &mut OwnedQuery<'_, '_, ToolkitError>,
    value: &OwnedValue,
    want_value: &str,
    want_run: &str,
    added: bool,
    dynamic: bool,
) {
    let mut keys: Vec<_> = q
        .member_infos(value)
        .unwrap()
        .iter()
        .filter_map(|m| m.key.name().map(str::to_owned))
        .collect();
    keys.sort();
    keys.dedup();
    if !dynamic {
        assert_eq!(
            keys,
            if added {
                vec!["added", "run", "value"]
            } else {
                vec!["run", "value"]
            }
        );
    }
    for (field, want) in [
        ("value", Some(want_value)),
        ("run", Some(want_run)),
        ("added", added.then_some("boolean")),
    ] {
        let key = LuaMemberKey::Name(field.into());
        let one = q
            .member_info(value, &key)
            .unwrap()
            .map(|m| label(m.typ.structural_type()));
        let selected: Vec<_> = q
            .member_infos_with_key(value, &key)
            .unwrap()
            .iter()
            .map(|m| label(m.typ.structural_type()))
            .collect();
        let all: Vec<_> = q
            .member_infos_with_key_all(value, &key)
            .unwrap()
            .iter()
            .map(|m| label(m.typ.structural_type()))
            .collect();
        let typ = q
            .member_type(value, &key)
            .unwrap()
            .map(|m| label(m.structural_type()));
        if dynamic {
            assert!(
                one.into_iter()
                    .chain(typ)
                    .chain(selected)
                    .chain(all)
                    .all(|v| matches!(v, "unknown" | "any")),
                "dynamic fields must not acquire known member types"
            );
        } else {
            assert_eq!(one, want);
            assert_eq!(typ, want);
            assert_eq!(selected, want.into_iter().collect::<Vec<_>>());
            assert_eq!(all, want.into_iter().collect::<Vec<_>>());
        }
    }
}

#[test]
fn object_alias_writes_preserve_the_selected_scalar_values() {
    for (file, marker, expression, want) in [
        (
            "audit-rebind.lua",
            "local after = Alias.value",
            "Alias.value",
            "number",
        ),
        (
            "audit-colon.lua",
            "local after = o.value",
            "o.value",
            "string",
        ),
        (
            "audit-field-closure.lua",
            "local after = hidden.value",
            "hidden.value",
            "string",
        ),
        (
            "audit-expression.lua",
            "local after = o.value",
            "o.value",
            "string",
        ),
    ] {
        let names = ["profile.lua", file];
        let p = prepared(&names);
        let g = p
            .build_plan(
                file,
                "after_load",
                LoadPlan::Known(names.iter().map(|name| (*name).to_owned()).collect()),
                None,
                Some("independent-object-audit-v1"),
                p.operation_control(),
            )
            .unwrap();
        let provider = p
            .freeze(
                &g,
                NativeViewSelection::SourceTemplates,
                p.operation_control(),
            )
            .unwrap();
        let session =
            BindingSession::new_checked(provider, || p.operation_control().check()).unwrap();
        let mut check = || p.operation_control().check();
        let mut q = query(&p, &session, p.files()[file], &mut check);
        assert_eq!(
            label(
                q.type_of_expr(
                    p.binding_site(&site(file, marker, expression))
                        .unwrap()
                        .syntax
                )
                .unwrap()
                .structural_type()
            ),
            want,
            "{file}"
        );
    }
}

#[test]
fn capture_rebinding_escape_short_circuit_and_selected_invocations_preserve_values() {
    // Each tuple is a source-language expectation, not output captured from the engine.
    let cases: &[(&str, &[(&str, &str, &str)])] = &[
        (
            "invocation-captures.lua",
            &[
                (
                    "local observedFirstBefore = firstBefore",
                    "firstBefore",
                    "number",
                ),
                (
                    "local observedSecondBefore = secondBefore",
                    "secondBefore",
                    "string",
                ),
                (
                    "local observedFirstAfter = firstAfter",
                    "firstAfter",
                    "string",
                ),
                (
                    "local observedSecondAfter = secondAfter",
                    "secondAfter",
                    "string",
                ),
                ("local firstBefore = first(nil)", "first(nil)", "number"),
                ("local secondBefore = second(nil)", "second(nil)", "string"),
            ],
        ),
        (
            "invocation-reassign.lua",
            &[
                (
                    "local afterOriginal = original.value",
                    "original.value",
                    "number",
                ),
                (
                    "local afterRebound = rebound.value",
                    "rebound.value",
                    "string",
                ),
                ("local afterSelf = same.value", "same.value", "number"),
            ],
        ),
        (
            "invocation-short-circuit.lua",
            &[
                ("local beforeRight = Trace.b", "Trace.b", "number"),
                ("local afterRight = Trace.b", "Trace.b", "string"),
                ("local observedAnd = selectedAnd", "selectedAnd", "boolean"),
                ("local observedZero = zeroTruthy", "zeroTruthy", "number"),
            ],
        ),
        (
            "invocation-selector.lua",
            &[
                ("local observedFirst = first", "first", "number"),
                ("local observedSecond = second", "second", "string"),
            ],
        ),
        (
            "invocation-escape.lua",
            &[
                ("local afterEscaped = t.value", "t.value", "unknown"),
                (
                    "local afterUntouched = untouched.value",
                    "untouched.value",
                    "number",
                ),
            ],
        ),
        (
            "invocation-escape-closure.lua",
            &[(
                "local afterEscapedClosure = hidden.value",
                "hidden.value",
                "unknown",
            )],
        ),
        (
            "invocation-escape-rebind.lua",
            &[
                (
                    "local escapedCurrent = hidden.value",
                    "hidden.value",
                    "unknown",
                ),
                (
                    "local untouchedCurrent = untouched.value",
                    "untouched.value",
                    "number",
                ),
            ],
        ),
        (
            "invocation-call-capture.lua",
            &[
                ("local first = f()", "f()", "number"),
                ("local second = f()", "f()", "string"),
                ("local firstRead = first", "first", "number"),
                ("local secondRead = second", "second", "string"),
            ],
        ),
    ];
    for &(file, rows) in cases {
        let names = ["profile.lua", file];
        let p = prepared(&names);
        let g = p
            .build_plan(
                file,
                "after_load",
                LoadPlan::Known(names.iter().map(|name| (*name).to_owned()).collect()),
                None,
                Some("invocation-lifecycle-v1"),
                p.operation_control(),
            )
            .unwrap();
        let provider = p
            .freeze(
                &g,
                NativeViewSelection::SourceTemplates,
                p.operation_control(),
            )
            .unwrap();
        let session =
            BindingSession::new_checked(provider, || p.operation_control().check()).unwrap();
        let mut check = || p.operation_control().check();
        let mut q = query(&p, &session, p.files()[file], &mut check);
        for &(marker, expression, want) in rows {
            let s = site(file, marker, expression);
            let mut facts = g.uses.iter().filter(|f| f.site == s);
            assert!(facts.next().is_some(), "must query a real graph use");
            assert!(
                facts.next().is_none(),
                "lifecycle observation must have one execution context"
            );
            assert_eq!(
                label(
                    q.type_of_expr(p.binding_site(&s).unwrap().syntax)
                        .unwrap()
                        .structural_type()
                ),
                want,
                "{file}: {marker}"
            );
        }
        if file == "invocation-selector.lua" {
            for (marker, expression, want) in [
                ("local first = Identity(1)", "Identity(1)", "number"),
                (
                    "local second = Identity(\"two\")",
                    "Identity(\"two\")",
                    "string",
                ),
            ] {
                let s = site(file, marker, expression);
                let call = g
                    .calls
                    .iter()
                    .find(|c| c.site == s && c.context.starts_with("load:"))
                    .unwrap();
                let frame = g
                    .native_invocations
                    .as_ref()
                    .unwrap()
                    .frames
                    .iter()
                    .find(|f| f.call == Some(call.id))
                    .unwrap();
                let provider = p
                    .freeze(
                        &g,
                        NativeViewSelection::Invocation(frame.id),
                        p.operation_control(),
                    )
                    .unwrap();
                let selected_session =
                    BindingSession::new_checked(provider, || p.operation_control().check())
                        .unwrap();
                assert_ne!(session.identity(), selected_session.identity());
                let mut selected_check = || p.operation_control().check();
                let mut selected =
                    query(&p, &selected_session, p.files()[file], &mut selected_check);
                assert_eq!(
                    label(
                        selected
                            .type_of_expr(
                                p.binding_site(&site(file, "return copy", "copy"))
                                    .unwrap()
                                    .syntax
                            )
                            .unwrap()
                            .structural_type()
                    ),
                    want
                );
            }
        }
    }
}

#[test]
fn lua_scalar_tail_parenthesized_and_forwarded_calls_keep_all_fourteen_return_slot_contracts() {
    let file = "invocation-return-slots.lua";
    let names = ["profile.lua", file];
    let p = prepared(&names);
    let g = p
        .build_plan(
            "return-slots",
            "after_load",
            LoadPlan::Known(names.iter().map(|name| (*name).to_owned()).collect()),
            None,
            Some("invocation-return-slots-v1"),
            p.operation_control(),
        )
        .unwrap();
    let provider = p
        .freeze(
            &g,
            NativeViewSelection::SourceTemplates,
            p.operation_control(),
        )
        .unwrap();
    let session = BindingSession::new_checked(provider, || p.operation_control().check()).unwrap();
    let mut check = || p.operation_control().check();
    let mut q = query(&p, &session, p.files()[file], &mut check);
    let rows: &[(&str, &str, &[&str])] = &[
        ("local scalarZero = Empty()", "Empty()", &["nil"]),
        ("local emptyA, emptyB = Empty()", "Empty()", &[]),
        ("local scalarPair = Pair()", "Pair()", &["number"]),
        (
            "local pairA, pairB = Pair()",
            "Pair()",
            &["number", "string"],
        ),
        ("local prefixA, prefixB = Pair(), 9", "Pair()", &["number"]),
        ("local parenA, parenB = (Pair())", "Pair()", &["number"]),
        (
            "local forwardA, forwardB = Forward()",
            "Forward()",
            &["number", "string"],
        ),
        (
            "local singleA, singleB = Parenthesized()",
            "Parenthesized()",
            &["number"],
        ),
        (
            "local contractValue = Annotated()",
            "Annotated()",
            &["number"],
        ),
        ("local readEmptyB = emptyB", "emptyB", &["nil"]),
        ("local readPairB = pairB", "pairB", &["string"]),
        ("local readParenB = parenB", "parenB", &["nil"]),
        ("local readForwardB = forwardB", "forwardB", &["string"]),
        ("local readSingleB = singleB", "singleB", &["nil"]),
    ];
    for &(marker, expression, want) in rows {
        let ty = q
            .type_of_expr(
                p.binding_site(&site(file, marker, expression))
                    .unwrap()
                    .syntax,
            )
            .unwrap();
        let actual = match ty.structural_type() {
            LuaType::Variadic(values) => match values.as_ref() {
                VariadicType::Multi(values) => values.iter().map(label).collect::<Vec<_>>(),
                VariadicType::Base(ty) => vec![label(ty)],
            },
            other => vec![label(other)],
        };
        assert_eq!(actual, want, "{marker}");
    }
    let declaration = g
        .functions
        .iter()
        .find(|f| f.name == "Annotated" && f.declaration.file == file)
        .unwrap();
    let signature = q
        .type_of_signature_in_file(
            p.files()[file],
            p.binding_site(&declaration.closure).unwrap().syntax,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        signature.function_return().unwrap().structural_type(),
        &LuaType::String
    );
    let body_return = site(file, "local function Annotated()\n    return 1", "1");
    assert_eq!(
        diagnostics(&p, &session, file),
        vec![(
            "return-type-mismatch".into(),
            body_return.start,
            body_return.end
        )]
    );
    let call = p
        .binding_site(&site(
            file,
            "local contractValue = Annotated()",
            "Annotated()",
        ))
        .unwrap();
    let coverage = session
        .coverage_checked(|| p.operation_control().check())
        .unwrap();
    assert_eq!(
        coverage
            .iter()
            .find(|r| r.site == call && r.operation == "call_result")
            .map(|r| r.state),
        Some("native_projected")
    );
}

#[test]
fn fixed_return_with_unknown_argument_preserves_result_and_independent_check_coverage() {
    let file = "coverage-unknown-argument.lua";
    let names = [
        "profile.lua",
        "generic-defs.lua",
        "generic-calls.lua",
        "generic-replace.lua",
        file,
    ];
    let p = prepared(&names);
    let g = p
        .build_plan(
            "fixed-return-unknown-argument",
            "after_load",
            LoadPlan::Known(names.iter().map(|name| (*name).to_owned()).collect()),
            None,
            Some("fixed-return-unknown-argument-v1"),
            p.operation_control(),
        )
        .unwrap();
    let provider = p
        .freeze(
            &g,
            NativeViewSelection::SourceTemplates,
            p.operation_control(),
        )
        .unwrap();
    let session =
        BindingSession::new_checked(provider.clone(), || p.operation_control().check()).unwrap();
    let s = site(
        file,
        "return Replacement(object[key])",
        "Replacement(object[key])",
    );
    // The published fixture's byte coordinates are also the diagnostic contract.
    assert_eq!((source(file).len(), s.start, s.end), (127, 98, 122));
    let native = p.binding_site(&s).unwrap();
    let call = g.calls.iter().find(|c| c.site == s).unwrap();
    assert!(matches!(
        provider.resolve(p.binding_site(&g.uses[call.callee].site).unwrap()),
        BindingDecision::Bound(BoundValue {
            value: SelectedValue::Function(_),
            ..
        })
    ));
    assert!(matches!(
        provider.resolve(p.binding_site(&g.uses[call.arguments[0]].site).unwrap()),
        BindingDecision::Unavailable { .. }
    ));
    let mut check = || p.operation_control().check();
    let mut q = query(&p, &session, p.files()[file], &mut check);
    assert!(
        matches!(provider.call_observation(native), BindingDecision::Unavailable { reason, .. } if reason == "native_call_return_observation_missing")
    );
    let declaration = g
        .functions
        .iter()
        .find(|f| f.name == "WrongFixedReturn" && f.declaration.file == file)
        .unwrap();
    let signature = q
        .type_of_signature_in_file(
            p.files()[file],
            p.binding_site(&declaration.closure).unwrap().syntax,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        signature.function_return().unwrap().structural_type(),
        &LuaType::Number
    );

    assert!(matches!(
        q.type_of_expr(native.syntax).unwrap().structural_type(),
        LuaType::String
    ));
    let model = p.analysis().semantic_model(p.files()[file]);
    let tree = model.syntax_tree().unwrap();
    let call_ast = LuaCallExpr::cast(
        native
            .syntax
            .to_node_from_root(&tree.get_red_root())
            .unwrap(),
    )
    .unwrap();
    let candidates = q
        .callable_candidates_for_expr(&call_ast.get_prefix_expr().unwrap())
        .unwrap();
    assert_eq!(candidates.len(), 1);
    let LuaType::DocFunction(candidate) = candidates[0].structural_type() else {
        panic!("selected native callable")
    };
    assert!(matches!(candidate.get_ret(), LuaType::String));
    assert!(candidate.get_generic_params().is_empty());
    assert_eq!(session.identity(), g.snapshot_key);
    assert_eq!(
        diagnostics(&p, &session, file),
        vec![("return-type-mismatch".into(), 98, 122)]
    );
    let coverage = session
        .coverage_checked(|| p.operation_control().check())
        .unwrap();
    let proof = &g.uses[call.result_fact.unwrap()].proof_id;
    // The string declaration supports a real mismatch, but no invocation proves its slot count.
    for (operation, state) in [
        ("call_result", "native_projected"),
        ("argument_check", "unavailable"),
        ("return_check", "unavailable"),
    ] {
        let record = coverage
            .iter()
            .find(|r| r.site == native && r.operation == operation)
            .unwrap();
        assert_eq!(record.state, state);
        assert_eq!(&record.proof, proof);
    }
}
