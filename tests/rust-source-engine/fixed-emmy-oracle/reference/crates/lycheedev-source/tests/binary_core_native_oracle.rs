//! Full type/ledger comparisons against the real fixed contextual engine for
//! the completed binary core. General logical projection remains deferred.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/expression_execution_cases.rs"]
mod support;
#[path = "support/fixed_oracle_trace.rs"]
mod trace;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType, binding_context as native};
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
fn to_native(site: owned::BindingSite) -> native::BindingSite {
    native::BindingSite {
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
            Some(owned::BindingDecision::Bound(value)) => {
                let owned::SelectedValue::Expression(origin) = &value.value else {
                    panic!("only real expression projection is in this oracle")
                };
                native::BindingDecision::Bound(native::BoundValue {
                    identity: value.identity.clone(),
                    proof: value.proof.clone(),
                    value: native::SelectedValue::Expression(to_native(*origin)),
                })
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
fn compare(
    analysis: &EmmyLuaAnalysis,
    query: owned::BindingSite,
    provider: Provider,
    name_types: &[(&str, LuaType)],
) -> (LuaType, Vec<owned::BindingCoverage>) {
    let native = native::BindingSession::new(Arc::new(NativeProvider(provider.clone())));
    let old = analysis
        .semantic_model_with_context(query.file_id, native.clone())
        .type_of_expr(query.syntax);
    let old_rows = native.coverage().into_iter().map(row).collect::<Vec<_>>();
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
    trace::record(&format!("{expression}:full_type"), &(old));
    assert_eq!(result.ty, old, "{expression} complete binary type");
    trace::record(&format!("{expression}:ordered_ledger"), &(old_rows));
    assert_eq!(rows, old_rows, "{expression} complete ordered evidence");
    println!(
        "BINARY_CORE_NATIVE {}",
        serde_json::json!({"expression":expression,"oldType":format!("{old:?}"),"ownedType":format!("{:?}",result.ty),"coverage":format!("{rows:?}")})
    );
    (old, rows)
}
const OPS: [&str; 19] = [
    "+", "-", "*", "/", "//", "%", "^", "..", "<", "<=", ">", ">=", "==", "~=", "&", "|", "~",
    "<<", ">>",
];

#[test]
fn native_binary_literal_matrix_matches_full_types_without_raw_binary_fallback() {
    let _trace = trace::begin_reference();
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
    let _trace = trace::begin_reference();
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
    let _trace = trace::begin_reference();
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
    let _trace = trace::begin_reference();
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
