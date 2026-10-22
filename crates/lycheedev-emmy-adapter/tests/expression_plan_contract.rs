#[path = "support/selected_query_cases.rs"]
mod support;
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaSyntaxKind};
use lycheedev_emmy_adapter::session::*;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use support::{Backend, Stop};

struct Provider {
    text: HashMap<BindingSite, String>,
    selected: Option<BindingSite>,
    visits: Mutex<Vec<BindingSite>>,
}
impl BindingContext for Provider {
    fn identity(&self) -> &str {
        "expression-plan"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.visits.lock().unwrap().push(site);
        if self.selected == Some(site) {
            BindingDecision::KnownAbsent {
                proof: "selected-proof".into(),
            }
        } else {
            BindingDecision::NotManaged
        }
    }
}
fn load(expr: &str, selected: Option<&str>) -> (EmmyLuaAnalysis, BindingSite, Arc<Provider>) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("expression-plan-logical");
    let file = analysis.update_files_by_path(vec![(
        root.join("input.lua"),
        Some(format!("local function outer(...) return {expr} end")),
    )])[0];
    let model = analysis.semantic_model(file);
    let tree = model.syntax_tree().unwrap();
    let expressions = tree
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
        .collect::<Vec<_>>();
    let target = expressions
        .iter()
        .find(|value| value.syntax().text().to_string() == expr)
        .unwrap();
    let site = BindingSite {
        file_id: file,
        syntax: target.get_syntax_id(),
    };
    let text = expressions
        .into_iter()
        .map(|expr| {
            (
                BindingSite {
                    file_id: file,
                    syntax: expr.get_syntax_id(),
                },
                expr.syntax().text().to_string(),
            )
        })
        .collect::<HashMap<_, _>>();
    let selected = selected.map(|name| {
        *text
            .iter()
            .find(|(_, value)| value.as_str() == name)
            .unwrap()
            .0
    });
    (
        analysis,
        site,
        Arc::new(Provider {
            text,
            selected,
            visits: Mutex::new(vec![]),
        }),
    )
}
fn labels(plan: &ExpressionPlan<LuaType>) -> Vec<String> {
    plan.instructions
        .iter()
        .map(|instruction| match instruction {
            ExpressionInstruction::PushValue(value) => format!("value:{value:?}"),
            ExpressionInstruction::PushClosure { .. } => "closure".into(),
            ExpressionInstruction::LoadName { name, .. } => format!("name:{name}"),
            ExpressionInstruction::IndexMember { key, dynamic } => {
                format!("index:{key:?}:{dynamic}")
            }
            ExpressionInstruction::Call {
                arg_count,
                colon_call,
                ..
            } => format!("call:{arg_count}:{colon_call}"),
            ExpressionInstruction::Unary { op } => format!("unary:{op:?}"),
            ExpressionInstruction::Binary { op, .. } => format!("binary:{op:?}"),
            ExpressionInstruction::Result => "result".into(),
        })
        .collect()
}
fn visit_text(provider: &Provider) -> Vec<String> {
    provider
        .visits
        .lock()
        .unwrap()
        .iter()
        .map(|site| provider.text[site].clone())
        .collect()
}

