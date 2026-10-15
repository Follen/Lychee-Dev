//! Differential oracle against the exact patched production projector. This
//! target requires that baseline engine; it is not part of the clean API proof.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/selected_projector_cases.rs"]
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
fn native_value(value: &owned::SelectedValue) -> native::SelectedValue {
    match value {
        owned::SelectedValue::DerivedNumber(number) => {
            native::SelectedValue::DerivedNumber(native::DerivedNumber {
                value: number.value,
                origin: native::SourceSpan {
                    file_id: number.origin.file_id,
                    range: rowan::TextRange::new(
                        number.origin.range.start.into(),
                        number.origin.range.end.into(),
                    ),
                },
                occurrence_identity: number.occurrence_identity.clone(),
                proof: number.proof.clone(),
                rule: match &number.rule {
                    owned::DerivedNumberRule::NumericForIteration {
                        statement,
                        header,
                        iteration,
                        predecessor,
                    } => native::DerivedNumberRule::NumericForIteration {
                        statement: native_site(*statement),
                        header: header.iter().map(|site| native_site(*site)).collect(),
                        iteration: *iteration,
                        predecessor: predecessor.clone(),
                    },
                },
            })
        }
        owned::SelectedValue::EnvironmentCallable(callable) => {
            native::SelectedValue::EnvironmentCallable(callable.clone())
        }
        owned::SelectedValue::EnvironmentReturn(result) => {
            native::SelectedValue::EnvironmentReturn(result.clone())
        }
        _ => panic!("the atomic differential must not bypass missing recursive implementations"),
    }
}
fn native_decision(
    decision: &owned::BindingDecision<owned::BoundValue>,
) -> native::BindingDecision<native::BoundValue> {
    match decision {
        owned::BindingDecision::NotManaged => native::BindingDecision::NotManaged,
        owned::BindingDecision::KnownAbsent { proof } => native::BindingDecision::KnownAbsent {
            proof: proof.clone(),
        },
        owned::BindingDecision::Unavailable { proof, reason } => {
            native::BindingDecision::Unavailable {
                proof: proof.clone(),
                reason: reason.clone(),
            }
        }
        owned::BindingDecision::Bound(value) => {
            native::BindingDecision::Bound(native::BoundValue {
                identity: value.identity.clone(),
                proof: value.proof.clone(),
                value: native_value(&value.value),
            })
        }
    }
}
struct NativeProvider {
    site: native::BindingSite,
    decision: native::BindingDecision<native::BoundValue>,
}
impl native::BindingContext for NativeProvider {
    fn identity(&self) -> &str {
        "selected-projector-oracle"
    }
    fn resolve(&self, site: native::BindingSite) -> native::BindingDecision<native::BoundValue> {
        if site == self.site {
            self.decision.clone()
        } else {
            native::BindingDecision::NotManaged
        }
    }
}
fn owned_site(site: native::BindingSite) -> owned::BindingSite {
    owned::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
fn owned_row(row: native::BindingCoverage) -> owned::BindingCoverage {
    owned::BindingCoverage {
        site: owned_site(row.site),
        operation: row.operation,
        state: row.state,
        proof: row.proof,
        reason: row.reason,
        parameter: row.parameter,
        parameter_index: row.parameter_index,
        argument: row.argument.map(owned_site),
    }
}

#[test]
fn every_atomic_selected_type_and_all_coverage_fields_match_real_patched_model() {
    let (analysis, query, statement) = load();
    let ordinary = analysis
        .semantic_model(query.file_id)
        .type_of_expr(query.syntax);
    for case in cases(query, statement) {
        let native = native::BindingSession::new(Arc::new(NativeProvider {
            site: native_site(query),
            decision: native_decision(&case.decision),
        }));
        let old = analysis
            .semantic_model_with_context(query.file_id, native.clone())
            .type_of_expr(query.syntax);
        let expected = native
            .coverage()
            .into_iter()
            .map(owned_row)
            .collect::<Vec<_>>();
        let session =
            owned::OwnedBindingSession::new_checked(Arc::new(Provider), || Ok::<_, Stop>(()))
                .unwrap();
        let value = owned::SelectedProjector::new(&analysis, &session)
            .project_decision(
                query,
                case.decision.clone(),
                "resolve",
                &mut AtomicBackend::default(),
            )
            .unwrap();
        let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_case(&case, &value, &rows, query);
        assert_eq!(
            value.as_ref().unwrap_or(&ordinary),
            &old,
            "{} full LuaType",
            case.name
        );
        assert_eq!(
            format!("{:?}", value.as_ref().unwrap_or(&ordinary)),
            format!("{old:?}"),
            "{} all debug fields",
            case.name
        );
        assert_eq!(rows, expected, "{} all ordered ledger fields", case.name);
        println!(
            "SELECTED_ATOMIC_NATIVE_ORACLE {}",
            serde_json::json!({"case":case.name,"type":format!("{old:?}"),"coverage":format!("{rows:?}")})
        );
    }
}
