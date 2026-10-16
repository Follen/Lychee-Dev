//! Owned argument-count and callback parameter-count checker.
mod generic;
use super::super::{
    OwnedQuery, QueryResult, call_access::first_param_is_self, call_unify::Bindings,
};
use super::CheckContext;
use crate::session::{BindingDecision, OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    AsyncState, DiagnosticCode, FileId, LuaFunctionType, LuaMemberKey, LuaType, LuaTypeDeclId,
    SemanticId, TypeDef, TypeDefKind, VariadicType,
};
use emmylua_parser::{
    LuaAstNode, LuaCallArgList, LuaCallExpr, LuaClosureExpr, LuaDocType, LuaExpr, LuaIndexExpr,
    LuaIndexKey, LuaSyntaxId, LuaTableExpr, LuaTableField, LuaTypeBinaryOperator,
};
use std::sync::Arc;
#[derive(Clone, Copy)]
struct CountRange {
    min: usize,
    max: Option<usize>,
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_param_count(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for call in root.descendants().filter_map(LuaCallExpr::cast) {
            self.count_check_call(context, &call)?;
            self.binding_finish_call_check(&call, "argument_count_check")?;
        }
        for closure in root.descendants().filter_map(LuaClosureExpr::cast) {
            self.count_check_closure(context, &closure)?;
        }
        Ok(())
    }
    fn count_check_call(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
    ) -> QueryResult<(), E> {
        if !self.binding_call_allowed(call, "argument_count_check")? {
            return Ok(());
        }
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(());
        };
        let managed = self.count_manages(callee.get_syntax_id())?;
        let callee_ty = self.type_of_expr(callee.get_syntax_id())?;
        let analysis = self.call_site_analysis(call)?;
        let mut candidates = analysis.candidates;
        if candidates.is_empty() {
            candidates = self.callable_functions_cached(&callee_ty)?;
        }
        if !managed
            && candidates.is_empty()
            && matches!(callee_ty.structural_type(), LuaType::Generic(_))
        {
            candidates = self.count_generic_fallback(&callee, &callee_ty)?;
        }
        if !managed
            && let LuaExpr::NameExpr(name) = &callee
            && let Some(id) = self.model.resolve_name(name.get_position())
            && !self.owner_initializer_managed(&id)?
            && let SemanticId::Decl(key) = &id
            && let Some(decl) = self
                .model
                .file_facts_of(key.file_id)
                .and_then(|f| f.decl_by_id(&id))
            && let Some(syntax) = decl.value_expr_syntax
            && let Some(docs) = self
                .model
                .file_facts_of(key.file_id)
                .and_then(|f| f.signature_by_closure(syntax))
                .and_then(|s| s.docs.as_ref())
        {
            for syntax in &docs.overloads {
                if let Some(function) = self.count_doc_function(decl.file_id, *syntax)? {
                    candidates.push(function);
                }
            }
        }
        if !managed
            && candidates.is_empty()
            && let Some(index) = LuaIndexExpr::cast(callee.syntax().clone())
            && let Some(resolved) = self.resolve_owned_member(&index)?
            && resolved.binding_token.is_none()
            && let Some(id) = resolved.member_id
            && !self.owner_initializer_managed(&id)?
        {
            if let Some(value) = self.type_of_member(&id)? {
                candidates = self.callable_functions_cached(&value)?;
            }
            if candidates.is_empty()
                && let Some(file) = resolved.file_id
                && let Some(syntax) = self
                    .model
                    .file_facts_of(file)
                    .and_then(|f| f.member_by_id(&id))
                    .and_then(|m| m.value_syntax)
                && let Some(closure) = self
                    .model
                    .syntax_tree_of(file)
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                    .and_then(LuaClosureExpr::cast)
                && let Ok(function) = self.signature_in(file, closure.get_syntax_id())?
            {
                candidates.push(function);
            }
        }
        if candidates.is_empty() {
            return Ok(());
        }
        let args = call
            .get_args_list()
            .map(|list| list.get_args().collect::<Vec<_>>())
            .unwrap_or_default();
        if args.iter().any(|arg| arg.syntax().text() == "...") {
            return Ok(());
        }
        let args_range = self.count_expanded_args(&args)?;
        let colon = analysis.colon_call;
        for candidate in &candidates {
            let range = self.count_param_range(candidate, colon)?;
            let LuaType::DocFunction(fun) = candidate.structural_type() else {
                unreachable!("candidate")
            };
            let extra =
                usize::from(colon && !fun.is_colon_define() && !first_param_is_self(candidate));
            let min = args_range.min + extra;
            let max = args_range.max.map(|m| m + extra);
            if max.is_none_or(|m| m >= range.min) && range.max.is_none_or(|m| min <= m) {
                return Ok(());
            }
        }
        let mut best: Option<(usize, bool, usize, usize, usize)> = None;
        for candidate in &candidates {
            let range = self.count_param_range(candidate, colon)?;
            let LuaType::DocFunction(fun) = candidate.structural_type() else {
                unreachable!("candidate")
            };
            let extra =
                usize::from(colon && !fun.is_colon_define() && !first_param_is_self(candidate));
            let min = args_range.min + extra;
            let max = args_range.max.map(|m| m + extra);
            let mismatch = if max.is_some_and(|m| m < range.min) {
                let max = max.unwrap_or(min);
                Some((range.min - max, true, range.min, max, extra))
            } else if let Some(max) = range.max
                && min > max
            {
                Some((min - max, false, max, min, extra))
            } else {
                None
            };
            if let Some(candidate) = mismatch
                && best.is_none_or(|b| {
                    candidate.0 < b.0 || (candidate.0 == b.0 && candidate.1 && !b.1)
                })
            {
                best = Some(candidate);
            }
        }
        let Some((_, missing, expected, found, extra)) = best else {
            return Ok(());
        };
        if missing {
            if let Some(list) = call.get_args_list() {
                context.add_diagnostic(
                    DiagnosticCode::MissingParameter,
                    list.get_range(),
                    t!(
                        "expected at least %{num} parameters but found %{found_num}",
                        num = expected,
                        found_num = found
                    ),
                );
            }
        } else {
            for arg in args.iter().skip(expected.saturating_sub(extra)) {
                self.checkpoint()?;
                context.add_diagnostic(
                    DiagnosticCode::RedundantParameter,
                    arg.get_range(),
                    t!(
                        "expected %{num} parameters but found %{found_num}",
                        num = expected,
                        found_num = found
                    ),
                );
            }
        }
        Ok(())
    }
    fn count_manages(&mut self, syntax: LuaSyntaxId) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let managed = !matches!(
            self.session.context().resolve(self.site(syntax)),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        Ok(managed)
    }
    fn count_manages_call(&mut self, syntax: LuaSyntaxId) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let managed = !matches!(
            self.session.context().call(self.site(syntax)),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        if managed {
            Ok(true)
        } else {
            self.count_manages(syntax)
        }
    }
    fn count_check_closure(
        &mut self,
        context: &mut CheckContext<'_>,
        closure: &LuaClosureExpr,
    ) -> QueryResult<(), E> {
        if self.count_manages(closure.get_syntax_id())? {
            let _ = self.type_of_expr(closure.get_syntax_id())?;
            return Ok(());
        }
        let expected = self.count_closure_expected(closure)?;
        if expected.is_empty() {
            return Ok(());
        }
        let mut max = 0;
        for function in &expected {
            self.checkpoint()?;
            let LuaType::DocFunction(fun) = function.structural_type() else {
                unreachable!("function")
            };
            if fun.is_variadic()
                || fun
                    .get_params()
                    .last()
                    .is_some_and(|(n, t)| n == "..." || matches!(t, Some(LuaType::Variadic(_))))
            {
                return Ok(());
            }
            max = max.max(fun.get_params().len());
        }
        let Some(list) = closure.get_params_list() else {
            return Ok(());
        };
        let params = list.get_params().collect::<Vec<_>>();
        if params.len() <= max {
            return Ok(());
        }
        let found = params.len();
        for param in params.iter().skip(max) {
            self.checkpoint()?;
            context.add_diagnostic(
                DiagnosticCode::RedundantParameter,
                param.get_range(),
                t!(
                    "expected %{num} parameters but found %{found_num}",
                    num = max,
                    found_num = found
                ),
            );
        }
        Ok(())
    }
    fn count_closure_expected(
        &mut self,
        closure: &LuaClosureExpr,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        if let Some((call, index)) = count_closure_arg(closure) {
            if !self.binding_call_allowed(&call, "callback_parameter_check")? {
                return Ok(Vec::new());
            }
            let Some(callee) = call.get_prefix_expr() else {
                return Ok(Vec::new());
            };
            let candidates = self.callable_candidates_cached(&callee)?;
            let colon = call.is_colon_call();
            let mut out = Vec::new();
            for candidate in candidates {
                let LuaType::DocFunction(fun) = candidate.structural_type() else {
                    unreachable!("function")
                };
                let index = match (fun.is_colon_define(), colon) {
                    (true, false) => index + 1,
                    (false, true) => {
                        if index == 0 {
                            continue;
                        }
                        index - 1
                    }
                    _ => index,
                };
                if let Some(Some(value)) = candidate
                    .function_parameters()
                    .expect("parameters")
                    .get(index)
                {
                    out.extend(self.callable_functions_cached(value)?);
                }
            }
            return Ok(out);
        }
        if let Some((table, key)) = count_closure_field(closure) {
            if self.count_manages(table.get_syntax_id())? {
                let _ = self.type_of_expr(table.get_syntax_id())?;
                return Ok(Vec::new());
            }
            let syntax = self
                .model
                .file_facts()
                .and_then(|f| f.decl_by_value_syntax(table.get_syntax_id()))
                .and_then(|d| d.doc_type_syntax);
            let Some(syntax) = syntax else {
                return Ok(Vec::new());
            };
            let ty = self.model.doc_type_lua(syntax);
            if matches!(ty, LuaType::Unknown) {
                return Ok(Vec::new());
            }
            if let Some(value) = self.member_type(&OwnedValue::plain(ty), &key)? {
                return self.callable_functions_cached(&value);
            }
        }
        Ok(Vec::new())
    }
    fn count_doc_function(
        &mut self,
        file: FileId,
        syntax: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let doc = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaDocType::cast);
        let Some(LuaDocType::Func(fun)) = doc else {
            return Ok(None);
        };
        let mut params = Vec::new();
        for param in fun.get_params() {
            self.checkpoint()?;
            params.push((
                String::new(),
                param
                    .get_type()
                    .map(|t| self.model.doc_type_lua_rich_in(file, t.get_syntax_id())),
            ));
        }
        Ok(Some(OwnedValue::plain(LuaType::DocFunction(Arc::new(
            LuaFunctionType::new(
                AsyncState::None,
                false,
                false,
                params,
                LuaType::Unknown,
                None,
            ),
        )))))
    }
    fn count_expanded_args(&mut self, args: &[LuaExpr]) -> QueryResult<CountRange, E> {
        let base = args.len().saturating_sub(1);
        if let Some(last) = args.last()
            && let Some(variadic) = self.count_last_variadic(last)?
        {
            return Ok(CountRange {
                min: base + variadic.get_min_len().unwrap_or(0),
                max: variadic.get_max_len().map(|n| base + n),
            });
        }
        Ok(CountRange {
            min: args.len(),
            max: Some(args.len()),
        })
    }
    fn count_last_variadic(&mut self, expr: &LuaExpr) -> QueryResult<Option<VariadicType>, E> {
        let value = self.type_of_expr(expr.get_syntax_id())?;
        if let LuaType::Variadic(pack) = value.structural_type() {
            return Ok(Some(pack.as_ref().clone()));
        }
        if !matches!(value.structural_type(), LuaType::Unknown) {
            return Ok(None);
        }
        let LuaExpr::CallExpr(call) = expr else {
            return Ok(None);
        };
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(None);
        };
        if self.count_manages_call(call.get_syntax_id())?
            || self.count_manages(callee.get_syntax_id())?
        {
            return Ok(Some(VariadicType::Base(LuaType::Unknown)));
        }
        for function in self.callable_candidates_cached(&callee)? {
            if let LuaType::Variadic(pack) = function
                .function_return()
                .expect("return")
                .structural_type()
            {
                return Ok(Some(pack.as_ref().clone()));
            }
        }
        if self.count_has_variadic_doc(&callee)? {
            return Ok(Some(VariadicType::Base(LuaType::Any)));
        }
        Ok(None)
    }
    fn count_has_variadic_doc(&mut self, callee: &LuaExpr) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if self.binding_manages(callee.get_syntax_id())? {
            return Ok(false);
        }
        let source = match callee {
            LuaExpr::NameExpr(name) => {
                let Some(text) = name.get_name_text() else {
                    return Ok(false);
                };
                let Some(id) = self
                    .model
                    .resolve_name(name.get_position())
                    .or_else(|| self.model.global_decl(&text))
                else {
                    return Ok(false);
                };
                if self.owner_initializer_managed(&id)? {
                    return Ok(false);
                }
                let SemanticId::Decl(key) = &id else {
                    return Ok(false);
                };
                self.model
                    .file_facts_of(key.file_id)
                    .and_then(|f| f.decl_by_id(&id))
                    .and_then(|d| d.value_expr_syntax)
                    .map(|s| (key.file_id, s))
            }
            LuaExpr::IndexExpr(index) => {
                let Some(resolved) = self.resolve_owned_member(index)? else {
                    return Ok(false);
                };
                if resolved.binding_token.is_some() {
                    return Ok(false);
                }
                if let Some(id) = &resolved.member_id
                    && self.owner_initializer_managed(id)?
                {
                    return Ok(false);
                }
                match (resolved.file_id, resolved.member_id) {
                    (Some(file), Some(id)) => self
                        .model
                        .file_facts_of(file)
                        .and_then(|f| f.member_by_id(&id))
                        .and_then(|m| m.value_syntax)
                        .map(|s| (file, s)),
                    _ => None,
                }
            }
            _ => None,
        };
        let Some((file, syntax)) = source else {
            return Ok(false);
        };
        let Some(docs) = self
            .model
            .file_facts_of(file)
            .and_then(|f| f.signature_by_closure(syntax))
            .and_then(|s| s.docs.as_ref())
        else {
            return Ok(false);
        };
        let Some(tree) = self.model.syntax_tree_of(file) else {
            return Ok(false);
        };
        for syntax in &docs.returns {
            self.checkpoint()?;
            if matches!(
                syntax
                    .to_node_from_root(&tree.get_red_root())
                    .and_then(LuaDocType::cast),
                Some(LuaDocType::Variadic(_))
            ) {
                return Ok(true);
            }
        }
        Ok(false)
    }
    fn count_param_range(
        &mut self,
        function: &OwnedValue,
        colon: bool,
    ) -> QueryResult<CountRange, E> {
        self.checkpoint()?;
        let LuaType::DocFunction(fun) = function.structural_type() else {
            unreachable!("function")
        };
        let has_self = first_param_is_self(function);
        let start = usize::from(colon && has_self);
        let params = &fun.get_params()[start..];
        let carried = function.function_parameters().expect("parameters");
        let carried = &carried[start..];
        let variadic_index = if fun.is_variadic() {
            Some(params.len().saturating_sub(1))
        } else {
            params
                .iter()
                .rposition(|(n, t)| n == "..." || matches!(t, Some(LuaType::Variadic(_))))
        };
        let mut min = 0;
        for (index, ((name, raw), value)) in params.iter().zip(carried).enumerate() {
            self.checkpoint()?;
            if Some(index) != variadic_index
                && name != "..."
                && !matches!(raw, None | Some(LuaType::Any | LuaType::Unknown))
                && let Some(value) = value
                && !self.count_optional(value, &mut Vec::new())?
            {
                min += 1;
            }
        }
        let variadic_slot = variadic_index.and_then(|i| params.get(i));
        let unresolved=variadic_slot.is_some_and(|(_,t)|matches!(t,Some(LuaType::Variadic(v)) if matches!(v.as_ref(),VariadicType::Base(LuaType::Ref(id)|LuaType::Def(id)) if self.model.type_def_of(id).is_none())));
        let length = variadic_slot.and_then(|(_, t)| match t.as_ref()? {
            LuaType::Variadic(v) => match v.as_ref() {
                VariadicType::Base(LuaType::Tuple(t)) => Some(t.get_types().len()),
                VariadicType::Multi(types) => Some(types.len()),
                _ => None,
            },
            _ => None,
        });
        let mut max = if let Some(len) = length {
            Some(params.len() - 1 + len)
        } else if !unresolved && (fun.is_variadic() || variadic_index.is_some()) {
            None
        } else {
            Some(params.len())
        };
        if !colon && !has_self && (fun.is_colon_define() || has_self) {
            min += 1;
            max = max.map(|m| m + 1);
        }
        Ok(CountRange { min, max })
    }
    fn count_optional(
        &mut self,
        value: &OwnedValue,
        visited: &mut Vec<LuaTypeDeclId>,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if value.structural_type().is_optional() {
            return Ok(true);
        }
        let (LuaType::Ref(id) | LuaType::Def(id)) = value.structural_type() else {
            return Ok(false);
        };
        if visited.contains(id) {
            return Ok(false);
        }
        visited.push(id.clone());
        let Some(def) = self.model.type_def_of(id) else {
            return Ok(false);
        };
        if def.kind != TypeDefKind::Alias {
            return Ok(false);
        }
        match self.model.alias_target(&def) {
            Some(target) => self.count_optional(&OwnedValue::plain(target), visited),
            None => Ok(false),
        }
    }
}
fn count_closure_arg(closure: &LuaClosureExpr) -> Option<(LuaCallExpr, usize)> {
    let list = closure.get_parent::<LuaCallArgList>()?;
    let call = list.get_parent::<LuaCallExpr>()?;
    let index = list
        .get_args()
        .position(|a| a.get_position() == closure.get_position())?;
    Some((call, index))
}
fn count_closure_field(closure: &LuaClosureExpr) -> Option<(LuaTableExpr, LuaMemberKey)> {
    let field = closure.get_parent::<LuaTableField>()?;
    let table = field.get_parent::<LuaTableExpr>()?;
    let key = match field.get_field_key()? {
        LuaIndexKey::Name(n) => LuaMemberKey::Name(n.get_name_text().into()),
        LuaIndexKey::String(s) => LuaMemberKey::Name(s.get_value().into()),
        _ => return None,
    };
    Some((table, key))
}
