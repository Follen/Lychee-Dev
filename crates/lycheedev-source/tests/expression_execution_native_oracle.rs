//! Actual fixed-engine differentials for the completed primitive/unary/executor
//! subset. The test backend is not a production name/member/call/flow solution.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/expression_execution_cases.rs"]
mod support;
use emmylua_code_analysis::{LuaType, binding_context as native};
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::{Backend, Provider, Stop, absent, load, select};

struct NativeProvider(Provider);
fn to_owned(site: native::BindingSite) -> owned::BindingSite {
    owned::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
impl native::BindingContext for NativeProvider {
    fn identity(&self) -> &str {
        "expression-execution-oracle"
    }
    fn resolve(&self, site: native::BindingSite) -> native::BindingDecision<native::BoundValue> {
        match self.0.values.get(&to_owned(site)) {
            Some(owned::BindingDecision::KnownAbsent { proof }) => {
                native::BindingDecision::KnownAbsent {
                    proof: proof.clone(),
                }
            }
            Some(owned::BindingDecision::Unavailable { proof, reason }) => {
                native::BindingDecision::Unavailable {
                    proof: proof.clone(),
                    reason: reason.clone(),
                }
            }
            Some(owned::BindingDecision::Bound(_)) => {
                panic!("this differential deliberately has no unimplemented Bound projections")
            }
            _ => native::BindingDecision::NotManaged,
        }
    }
}
fn row(row: native::BindingCoverage) -> owned::BindingCoverage {
    owned::BindingCoverage {
        site: to_owned(row.site),
        operation: row.operation,
        state: row.state,
        proof: row.proof,
        reason: row.reason,
        parameter: row.parameter,
        parameter_index: row.parameter_index,
        argument: row.argument.map(to_owned),
    }
}

#[test]
fn actual_native_full_types_and_all_coverage_fields_match_primitive_unary_selected_subset() {
    let mut count = 0;
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
        "managed",
        "-managed",
        "#managed",
        "not managed",
        "(managed)",
        "{managed}",
    ] {
        for decision in [
            absent("absent-proof"),
            owned::BindingDecision::Unavailable {
                proof: "unavailable-proof".into(),
                reason: "selected-context-denied".into(),
            },
            owned::BindingDecision::NotManaged,
        ] {
            let (analysis, query, mut provider) = load(&format!("return {expression}"), expression);
            if provider.names.values().any(|text| text == "managed") {
                select(&mut provider, "managed", decision);
            }
            let native = native::BindingSession::new(Arc::new(NativeProvider(provider.clone())));
            let old = analysis
                .semantic_model_with_context(query.file_id, native.clone())
                .type_of_expr(query.syntax);
            let old_rows = native.coverage().into_iter().map(row).collect::<Vec<_>>();
            let session =
                owned::OwnedBindingSession::new_checked(Arc::new(provider.clone()), || {
                    Ok::<_, Stop>(())
                })
                .unwrap();
            let mut backend = Backend::new(provider.events.clone());
            let new = owned::SelectedProjector::new(&analysis, &session)
                .evaluate_expression(query, &mut backend)
                .unwrap();
            let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
            assert_eq!(new.ty, old, "{expression} complete type equality");
            assert_eq!(
                rows, old_rows,
                "{expression} complete ordered evidence equality"
            );
            println!(
                "EXPRESSION_EXECUTION_NATIVE_SUBSET {}",
                serde_json::json!({"expression":expression,"oldType":format!("{old:?}"),"ownedType":format!("{:?}",new.ty),"coverage":format!("{rows:?}"),"scope":"primitive-unary-selected-overlay; general backend is not implemented"})
            );
            count += 1;
        }
    }
    assert_eq!(count, 63);
}

#[test]
fn actual_native_two_phase_order_matches_owned_execution_including_lhs_initializer() {
    let (analysis, query, mut provider) = load(
        "local left=1\nlocal right=2\nreturn left+right",
        "left+right",
    );
    select(&mut provider, "right", absent("right-read"));
    select(&mut provider, "1", absent("left-initializer"));
    let native = native::BindingSession::new(Arc::new(NativeProvider(provider.clone())));
    let old = analysis
        .semantic_model_with_context(query.file_id, native.clone())
        .type_of_expr(query.syntax);
    let old_rows = native.coverage().into_iter().map(row).collect::<Vec<_>>();
    assert_eq!(
        old_rows
            .iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["right-read", "left-initializer"]
    );
    let session =
        owned::OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
            .unwrap();
    let mut backend = Backend::new(provider.events.clone());
    backend.recursive_initializers = true;
    let new = owned::SelectedProjector::new(&analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(old, LuaType::Number);
    assert_eq!(new.ty, old);
    assert_eq!(
        rows, old_rows,
        "no prefix slicing or dropped producers allowed"
    );
    println!(
        "EXPRESSION_EXECUTION_NATIVE_TWO_PHASE {}",
        serde_json::json!({"oldType":format!("{old:?}"),"ownedType":format!("{:?}",new.ty),"coverage":format!("{rows:?}"),"scope":"single-initializer fixture backend; general declaration flow is not implemented"})
    );
}

#[test]
fn actual_native_unary_operator_return_types_match_owned_public_api() {
    for expression in ["-a", "#a"] {
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
        let raw_operand = analysis
            .semantic_model(query.file_id)
            .type_of_expr(a.syntax);
        assert!(matches!(raw_operand, LuaType::Ref(_) | LuaType::Def(_)));
        let native = native::BindingSession::new(Arc::new(NativeProvider(provider.clone())));
        let old = analysis
            .semantic_model_with_context(query.file_id, native.clone())
            .type_of_expr(query.syntax);
        let old_rows = native.coverage().into_iter().map(row).collect::<Vec<_>>();
        let session = owned::OwnedBindingSession::new_checked(Arc::new(provider.clone()), || {
            Ok::<_, Stop>(())
        })
        .unwrap();
        let mut backend = Backend::new(provider.events.clone());
        // Operand type is test setup from the same unselected docs; this is not
        // a raw-inference fallback in the production executor.
        backend.name_types.insert("a".into(), raw_operand);
        let new = owned::SelectedProjector::new(&analysis, &session)
            .evaluate_expression(query, &mut backend)
            .unwrap();
        assert_eq!(new.ty, old);
        assert_eq!(
            session.coverage_checked(|| Ok::<_, Stop>(())).unwrap(),
            old_rows
        );
        assert_eq!(
            old,
            if expression == "-a" {
                LuaType::String
            } else {
                LuaType::Number
            }
        );
    }
}
