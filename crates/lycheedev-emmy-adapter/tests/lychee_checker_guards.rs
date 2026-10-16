//! Public diagnostic regressions for context-owned values and raw AST shortcuts.
// Original case names, fixtures, counterexamples and semantic assertions are
// retained. Diagnostics and coverage now execute on one owned session/registry.
#[path = "support/owned_query.rs"]
mod owned_query;
use emmylua_code_analysis::{CheckConfig, DiagnosticCode, EmmyLuaAnalysis, FileId};
use emmylua_parser::{LuaAstNode, LuaClosureExpr, LuaExpr};
use lycheedev_emmy_adapter::session::*;
use owned_query::{OwnedQueryTest, SessionCoverageTest};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Default)]
struct Context {
    decisions: HashMap<BindingSite, BindingDecision<BoundValue>>,
    calls: HashMap<BindingSite, BindingDecision<CallPermit>>,
    observations: HashMap<BindingSite, CallObservation>,
}
impl BindingContext for Context {
    fn identity(&self) -> &str {
        "checker-guard-context"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        self.decisions
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        self.calls
            .get(&site)
            .cloned()
            .unwrap_or(BindingDecision::NotManaged)
    }
    fn call_observation(&self, site: BindingSite) -> BindingDecision<CallObservation> {
        self.observations
            .get(&site)
            .cloned()
            .map(BindingDecision::Bound)
            .unwrap_or(BindingDecision::NotManaged)
    }
}
fn load(source: &str) -> (OwnedQueryTest, FileId) {
    let mut analysis = EmmyLuaAnalysis::new();
    let root = std::env::current_dir()
        .unwrap()
        .join("binding-checker-guard-input");
    analysis.add_main_workspace(root.clone());
    let analysis = OwnedQueryTest::ingest(
        analysis,
        vec![(root.join("guard.lua"), Some(Arc::from(source)))],
    )
    .unwrap();
    let file = analysis.file_ids()[0];
    (analysis, file)
}
fn site(analysis: &EmmyLuaAnalysis, file_id: FileId, text: &str) -> BindingSite {
    let expr = analysis
        .semantic_model(file_id)
        .syntax_tree()
        .unwrap()
        .get_red_root()
        .descendants()
        .filter_map(LuaExpr::cast)
        .find(|expr| expr.syntax().text().to_string() == text)
        .unwrap_or_else(|| panic!("missing expression: {text}"));
    BindingSite {
        file_id,
        syntax: expr.get_syntax_id(),
    }
}
fn unavailable() -> BindingDecision<BoundValue> {
    BindingDecision::Unavailable {
        proof: "guard-proof".into(),
        reason: "selected_value_unavailable".into(),
    }
}
fn expression(origin: BindingSite) -> BindingDecision<BoundValue> {
    BindingDecision::Bound(BoundValue {
        identity: "selected-expression".into(),
        proof: "selected-proof".into(),
        value: SelectedValue::Expression(origin),
    })
}
fn codes(
    analysis: &OwnedQueryTest,
    file: FileId,
    session: Arc<OwnedBindingSession>,
) -> Vec<DiagnosticCode> {
    let run = analysis
        .diagnose(file, Arc::new(CheckConfig::default()), &session)
        .unwrap();
    assert!(run.complete, "owned diagnostic registry did not complete");
    run.diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.code)
        .collect()
}

#[test]
fn selected_table_syntax_uses_selected_scalar_for_assignment_checks() {
    for (source, selected_table) in [
        (
            "local selected = 7\n---@type number\nlocal target\ntarget = {raw_only=901}\n",
            "{raw_only=901}",
        ),
        (
            "local selected = 7\n---@type { field: number }\nlocal target\ntarget = {field={raw_only=901}}\n",
            "{raw_only=901}",
        ),
    ] {
        let (analysis, file) = load(source);
        let mut provider = Context::default();
        provider.decisions.insert(
            site(&analysis, file, selected_table),
            expression(site(&analysis, file, "7")),
        );
        let session = owned_query::session(Arc::new(provider));
        let diagnostics = codes(&analysis, file, session);
        assert!(
            !diagnostics.contains(&DiagnosticCode::AssignTypeMismatch),
            "{source}: {diagnostics:?}"
        );
    }
}

#[test]
fn absent_or_unavailable_table_argument_does_not_check_raw_fields() {
    let source =
        "---@param value { field: number }\nlocal function F(value) end\nF({field='raw-wrong'})\n";
    for decision in [
        unavailable(),
        BindingDecision::KnownAbsent {
            proof: "known-nil".into(),
        },
    ] {
        let (analysis, file) = load(source);
        let mut provider = Context::default();
        let table = site(&analysis, file, "{field='raw-wrong'}");
        provider.decisions.insert(table, decision);
        let session = owned_query::session(Arc::new(provider));
        let diagnostics = codes(&analysis, file, session.clone());
        assert!(
            !diagnostics.contains(&DiagnosticCode::AssignTypeMismatch),
            "{diagnostics:?}"
        );
        assert!(session.coverage().iter().any(|row| row.site == table));
    }
}

