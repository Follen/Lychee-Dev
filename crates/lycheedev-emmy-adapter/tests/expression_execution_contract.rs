#[path = "support/expression_execution_cases.rs"]
mod support;
use emmylua_code_analysis::LuaType;
use lycheedev_emmy_adapter::session::*;
use std::sync::Arc;
use support::{Backend, Marker, Stop, absent, load, select};

#[test]
fn all_selected_compilation_precedes_name_index_argument_flow_and_call_execution() {
    let (analysis, query, mut provider) = load("return f(a,b[c],d)", "f(a,b[c],d)");
    select(&mut provider, "d", absent("d-proof"));
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut backend = Backend::new(provider.events.clone());
    backend
        .name_types
        .insert("a".into(), LuaType::IntegerConst(1));
    backend.flow_types.insert("a".into(), LuaType::Any);
    backend.flow_types.insert("b[c]".into(), LuaType::String);
    backend
        .flow_types
        .insert("d".into(), LuaType::BooleanConst(true));
    let result = SelectedProjector::new(&analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    assert_eq!(
        result.ty,
        LuaType::IntegerConst(999),
        "fixture call return, not a production call inference claim"
    );
    assert_eq!(
        *provider.events.lock().unwrap(),
        [
            "resolve:f(a,b[c],d)",
            "resolve:f",
            "resolve:a",
            "resolve:b[c]",
            "resolve:b",
            "resolve:c",
            "resolve:d",
            "load:f",
            "load:a",
            "load:b",
            "load:c",
            "index:dynamic",
            "flow:a",
            "flow:b[c]",
            "flow:d",
            "call:3:false:0",
        ]
    );
    let args = &backend.last_arguments;
    assert_eq!(
        args.iter()
            .map(|value| value.value.ty.clone())
            .collect::<Vec<_>>(),
        [
            LuaType::IntegerConst(1),
            LuaType::String,
            LuaType::BooleanConst(true)
        ]
    );
    assert!(args[0].owner.is_some() && args[1].owner.is_some() && args[2].owner.is_none());
    assert!(args[0].receiver.is_some() && args[1].receiver.is_some());
    assert!(backend.last_callee.as_ref().unwrap().owner.is_some());
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (rows[0].operation, rows[0].state, rows[0].proof.as_str()),
        ("resolve", "known_absent", "d-proof")
    );
}

