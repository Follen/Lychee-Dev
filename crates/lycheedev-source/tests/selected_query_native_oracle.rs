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

#[test]
fn all_call_evidence_and_lua_positions_match_real_fixed_projector_full_fields() {
    for (position, source, _) in positions() {
        let (analysis, provider) = load(source);
        let ordinary = analysis
            .semantic_model(provider.query.file_id)
            .type_of_expr(provider.query.syntax);
        for case in cases(&provider) {
            let session =
                native::BindingSession::new(Arc::new(NativeProvider(case.provider.clone())));
            let expected = analysis
                .semantic_model_with_context(provider.query.file_id, session.clone())
                .type_of_expr(provider.query.syntax);
            let expected_rows = session.coverage().into_iter().map(row).collect::<Vec<_>>();
            let (selected, rows, _) = run(&analysis, &case.provider);
            let actual = selected.as_ref().unwrap_or(&ordinary);
            assert_eq!(actual, &expected, "{position}/{} full type", case.name);
            assert_eq!(
                format!("{actual:?}"),
                format!("{expected:?}"),
                "{position}/{} all type fields",
                case.name
            );
            assert_eq!(
                rows, expected_rows,
                "{position}/{} all ledger fields in actual producer order",
                case.name
            );
            println!(
                "SELECTED_QUERY_NATIVE_ORACLE {}",
                serde_json::json!({"position":position,"case":case.name,"type":format!("{actual:?}"),"coverage":format!("{rows:?}")})
            );
        }
    }
}
