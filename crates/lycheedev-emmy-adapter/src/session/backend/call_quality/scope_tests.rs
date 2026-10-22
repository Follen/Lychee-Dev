//! The same selected call entry used by return-count diagnostics must own a VM
//! scope when called outside ordinary expression inference, and preserve an
//! existing callback scope when called inside it.
use super::super::{OwnedQuery, OwnedQueryFailure, environment::Environments};
use crate::session::{
    BindingContext, BindingCoverage, BindingDecision, BindingSite, BoundValue, CallPermit,
    FunctionRef, OwnedBindingSession, OwnedValue, SelectedValue,
};
use crate::{IngestedBatch, ingest_public_batch};
use emmylua_code_analysis::{EmmyLuaAnalysis, LuaType};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaClosureExpr};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

#[derive(Clone)]
struct Context {
    call: BindingSite,
    callee: BindingSite,
    argument: BindingSite,
    function: FunctionRef,
}
impl BindingContext for Context {
    fn identity(&self) -> &str {
        "permitted-call-scope"
    }
    fn resolve(&self, site: BindingSite) -> BindingDecision<BoundValue> {
        if site == self.callee {
            BindingDecision::Bound(BoundValue {
                identity: "actual-function".into(),
                proof: "callee-proof".into(),
                value: SelectedValue::Function(self.function.clone()),
            })
        } else if site == self.argument {
            BindingDecision::Unavailable {
                proof: "argument-proof".into(),
                reason: "not-observed".into(),
            }
        } else {
            BindingDecision::NotManaged
        }
    }
    fn call(&self, site: BindingSite) -> BindingDecision<CallPermit> {
        if site == self.call {
            BindingDecision::Bound(CallPermit {
                callee: self.callee,
                proof: "permit-proof".into(),
                capability_profile: "typed-call-v1".into(),
            })
        } else {
            BindingDecision::NotManaged
        }
    }
}
struct Fixture {
    analysis: EmmyLuaAnalysis,
    batch: IngestedBatch,
    context: Context,
}
impl Fixture {
    fn new() -> Self {
        let mut analysis = EmmyLuaAnalysis::new();
        let root = std::env::current_dir().unwrap().join("owned-call-scope");
        analysis.add_main_workspace(root.clone());
        let batch = ingest_public_batch(&mut analysis, vec![(root.join("main.lua"), Some(Arc::from(
            "---@param value string\n---@return string\nlocal function selected(value) return 'ok' end\nreturn selected(unknown)\n"
        )))], 1 << 20, || Ok::<_, ()>(())).unwrap();
        let file_id = batch.file_ids[0];
        let model = analysis.semantic_model(file_id);
        let root = model.syntax_tree().unwrap().get_red_root();
        let call = root.descendants().find_map(LuaCallExpr::cast).unwrap();
        let closure = root.descendants().find_map(LuaClosureExpr::cast).unwrap();
        let context = Context {
            call: BindingSite {
                file_id,
                syntax: call.get_syntax_id(),
            },
            callee: BindingSite {
                file_id,
                syntax: call.get_prefix_expr().unwrap().get_syntax_id(),
            },
            argument: BindingSite {
                file_id,
                syntax: call
                    .get_args_list()
                    .unwrap()
                    .get_args()
                    .next()
                    .unwrap()
                    .get_syntax_id(),
            },
            function: FunctionRef {
                file_id,
                closure: closure.get_syntax_id(),
                declaration: None,
            },
        };
        Self {
            analysis,
            batch,
            context,
        }
    }
}
fn result_rows(session: &OwnedBindingSession) -> Vec<BindingCoverage> {
    session
        .coverage_checked(|| Ok::<_, ()>(()))
        .unwrap()
        .into_iter()
        .filter(|row| row.operation == "call_result")
        .collect()
}
fn expected_result(site: BindingSite) -> BindingCoverage {
    BindingCoverage {
        site,
        operation: "call_result",
        state: "native_projected",
        proof: "permit-proof".into(),
        reason: None,
        parameter: None,
        parameter_index: None,
        argument: None,
    }
}