#[test]
fn managed_callback_and_managed_container_skip_raw_parameter_counts() {
    for (source, managed_text) in [
        (
            "---@param callback fun()\nlocal function F(callback) end\nF(function(a,b) end)\n",
            "function(a,b) end",
        ),
        (
            "---@type { callback: fun() }\nlocal target = {callback=function(a,b) end}\n",
            "{callback=function(a,b) end}",
        ),
    ] {
        let (analysis, file) = load(source);
        let mut provider = Context::default();
        let managed = site(&analysis, file, managed_text);
        provider.decisions.insert(managed, unavailable());
        let session = owned_query::session(Arc::new(provider));
        let diagnostics = codes(&analysis, file, session.clone());
        assert!(
            !diagnostics.contains(&DiagnosticCode::RedundantParameter),
            "{source}: {diagnostics:?}"
        );
        assert!(
            session
                .coverage()
                .iter()
                .any(|row| row.site == managed && row.state == "unavailable")
        );
    }
}

#[test]
fn managed_condition_roots_and_children_do_not_fold_original_literals() {
    for (condition, selected_text) in [
        ("true", "true"),
        ("(true)", "true"),
        ("{}", "{}"),
        ("true and true", "true"),
    ] {
        let source = format!(
            "---@return number\nlocal function F()\n if {condition} then return 1 end\nend\n"
        );
        for decision in [
            unavailable(),
            BindingDecision::KnownAbsent {
                proof: "known-false".into(),
            },
        ] {
            let (analysis, file) = load(&source);
            let mut provider = Context::default();
            let managed = site(&analysis, file, selected_text);
            provider.decisions.insert(managed, decision);
            let session = owned_query::session(Arc::new(provider));
            let diagnostics = codes(&analysis, file, session.clone());
            assert!(
                diagnostics.contains(&DiagnosticCode::MissingReturn),
                "{condition}: {diagnostics:?}"
            );
            assert!(session.coverage().iter().any(|row| row.site == managed));
        }
    }
}

#[test]
fn managed_variadic_argument_packs_never_claim_completed_argument_type_checks() {
    for (statement, count) in [("F(G())", 2), ("F(G())", 0), ("F(G())", 1), ("F(0,G())", 2)] {
        let source = format!(
            "---@param a number\n---@param b string\nlocal function F(a,b) end\nlocal function G() return 1, true end\n{statement}\n"
        );
        let (analysis, file) = load(&source);
        let closures = analysis
            .semantic_model(file)
            .syntax_tree()
            .unwrap()
            .get_red_root()
            .descendants()
            .filter_map(LuaClosureExpr::cast)
            .collect::<Vec<_>>();
        let mut provider = Context::default();
        let outer = site(&analysis, file, statement);
        let inner = site(&analysis, file, "G()");
        for (name, call, closure) in [("F", outer, &closures[0]), ("G", inner, &closures[1])] {
            let callee = site(&analysis, file, name);
            provider.decisions.insert(
                callee,
                BindingDecision::Bound(BoundValue {
                    identity: name.into(),
                    proof: "callee-proof".into(),
                    value: SelectedValue::Function(FunctionRef {
                        file_id: file,
                        closure: closure.get_syntax_id(),
                        declaration: None,
                    }),
                }),
            );
            provider.calls.insert(
                call,
                BindingDecision::Bound(CallPermit {
                    callee,
                    proof: "permit-proof".into(),
                    capability_profile: "typed-call-v1".into(),
                }),
            );
        }
        let returned_literals = if count == 1 {
            vec!["true"]
        } else {
            vec!["1", "true"]
        };
        let returns = returned_literals
            .into_iter()
            .take(count)
            .map(|text| expression(site(&analysis, file, text)))
            .collect();
        provider.observations.insert(
            inner,
            CallObservation {
                callee: ObservedCallee::Source(FunctionRef {
                    file_id: file,
                    closure: closures[1].get_syntax_id(),
                    declaration: None,
                }),
                selected_callee_identity: "G".into(),
                closure_identity: "G-closure".into(),
                invocation_identity: "G-invocation".into(),
                returns,
                proof: "observed-return-proof".into(),
            },
        );
        let session = owned_query::session(Arc::new(provider));
        let _diagnostics = codes(&analysis, file, session.clone());
        let coverage = session.coverage();
        assert!(
            coverage.iter().any(|row| row.site == outer
                && row.operation == "argument_check"
                && row.state == "unavailable"
                && row.reason.as_deref() == Some("native_multi_argument_projection_unavailable")),
            "{statement}: {count} slots: {coverage:?}"
        );
        assert!(!coverage.iter().any(|row| row.site == outer
            && row.operation == "argument_check"
            && row.state == "owned_checked"));
    }
}

#[test]
fn managed_initializer_does_not_replace_its_declared_assignment_contract() {
    let source = "local selected = 2\n---@type string\nlocal target = 1\n";
    let (analysis, file) = load(source);
    let mut provider = Context::default();
    provider.decisions.insert(
        site(&analysis, file, "1"),
        expression(site(&analysis, file, "2")),
    );
    let session = owned_query::session(Arc::new(provider));
    let diagnostics = codes(&analysis, file, session);
    assert!(
        diagnostics.contains(&DiagnosticCode::AssignTypeMismatch),
        "{diagnostics:?}"
    );
}