#[test]
fn compile_order_defers_names_and_visits_call_arguments_left_to_right() {
    let (analysis, site, provider) = load("f(a,b[c],d)", Some("d"));
    let session = OwnedBindingSession::new_checked(provider.clone(), || Ok::<_, Stop>(())).unwrap();
    let plan = SelectedProjector::new(&analysis, &session)
        .compile_expression(site, &mut Backend::default())
        .unwrap();
    assert_eq!(
        visit_text(&provider),
        ["f(a,b[c],d)", "f", "a", "b[c]", "b", "c", "d"]
    );
    let values = labels(&plan);
    assert_eq!(
        &values[..5],
        ["name:f", "name:a", "name:b", "name:c", "index:None:true"]
    );
    assert_eq!(&values[5..], ["value:Nil", "call:3:false", "result"]);
    let ExpressionInstruction::Call {
        arg_count,
        generic_type_syntaxes,
        arg_syntaxes,
        ..
    } = &plan.instructions[6]
    else {
        unreachable!()
    };
    assert_eq!(*arg_count, 3);
    assert!(generic_type_syntaxes.is_empty());
    assert_eq!(arg_syntaxes.len(), 3);
    assert_eq!(plan.file_id, site.file_id);
    let rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].site, provider.selected.unwrap());
    assert_eq!(rows[0].operation, "resolve");
    assert!(
        rows.iter()
            .all(|row| row.operation != "argument_check" && row.operation != "callable_check")
    );
}

#[test]
fn not_and_table_compile_without_visiting_ignored_children() {
    for (source, selected, expected) in [
        ("not selected", "selected", "value:Boolean"),
        ("{selected, other=1}", "selected", ""),
    ] {
        let (analysis, site, provider) = load(source, Some(selected));
        let session =
            OwnedBindingSession::new_checked(provider.clone(), || Ok::<_, Stop>(())).unwrap();
        let plan = SelectedProjector::new(&analysis, &session)
            .compile_expression(site, &mut Backend::default())
            .unwrap();
        assert_eq!(visit_text(&provider), [source]);
        assert!(
            session
                .coverage_checked(|| Ok::<_, Stop>(()))
                .unwrap()
                .is_empty()
        );
        assert_eq!(plan.instructions.len(), 2);
        if expected.is_empty() {
            assert!(matches!(
                &plan.instructions[0],
                ExpressionInstruction::PushValue(LuaType::TableConst(_))
            ));
        } else {
            assert_eq!(labels(&plan)[0], expected);
        }
    }
}

#[test]
fn parentheses_binary_and_unary_keep_fixed_task_order_and_operands() {
    for (source, visits) in [
        (
            "(left + selected)",
            vec!["(left + selected)", "left + selected", "left", "selected"],
        ),
        (
            "left and selected",
            vec!["left and selected", "left", "selected"],
        ),
        ("-selected", vec!["-selected", "selected"]),
        ("#selected", vec!["#selected", "selected"]),
    ] {
        let (analysis, site, provider) = load(source, Some("selected"));
        let session =
            OwnedBindingSession::new_checked(provider.clone(), || Ok::<_, Stop>(())).unwrap();
        let plan = SelectedProjector::new(&analysis, &session)
            .compile_expression(site, &mut Backend::default())
            .unwrap();
        assert_eq!(visit_text(&provider), visits, "{source}");
        if source.contains("left") {
            assert!(
                matches!(&plan.instructions[0],ExpressionInstruction::LoadName {name,..} if name=="left")
            );
            assert!(matches!(
                &plan.instructions[1],
                ExpressionInstruction::PushValue(LuaType::Nil)
            ));
            let ExpressionInstruction::Binary {
                left_syntax: Some(left),
                right_syntax: Some(right),
                ..
            } = &plan.instructions[2]
            else {
                panic!("expected deferred binary")
            };
            assert_eq!(
                provider.text[&BindingSite {
                    file_id: site.file_id,
                    syntax: *left
                }],
                "left"
            );
            assert_eq!(*right, provider.selected.unwrap().syntax);
        } else {
            assert!(matches!(
                &plan.instructions[1],
                ExpressionInstruction::Unary { .. }
            ));
        }
    }
}

