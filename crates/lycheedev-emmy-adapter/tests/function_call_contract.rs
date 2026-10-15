#[path = "support/function_call_cases.rs"]
mod support;
use emmylua_code_analysis::{LuaType, VariadicType};
use lycheedev_emmy_adapter::session::*;
use std::{cell::Cell, sync::Arc};
use support::*;

#[test]
fn real_functions_with_distinct_bodies_flow_into_the_owned_direct_call() {
    for body in ordinary_call_bodies() {
        let source = format!("local function f() {body} end\nreturn selected()");
        let (analysis, site, _, provider) = load_function(&source, "selected()");
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        let value = query.type_of_expr(site.syntax).unwrap();
        assert!(!matches!(value, LuaType::Unknown));
        assert_eq!(query.type_of_expr(site.syntax).unwrap(), value);
        let rows = rows(&session);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].proof, "selected-function-proof");
        assert_eq!(rows[0].operation, "resolve");
    }
}

#[test]
fn child_producers_precede_the_selected_callee_receipt_and_survive_the_call() {
    let (analysis, site, _, mut provider) = load_function(
        "local function f() return absent, denied() end\nreturn selected()",
        "selected()",
    );
    provider.select("absent", absent("nil-slot"));
    provider.calls.insert(
        provider.named("denied()"),
        BindingDecision::Unavailable {
            proof: "denied-call".into(),
            reason: "no-call-evidence".into(),
        },
    );
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    let value = query.type_of_expr(site.syntax).unwrap();
    assert_eq!(
        value,
        LuaType::Variadic(Arc::new(VariadicType::Multi(vec![
            LuaType::Nil,
            LuaType::Variadic(Arc::new(VariadicType::Base(LuaType::Unknown))),
        ])))
    );
    assert_eq!(
        rows(&session)
            .iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["nil-slot", "denied-call", "selected-function-proof"]
    );
}

#[test]
fn explicit_docs_parameters_and_async_metadata_do_not_query_body_returns() {
    let (analysis, site, _, mut provider) = load_function(
        "---@param x? number\n---@async\n---@return string,boolean\nlocal function f(x) return unobserved end\nreturn selected",
        "selected",
    );
    provider.select("unobserved", absent("must-not-query-body"));
    let session = session(&provider);
    let mut check = || Ok::<_, Stop>(());
    let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
    let LuaType::DocFunction(function) = query.type_of_expr(site.syntax).unwrap() else {
        panic!("expected function")
    };
    assert_eq!(
        function.get_async_state(),
        emmylua_code_analysis::AsyncState::Async
    );
    assert_eq!(function.get_params().len(), 1);
    assert!(function.get_params()[0].1.as_ref().unwrap().is_nullable());
    assert_eq!(
        function.get_ret(),
        &LuaType::Variadic(Arc::new(VariadicType::Multi(vec![
            LuaType::String,
            LuaType::Boolean
        ])))
    );
    assert_eq!(rows(&session).len(), 1);
    assert!(
        !provider
            .events
            .lock()
            .unwrap()
            .contains(&Event::Resolve(provider.named("unobserved")))
    );
}

#[test]
fn unavailable_raw_fallback_and_complex_metadata_keep_typed_continuations() {
    for (source, expected) in [
        (
            "local function f() return unavailable end\nreturn selected",
            OwnedQueryCapability::FunctionRawReturnFallback,
        ),
        (
            "---@generic T\nlocal function f() return 1 end\nreturn selected",
            OwnedQueryCapability::FunctionMetadata,
        ),
        (
            "local function f() return {} end\nreturn selected",
            OwnedQueryCapability::FunctionReturnType,
        ),
    ] {
        let (analysis, site, _, mut provider) = load_function(source, "selected");
        if provider.names.values().any(|name| name == "unavailable") {
            provider.select(
                "unavailable",
                BindingDecision::Unavailable {
                    proof: "uncertain".into(),
                    reason: "missing".into(),
                },
            );
        }
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        for _ in 0..2 {
            let Err(OwnedQueryFailure::Continue(continuation)) = query.type_of_expr(site.syntax)
            else {
                panic!("no fabricated Unknown/Ready result")
            };
            assert_eq!(continuation.capability, expected);
        }
        assert!(
            !rows(&session)
                .iter()
                .any(|row| row.proof == "selected-function-proof")
        );
    }
}

#[test]
fn every_signature_and_call_checkpoint_preserves_error_identity_prefix_and_retry() {
    let (analysis, site, _, mut provider) = load_function(
        "local function f() return first,second end\nreturn selected()",
        "selected()",
    );
    provider.select("first", absent("first"));
    provider.select("second", absent("second"));
    let count = Cell::new(0);
    let full_session = session(&provider.isolated());
    let mut check = || {
        count.set(count.get() + 1);
        Ok::<_, Stop>(())
    };
    let mut query = OwnedQuery::new(&analysis, &full_session, site.file_id, &mut check);
    let expected = query.type_of_expr(site.syntax).unwrap();
    let total = count.get();
    let full_rows = rows(&full_session);
    assert!(total > 30);
    for stop_at in 1..=total {
        let session = session(&provider.isolated());
        let token = Arc::new(());
        let count = Cell::new(0);
        let mut check = || {
            count.set(count.get() + 1);
            if count.get() == stop_at {
                Err(Stop {
                    ordinal: stop_at,
                    token: token.clone(),
                })
            } else {
                Ok(())
            }
        };
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        let Err(OwnedQueryFailure::Control(error)) = query.type_of_expr(site.syntax) else {
            panic!("stop {stop_at}")
        };
        assert_eq!(error.ordinal, stop_at);
        assert!(Arc::ptr_eq(&error.token, &token));
        let prefix = rows(&session);
        assert!(full_rows.starts_with(&prefix));
        assert_eq!(query.type_of_expr(site.syntax).unwrap(), expected);
        assert_eq!(rows(&session), full_rows);
    }
}

#[test]
fn selected_nil_and_unknown_callees_keep_noncallable_expression_results_without_checker_claims() {
    for decision in [
        absent("nil-callee"),
        BindingDecision::Unavailable {
            proof: "unknown-callee".into(),
            reason: "unknown-evidence".into(),
        },
    ] {
        let (analysis, site, _, mut provider) =
            load_function("local function f() end\nreturn selected()", "selected()");
        provider.select("selected", decision);
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        assert_eq!(query.type_of_expr(site.syntax).unwrap(), LuaType::Unknown);
        assert_eq!(rows(&session).len(), 1);
        assert_eq!(rows(&session)[0].operation, "resolve");
        assert!(
            rows(&session)
                .iter()
                .all(|row| row.operation != "type_check")
        );
    }
}
