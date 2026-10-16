//! Candidate consumes full values from a completed fixed-engine A/B reference run.
//! Actual fixed-engine differentials for the completed primitive/unary/executor
//! subset. The test backend is not a production name/member/call/flow solution.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/expression_execution_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use emmylua_code_analysis::LuaType;
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::{Backend, Stop, absent, load, select};

#[test]
fn actual_native_full_types_and_all_coverage_fields_match_primitive_unary_selected_subset() {
    let _trace = trace::begin_candidate();
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
            trace::check(&format!("case:{count}:{expression}:full_type"), &(new.ty));
            trace::check(
                &format!("case:{count}:{expression}:ordered_ledger"),
                &(rows),
            );
            println!(
                "EXPRESSION_EXECUTION_NATIVE_SUBSET {}",
                serde_json::json!({"expression":expression,"ownedType":format!("{:?}",new.ty),"coverage":format!("{rows:?}"),"scope":"primitive-unary-selected-overlay; general backend is not implemented"})
            );
            count += 1;
        }
    }
    assert_eq!(count, 63);
}

#[test]
fn actual_native_two_phase_order_matches_owned_execution_including_lhs_initializer() {
    let _trace = trace::begin_candidate();
    let (analysis, query, mut provider) = load(
        "local left=1\nlocal right=2\nreturn left+right",
        "left+right",
    );
    select(&mut provider, "right", absent("right-read"));
    select(&mut provider, "1", absent("left-initializer"));

    let session =
        owned::OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
            .unwrap();
    let mut backend = Backend::new(provider.events.clone());
    backend.recursive_initializers = true;
    let new = owned::SelectedProjector::new(&analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["right-read", "left-initializer"]
    );
    assert_eq!(new.ty, LuaType::Number);
    trace::check("full_type", &new.ty);
    trace::check("ordered_ledger", &(rows));
    println!(
        "EXPRESSION_EXECUTION_NATIVE_TWO_PHASE {}",
        serde_json::json!({"ownedType":format!("{:?}",new.ty),"coverage":format!("{rows:?}"),"scope":"single-initializer fixture backend; general declaration flow is not implemented"})
    );
}

#[test]
fn actual_native_unary_operator_return_types_match_owned_public_api() {
    let _trace = trace::begin_candidate();
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
        trace::check(&format!("{expression}:full_type"), &new.ty);
        trace::check(
            &format!("{expression}:ordered_ledger"),
            &(session.coverage_checked(|| Ok::<_, Stop>(())).unwrap()),
        );
        assert_eq!(
            new.ty,
            if expression == "-a" {
                LuaType::String
            } else {
                LuaType::Number
            }
        );
    }
}
