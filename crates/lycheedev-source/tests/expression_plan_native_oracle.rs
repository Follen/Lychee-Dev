//! Observe real fixed compiler/VM producer order. The owned plan is not an
//! executor, so this target never labels its missing execution phase equivalent.
#![cfg(feature = "emmy-prototype")]
#[path = "../../lycheedev-emmy-adapter/tests/support/selected_query_cases.rs"]
mod support;
use emmylua_code_analysis::{EmmyLuaAnalysis, binding_context as native};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaReturnStat};
use lycheedev_emmy_adapter::session as owned;
use std::{collections::HashMap, sync::Arc};
use support::{Backend, Stop};

#[derive(Clone)]
struct Provider {
    values: HashMap<owned::BindingSite, String>,
}
impl owned::BindingContext for Provider {
    fn identity(&self) -> &str {
        "expression-plan-oracle"
    }
    fn resolve(&self, site: owned::BindingSite) -> owned::BindingDecision<owned::BoundValue> {
        self.values
            .get(&site)
            .map(|proof| owned::BindingDecision::KnownAbsent {
                proof: proof.clone(),
            })
            .unwrap_or(owned::BindingDecision::NotManaged)
    }
}
struct NativeProvider(Provider);
fn to_owned(site: native::BindingSite) -> owned::BindingSite {
    owned::BindingSite {
        file_id: site.file_id,
        syntax: site.syntax,
    }
}
impl native::BindingContext for NativeProvider {
    fn identity(&self) -> &str {
        "expression-plan-oracle"
    }
    fn resolve(&self, site: native::BindingSite) -> native::BindingDecision<native::BoundValue> {
        self.0
            .values
            .get(&to_owned(site))
            .map(|proof| native::BindingDecision::KnownAbsent {
                proof: proof.clone(),
            })
            .unwrap_or(native::BindingDecision::NotManaged)
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
fn load(
    source: &str,
    selected: &[(&str, &str)],
) -> (EmmyLuaAnalysis, owned::BindingSite, Provider) {
    let mut analysis = EmmyLuaAnalysis::new();
    let path = std::env::current_dir()
        .unwrap()
        .join("expression-plan-oracle.lua");
    let file = analysis.update_files_by_path(vec![(path, Some(source.into()))])[0];
    let model = analysis.semantic_model(file);
    let root = model.syntax_tree().unwrap().get_red_root();
    let expr = root
        .descendants()
        .filter_map(LuaReturnStat::cast)
        .last()
        .unwrap()
        .get_expr_list()
        .next()
        .unwrap();
    let query = owned::BindingSite {
        file_id: file,
        syntax: expr.get_syntax_id(),
    };
    let mut values = HashMap::new();
    for expr in root.descendants().filter_map(LuaExpr::cast) {
        let text = expr.syntax().text().to_string();
        if let Some((_, proof)) = selected.iter().find(|(name, _)| *name == text) {
            values.insert(
                owned::BindingSite {
                    file_id: file,
                    syntax: expr.get_syntax_id(),
                },
                (*proof).into(),
            );
        }
    }
    (analysis, query, Provider { values })
}

#[test]
fn selected_compile_producers_match_actual_native_rows_when_execution_adds_no_producers() {
    for expression in [
        "left + right",
        "left and right",
        "left or right",
        "not left",
        "(left)",
        "{left}",
        "obj[left]",
        "g(left,right)",
        "-left",
        "#left",
    ] {
        let (analysis, query, provider) = load(
            &format!("return {expression}"),
            &[
                ("left", "left-proof"),
                ("right", "right-proof"),
                ("obj", "object-proof"),
                ("g", "callee-proof"),
            ],
        );
        let native = native::BindingSession::new(Arc::new(NativeProvider(provider.clone())));
        let old_type = analysis
            .semantic_model_with_context(query.file_id, native.clone())
            .type_of_expr(query.syntax);
        let old_rows = native.coverage().into_iter().map(row).collect::<Vec<_>>();
        let session =
            owned::OwnedBindingSession::new_checked(Arc::new(provider), || Ok::<_, Stop>(()))
                .unwrap();
        let plan = owned::SelectedProjector::new(&analysis, &session)
            .compile_expression(query, &mut Backend::default())
            .unwrap();
        let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_eq!(
            rows, old_rows,
            "{expression} all compiler-produced ledger fields"
        );
        if expression == "left and right" || expression == "left or right" {
            assert_eq!(
                rows.iter()
                    .map(|row| row.proof.as_str())
                    .collect::<Vec<_>>(),
                ["left-proof", "right-proof"],
                "both operands compile before short-circuit execution"
            );
        }
        if expression == "not left" || expression == "{left}" {
            assert!(rows.is_empty());
        }
        println!(
            "EXPRESSION_PLAN_NATIVE_PREFIX {}",
            serde_json::json!({"expression":expression,"referenceType":format!("{old_type:?}"),"ownedPlan":format!("{plan:?}"),"coverage":format!("{rows:?}"),"ownedExecution":"NOT_IMPLEMENTED"})
        );
    }
}

#[test]
fn real_native_compiles_rhs_selected_before_evaluating_lhs_declaration() {
    let (analysis, query, provider) = load(
        "local left=1\nlocal right=2\nreturn left+right",
        &[
            ("1", "left-initializer-proof"),
            ("right", "right-expression-proof"),
        ],
    );
    let native = native::BindingSession::new(Arc::new(NativeProvider(provider.clone())));
    let old_type = analysis
        .semantic_model_with_context(query.file_id, native.clone())
        .type_of_expr(query.syntax);
    let old_rows = native.coverage().into_iter().map(row).collect::<Vec<_>>();
    assert_eq!(
        old_rows
            .iter()
            .map(|row| row.proof.as_str())
            .collect::<Vec<_>>(),
        ["right-expression-proof", "left-initializer-proof"],
        "source-level recursive evaluation would reverse these producers"
    );
    let session =
        owned::OwnedBindingSession::new_checked(Arc::new(provider), || Ok::<_, Stop>(())).unwrap();
    let plan = owned::SelectedProjector::new(&analysis, &session)
        .compile_expression(query, &mut Backend::default())
        .unwrap();
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(
        rows,
        old_rows[..1],
        "compiler finishes only the RHS selected producer"
    );
    assert!(
        matches!(&plan.instructions[0],owned::ExpressionInstruction::LoadName {name,..} if name=="left")
    );
    assert!(matches!(
        &plan.instructions[1],
        owned::ExpressionInstruction::PushValue(emmylua_code_analysis::LuaType::Nil)
    ));
    assert!(matches!(
        &plan.instructions[2],
        owned::ExpressionInstruction::Binary { .. }
    ));
    println!(
        "EXPRESSION_TWO_PHASE_COUNTEREXAMPLE {}",
        serde_json::json!({"referenceType":format!("{old_type:?}"),"nativeFullCoverage":format!("{old_rows:?}"),"ownedCompilePrefix":format!("{rows:?}"),"deferred":"LoadName(left) must append initializer after this prefix"})
    );
}
