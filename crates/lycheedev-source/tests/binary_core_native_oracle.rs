//! Candidate consumes full values from a completed fixed-engine A/B reference run.
//! Full type/ledger comparisons against the real fixed contextual engine for
//! the completed binary core. General logical projection remains deferred.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/expression_execution_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType};
use lycheedev_emmy_adapter::session as owned;
use std::sync::Arc;
use support::{Backend, Provider, Stop, absent, load, select};

fn compare(
    analysis: &EmmyLuaAnalysis,
    query: owned::BindingSite,
    provider: Provider,
    name_types: &[(&str, LuaType)],
) -> (LuaType, Vec<owned::BindingCoverage>) {
    let session =
        owned::OwnedBindingSession::new_checked(Arc::new(provider.clone()), || Ok::<_, Stop>(()))
            .unwrap();
    let mut backend = Backend::new(provider.events.clone());
    backend.primitive_binary = true;
    backend.name_types.extend(
        name_types
            .iter()
            .map(|(name, ty)| ((*name).into(), ty.clone())),
    );
    let result = owned::SelectedProjector::new(analysis, &session)
        .evaluate_expression(query, &mut backend)
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    let expression = &provider.names[&query];
    trace::check(&format!("{expression}:full_type"), &(result.ty));
    trace::check(&format!("{expression}:ordered_ledger"), &(rows));
    println!(
        "BINARY_CORE_NATIVE {}",
        serde_json::json!({"expression":expression,"ownedType":format!("{:?}",result.ty),"coverage":format!("{rows:?}")})
    );
    (result.ty, rows)
}
const OPS: [&str; 19] = [
    "+", "-", "*", "/", "//", "%", "^", "..", "<", "<=", ">", ">=", "==", "~=", "&", "|", "~",
    "<<", ">>",
];

#[test]
fn native_binary_literal_matrix_matches_full_types_without_raw_binary_fallback() {
    let _trace = trace::begin_candidate();
    let mut count = 0;
    for op in OPS {
        for (left, right) in [
            ("1", "2"),
            ("3", "3"),
            ("1.5", "2"),
            ("nil", "nil"),
            ("true", "false"),
            ("'left'", "'right'"),
            ("0x7fffffffffffffff", "1"),
            ("0x8000000000000000", "1"),
            ("0x8000000000000000", "0xffffffffffffffff"),
            ("{}", "2"),
            ("1", "{}"),
        ] {
            let expression = format!("{left} {op} {right}");
            let (analysis, query, provider) = load(&format!("return {expression}"), &expression);
            let (_, rows) = compare(&analysis, query, provider, &[]);
            assert!(rows.is_empty());
            count += 1;
        }
    }
    assert_eq!(count, 209);
}

fn bind(provider: &mut Provider, name: &str, origin_text: &str) {
    let origin = *provider
        .names
        .iter()
        .find(|(_, text)| text.as_str() == origin_text)
        .unwrap()
        .0;
    select(
        provider,
        name,
        owned::BindingDecision::Bound(owned::BoundValue {
            identity: format!("selected-{name}"),
            proof: format!("{name}-proof"),
            value: owned::SelectedValue::Expression(origin),
        }),
    );
}
fn unavailable() -> owned::BindingDecision<owned::BoundValue> {
    owned::BindingDecision::Unavailable {
        proof: "denied-proof".into(),
        reason: "selected-binary-operand-denied".into(),
    }
}

#[test]
fn native_selected_operand_matrix_preserves_unknown_nil_and_all_producer_fields() {
    let _trace = trace::begin_candidate();
    let mut count = 0;
    for op in OPS {
        for variant in 0..4 {
            let expression = format!("left {op} right");
            let source = format!("local origin_left=1\nlocal origin_right=2\nreturn {expression}");
            let (analysis, query, mut provider) = load(&source, &expression);
            bind(&mut provider, "left", "1");
            bind(&mut provider, "right", "2");
            match variant {
                1 => select(&mut provider, "left", unavailable()),
                2 => select(&mut provider, "right", unavailable()),
                3 => {
                    select(&mut provider, "left", absent("left-absent"));
                    select(&mut provider, "right", absent("right-absent"));
                }
                _ => {}
            }
            let (ty, rows) = compare(&analysis, query, provider, &[]);
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[0].operation, "resolve");
            assert_eq!(rows[1].operation, "resolve");
            if variant == 1 || variant == 2 {
                assert_eq!(ty, LuaType::Unknown);
            }
            count += 1;
        }
    }
    assert_eq!(count, 76);
    for (expression, truthy) in [("left and right", false), ("left or right", true)] {
        let source = format!("local origin_left=true\nreturn {expression}");
        let (analysis, query, mut provider) = load(&source, expression);
        if truthy {
            bind(&mut provider, "left", "true");
        } else {
            select(&mut provider, "left", absent("left-absent"));
        }
        select(&mut provider, "right", unavailable());
        let (ty, rows) = compare(&analysis, query, provider, &[]);
        assert_eq!(
            ty,
            if truthy {
                LuaType::BooleanConst(true)
            } else {
                LuaType::Nil
            }
        );
        assert_eq!(
            rows.len(),
            2,
            "RHS selected producer runs even when binary returns LHS"
        );
    }
}

#[test]
fn native_operator_overloads_keep_left_precedence_and_contextual_unknown_gate() {
    let _trace = trace::begin_candidate();
    let source = "---@class Left\n---@operator add: string\nlocal Left={}\n---@class Right\n---@operator add: boolean\nlocal Right={}\n---@type Left\nlocal a\n---@type Right\nlocal b\nreturn a+b";
    let (analysis, query, provider) = load(source, "a+b");
    let model = analysis.semantic_model(query.file_id);
    let a = *provider
        .names
        .iter()
        .find(|(_, text)| text.as_str() == "a")
        .unwrap()
        .0;
    let b = *provider
        .names
        .iter()
        .find(|(_, text)| text.as_str() == "b")
        .unwrap()
        .0;
    let a_type = model.type_of_expr(a.syntax);
    let b_type = model.type_of_expr(b.syntax);
    let (ty, rows) = compare(
        &analysis,
        query,
        provider.clone(),
        &[("a", a_type.clone()), ("b", b_type.clone())],
    );
    assert_eq!(ty, LuaType::String);
    assert!(rows.is_empty());
    let mut denied = provider;
    select(&mut denied, "b", unavailable());
    let (ty, rows) = compare(&analysis, query, denied, &[("a", a_type), ("b", b_type)]);
    assert_eq!(ty, LuaType::Unknown);
    assert_eq!(rows.len(), 1);
}

#[test]
fn native_contextual_any_is_unknown_for_every_nonlogical_operator() {
    let _trace = trace::begin_candidate();
    for op in OPS {
        let expression = format!("a {op} 1");
        let source = format!("---@type any\nlocal a\nreturn {expression}");
        let (analysis, query, provider) = load(&source, &expression);
        let a = *provider
            .names
            .iter()
            .find(|(_, text)| text.as_str() == "a")
            .unwrap()
            .0;
        let operand = analysis
            .semantic_model(query.file_id)
            .type_of_expr(a.syntax);
        assert_eq!(operand, LuaType::Any);
        let (ty, rows) = compare(&analysis, query, provider, &[("a", operand)]);
        assert_eq!(
            ty,
            LuaType::Unknown,
            "contextual Any must not use raw Any propagation"
        );
        assert!(rows.is_empty());
    }
}