#[test]
fn public_literal_types_varargs_static_keys_and_closures_keep_distinct_instructions() {
    for source in [
        "1",
        "1.25",
        "0xffffffffffffffff",
        "'界🙂'",
        "true",
        "false",
        "nil",
    ] {
        let (analysis, site, provider) = load(source, None);
        let session = OwnedBindingSession::new_checked(provider, || Ok::<_, Stop>(())).unwrap();
        let expected = analysis
            .semantic_model(site.file_id)
            .type_of_expr(site.syntax);
        let plan = SelectedProjector::new(&analysis, &session)
            .compile_expression(site, &mut Backend::default())
            .unwrap();
        let ExpressionInstruction::PushValue(value) = &plan.instructions[0] else {
            panic!("expected literal")
        };
        assert_eq!(value, &expected, "{source}");
        assert_eq!(plan.instructions.len(), 2);
    }
    for source in [
        "...",
        "obj.name",
        "obj['name']",
        "obj[3]",
        "obj:method(a)",
        "function() return 1 end",
    ] {
        let (analysis, site, provider) = load(source, None);
        let session = OwnedBindingSession::new_checked(provider, || Ok::<_, Stop>(())).unwrap();
        let plan = SelectedProjector::new(&analysis, &session)
            .compile_expression(site, &mut Backend::default())
            .unwrap();
        match source {
            "..." => assert!(
                matches!(&plan.instructions[0],ExpressionInstruction::LoadName {name,..} if name=="...")
            ),
            "function() return 1 end" => assert!(
                matches!(&plan.instructions[0],ExpressionInstruction::PushClosure {syntax} if *syntax==site.syntax)
            ),
            "obj:method(a)" => assert!(plan.instructions.iter().any(|instruction| matches!(
                instruction,
                ExpressionInstruction::Call {
                    colon_call: true,
                    arg_count: 1,
                    ..
                }
            ))),
            _ => assert!(plan.instructions.iter().any(|instruction| matches!(
                instruction,
                ExpressionInstruction::IndexMember {
                    key: Some(_),
                    dynamic: false
                }
            ))),
        }
    }
}

#[test]
fn every_compile_checkpoint_returns_original_failure_without_partial_plan() {
    let (analysis, site, provider) = load("f(left + selected, obj[index])", Some("selected"));
    let session = OwnedBindingSession::new_checked(provider.clone(), || Ok::<_, Stop>(())).unwrap();
    let mut completed = Backend::default();
    SelectedProjector::new(&analysis, &session)
        .compile_expression(site, &mut completed)
        .unwrap();
    let full_rows = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
    for ordinal in 1..=completed.checkpoints {
        let session =
            OwnedBindingSession::new_checked(provider.clone(), || Ok::<_, Stop>(())).unwrap();
        let token = Arc::new(());
        let mut backend = Backend {
            checkpoints: 0,
            stop_at: ordinal,
            token: token.clone(),
        };
        let error = SelectedProjector::new(&analysis, &session)
            .compile_expression(site, &mut backend)
            .unwrap_err();
        assert_eq!(error.ordinal, ordinal);
        assert!(Arc::ptr_eq(&error.token, &token));
        let prefix = session.coverage_checked(|| Ok::<_, Stop>(())).unwrap();
        assert_eq!(prefix, full_rows[..prefix.len()]);
    }
}

#[test]
fn malformed_expression_does_not_consult_selected_provider() {
    let (analysis, site, provider) = load("selected", Some("selected"));
    let session = OwnedBindingSession::new_checked(provider.clone(), || Ok::<_, Stop>(())).unwrap();
    let range = analysis
        .semantic_model(site.file_id)
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .text_range();
    let invalid = BindingSite {
        file_id: site.file_id,
        syntax: emmylua_parser::LuaSyntaxId::new(LuaSyntaxKind::NameExpr.into(), range),
    };
    let plan = SelectedProjector::new(&analysis, &session)
        .compile_expression(invalid, &mut Backend::default())
        .unwrap();
    assert_eq!(labels(&plan), ["value:Unknown", "result"]);
    assert!(provider.visits.lock().unwrap().is_empty());
    assert!(
        session
            .coverage_checked(|| Ok::<_, Stop>(()))
            .unwrap()
            .is_empty()
    );
}
