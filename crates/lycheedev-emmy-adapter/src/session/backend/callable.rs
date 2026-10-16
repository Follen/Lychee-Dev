//! Callable candidate discovery using owned members, signatures and substitutions.
use super::{OwnedMemberInfo, OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    AsyncState, DeclKind, DocGenericParam, FileId, GenericTplId, LuaFunctionType, LuaMemberKey,
    LuaType, LuaTypeDeclId, SemanticId, Signature, SignatureDoc, TypeDef, TypeDefKind,
    VariadicType,
};
use emmylua_parser::{LuaAstNode, LuaDocFuncType, LuaDocType, LuaSyntaxId};
use std::{collections::HashMap, sync::Arc};

#[cfg(test)]
mod selected_member_tests;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn callable_candidates_from_type(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.expand_owned_callables(value, &mut Vec::new())
    }
    fn expand_owned_callables(
        &mut self,
        value: &OwnedValue,
        visited: &mut Vec<LuaTypeDeclId>,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::DocFunction(_) => Ok(vec![value.clone()]),
            LuaType::Union(_) => {
                let mut out = Vec::new();
                for component in value.union_components().expect("union components") {
                    out.extend(self.expand_owned_callables(&component, visited)?);
                }
                Ok(out)
            }
            LuaType::Intersection(intersection) => {
                let mut out = Vec::new();
                for (index, ty) in intersection.get_types().iter().enumerate() {
                    out.extend(
                        self.expand_owned_callables(&self.owned_child(value, index, ty), visited)?,
                    );
                }
                Ok(out)
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                if visited.contains(id) {
                    return Ok(Vec::new());
                }
                visited.push(id.clone());
                let mut out = Vec::new();
                if let Some(def) = self.model.type_def_of(id)
                    && let Some(alias) = self.model.alias_target(&def)
                {
                    out = self.expand_owned_callables(&OwnedValue::plain(alias), visited)?;
                }
                if out.is_empty()
                    && let Some(def) = self.model.type_def_of(id)
                {
                    out = self.callable_type_def(&def)?;
                }
                visited.pop();
                Ok(out)
            }
            LuaType::Generic(generic) => {
                let id = generic.get_base_type_id();
                if visited.contains(&id) {
                    return Ok(Vec::new());
                }
                visited.push(id.clone());
                let mut out = Vec::new();
                if let Some(def) = self.model.type_def_of(&id)
                    && def.kind == TypeDefKind::Alias
                    && let Some(alias) = self.model.alias_target(&def)
                {
                    let bindings = generic
                        .get_params()
                        .iter()
                        .enumerate()
                        .map(|(index, ty)| {
                            (
                                GenericTplId::Type(index as u32),
                                self.owned_child(value, index, ty),
                            )
                        })
                        .collect();
                    let instantiated =
                        self.substitute_owned(OwnedValue::plain(alias), &bindings)?;
                    out = self.expand_owned_callables(&instantiated, visited)?;
                }
                if out.is_empty()
                    && let Some(def) = self.model.type_def_of(&id)
                {
                    out = self.callable_type_def(&def)?;
                }
                visited.pop();
                Ok(out)
            }
            _ => Ok(Vec::new()),
        }
    }
    fn callable_type_def(&mut self, def: &TypeDef) -> QueryResult<Vec<OwnedValue>, E> {
        let mut out = Vec::new();
        for syntax in &def.call_overloads {
            if let Some(fun) =
                self.callable_doc_function(def.file_id, *syntax, &def.generic_params, false)?
            {
                out.push(fun);
            }
        }
        if let Some(op) = self
            .model
            .file_facts_of(def.file_id)
            .and_then(|facts| facts.operator_of(&def.id, "call"))
        {
            let mut params = Vec::new();
            for syntax in &op.params {
                self.checkpoint()?;
                params.push((
                    String::new(),
                    Some(self.model.doc_type_lua_rich_in(def.file_id, *syntax)),
                ));
            }
            self.checkpoint()?;
            out.push(OwnedValue::plain(LuaType::DocFunction(Arc::new(
                LuaFunctionType::new(
                    AsyncState::None,
                    false,
                    false,
                    params,
                    self.model.doc_type_lua_rich_in(def.file_id, op.returns),
                    None,
                ),
            ))));
        }
        Ok(out)
    }
    pub(super) fn callable_candidates_from_prefix(
        &mut self,
        prefix: &OwnedValue,
        key: &LuaMemberKey,
        member: Option<&SemanticId>,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        if let Some(members) = self.selected_members(prefix)? {
            // A selected field's id names the destination write for reference
            // provenance. Its callable value may come from a different closure
            // (or be absent/unavailable), so that id cannot supply signatures.
            // Validate the selected shape even when no callable is found.
            let mut out = Vec::new();
            for info in members {
                self.checkpoint()?;
                if &info.key != key {
                    continue;
                }
                for candidate in self.callable_candidates_from_type(&info.typ)? {
                    self.checkpoint()?;
                    if !out.contains(&candidate) {
                        out.push(candidate);
                    }
                }
            }
            return Ok(out);
        }
        if let Some(components) = self.selected_member_prefix_components(prefix)? {
            // Member collection merges composite rows and retains one source
            // id. Resolve each carried component before that merge so a Lua
            // destination id cannot replace any selected component's value.
            let mut out = Vec::new();
            for component in components {
                self.checkpoint()?;
                for candidate in self.callable_candidates_from_prefix(&component, key, None)? {
                    self.checkpoint()?;
                    if !out.contains(&candidate) {
                        out.push(candidate);
                    }
                }
            }
            return Ok(out);
        }
        let allow_cross_file = member
            .and_then(|member| self.callable_member_cross_file(member))
            .unwrap_or(true);
        let member_file = member.and_then(callable_member_file);
        let mut out = Vec::new();
        if let Some(member) = member
            && let Some(candidates) = self.callable_candidates_for_owner_single(member)?
        {
            out.extend(candidates);
        }
        for info in self.member_infos_with_key_all(prefix, key)? {
            self.checkpoint()?;
            if !allow_cross_file
                && let (Some(file), Some(member_file)) = (info.file_id, member_file)
                && file != member_file
            {
                continue;
            }
            self.push_owned_callable_member(&info, &mut out)?;
        }
        let mut unique = Vec::new();
        for candidate in out {
            self.checkpoint()?;
            if !unique.contains(&candidate) {
                unique.push(candidate);
            }
        }
        Ok(unique)
    }
    fn selected_member_prefix_components(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<Vec<OwnedValue>>, E> {
        self.checkpoint()?;
        if value.is_context_free() {
            return Ok(None);
        }
        Ok(match value.structural_type() {
            LuaType::Union(_) => value.union_components(),
            LuaType::Intersection(intersection) => {
                let mut components = Vec::new();
                for (index, ty) in intersection.get_types().iter().enumerate() {
                    self.checkpoint()?;
                    components.push(self.owned_child(value, index, ty));
                }
                Some(components)
            }
            LuaType::Array(array) => Some(vec![self.owned_child(value, 0, array.get_base())]),
            _ => None,
        })
    }
    fn push_owned_callable_member(
        &mut self,
        info: &OwnedMemberInfo,
        out: &mut Vec<OwnedValue>,
    ) -> QueryResult<(), E> {
        let before = out.len();
        if let (Some(member), Some(_)) = (&info.id, info.file_id)
            && let Some(candidates) = self.callable_candidates_for_owner_single(member)?
        {
            out.extend(candidates);
        }
        if out.len() == before {
            out.extend(self.callable_candidates_from_type(&info.typ)?);
        }
        if out.len() == before
            && let (Some(member), Some(file)) = (&info.id, info.file_id)
            && !self.owner_initializer_managed(member)?
            && let Some(syntax) = self
                .model
                .file_facts_of(file)
                .and_then(|facts| facts.member_by_id(member))
                .and_then(|member| member.value_syntax)
            && let Ok(fun) = self.signature_in(file, syntax)?
        {
            out.push(fun);
        }
        Ok(())
    }
    fn callable_member_cross_file(&self, member: &SemanticId) -> Option<bool> {
        let SemanticId::Member(key) = member else {
            return Some(true);
        };
        let facts = self.model.file_facts_of(key.file_id)?;
        let member = facts.member_by_id(member)?;
        match &member.owner {
            SemanticId::TypeDef(_) | SemanticId::Name(_) => Some(true),
            SemanticId::Decl(key) => facts
                .decl_by_id(&SemanticId::Decl(key.clone()))
                .map(|decl| decl.kind == DeclKind::Global),
            _ => Some(false),
        }
    }
    pub(super) fn callable_candidates_for_owner(
        &mut self,
        owner: &SemanticId,
    ) -> QueryResult<Option<Vec<OwnedValue>>, E> {
        self.checkpoint()?;
        if self.owner_initializer_managed(owner)? {
            return self.callable_candidates_for_owner_single(owner);
        }
        if let SemanticId::Decl(key) = owner
            && let Some(decl) = self
                .model
                .file_facts_of(key.file_id)
                .and_then(|facts| facts.decl_by_id(owner))
            && decl.kind == DeclKind::Global
        {
            let decls = self
                .model
                .global_decls_named_for_file(key.file_id, decl.name.as_str());
            if decls.len() > 1 {
                let mut out = Vec::new();
                for decl in decls {
                    if let Some(candidates) = self.callable_candidates_for_owner_single(&decl)? {
                        out.extend(candidates);
                    }
                }
                return Ok((!out.is_empty()).then_some(out));
            }
        }
        self.callable_candidates_for_owner_single(owner)
    }
    pub(super) fn callable_candidates_for_owner_single(
        &mut self,
        owner: &SemanticId,
    ) -> QueryResult<Option<Vec<OwnedValue>>, E> {
        self.checkpoint()?;
        if self.owner_initializer_managed(owner)? {
            let selected_owner = match owner {
                SemanticId::Name(_) => self.model.resolve_owner(owner),
                _ => Some(owner.clone()),
            };
            let value = match selected_owner.as_ref() {
                Some(id @ SemanticId::Decl(_)) => self.type_of_decl(id)?,
                Some(id @ SemanticId::Member(_)) => self.type_of_member(id)?,
                _ => None,
            };
            return match value {
                Some(value) => self.callable_functions_cached(&value).map(Some),
                None => Ok(Some(Vec::new())),
            };
        }
        let (file, syntax) = match owner {
            SemanticId::Decl(key) => match self
                .model
                .file_facts_of(key.file_id)
                .and_then(|facts| facts.decl_by_id(owner))
                .and_then(|decl| decl.value_expr_syntax)
            {
                Some(syntax) => (key.file_id, syntax),
                None => return Ok(None),
            },
            SemanticId::Member(key) => match self
                .model
                .file_facts_of(key.file_id)
                .and_then(|facts| facts.member_by_id(owner))
                .and_then(|member| member.value_syntax)
            {
                Some(syntax) => (key.file_id, syntax),
                None => return Ok(None),
            },
            SemanticId::Name(name) => {
                let mut out = Vec::new();
                for decl in self.model.global_decls_named_primary(name.as_str()) {
                    if let Some(candidates) = self.callable_candidates_for_owner(&decl)? {
                        out.extend(candidates);
                    }
                }
                return Ok((!out.is_empty()).then_some(out));
            }
            _ => return Ok(None),
        };
        let Some(signature) = self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.signature_by_closure(syntax))
            .cloned()
        else {
            return Ok(None);
        };
        let Some(docs) = &signature.docs else {
            return if matches!(owner, SemanticId::Member(_)) {
                self.signature_in(file, syntax)
                    .map(|result| result.ok().map(|fun| vec![fun]))
            } else {
                Ok(None)
            };
        };
        let mut out = Vec::new();
        for syntax in &docs.overloads {
            if let Some(fun) =
                self.callable_doc_function(file, *syntax, &docs.generic_params, false)?
            {
                out.push(fun);
            }
        }
        if let Some(fun) = self.callable_main_signature(file, &signature, &docs.generic_params)? {
            out.push(fun);
        }
        Ok(Some(out))
    }
    fn callable_main_signature(
        &mut self,
        file: FileId,
        signature: &Signature,
        generics: &[DocGenericParam],
    ) -> QueryResult<Option<OwnedValue>, E> {
        let Some(docs) = &signature.docs else {
            return Ok(None);
        };
        let mut params = Vec::new();
        let mut metadata_params = Vec::new();
        for name in &signature.param_names {
            self.checkpoint()?;
            let mut ty = match docs.param_types.iter().find(|(found, _)| found == name) {
                Some((_, syntax)) => self.callable_doc_type(file, *syntax, generics)?,
                None => OwnedValue::plain(LuaType::Any),
            };
            if docs.nullable_params.iter().any(|found| found == name)
                && !ty.structural_type().is_nullable()
            {
                ty = OwnedValue::raw_union(vec![ty, OwnedValue::plain(LuaType::Nil)]);
            }
            metadata_params.push((name.to_string(), Some(ty.structural_type().clone())));
            params.push(Some(ty));
        }
        let ret = self.callable_return_overloads(file, docs, generics, None)?;
        let generic_tpls = self.model.generic_tpls_with_metadata(file, generics);
        let ret = self.bind_owned_signature_generics(ret, &generic_tpls)?;
        let metadata = LuaFunctionType::new(
            if docs.is_async {
                AsyncState::Async
            } else {
                AsyncState::None
            },
            signature.is_method,
            signature.is_variadic,
            metadata_params,
            ret.structural_type().clone(),
            Some(generic_tpls),
        );
        Ok(Some(OwnedValue::function(&metadata, params, ret)))
    }
    pub(super) fn callable_doc_function(
        &mut self,
        file: FileId,
        syntax: LuaSyntaxId,
        generics: &[DocGenericParam],
        is_method: bool,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(LuaDocType::Func(fun)) = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaDocType::cast)
        else {
            return Ok(None);
        };
        self.callable_doc_function_ast(file, &fun, generics, is_method)
            .map(Some)
    }
    fn callable_doc_function_ast(
        &mut self,
        file: FileId,
        fun: &LuaDocFuncType,
        outer: &[DocGenericParam],
        is_method: bool,
    ) -> QueryResult<OwnedValue, E> {
        let mut own = Vec::new();
        if let Some(list) = fun.get_generic_decl_list() {
            for decl in list.get_generic_decl() {
                self.checkpoint()?;
                if let Some(token) = decl.get_name_token() {
                    own.push(DocGenericParam::new(
                        token.get_name_text().into(),
                        decl.get_constraint_type().map(|ty| ty.get_syntax_id()),
                        decl.get_default_type().map(|ty| ty.get_syntax_id()),
                        decl.has_const_modifier(),
                        decl.is_variadic(),
                    ));
                }
            }
        }
        let has_own = !own.is_empty();
        let generics = if has_own { own.as_slice() } else { outer };
        let mut params = Vec::new();
        let mut metadata_params = Vec::new();
        let mut variadic = false;
        for param in fun.get_params() {
            self.checkpoint()?;
            variadic |= param.is_dots();
            let name = param
                .get_name_token()
                .map(|token| token.get_name_text().to_string())
                .unwrap_or_else(|| {
                    if param.is_dots() {
                        "...".into()
                    } else {
                        String::new()
                    }
                });
            let ty = match param.get_type() {
                Some(ty) => self.callable_doc_type(file, ty.get_syntax_id(), generics)?,
                None => OwnedValue::plain(LuaType::Unknown),
            };
            metadata_params.push((name, Some(ty.structural_type().clone())));
            params.push(Some(ty));
        }
        let mut returns = Vec::new();
        if let Some(list) = fun.get_return_type_list() {
            for ret in list.get_return_type_list() {
                if let (_, Some(ty)) = ret.get_name_and_type() {
                    returns.push(self.callable_doc_type(file, ty.get_syntax_id(), generics)?);
                }
            }
        }
        let ret = owned_return_pack(returns);
        let metadata = LuaFunctionType::new(
            if fun.is_async() {
                AsyncState::Async
            } else {
                AsyncState::None
            },
            is_method,
            variadic,
            metadata_params,
            ret.structural_type().clone(),
            Some(self.model.generic_tpls_with_metadata(file, generics)),
        );
        let value = OwnedValue::function(&metadata, params, ret);
        if has_own {
            self.reassign_owned_function_generics(value)
        } else {
            Ok(value)
        }
    }
    pub(super) fn callable_doc_type(
        &mut self,
        file: FileId,
        syntax: LuaSyntaxId,
        generics: &[DocGenericParam],
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let ty = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaDocType::cast);
        match ty {
            Some(LuaDocType::Variadic(variadic)) => {
                let base = match variadic.get_type() {
                    Some(inner) => self.callable_doc_type(file, inner.get_syntax_id(), generics)?,
                    None => OwnedValue::plain(LuaType::Unknown),
                };
                Ok(owned_base_variadic(base))
            }
            Some(LuaDocType::Func(fun)) => {
                self.callable_doc_function_ast(file, &fun, generics, false)
            }
            Some(
                LuaDocType::Literal(_)
                | LuaDocType::Object(_)
                | LuaDocType::Binary(_)
                | LuaDocType::IndexAccess(_)
                | LuaDocType::Mapped(_)
                | LuaDocType::Unary(_)
                | LuaDocType::Conditional(_),
            ) => Ok(OwnedValue::plain(
                self.model.doc_type_lua_rich_in(file, syntax),
            )),
            _ => Ok(OwnedValue::plain(
                self.model.doc_type_lua_in(file, syntax, generics),
            )),
        }
    }
    pub(super) fn callable_return_overloads(
        &mut self,
        file: FileId,
        docs: &SignatureDoc,
        generics: &[DocGenericParam],
        bindings: Option<&HashMap<GenericTplId, OwnedValue>>,
    ) -> QueryResult<OwnedValue, E> {
        if docs.return_overload_rows.is_empty() {
            let mut returns = Vec::new();
            for syntax in &docs.returns {
                returns.push(self.callable_doc_type(file, *syntax, generics)?);
            }
            return Ok(owned_return_pack(returns));
        }
        let templates = self.model.generic_tpls_with_metadata(file, generics);
        let mut rows = Vec::new();
        let mut tails = Vec::new();
        let mut index = 0;
        for len in &docs.return_overload_rows {
            self.checkpoint()?;
            let end = (index + *len).min(docs.return_overloads.len());
            let mut raw = Vec::new();
            for (_, syntax) in &docs.return_overloads[index..end] {
                let ty = self.callable_doc_type(file, *syntax, generics)?;
                let ty = self.bind_owned_signature_generics(ty, &templates)?;
                raw.push(if let Some(bindings) = bindings {
                    self.substitute_owned(ty, bindings)?
                } else {
                    ty
                });
            }
            let (fixed, tail) = self.flatten_owned_return_row(raw)?;
            rows.push(fixed);
            tails.push(tail);
            index = end;
        }
        let mut main = Vec::new();
        for syntax in &docs.returns {
            let ty = self.callable_doc_type(file, *syntax, generics)?;
            let ty = self.bind_owned_signature_generics(ty, &templates)?;
            main.push(if let Some(bindings) = bindings {
                self.substitute_owned(ty, bindings)?
            } else {
                ty
            });
        }
        let prefix = !main.is_empty()
            && !main
                .iter()
                .any(|ty| matches!(ty.structural_type(), LuaType::Variadic(_)));
        let mut all_rows = rows.clone();
        let mut all_tails = tails.clone();
        if !prefix && !main.is_empty() {
            let (fixed, tail) = self.flatten_owned_return_row(main.clone())?;
            all_rows.insert(0, fixed);
            all_tails.insert(0, tail);
        }
        let fixed_count = all_rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut slots = Vec::new();
        for slot in 0..fixed_count {
            self.checkpoint()?;
            if prefix && slot < main.len() {
                slots.push(main[slot].clone());
                continue;
            }
            let mut components = if prefix {
                vec![OwnedValue::plain(LuaType::Nil)]
            } else {
                Vec::new()
            };
            for (row, tail) in all_rows.iter().zip(&all_tails) {
                let value = row
                    .get(slot)
                    .cloned()
                    .or_else(|| tail.clone())
                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Nil));
                append_owned_union_members(&mut components, value);
            }
            slots.push(owned_raw_single_union(components));
        }
        if all_tails.iter().any(Option::is_some) {
            let mut components = if prefix {
                vec![OwnedValue::plain(LuaType::Nil)]
            } else {
                Vec::new()
            };
            let selected = if prefix { &tails } else { &all_tails };
            for tail in selected {
                append_owned_union_members(
                    &mut components,
                    tail.clone()
                        .unwrap_or_else(|| OwnedValue::plain(LuaType::Nil)),
                );
            }
            slots.push(owned_base_variadic(owned_raw_single_union(components)));
        }
        Ok(if slots.len() == 1 {
            slots.pop().expect("single slot")
        } else {
            OwnedValue::pack(slots)
        })
    }
    fn flatten_owned_return_row(
        &mut self,
        values: Vec<OwnedValue>,
    ) -> QueryResult<(Vec<OwnedValue>, Option<OwnedValue>), E> {
        let mut stack = values.into_iter().rev().collect::<Vec<_>>();
        let mut fixed = Vec::new();
        let mut tail = None;
        while let Some(value) = stack.pop() {
            self.checkpoint()?;
            match value.structural_type() {
                LuaType::Variadic(variadic) => match variadic.as_ref() {
                    VariadicType::Base(base) => {
                        if tail.is_none() {
                            tail = Some(self.owned_child(&value, 0, base));
                        }
                    }
                    VariadicType::Multi(types) => {
                        for (index, ty) in types.iter().enumerate().rev() {
                            stack.push(self.owned_child(&value, index, ty));
                        }
                    }
                },
                _ => fixed.push(value),
            }
        }
        Ok((fixed, tail))
    }
}
fn callable_member_file(id: &SemanticId) -> Option<FileId> {
    match id {
        SemanticId::Member(key) => Some(key.file_id),
        SemanticId::Decl(key) => Some(key.file_id),
        _ => None,
    }
}
fn owned_return_pack(mut values: Vec<OwnedValue>) -> OwnedValue {
    match values.len() {
        0 => OwnedValue::plain(LuaType::Unknown),
        1 => values.pop().expect("single return"),
        _ => OwnedValue::pack(values),
    }
}
fn owned_base_variadic(base: OwnedValue) -> OwnedValue {
    OwnedValue::with_structural_children(
        LuaType::Variadic(Arc::new(VariadicType::Base(base.structural_type().clone()))),
        vec![base],
    )
}
fn append_owned_union_members(out: &mut Vec<OwnedValue>, value: OwnedValue) {
    for component in value.union_components().unwrap_or_else(|| vec![value]) {
        if !out.contains(&component) {
            out.push(component);
        }
    }
}
fn owned_raw_single_union(mut values: Vec<OwnedValue>) -> OwnedValue {
    if values.len() == 1 {
        values.pop().expect("single component")
    } else {
        OwnedValue::raw_union(values)
    }
}