#[test]
fn direct_diagnostic_projection_owns_and_releases_its_root_execution_scope() {
    let fixture = Fixture::new();
    let session =
        OwnedBindingSession::new_checked(Arc::new(fixture.context.clone()), || Ok::<_, ()>(()))
            .unwrap();
    let mut control = || Ok::<_, ()>(());
    let mut query = OwnedQuery::new_with_contribution_order(
        &fixture.analysis,
        &session,
        fixture.context.call.file_id,
        &mut control,
        Arc::clone(&fixture.batch.contribution_order),
    );
    assert!(query.environments.borrow().is_empty());
    let actual = query
        .binding_type_of_expr(fixture.context.call.syntax)
        .unwrap()
        .unwrap();
    assert_eq!(actual.structural_type(), &LuaType::String);
    assert!(query.environments.borrow().is_empty());
    assert_eq!(
        result_rows(&session),
        [expected_result(fixture.context.call)]
    );
}

#[test]
fn nested_diagnostic_projection_reuses_callback_environment_without_popping_it() {
    let fixture = Fixture::new();
    let session =
        OwnedBindingSession::new_checked(Arc::new(fixture.context.clone()), || Ok::<_, ()>(()))
            .unwrap();
    let mut control = || Ok::<_, ()>(());
    let mut query = OwnedQuery::new_with_contribution_order(
        &fixture.analysis,
        &session,
        fixture.context.call.file_id,
        &mut control,
        Arc::clone(&fixture.batch.contribution_order),
    );
    let parameter = (fixture.context.function.closure, 0);
    let sentinel = OwnedValue::plain(LuaType::BooleanConst(false));
    let scope = query.enter_execution(HashMap::from([(parameter, sentinel.clone())]));
    let environment = query.environment();
    environment.borrow_mut().callback_depth = 3;
    let actual = query
        .binding_type_of_expr(fixture.context.call.syntax)
        .unwrap()
        .unwrap();
    assert_eq!(actual.structural_type(), &LuaType::String);
    assert_eq!(query.environments.borrow().len(), 1);
    assert!(Rc::ptr_eq(&query.environment(), &environment));
    assert_eq!(environment.borrow().callback_depth, 3);
    assert_eq!(
        environment.borrow().parameters.get(&parameter),
        Some(&sentinel)
    );
    assert_eq!(
        result_rows(&session),
        [expected_result(fixture.context.call)]
    );
    drop(scope);
    assert!(query.environments.borrow().is_empty());
}

#[test]
fn direct_projection_control_error_releases_scope_and_allows_a_real_retry() {
    let fixture = Fixture::new();
    let session =
        OwnedBindingSession::new_checked(Arc::new(fixture.context.clone()), || Ok::<_, ()>(()))
            .unwrap();
    let observed: Rc<RefCell<Option<Environments>>> = Rc::new(RefCell::new(None));
    let armed = Rc::new(Cell::new(true));
    let mut control = {
        let observed = Rc::clone(&observed);
        let armed = Rc::clone(&armed);
        move || {
            let inside = observed
                .borrow()
                .as_ref()
                .is_some_and(|stack| !stack.borrow().is_empty());
            if inside && armed.replace(false) {
                Err("original-deadline")
            } else {
                Ok(())
            }
        }
    };
    let mut query = OwnedQuery::new_with_contribution_order(
        &fixture.analysis,
        &session,
        fixture.context.call.file_id,
        &mut control,
        Arc::clone(&fixture.batch.contribution_order),
    );
    *observed.borrow_mut() = Some(Rc::clone(&query.environments));
    assert!(matches!(
        query.binding_type_of_expr(fixture.context.call.syntax),
        Err(OwnedQueryFailure::Control("original-deadline"))
    ));
    assert!(
        !armed.get(),
        "control must interrupt inside the acquired root scope"
    );
    assert!(query.environments.borrow().is_empty());
    assert!(result_rows(&session).is_empty());
    let retried = query
        .binding_type_of_expr(fixture.context.call.syntax)
        .unwrap()
        .unwrap();
    assert_eq!(retried.structural_type(), &LuaType::String);
    assert!(query.environments.borrow().is_empty());
    assert_eq!(
        result_rows(&session),
        [expected_result(fixture.context.call)]
    );
}
