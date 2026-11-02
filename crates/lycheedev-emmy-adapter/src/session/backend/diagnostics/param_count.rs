//! Owned argument-count and callback parameter-count checker.
mod generic;
use super::super::{
    OwnedQuery, QueryResult, call_access::first_param_is_self, call_unify::Bindings,
};
use super::CheckContext;
use crate::session::{
    BindingDecision, EnvironmentCallable, OwnedValue, SelectedProjectionBackend, SelectedValue,
};
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
            if let Some(ready) = self.count_check_call(context, &call)?
                && let BindingDecision::Bound(permit) =
                    self.session.context().call(self.site(call.get_syntax_id()))
            {
                self.record_check(
                    call.get_syntax_id(),
                    "argument_count_check",
                    if ready {
                        "native_checked"
                    } else {
                        "unavailable"
                    },
                    permit.proof,
                    (!ready).then(|| "native_argument_arity_unavailable".into()),
                )?;
            }
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
    ) -> QueryResult<Option<bool>, E> {
        if !self.binding_call_allowed(call, "argument_count_check")? {
            return Ok(None);
        }
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(Some(false));
        };
        let managed = self.count_manages(callee.get_syntax_id())?;
        let environment = match self
            .session
            .context()
            .resolve(self.site(callee.get_syntax_id()))
        {
            BindingDecision::Bound(bound) => match bound.value {
                SelectedValue::EnvironmentCallable(callable) => Some(callable),
                _ => None,
            },
            _ => None,
        };
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
            return Ok(Some(false));
        }
        let args = call
            .get_args_list()
            .map(|list| list.get_args().collect::<Vec<_>>())
            .unwrap_or_default();
        let args_range = self.count_expanded_args(&args)?;
        // An unresolved trailing call can expand to an unknown number of
        // arguments. It must not turn an overlapping range into a checked call.
        let known_arity = args_range.max == Some(args_range.min);
        let colon = analysis.colon_call;
        for candidate in &candidates {
            let range = self.count_param_range(candidate, colon, environment.as_deref())?;
            let LuaType::DocFunction(fun) = candidate.structural_type() else {
                unreachable!("candidate")
            };
            let extra =
                usize::from(colon && !fun.is_colon_define() && !first_param_is_self(candidate));
            let min = args_range.min + extra;
            let max = args_range.max.map(|m| m + extra);
            if max.is_none_or(|m| m >= range.min) && range.max.is_none_or(|m| min <= m) {
                return Ok(Some(known_arity));
            }
        }
        let mut best: Option<(usize, bool, usize, usize, usize)> = None;
        for candidate in &candidates {
            let range = self.count_param_range(candidate, colon, environment.as_deref())?;
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
            return Ok(Some(false));
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
            // A single final call can contribute the excess values. Point at
            // that original expression rather than losing its diagnostic.
            let first = expected
                .saturating_sub(extra)
                .min(args.len().saturating_sub(1));
            for arg in args.iter().skip(first) {
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
        Ok(Some(known_arity))
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
            return self.count_callback_receipt(closure, false);
        }
        let mut max = Some(0usize);
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
                max = None;
                break;
            }
            max = max.map(|max| max.max(fun.get_params().len()));
        }
        let Some(list) = closure.get_params_list() else {
            return self.count_callback_receipt(closure, false);
        };
        let params = list.get_params().collect::<Vec<_>>();
        let Some(max) = max.filter(|max| params.len() > *max) else {
            return self.count_callback_receipt(closure, true);
        };
        let found = params.len();
        let owner = count_closure_arg(closure).map(|(call, _)| {
            let range = call.get_range();
            serde_json::json!({"bindingCheckOwner": {
                "start": u32::from(range.start()), "end": u32::from(range.end())
            }})
        });
        for param in params.iter().skip(max) {
            self.checkpoint()?;
            context.add_diagnostic_with_data(
                DiagnosticCode::RedundantParameter,
                param.get_range(),
                t!(
                    "expected %{num} parameters but found %{found_num}",
                    num = max,
                    found_num = found
                ),
                owner.clone(),
            );
        }
        self.count_callback_receipt(closure, true)
    }
    fn count_callback_receipt(
        &mut self,
        closure: &LuaClosureExpr,
        ready: bool,
    ) -> QueryResult<(), E> {
        let Some((call, _)) = count_closure_arg(closure) else {
            return Ok(());
        };
        let BindingDecision::Bound(permit) =
            self.session.context().call(self.site(call.get_syntax_id()))
        else {
            return Ok(());
        };
        self.record_check(
            call.get_syntax_id(),
            "callback_count_check",
            if ready {
                "native_checked"
            } else {
                "unavailable"
            },
            permit.proof,
            (!ready).then(|| "native_callback_arity_unavailable".into()),
        )
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
                let index = if first_param_is_self(&candidate) {
                    index + usize::from(colon)
                } else {
                    match (fun.is_colon_define(), colon) {
                        (true, false) => index + 1,
                        (false, true) => {
                            if index == 0 {
                                continue;
                            }
                            index - 1
                        }
                        _ => index,
                    }
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
            && super::super::return_pack::return_expands(last, true)
        {
            let range = self.count_last_range(last)?;
            return Ok(CountRange {
                min: base + range.min,
                max: range.max.map(|n| base + n),
            });
        }
        Ok(CountRange {
            min: args.len(),
            max: Some(args.len()),
        })
    }
    fn count_last_range(&mut self, expr: &LuaExpr) -> QueryResult<CountRange, E> {
        let open = CountRange { min: 0, max: None };
        if let LuaExpr::CallExpr(call) = expr
            && let Some(range) = self.binding_call_return_arity(call)?
        {
            return Ok(CountRange {
                min: range.min,
                max: range.max,
            });
        }
        let value = self.type_of_expr(expr.get_syntax_id())?;
        if !matches!(value.structural_type(), LuaType::Unknown | LuaType::Any) {
            return Ok(
                super::super::return_pack::return_arity(value.structural_type())
                    .map(|range| CountRange {
                        min: range.min,
                        max: range.max,
                    })
                    .unwrap_or(open),
            );
        }
        let LuaExpr::CallExpr(call) = expr else {
            return Ok(open);
        };
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(open);
        };
        if self.count_manages_call(call.get_syntax_id())?
            || self.count_manages(callee.get_syntax_id())?
        {
            return Ok(open);
        }
        for function in self.callable_candidates_cached(&callee)? {
            if let LuaType::Variadic(pack) = function
                .function_return()
                .expect("return")
                .structural_type()
            {
                return Ok(super::super::return_pack::return_arity(&LuaType::Variadic(
                    pack.clone(),
                ))
                .map(|range| CountRange {
                    min: range.min,
                    max: range.max,
                })
                .unwrap_or(open));
            }
        }
        Ok(open)
    }
    fn count_param_range(
        &mut self,
        function: &OwnedValue,
        colon: bool,
        environment: Option<&EnvironmentCallable>,
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
        if let Some(environment) = environment {
            // The authenticated API descriptor knows required slots even when
            // a parameter's value type could not be projected.
            min = environment
                .declaration
                .parameters
                .iter()
                .filter(|slot| !slot.nilable && slot.name != "...")
                .count()
                + usize::from(has_self && !colon);
        }
        let variadic_slot = variadic_index.and_then(|i| params.get(i));
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
        } else if fun.is_variadic() || variadic_index.is_some() {
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
