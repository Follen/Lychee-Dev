//! Call-site/diagnostic candidate queries with the original separate caches.
//! These candidates intentionally differ from VM alias/callable expansion.
mod constraints;
#[cfg(test)]
mod selected_initializer_tests;
use super::{OwnedQuery, QueryResult, call_unify::Bindings};
use crate::session::{OwnedValue, SelectedProjectionBackend, SelectedProjector};
use emmylua_code_analysis::{
    AsyncState, DeclKind, GenericTplId, LuaFunctionType, LuaMemberKey, LuaType, SemanticId,
    TypeDef, TypeDefKind,
};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaExpr, LuaIndexExpr, LuaIndexKey, LuaSyntaxId};

#[derive(Clone)]
pub(super) struct OwnedCallSiteAnalysis {
    pub(super) candidates: Vec<OwnedValue>,
    pub(super) arg_types: Vec<OwnedValue>,
    pub(super) colon_call: bool,
    pub(super) receiver_ty: OwnedValue,
    pub(super) explicit_generics: Vec<LuaSyntaxId>,
}
impl OwnedCallSiteAnalysis {
    fn empty(colon_call: bool) -> Self {
        Self {
            candidates: Vec::new(),
            arg_types: Vec::new(),
            colon_call,
            receiver_ty: OwnedValue::plain(LuaType::Unknown),
            explicit_generics: Vec::new(),
        }
    }
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn callable_functions_cached(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        let cached = self.callable_functions.borrow().get(value).cloned();
        if let Some(cached) = cached {
            self.checkpoint()?;
            return Ok(cached);
        }
        let functions = self.access_callable_functions(value)?;
        self.checkpoint()?;
        self.callable_functions
            .borrow_mut()
            .insert(value.clone(), functions.clone());
        Ok(functions)
    }
    fn access_callable_functions(&mut self, value: &OwnedValue) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::DocFunction(_) => Ok(vec![value.clone()]),
            LuaType::Signature(id) => Ok(self.signature_by_id(id)?.into_iter().collect()),
            LuaType::Ref(id) | LuaType::Def(id) => match self.model.type_def_of(id) {
                Some(def) => self.access_type_callables(&def),
                None => Ok(Vec::new()),
            },
            LuaType::TableConst(_) => {
                Ok(self.call_metatable_candidate(value)?.into_iter().collect())
            }
            LuaType::Union(_) => {
                let mut out = Vec::new();
                for component in value.union_components().expect("union") {
                    out.extend(self.access_callable_functions(&component)?);
                }
                Ok(out)
            }
            LuaType::Generic(generic) => {
                let Some(def) = self.model.type_def_of(&generic.get_base_type_id()) else {
                    return Ok(Vec::new());
                };
                if def.kind == TypeDefKind::Alias
                    && let Some(target) = self.model.alias_target(&def)
                {
                    let bindings = generic
                        .get_params()
                        .iter()
                        .enumerate()
                        .map(|(i, t)| (GenericTplId::Type(i as u32), self.owned_child(value, i, t)))
                        .collect();
                    let instantiated =
                        self.substitute_owned(OwnedValue::plain(target), &bindings)?;
                    return self.access_callable_functions(&instantiated);
                }
                self.access_type_callables(&def)
            }
            _ => Ok(Vec::new()),
        }
    }
    fn access_type_callables(&mut self, def: &TypeDef) -> QueryResult<Vec<OwnedValue>, E> {
        let mut out = Vec::new();
        for syntax in &def.call_overloads {
            self.checkpoint()?;
            // Keep doc_type_lua's current-file lookup (not def.file_id/rich).
            let ty = self.model.doc_type_lua(*syntax);
            if matches!(ty, LuaType::DocFunction(_)) {
                out.push(OwnedValue::plain(ty));
            }
        }
        if let Some(op) = self
            .model
            .file_facts_of(def.file_id)
            .and_then(|f| f.operator_of(&def.id, "call"))
        {
            let mut params = Vec::new();
            for syntax in &op.params {
                self.checkpoint()?;
                params.push((
                    String::new(),
                    Some(self.model.doc_type_lua_rich_in(def.file_id, *syntax)),
                ));
            }
            let ret = self.model.doc_type_lua_rich_in(def.file_id, op.returns);
            out.push(OwnedValue::plain(LuaType::DocFunction(
                std::sync::Arc::new(LuaFunctionType::new(
                    AsyncState::None,
                    false,
                    false,
                    params,
                    ret,
                    None,
                )),
            )));
        }
        Ok(out)
    }
    pub(super) fn callable_candidates_for_name_expr(
        &mut self,
        callee: &LuaExpr,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        let LuaExpr::NameExpr(name) = callee else {
            return Ok(Vec::new());
        };
        let Some(owner) = self.model.resolve_name(name.get_position()) else {
            return Ok(Vec::new());
        };
        if self.owner_initializer_managed(&owner)? {
            let value = self.type_of_expr(callee.get_syntax_id())?;
            return self.callable_functions_cached(&value);
        }
        let SemanticId::Decl(key) = &owner else {
            return Ok(Vec::new());
        };
        let Some(decl) = self
            .model
            .file_facts_of(key.file_id)
            .and_then(|f| f.decl_by_id(&owner))
        else {
            return Ok(Vec::new());
        };
        if !matches!(decl.kind, DeclKind::Global) {
            return Ok(Vec::new());
        }
        let declarations = self
            .model
            .global_decls_named_for_file(key.file_id, &decl.name);
        if declarations.len() <= 1 {
            return Ok(Vec::new());
        }
        let mut per_decl = Vec::new();
        for decl in &declarations {
            per_decl.push(
                self.callable_candidates_for_owner_single(decl)?
                    .unwrap_or_default(),
            );
        }
        let Some(first) = per_decl.first() else {
            return Ok(Vec::new());
        };
        if per_decl.iter().all(|c| c == first) {
            return Ok(Vec::new());
        }
        Ok(per_decl.into_iter().flatten().collect())
    }
    pub fn callable_candidates_for_expr(
        &mut self,
        callee: &LuaExpr,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        let projector = SelectedProjector::new(self.analysis, self.session);
        if let Some(value) = projector.project_selected(self.site(callee.get_syntax_id()), self)? {
            return self.callable_functions_cached(&value);
        }
        let mut out = self.callable_candidates_uncached(callee)?;
        if let LuaExpr::NameExpr(name) = callee
            && let Some(owner) = self.model.resolve_name(name.get_position())
            && !self.owner_initializer_managed(&owner)?
            && let Some(candidates) = self.callable_candidates_for_owner(&owner)?
        {
            for candidate in candidates {
                self.checkpoint()?;
                if !out.contains(&candidate) {
                    out.push(candidate);
                }
            }
        }
        Ok(out)
    }
    pub(super) fn callable_candidates_cached(
        &mut self,
        callee: &LuaExpr,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        let key = (self.model.file_id(), callee.get_syntax_id());
        let cached = self.callable_candidates.borrow().get(&key).cloned();
        if let Some(cached) = cached {
            self.checkpoint()?;
            return Ok(cached);
        }
        let value = self.callable_candidates_uncached(callee)?;
        self.checkpoint()?;
        self.callable_candidates
            .borrow_mut()
            .insert(key, value.clone());
        Ok(value)
    }
    fn callable_candidates_uncached(
        &mut self,
        callee: &LuaExpr,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        let projector = SelectedProjector::new(self.analysis, self.session);
        if let Some(value) = projector.project_selected(self.site(callee.get_syntax_id()), self)? {
            return self.callable_functions_cached(&value);
        }
        if matches!(callee, LuaExpr::NameExpr(_)) {
            let candidates = self.callable_candidates_for_name_expr(callee)?;
            if !candidates.is_empty() {
                return Ok(candidates);
            }
        }
        if let LuaExpr::IndexExpr(index) = callee {
            let candidates = self.access_index_callables(index)?;
            if !candidates.is_empty() {
                return Ok(candidates);
            }
            if index.get_index_key().is_some_and(|k| {
                matches!(
                    k,
                    LuaIndexKey::Name(_) | LuaIndexKey::String(_) | LuaIndexKey::Integer(_)
                )
            }) {
                return Ok(Vec::new());
            }
        }
        let value = self.type_of_expr(callee.get_syntax_id())?;
        let mut candidates = self.callable_functions_cached(&value)?;
        if candidates.is_empty()
            && let LuaExpr::IndexExpr(index) = callee
        {
            candidates = self.access_index_callables(index)?;
        }
        if candidates.is_empty()
            && let LuaExpr::NameExpr(name) = callee
            && let Some(decl) = self.model.resolve_name(name.get_position())
            && let Some(function) = self.access_decl_signature(&decl)?
        {
            candidates.push(function);
        }
        Ok(candidates)
    }
    pub(super) fn access_decl_signature(
        &mut self,
        decl: &SemanticId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        if self.owner_initializer_managed(decl)? {
            return Ok(None);
        }
        let SemanticId::Decl(key) = decl else {
            return Ok(None);
        };
        let Some(syntax) = self
            .model
            .file_facts_of(key.file_id)
            .and_then(|f| f.decl_by_id(decl))
            .and_then(|d| d.value_expr_syntax)
        else {
            return Ok(None);
        };
        Ok(self.signature_in(key.file_id, syntax)?.ok())
    }
    fn access_index_callables(&mut self, index: &LuaIndexExpr) -> QueryResult<Vec<OwnedValue>, E> {
        let Some(resolved) = self.resolve_owned_member(index)? else {
            // A carried union/intersection can have a known callable value
            // without one declaration id. Preserve that value-level answer;
            // failure to name a source write is not failed type inference.
            if let Some(prefix) = index.get_prefix_expr()
                && let Some(key) = index.get_index_key().and_then(access_literal_member_key)
            {
                let value = self.type_of_expr(prefix.get_syntax_id())?;
                if !value.is_context_free() {
                    return self.callable_candidates_from_prefix(&value, &key, None);
                }
            }
            return Ok(Vec::new());
        };
        if resolved.binding_token.is_some() {
            // The member id is reference provenance for the destination write.
            // Even an empty actual callable set is authoritative here.
            return match resolved.member_type.as_ref() {
                Some(value) => self.callable_functions_cached(value),
                None => Ok(Vec::new()),
            };
        }
        if let Some(prefix) = index.get_prefix_expr() {
            let value = self.type_of_expr(prefix.get_syntax_id())?;
            let key = LuaMemberKey::Name(resolved.name.clone().into());
            let candidates =
                self.callable_candidates_from_prefix(&value, &key, resolved.member_id.as_ref())?;
            if value.object_info().is_some() || !candidates.is_empty() {
                return Ok(candidates);
            }
        }
        let mut candidates = Vec::new();
        if let Some(id) = &resolved.member_id
            && let Some(value) = self.type_of_member(id)?
        {
            candidates = self.callable_functions_cached(&value)?;
        }
        if candidates.is_empty()
            && let Some(file) = resolved.file_id
            && let Some(id) = &resolved.member_id
            && !self.owner_initializer_managed(id)?
            && let Some(syntax) = self
                .model
                .file_facts_of(file)
                .and_then(|f| f.member_by_id(id))
                .and_then(|m| m.value_syntax)
            && let Ok(function) = self.signature_in(file, syntax)?
        {
            candidates.push(function);
        }
        Ok(candidates)
    }
    pub(super) fn call_site_analysis(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<OwnedCallSiteAnalysis, E> {
        self.checkpoint()?;
        let key = (self.model.file_id(), call.get_syntax_id());
        let cached = self.call_sites.borrow().get(&key).cloned();
        if let Some(cached) = cached {
            self.checkpoint()?;
            return Ok(cached);
        }
        let analysis = self.call_site_analysis_uncached(call)?;
        self.checkpoint()?;
        self.call_sites.borrow_mut().insert(key, analysis.clone());
        Ok(analysis)
    }
    pub(super) fn call_site_analysis_uncached(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<OwnedCallSiteAnalysis, E> {
        if !self.binding_call_allowed(call, "call_check")? {
            return Ok(OwnedCallSiteAnalysis::empty(call.is_colon_call()));
        }
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(OwnedCallSiteAnalysis::empty(call.is_colon_call()));
        };
        let candidates = self.callable_candidates_cached(&callee)?;
        if candidates.is_empty() {
            return Ok(OwnedCallSiteAnalysis::empty(call.is_colon_call()));
        }
        let mut args = Vec::new();
        if let Some(list) = call.get_args_list() {
            for arg in list.get_args() {
                args.push(self.type_of_expr(arg.get_syntax_id())?);
            }
        }
        let colon = call.is_colon_call();
        let receiver = if colon {
            if let Some(prefix) =
                LuaIndexExpr::cast(callee.syntax().clone()).and_then(|i| i.get_prefix_expr())
            {
                self.type_of_expr(prefix.get_syntax_id())?
            } else {
                OwnedValue::plain(LuaType::Unknown)
            }
        } else {
            OwnedValue::plain(LuaType::Unknown)
        };
        let explicit_generics = call
            .get_call_generic_type_list()
            .map(|list| list.get_types().map(|t| t.get_syntax_id()).collect())
            .unwrap_or_default();
        Ok(OwnedCallSiteAnalysis {
            candidates,
            arg_types: args,
            colon_call: colon,
            receiver_ty: receiver,
            explicit_generics,
        })
    }
    pub(super) fn call_site_signatures(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Vec<(OwnedValue, Bindings)>, E> {
        self.checkpoint()?;
        let key = (self.model.file_id(), call.get_syntax_id());
        let cached = self.call_site_signatures.borrow().get(&key).cloned();
        if let Some(cached) = cached {
            self.checkpoint()?;
            return Ok(cached);
        }
        let analysis = self.call_site_analysis(call)?;
        let signatures = self.resolved_owned_call_signatures(call, &analysis)?;
        self.checkpoint()?;
        self.call_site_signatures
            .borrow_mut()
            .insert(key, signatures.clone());
        Ok(signatures)
    }
    fn resolved_owned_call_signatures(
        &mut self,
        call: &LuaCallExpr,
        analysis: &OwnedCallSiteAnalysis,
    ) -> QueryResult<Vec<(OwnedValue, Bindings)>, E> {
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(Vec::new());
        };
        let constraints = self.access_owner_constraints(&callee)?;
        let mut out = Vec::new();
        for function in &analysis.candidates {
            self.checkpoint()?;
            let function = self.access_apply_owner_generics(function, &constraints)?;
            let params = function.function_parameters().expect("function parameters");
            let has_self = first_param_is_self(&function);
            let start = usize::from(analysis.colon_call && has_self);
            let mut bindings = Bindings::new();
            if analysis.colon_call
                && !has_self
                && let Some(Some(param)) = params.first()
            {
                let _ = self.unify_structural(param, &analysis.receiver_ty, &mut bindings)?;
            }
            for (param, arg) in params[start..].iter().zip(&analysis.arg_types) {
                if let Some(param) = param {
                    let _ = self.unify_structural(param, arg, &mut bindings)?;
                }
            }
            out.push((function, bindings));
        }
        Ok(out)
    }
}
pub(super) fn first_param_is_self(value: &OwnedValue) -> bool {
    let LuaType::DocFunction(function) = value.structural_type() else {
        return false;
    };
    function.get_params().first().is_some_and(|(name, ty)| {
        name == "self"
            || ty.as_ref().is_some_and(|t| {
                t.is_self_infer() || matches!(t,LuaType::Ref(id) if id.get_name()=="self")
            })
    })
}

fn access_literal_member_key(key: LuaIndexKey) -> Option<LuaMemberKey> {
    match key {
        LuaIndexKey::Name(name) => Some(LuaMemberKey::Name(name.get_name_text().into())),
        LuaIndexKey::String(string) => Some(LuaMemberKey::Name(string.get_value().into())),
        LuaIndexKey::Integer(number) => match number.get_number_value() {
            emmylua_parser::NumberResult::Int(value) => Some(LuaMemberKey::Integer(value)),
            emmylua_parser::NumberResult::Uint(value) => Some(LuaMemberKey::Integer(value as i64)),
            _ => None,
        },
        LuaIndexKey::Idx(index) => Some(LuaMemberKey::Integer(index as i64)),
        LuaIndexKey::Expr(_) => None,
    }
}
