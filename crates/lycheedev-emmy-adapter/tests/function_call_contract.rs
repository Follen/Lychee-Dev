#[path = "support/function_call_cases.rs"]
mod support;
use emmylua_code_analysis::{
    AsyncState, GenericTpl, GenericTplId, InFiled, LuaFunctionType, LuaType, VariadicType,
};
use lycheedev_emmy_adapter::session::*;
use std::{cell::Cell, sync::Arc};
use support::*;
fn plain(value: OwnedValue) -> LuaType {
    assert!(
        value.is_context_free(),
        "this inherited test excludes selected objects"
    );
    value.structural_type().clone()
}

#[test]
fn real_functions_with_distinct_bodies_flow_into_the_owned_direct_call() {
    for body in ordinary_call_bodies() {
        let source = format!("local function f() {body} end\nreturn selected()");
        let (analysis, site, _, provider) = load_function(&source, "selected()");
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query = OwnedQuery::new(&analysis, &session, site.file_id, &mut check);
        let value = plain(query.type_of_expr(site.syntax).unwrap());
        assert!(!matches!(value, LuaType::Unknown));
        assert_eq!(plain(query.type_of_expr(site.syntax).unwrap()), value);
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
    let value = plain(query.type_of_expr(site.syntax).unwrap());
    assert_eq!(
        value,
        LuaType::Variadic(Arc::new(VariadicType::Multi(vec![
            LuaType::Nil,
            LuaType::Variadic(Arc::new(VariadicType::Base(LuaType::Unknown))),
        ])))
    );
    assert_eq!(
        rows(&session),
        vec![
            BindingCoverage {
                site: provider.named("absent"),
                operation: "resolve".into(),
                state: "known_absent".into(),
                proof: "nil-slot".into(),
                reason: None,
                parameter: None,
                parameter_index: None,
                argument: None
            },
            BindingCoverage {
                site: provider.named("denied()"),
                operation: "call".into(),
                state: "unavailable".into(),
                proof: "denied-call".into(),
                reason: Some("no-call-evidence".into()),
                parameter: None,
                parameter_index: None,
                argument: None
            },
            BindingCoverage {
                site: provider.named("selected"),
                operation: "resolve".into(),
                state: "bound".into(),
                proof: "selected-function-proof".into(),
                reason: None,
                parameter: None,
                parameter_index: None,
                argument: None
            },
        ]
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
    let LuaType::DocFunction(function) = plain(query.type_of_expr(site.syntax).unwrap()) else {
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
fn selected_function_preserves_unknown_returns_generic_metadata_and_table_identity() {
    for (case, source) in [
        (
            0,
            "local function f() return unavailable end\nreturn selected",
        ),
        (
            1,
            "---@generic T\nlocal function f() return 1 end\nreturn selected",
        ),
        (2, "local function f() return {} end\nreturn selected"),
        (
            3,
            "local function f() return undocumented end\nreturn selected",
        ),
    ] {
        let (analysis, site, _, mut provider) = load_function(source, "selected");
        if case == 0 {
            provider.select(
                "unavailable",
                BindingDecision::Unavailable {
                    proof: "uncertain".into(),
                    reason: "missing".into(),
                },
            );
        }
        let ret = match case {
            0 | 3 => LuaType::Unknown,
            1 => LuaType::IntegerConst(1),
            _ => LuaType::TableConst(InFiled::new(
                site.file_id,
                provider.named("{}").syntax.get_range(),
            )),
        };
        let generic = if case == 1 {
            Some(vec![GenericTpl::new(
                GenericTplId::Type(0),
                "T".into(),
                None,
                None,
                false,
                None,
            )])
        } else {
            None
        };
        let expected = LuaType::DocFunction(Arc::new(LuaFunctionType::new(
            AsyncState::None,
            false,
            false,
            vec![],
            ret,
            generic,
        )));
        let session = session(&provider);
        let mut check = || Ok::<_, Stop>(());
        let mut query =
            OwnedQuery::new_for_snapshot(&analysis, &session, site.file_id, &mut check).unwrap();
        assert_eq!(plain(query.type_of_expr(site.syntax).unwrap()), expected);
        let events = provider.events.lock().unwrap().clone();
        assert_eq!(plain(query.type_of_expr(site.syntax).unwrap()), expected);
        assert_eq!(
            *provider.events.lock().unwrap(),
            events,
            "completed signature is cached"
        );
        let mut expected_rows = vec![];
        if case == 0 {
            expected_rows.push(BindingCoverage {
                site: provider.named("unavailable"),
                operation: "resolve".into(),
                state: "unavailable".into(),
                proof: "uncertain".into(),
                reason: Some("missing".into()),
                parameter: None,
                parameter_index: None,
                argument: None,
            });
        }
        expected_rows.push(BindingCoverage {
            site,
            operation: "resolve".into(),
            state: "bound".into(),
            proof: "selected-function-proof".into(),
            reason: None,
            parameter: None,
            parameter_index: None,
            argument: None,
        });
        assert_eq!(rows(&session), expected_rows);
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
    let expected = plain(query.type_of_expr(site.syntax).unwrap());
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
        assert_eq!(plain(query.type_of_expr(site.syntax).unwrap()), expected);
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
        assert_eq!(
            plain(query.type_of_expr(site.syntax).unwrap()),
            LuaType::Unknown
        );
        assert_eq!(rows(&session).len(), 1);
        assert_eq!(rows(&session)[0].operation, "resolve");
        assert!(
            rows(&session)
                .iter()
                .all(|row| row.operation != "type_check")
        );
    }
}