#[test]
fn flow_type_equality_keeps_token_epoch_identity_and_unknown_any_rules() {
    let (analysis, query, provider) = load("return f(a,b,c,d)", "f(a,b,c,d)");
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut backend = Backend::new(provider.events.clone());
    let original = Arc::new(Marker {
        identity: "object".into(),
        epoch: "epoch-1".into(),
        token: Arc::new(()),
    });
    let foreign = Arc::new(Marker {
        identity: "object".into(),
        epoch: "epoch-1".into(),
        token: Arc::new(()),
    });
    let newer = Arc::new(Marker {
        identity: "object".into(),
        epoch: "epoch-2".into(),
        token: original.token.clone(),
    });
    for name in ["a", "b", "c", "d"] {
        backend.name_types.insert(name.into(), LuaType::Table);
        backend.name_markers.insert(name.into(), original.clone());
    }
    backend.flow_types.insert("a".into(), LuaType::Table);
    backend.flow_markers.insert("a".into(), foreign.clone());
    backend.flow_types.insert("b".into(), LuaType::Unknown);
    backend.flow_markers.insert("b".into(), foreign);
    backend.flow_types.insert("c".into(), LuaType::Any);
    backend.flow_markers.insert("c".into(), newer.clone());
    backend.flow_types.insert("d".into(), LuaType::Table);
    backend.flow_markers.insert("d".into(), newer.clone());
    SelectedProjector::new(&analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    let args = &backend.last_arguments;
    assert!(!Arc::ptr_eq(
        &args[0].value.marker.as_ref().unwrap().token,
        &original.token
    ));
    assert!(Arc::ptr_eq(
        args[1].value.marker.as_ref().unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        args[2].value.marker.as_ref().unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(args[3].value.marker.as_ref().unwrap(), &newer));
    assert!(
        args.iter()
            .all(|value| value.owner.is_some() && value.receiver.is_some())
    );
}

#[test]
fn unary_clears_vm_metadata_but_moves_the_original_nonclone_carrier() {
    let (analysis, query, provider) = load("return f(-a,function() end)", "f(-a,function() end)");
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut backend = Backend::new(provider.events.clone());
    let marker = Arc::new(Marker {
        identity: "selected-object".into(),
        epoch: "frozen-epoch".into(),
        token: Arc::new(()),
    });
    backend.name_types.insert("a".into(), LuaType::Table);
    backend.name_markers.insert("a".into(), marker.clone());
    SelectedProjector::new(&analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    let first = &backend.last_arguments[0];
    assert!(first.owner.is_none() && first.closure_syntax.is_none() && first.receiver.is_none());
    assert!(Arc::ptr_eq(first.value.marker.as_ref().unwrap(), &marker));
    let closure = &backend.last_arguments[1];
    assert_eq!(closure.value.ty, LuaType::Function);
    assert!(closure.closure_syntax.is_some());
    assert!(closure.owner.is_none() && closure.receiver.is_none());
}

#[test]
fn logical_nullable_queries_merge_left_before_right_after_both_loads() {
    for op in ["and", "or"] {
        let expression = format!("left {op} right");
        let (analysis, query, provider) = load(&format!("return {expression}"), &expression);
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
                .unwrap();
        let mut backend = Backend::new(provider.events.clone());
        backend.nullable = true;
        SelectedProjector::new(&analysis, &session)
            .evaluate_expression(query, &mut backend)
            .unwrap();
        let events = provider.events.lock().unwrap();
        assert_eq!(
            &events[3..8],
            [
                "load:left",
                "load:right",
                "nullable:left",
                "merge",
                "nullable:right"
            ]
        );
        assert_eq!(events[8], "merge");
        assert_eq!(
            events[9],
            format!("binary:Op{}", if op == "and" { "And" } else { "Or" })
        );
    }
}

#[test]
fn recursive_execution_appends_initializer_after_the_already_compiled_rhs() {
    let (analysis, query, mut provider) = load(
        "local left=1\nlocal right=2\nreturn left+right",
        "left+right",
    );
    select(&mut provider, "1", absent("left-initializer"));
    select(&mut provider, "right", absent("right-read"));
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut backend = Backend::new(provider.events.clone());
    backend.recursive_initializers = true;
    let result = SelectedProjector::new(&analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    assert_eq!(result.ty, LuaType::Number);
    assert_eq!(
        *provider.events.lock().unwrap(),
        [
            "resolve:left+right",
            "resolve:left",
            "resolve:right",
            "load:left",
            "resolve:1",
            "binary:OpAdd"
        ]
    );
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["right-read", "left-initializer"]
    );
}

#[test]
fn every_compile_and_execute_checkpoint_preserves_original_error_and_real_prefix() {
    let (analysis, query, mut provider) = load(
        "local left=1\nlocal right=2\nreturn left+right",
        "left+right",
    );
    select(&mut provider, "1", absent("left-initializer"));
    select(&mut provider, "right", absent("right-read"));
    let full =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let mut baseline = Backend::new(provider.events.clone());
    baseline.recursive_initializers = true;
    SelectedProjector::new(&analysis, &full)
        .evaluate_expression(query, &mut baseline)
        .unwrap();
    let full_rows = full.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    for ordinal in 1..=baseline.checkpoints {
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
                .unwrap();
        let mut backend = Backend::new(provider.events.clone());
        backend.recursive_initializers = true;
        backend.stop_at = ordinal;
        let token = backend.token.clone();
        let error = SelectedProjector::new(&analysis, &session)
            .evaluate_expression(query, &mut backend)
            .unwrap_err();
        assert_eq!(error.ordinal, ordinal);
        assert!(Arc::ptr_eq(&error.token, &token));
        let prefix = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_eq!(prefix, full_rows[..prefix.len()], "ordinal {ordinal}");
        backend.stop_at = usize::MAX;
        SelectedProjector::new(&analysis, &session)
            .evaluate_expression(query, &mut backend)
            .unwrap();
        assert_eq!(
            session.coverage_checked(|| Ok::<_, Stop>(())).unwrap(),
            full_rows,
            "retry releases guards and deduplicates completed facts"
        );
    }
}

#[test]
fn primitive_and_unary_subset_matches_real_public_types_without_deferred_operations() {
    for expression in [
        "nil",
        "true",
        "false",
        "7",
        "1.25",
        "'hello'",
        "{}",
        "-7",
        "#7",
        "#'hello'",
        "~7",
        "not missing",
        "not f()",
        "{missing}",
        "(nil)",
    ] {
        let (analysis, query, provider) = load(&format!("return {expression}"), expression);
        let expected = analysis
            .semantic_model(query.file_id)
            .type_of_expr(query.syntax);
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
                .unwrap();
        let mut backend = Backend::new(provider.events.clone());
        let actual = SelectedProjector::new(&analysis, &session)
            .evaluate_expression(query, &mut backend)
            .unwrap();
        assert_eq!(actual.ty, expected, "{expression}");
        assert!(
            provider
                .events
                .lock()
                .unwrap()
                .iter()
                .all(|event| event.starts_with("resolve:")),
            "{expression}: no deferred test operation should execute"
        );
        assert!(
            session
                .coverage_checked(|| Ok::<_, Stop>(()))
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn real_public_unary_operator_overloads_are_used_from_the_existing_analysis() {
    for (expression, expected) in [("-a", LuaType::String), ("#a", LuaType::Number)] {
        let source = format!(
            "---@class Vector\n---@operator unm: string\n---@operator len: number\nlocal Vector={{}}\n---@type Vector\nlocal a\nreturn {expression}"
        );
        let (analysis, query, provider) = load(&source, expression);
        let a = *provider
            .names
            .iter()
            .find(|(_, text)| text.as_str() == "a")
            .unwrap()
            .0;
        let model = analysis.semantic_model(query.file_id);
        let a_type = model.type_of_expr(a.syntax);
        assert!(matches!(a_type, LuaType::Ref(_) | LuaType::Def(_)));
        assert_eq!(model.type_of_expr(query.syntax), expected);
        let session =
            OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
                .unwrap();
        let mut backend = Backend::new(provider.events.clone());
        backend.name_types.insert("a".into(), a_type);
        let actual = SelectedProjector::new(&analysis, &session)
            .evaluate_expression(query, &mut backend)
            .unwrap();
        assert_eq!(actual.ty, expected);
        assert!(actual.marker.is_none());
    }
}

#[test]
fn execution_producer_at_the_final_slot_keeps_its_original_sentinel_site() {
    let (analysis, query, mut provider) = load(
        "local left=1\nlocal right=2\nreturn left+right",
        "left+right",
    );
    select(&mut provider, "1", absent("left-initializer"));
    select(&mut provider, "right", absent("right-read"));
    let initializer = *provider
        .names
        .iter()
        .find(|(_, text)| text.as_str() == "1")
        .unwrap()
        .0;
    let session =
        OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(())).unwrap();
    let projector = SelectedProjector::new(&analysis, &session);
    let mut backend = Backend::new(provider.events.clone());
    backend.recursive_initializers = true;
    for index in 0..16_382 {
        projector
            .project_decision(
                query,
                absent(&format!("prior-{index}")),
                "resolve",
                &mut backend,
            )
            .unwrap();
    }
    projector.evaluate_expression(query, &mut backend).unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 16_384);
    assert_eq!(rows[16_382].proof, "right-read");
    assert_eq!(
        rows[16_383],
        BindingCoverage {
            site: initializer,
            operation: "coverage",
            state: "unavailable",
            proof: String::new(),
            reason: Some("binding_coverage_budget".into()),
            parameter: None,
            parameter_index: None,
            argument: None
        }
    );
    assert!(
        provider
            .events
            .lock()
            .unwrap()
            .last()
            .unwrap()
            .starts_with("binary:"),
        "ledger saturation does not skip actual execution"
    );
}
