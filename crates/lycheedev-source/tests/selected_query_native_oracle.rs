//! Independent fixed-engine reference trace versus owned selected-call producers.
//! Each side retains one analysis/database per Lua position; no Lua execution.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/selected_query_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::*;

/// Keep every original case and compare only actual owned results. A callsite
/// projector returning None delegates to the same-context owned evaluator; it
/// never licenses ordinary context-free inference for the whole expression.
/// A stopped or missing successor remains a failure after all positions are
/// reported. Historical native values are never reused as actual results.
#[test]
fn all_call_evidence_and_lua_positions_match_real_fixed_projector_full_fields() {
    let _trace = trace::begin_candidate();
    let mut cases_seen = 0;
    let mut selected_cases = 0;
    let mut continuation_cases = 0;
    let mut failures = Vec::new();
    for (position, source, _) in positions() {
        let (analysis, provider) = load(source);
        for case in cases(&provider) {
            cases_seen += 1;
            let (selected, selected_rows, _) = run(&analysis, &case.provider);
            let (actual, rows, scope) = match selected {
                Some(value) => {
                    selected_cases += 1;
                    (Ok(value), selected_rows, "selected_projector")
                }
                None => {
                    continuation_cases += 1;
                    // This contract is only about this callsite. Its selected
                    // callee and any children still require the same provider.
                    assert_eq!(case.name, "fully-unmanaged");
                    assert!(selected_rows.is_empty());
                    assert!(matches!(
                        case.provider.selected_callee,
                        owned::BindingDecision::Bound(_)
                    ));
                    let owned_session = owned::OwnedBindingSession::new_checked(
                        Arc::new(case.provider.clone()),
                        || Ok::<_, ()>(()),
                    )
                    .unwrap();
                    let mut check = || Ok::<_, ()>(());
                    let mut query = owned::OwnedQuery::new_for_snapshot(
                        &analysis,
                        &owned_session,
                        provider.query.file_id,
                        &mut check,
                    )
                    .unwrap();
                    let result = query.type_of_expr(provider.query.syntax).map(|value| {
                        assert!(
                            value.is_context_free(),
                            "original S3 corpus contains no object"
                        );
                        value.structural_type().clone()
                    });
                    let rows = owned_session.coverage_checked(|| Ok::<_, ()>(())).unwrap();
                    (result, rows, "whole_expression_contextual_successor")
                }
            };
            let mut case_failures = Vec::new();
            match &actual {
                Ok(actual) => {
                    let equal_type = trace::matches("selected.type", actual);
                    let equal_debug = trace::matches("selected.type_debug", &format!("{actual:?}"));
                    if !equal_type || !equal_debug {
                        case_failures.push("full_type");
                    }
                }
                Err(_) => {
                    // Consume both fixed comparison frames while keeping the
                    // original typed failure. Never invent a successful type.
                    let _ = trace::matches("selected.type", &actual);
                    let _ = trace::matches("selected.type_debug", &format!("{actual:?}"));
                    case_failures.push("owned_successor_unimplemented_or_stopped");
                }
            }
            if !trace::matches("selected.coverage", &rows) {
                case_failures.push("all_ledger_fields_and_order");
            }
            println!(
                "SELECTED_QUERY_NATIVE_ORACLE {}",
                serde_json::json!({
                    "position": position,
                    "case": case.name,
                    "scope": scope,
                    "actual": format!("{actual:?}"),
                    "actualCoverage": format!("{rows:?}"),
                    "mismatches": case_failures,
                })
            );
            if !case_failures.is_empty() {
                failures.push(format!(
                    "{position}/{}: {case_failures:?}; actual={actual:?}; actual coverage={rows:?}",
                    case.name
                ));
            }
        }
    }
    assert_eq!(cases_seen, 448, "retain every original position/case");
    assert_eq!(selected_cases, 434);
    assert_eq!(continuation_cases, 14);
    assert!(
        failures.is_empty(),
        "full contextual equality remains unaccepted for {} cases:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn unmanaged_callsite_still_has_a_selected_callee_context_regression() {
    let _trace = trace::begin_candidate();
    let (analysis, provider) = load("return f()");
    let case = cases(&provider)
        .into_iter()
        .find(|case| case.name == "fully-unmanaged")
        .unwrap();
    let (selected, rows, _) = run(&analysis, &case.provider);
    assert!(selected.is_none());
    assert!(rows.is_empty());
    assert!(matches!(
        case.provider.selected_callee,
        owned::BindingDecision::Bound(_)
    ));
    let session =
        owned::OwnedBindingSession::new_checked(Arc::new(case.provider), || Ok::<_, ()>(()))
            .unwrap();
    let mut check = || Ok::<_, ()>(());
    let mut query = owned::OwnedQuery::new_for_snapshot(
        &analysis,
        &session,
        provider.query.file_id,
        &mut check,
    )
    .unwrap();
    let value = query.type_of_expr(provider.query.syntax).unwrap();
    assert!(
        value.is_context_free(),
        "original S3 corpus contains no object"
    );
    let contextual = value.structural_type().clone();
    let ordinary = analysis
        .semantic_model(provider.query.file_id)
        .type_of_expr(provider.query.syntax);
    trace::check("selected.regression.contextual", &contextual);
    trace::check("selected.regression.ordinary", &ordinary);
    assert_ne!(contextual, ordinary, "498 real failure must remain visible");
    assert_eq!(ordinary, emmylua_code_analysis::LuaType::Unknown);
    assert_eq!(
        contextual,
        emmylua_code_analysis::LuaType::Variadic(Arc::new(
            emmylua_code_analysis::VariadicType::Multi(Vec::new())
        ))
    );
    let rows = session.coverage_checked(|| Ok::<_, ()>(())).unwrap();
    trace::check("selected.regression.coverage", &rows);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].site, provider.callee);
    assert_eq!(rows[0].operation, "resolve");
    assert_eq!(rows[0].state, "bound");
    assert_eq!(rows[0].proof, "callee-proof");
    assert_eq!(rows[0].reason, None);
    assert_eq!(rows[0].parameter, None);
    assert_eq!(rows[0].parameter_index, None);
    assert_eq!(rows[0].argument, None);
}
