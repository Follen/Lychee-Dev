//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{AsyncState, LuaFunctionType, LuaUnionType, SemanticId};
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_parser::{
    LuaAst, LuaAstNode, LuaCallArgList, LuaCallExpr, LuaClosureExpr, LuaComment, LuaDocTag,
    LuaExpr, LuaSyntaxId,
};
use std::collections::HashSet;
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_await_in_sync(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        let async_closures = collect_async_closures(&root);
        for call in root.descendants().filter_map(LuaCallExpr::cast) {
            self.await_call_in_async(context, &call, &async_closures)?;
            self.await_call_as_arg(context, &call, &async_closures)?;
        }
        Ok(())
    }
    fn await_callee_candidates(&mut self, call: &LuaCallExpr) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(prefix) = call.get_prefix_expr() else {
            return Ok(Vec::new());
        };
        let mut candidates = self.callable_candidates_cached(&prefix)?;
        // Declaration docs supplement ordinary VM signatures, but cannot
        // resurrect a different callee after an authoritative selected read.
        if let LuaExpr::NameExpr(name) = &prefix
            && let Some(text) = name.get_name_text()
            && !self.binding_manages(prefix.get_syntax_id())?
        {
            let decl = self
                .model
                .resolve_name(name.get_position())
                .or_else(|| self.model.global_decl(text.as_str()));
            if let Some(decl) = decl
                && let Some(func) = doc_signature_for_decl(&self.model, &decl)
            {
                let func = OwnedValue::plain(LuaType::DocFunction(std::sync::Arc::new(func)));
                if !candidates.contains(&func) {
                    candidates.insert(0, func);
                }
            }
        }
        Ok(candidates)
    }
    fn await_call_in_async(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
        async_closures: &HashSet<LuaSyntaxId>,
    ) -> QueryResult<(), E> {
        let Some(prefix) = call.get_prefix_expr() else {
            return Ok(());
        };
        let candidates = self.await_callee_candidates(call)?;
        if !candidates.iter().any(is_async_function) {
            return Ok(());
        }
        if self.await_in_async_context(call, async_closures)? {
            return Ok(());
        }
        context.add_diagnostic(
            DiagnosticCode::AwaitInSync,
            prefix.get_range(),
            t!("Async function can only be called in async function."),
        );
        Ok(())
    }
    fn await_call_as_arg(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
        async_closures: &HashSet<LuaSyntaxId>,
    ) -> QueryResult<(), E> {
        let Some(args) = call.get_args_list() else {
            return Ok(());
        };
        let args = args.get_args().collect::<Vec<_>>();
        if args.is_empty() {
            return Ok(());
        }
        let candidates = self.await_callee_candidates(call)?;
        let colon_call = call.is_colon_call();
        let mut reported = HashSet::new();
        for func in &candidates {
            self.checkpoint()?;
            let LuaType::DocFunction(raw) = func.structural_type() else {
                unreachable!("callable candidate")
            };
            for (param_idx, param) in func
                .function_parameters()
                .expect("parameters")
                .iter()
                .enumerate()
            {
                let Some(param) = param else {
                    continue;
                };
                if param_async_state(param) != Some(AsyncState::Sync) {
                    continue;
                }
                let arg_idx = match (raw.is_colon_define(), colon_call) {
                    (true, false) => param_idx + 1,
                    (false, true) => {
                        if param_idx == 0 {
                            continue;
                        }
                        param_idx - 1
                    }
                    _ => param_idx,
                };
                let Some(arg) = args.get(arg_idx) else {
                    continue;
                };
                if !reported.insert(arg.get_range()) {
                    continue;
                }
                if !self
                    .callable_candidates_cached(arg)?
                    .iter()
                    .any(is_async_function)
                {
                    continue;
                }
                if self.await_in_async_context(call, async_closures)? {
                    continue;
                }
                context.add_diagnostic(
                    DiagnosticCode::AwaitInSync,
                    arg.get_range(),
                    t!("Async function can only be called in async function."),
                );
            }
        }
        Ok(())
    }
    fn await_in_async_context(
        &mut self,
        call: &LuaCallExpr,
        async_closures: &HashSet<LuaSyntaxId>,
    ) -> QueryResult<bool, E> {
        for closure in call.syntax().ancestors().filter_map(LuaClosureExpr::cast) {
            self.checkpoint()?;
            if self.await_closure_is_async(&closure, async_closures)? {
                return Ok(true);
            }
            if is_pcall_callback(&closure) {
                continue;
            }
            return Ok(false);
        }
        Ok(false)
    }
    fn await_closure_is_async(
        &mut self,
        closure: &LuaClosureExpr,
        async_closures: &HashSet<LuaSyntaxId>,
    ) -> QueryResult<bool, E> {
        if async_closures.contains(&closure.get_syntax_id()) {
            return Ok(true);
        }
        if self
            .model
            .file_facts()
            .and_then(|facts| facts.signature_by_closure(closure.get_syntax_id()))
            .and_then(|signature| signature.docs.as_ref())
            .is_some_and(|docs| docs.is_async)
        {
            return Ok(true);
        }
        let Some((call, param_idx)) = closure_call_arg(closure) else {
            return Ok(false);
        };
        let candidates = self.await_callee_candidates(&call)?;
        let colon_call = call.is_colon_call();
        for func in candidates {
            let LuaType::DocFunction(raw) = func.structural_type() else {
                unreachable!("callable candidate")
            };
            let idx = match (raw.is_colon_define(), colon_call) {
                (true, false) => param_idx + 1,
                (false, true) => {
                    if param_idx == 0 {
                        continue;
                    }
                    param_idx - 1
                }
                _ => param_idx,
            };
            if func
                .function_parameters()
                .expect("parameters")
                .get(idx)
                .and_then(|ty| ty.as_ref())
                .and_then(param_async_state)
                == Some(AsyncState::Async)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
fn is_async_function(value: &OwnedValue) -> bool {
    matches!(value.structural_type(),LuaType::DocFunction(func) if func.get_async_state()==AsyncState::Async)
}
fn param_async_state(value: &OwnedValue) -> Option<AsyncState> {
    match value.structural_type() {
        LuaType::DocFunction(func) => Some(func.get_async_state()),
        LuaType::Union(_) => {
            let mut state = None;
            for member in value.union_components().expect("union") {
                if let LuaType::DocFunction(func) = member.structural_type() {
                    state = Some(func.get_async_state());
                }
            }
            state
        }
        _ => None,
    }
}
fn collect_async_closures(root: &emmylua_parser::LuaSyntaxNode) -> HashSet<LuaSyntaxId> {
    let mut set = HashSet::new();
    for comment in root.descendants().filter_map(LuaComment::cast) {
        let is_async = comment
            .get_doc_tags()
            .any(|tag| matches!(tag, LuaDocTag::Async(_)));
        if !is_async {
            continue;
        }
        let Some(owner) = comment.get_owner() else {
            continue;
        };
        let closure = match owner {
            LuaAst::LuaFuncStat(stat) => stat.get_closure(),
            LuaAst::LuaLocalFuncStat(stat) => stat.get_closure(),
            owner => owner.descendants::<LuaClosureExpr>().next(),
        };
        if let Some(closure) = closure {
            set.insert(closure.get_syntax_id());
        }
    }
    set
}

fn doc_signature_for_decl(
    semantic_model: &emmylua_code_analysis::SemanticModel<'_>,
    decl: &SemanticId,
) -> Option<LuaFunctionType> {
    let SemanticId::Decl(decl_key) = decl else {
        return None;
    };
    let facts = semantic_model.file_facts_of(decl_key.file_id)?;
    let decl = facts.decl_by_id(decl)?;
    let closure_syntax = decl.value_expr_syntax?;
    let signature = facts.signature_by_closure(closure_syntax)?;
    let docs = signature.docs.as_ref()?;
    let mut params = Vec::new();
    for name in signature.param_names.iter() {
        let mut ty = docs
            .param_types
            .iter()
            .find(|(param_name, _)| param_name == name)
            .map(|(_, syntax)| semantic_model.doc_type_lua_rich_in(decl_key.file_id, *syntax))
            .unwrap_or(LuaType::Any);
        if docs.nullable_params.iter().any(|n| n == name) && !ty.is_nullable() {
            ty = LuaType::Union(std::sync::Arc::new(LuaUnionType::from_vec(vec![
                ty,
                LuaType::Nil,
            ])));
        }
        params.push((name.to_string(), Some(ty)));
    }
    let async_state = if docs.is_async {
        AsyncState::Async
    } else {
        AsyncState::None
    };
    Some(LuaFunctionType::new(
        async_state,
        signature.is_method,
        false,
        params,
        LuaType::Unknown,
        None,
    ))
}

fn is_pcall_callback(closure: &LuaClosureExpr) -> bool {
    let Some((call_expr, arg_idx)) = closure_call_arg(closure) else {
        return false;
    };
    if arg_idx != 0 {
        return false;
    }
    let Some(LuaExpr::NameExpr(name_expr)) = call_expr.get_prefix_expr() else {
        return false;
    };
    matches!(
        name_expr.get_name_text().as_deref(),
        Some("pcall") | Some("xpcall")
    )
}

/// Position of a closure as a call argument (directly inside LuaCallArgList).
fn closure_call_arg(closure: &LuaClosureExpr) -> Option<(LuaCallExpr, usize)> {
    let arg_list = closure.get_parent::<LuaCallArgList>()?;
    let call_expr = arg_list.get_parent::<LuaCallExpr>()?;
    let position = closure.get_position();
    let index = arg_list
        .get_args()
        .position(|arg| arg.get_position() == position)?;
    Some((call_expr, index))
}
