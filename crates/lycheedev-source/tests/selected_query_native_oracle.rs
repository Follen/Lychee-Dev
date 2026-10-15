//! Actual fixed patched SemanticModel versus owned selected-call producers.
//! One analysis/database per Lua position; no Lua execution or second DB.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/selected_query_cases.rs"]
mod support;
use emmylua_code_analysis::binding_context as native;
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::*;

fn native_site(site: owned::BindingSite) -> native::BindingSite {
    native::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
fn owned_site(site: native::BindingSite) -> owned::BindingSite {
    owned::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
fn function(function: &owned::FunctionRef) -> native::FunctionRef {
    native::FunctionRef {
        file_id: function.file_id,
        closure: function.closure,
        declaration: function.declaration.clone(),
    }
}
fn selected(value: &owned::SelectedValue) -> native::SelectedValue {
    match value {
        owned::SelectedValue::Function(value) => native::SelectedValue::Function(function(value)),
        owned::SelectedValue::EnvironmentCallable(value) => {
            native::SelectedValue::EnvironmentCallable(value.clone())
        }
        owned::SelectedValue::EnvironmentReturn(value) => {
            native::SelectedValue::EnvironmentReturn(value.clone())
        }
        _ => {
            panic!("call oracle cannot bypass unimplemented context-recursive or object projection")
        }
    }
}
fn decision<T, U>(
    value: owned::BindingDecision<T>,
    bound: impl FnOnce(T) -> U,
) -> native::BindingDecision<U> {
    match value {
        owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
        owned::BindingDecision::KnownAbsent { proof } => {
            native::BindingDecision::KnownAbsent { proof }
        }
        owned::BindingDecision::Unavailable { proof, reason } => {
            native::BindingDecision::Unavailable { proof, reason }
        }
        owned::BindingDecision::Bound(value) => native::BindingDecision::Bound(bound(value)),
    }
}
fn bound(value: owned::BoundValue) -> native::BoundValue {
    native::BoundValue {
        identity: value.identity,
        proof: value.proof,
        value: selected(&value.value),
    }
}
struct NativeProvider(Provider);
impl native::BindingContext for NativeProvider {
    fn identity(&self) -> &str {
        owned::BindingContext::identity(&self.0)
    }
    fn resolve(&self, site: native::BindingSite) -> native::BindingDecision<native::BoundValue> {
        decision(
            owned::BindingContext::resolve(&self.0, owned_site(site)),
            bound,
        )
    }
    fn call(&self, site: native::BindingSite) -> native::BindingDecision<native::CallPermit> {
        decision(
            owned::BindingContext::call(&self.0, owned_site(site)),
            |permit| native::CallPermit {
                callee: native_site(permit.callee),
                proof: permit.proof,
                capability_profile: permit.capability_profile,
            },
        )
    }
    fn call_observation(
        &self,
        site: native::BindingSite,
    ) -> native::BindingDecision<native::CallObservation> {
        decision(
            owned::BindingContext::call_observation(&self.0, owned_site(site)),
            |observation| native::CallObservation {
                callee: match observation.callee {
                    owned::ObservedCallee::Source(value) => {
                        native::ObservedCallee::Source(function(&value))
                    }
                    owned::ObservedCallee::Environment(value) => {
                        native::ObservedCallee::Environment(value)
                    }
                },
                selected_callee_identity: observation.selected_callee_identity,
                closure_identity: observation.closure_identity,
                invocation_identity: observation.invocation_identity,
                returns: observation
                    .returns
                    .into_iter()
                    .map(|value| decision(value, bound))
                    .collect(),
                proof: observation.proof,
            },
        )
    }
}
fn row(value: native::BindingCoverage) -> owned::BindingCoverage {
    owned::BindingCoverage {
        site: owned_site(value.site),
        operation: value.operation,
        state: value.state,
        proof: value.proof,
        reason: value.reason,
        parameter: value.parameter,
        parameter_index: value.parameter_index,
        argument: value.argument.map(owned_site),
    }
}

/// Keep every original case and compare only actual owned results. A callsite
/// projector returning None delegates to the same-context owned evaluator; it
/// never licenses ordinary context-free inference for the whole expression.
/// Until that evaluator implements Function/call, this full regression must
/// remain failing, after reporting all positions rather than stopping at the
/// first missing successor. The native value is never reused as the actual.
#[test]
fn all_call_evidence_and_lua_positions_match_real_fixed_projector_full_fields() {
    let mut cases_seen = 0;
    let mut selected_cases = 0;
    let mut continuation_cases = 0;
    let mut failures = Vec::new();
    for (position, source, _) in positions() {
        let (analysis, provider) = load(source);
        for case in cases(&provider) {
            cases_seen += 1;
            let session =
                native::BindingSession::new(Arc::new(NativeProvider(case.provider.clone())));
            let expected = analysis
                .semantic_model_with_context(provider.query.file_id, session.clone())
                .type_of_expr(provider.query.syntax);
            let expected_rows = session.coverage().into_iter().map(row).collect::<Vec<_>>();
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
                    let mut query = owned::OwnedQuery::new(
                        &analysis,
                        &owned_session,
                        provider.query.file_id,
                        &mut check,
                    );
                    let result = query.type_of_expr(provider.query.syntax);
                    let rows = owned_session.coverage_checked(|| Ok::<_, ()>(())).unwrap();
                    (result, rows, "whole_expression_contextual_successor")
                }
            };
            let mut case_failures = Vec::new();
            match &actual {
                Ok(actual) => {
                    if actual != &expected || format!("{actual:?}") != format!("{expected:?}") {
                        case_failures.push("full_type");
                    }
                }
                Err(_) => case_failures.push("owned_successor_unimplemented_or_stopped"),
            }
            if rows != expected_rows {
                case_failures.push("all_ledger_fields_and_order");
            }
            println!(
                "SELECTED_QUERY_NATIVE_ORACLE {}",
                serde_json::json!({
                    "position": position,
                    "case": case.name,
                    "scope": scope,
                    "actual": format!("{actual:?}"),
                    "expected": format!("{expected:?}"),
                    "actualCoverage": format!("{rows:?}"),
                    "expectedCoverage": format!("{expected_rows:?}"),
                    "mismatches": case_failures,
                })
            );
            if !case_failures.is_empty() {
                failures.push(format!("{position}/{}: {case_failures:?}; actual={actual:?}; expected={expected:?}; actual coverage={rows:?}; expected coverage={expected_rows:?}", case.name));
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
    let session = native::BindingSession::new(Arc::new(NativeProvider(case.provider)));
    let contextual = analysis
        .semantic_model_with_context(provider.query.file_id, session.clone())
        .type_of_expr(provider.query.syntax);
    let ordinary = analysis
        .semantic_model(provider.query.file_id)
        .type_of_expr(provider.query.syntax);
    assert_ne!(contextual, ordinary, "498 real failure must remain visible");
    assert_eq!(ordinary, emmylua_code_analysis::LuaType::Unknown);
    assert_eq!(
        contextual,
        emmylua_code_analysis::LuaType::Variadic(Arc::new(
            emmylua_code_analysis::VariadicType::Multi(Vec::new())
        ))
    );
    let rows = session.coverage().into_iter().map(row).collect::<Vec<_>>();
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
