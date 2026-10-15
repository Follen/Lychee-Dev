//! VM-level callable priority and constructor/metatable synthesis.
//! Adapted from fixed EmmyLua infer/vm.rs; discovery retains producer order.
use super::{ExpressionValue, OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    AsyncState, ConstructorAttribute, ConstructorReturnMode, LuaFunctionType, LuaGenericType,
    LuaMemberKey, LuaType, LuaTypeDeclId, SemanticId, TypeDefKind, TypeVisibility,
};
use std::sync::Arc;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn owned_callable_candidates(
        &mut self,
        callee: &ExpressionValue<OwnedValue>,
    ) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        if let Some(owner) = &callee.owner
            && let Some(candidates) = self.callable_candidates_for_owner(owner)?
            && !candidates.is_empty()
        {
            return Ok(candidates);
        }
        if matches!(callee.value.structural_type(), LuaType::DocFunction(_)) {
            return Ok(vec![callee.value.clone()]);
        }
        if let Some(class) = self.call_receiver_class(&callee.value)? {
            let candidates = self.callable_candidates_from_type(&class)?;
            if !candidates.is_empty() {
                return Ok(candidates);
            }
        }
        if let LuaType::Ref(id) | LuaType::Def(id) = callee.value.structural_type()
            && let Some(def) = self.model.type_def_of(id)
            && let Some(attribute) = self.model.constructor_attribute_of_type(&def.id)
            && let Some(candidate) = self.call_constructor_candidate(&callee.value, &attribute)?
        {
            return Ok(vec![candidate]);
        }
        let candidates = self.callable_candidates_from_type(&callee.value)?;
        if candidates.is_empty()
            && let Some(candidate) = self.call_metatable_candidate(&callee.value)?
        {
            return Ok(vec![candidate]);
        }
        Ok(candidates)
    }

    pub(super) fn call_receiver_class(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let LuaType::TableConst(table) = value.structural_type() else {
            return Ok(None);
        };
        let Some(facts) = self.model.file_facts_of(table.file_id) else {
            return Ok(None);
        };
        let Some(decl) = facts
            .decls
            .iter()
            .find(|decl| decl.value_expr_syntax.map(|s| s.get_range()) == Some(table.value))
        else {
            return Ok(None);
        };
        let Some(def) = facts
            .type_defs
            .iter()
            .find(|def| def.owner_syntax.is_some() && def.owner_syntax == decl.owner_syntax)
        else {
            return Ok(None);
        };
        let id = if matches!(def.visibility, TypeVisibility::Public) {
            LuaTypeDeclId::global(&def.full_name)
        } else {
            LuaTypeDeclId::file(def.file_id, &def.full_name)
        };
        Ok(Some(OwnedValue::plain(LuaType::Ref(id))))
    }

    pub(super) fn call_metatable_candidate(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let LuaType::TableConst(table) = value.structural_type() else {
            return Ok(None);
        };
        let Some(metatable) = self.table_metatable_type(table)? else {
            return Ok(None);
        };
        let Some(info) = self
            .member_infos_with_key(&metatable, &LuaMemberKey::Name("__call".into()))?
            .into_iter()
            .next()
        else {
            return Ok(None);
        };
        let (Some(file), Some(member)) = (info.file_id, info.id) else {
            return Ok(None);
        };
        let Some(syntax) = self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.member_by_id(&member))
            .and_then(|member| member.value_syntax)
        else {
            return Ok(None);
        };
        let Ok(function) = self.signature_in(file, syntax)? else {
            return Ok(None);
        };
        let LuaType::DocFunction(metadata) = function.structural_type() else {
            unreachable!();
        };
        let mut params = function.function_parameters().expect("function parameters");
        let mut names = metadata.get_params().to_vec();
        if !params.is_empty() {
            params.remove(0);
            names.remove(0);
        }
        let mut ret = function.function_return().expect("function return");
        if matches!(ret.structural_type(), LuaType::Unknown) {
            ret = OwnedValue::plain(LuaType::SelfInfer);
        }
        let metadata = LuaFunctionType::new(
            metadata.get_async_state(),
            false,
            metadata.is_variadic(),
            names,
            ret.structural_type().clone(),
            Some(metadata.get_generic_params().to_vec()),
        );
        Ok(Some(OwnedValue::function(&metadata, params, ret)))
    }

    fn call_constructor_candidate(
        &mut self,
        class: &OwnedValue,
        attribute: &ConstructorAttribute,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let (LuaType::Ref(id) | LuaType::Def(id)) = class.structural_type() else {
            return Ok(None);
        };
        let Some(def) = self.model.type_def_of(id) else {
            return Ok(None);
        };
        let key = LuaMemberKey::Name(attribute.name.clone());
        let info = self.member_infos_with_key(class, &key)?.into_iter().next();
        let (file, id) = if let Some(info) = info {
            let (Some(file), Some(id)) = (info.file_id, info.id) else {
                return Ok(None);
            };
            (file, id)
        } else {
            let Some(member) = self
                .model
                .members_of_owner(&SemanticId::name(def.name.clone()))
                .into_iter()
                .find(|member| member.name == attribute.name)
            else {
                return Ok(None);
            };
            // Original fallback computes the member type even though only its id
            // and file are consumed below. Keep that query/coverage work.
            let _ = self.type_of_member(&member.id)?;
            (member.file_id, member.id.clone())
        };
        let Some(syntax) = self
            .model
            .file_facts_of(file)
            .and_then(|facts| facts.member_by_id(&id))
            .and_then(|member| member.value_syntax)
        else {
            return Ok(None);
        };
        let Ok(method) = self.signature_in(file, syntax)? else {
            return Ok(None);
        };
        let LuaType::DocFunction(metadata) = method.structural_type() else {
            unreachable!();
        };
        let generics = self
            .model
            .generic_tpls_with_metadata(def.file_id, &def.generic_params);
        let mut params = Vec::new();
        let mut names = Vec::new();
        for ((name, _), ty) in metadata
            .get_params()
            .iter()
            .zip(method.function_parameters().expect("function parameters"))
        {
            self.checkpoint()?;
            if name == "self"
                || ty
                    .as_ref()
                    .is_some_and(|ty| matches!(ty.structural_type(), LuaType::SelfInfer))
            {
                continue;
            }
            let ty = match ty {
                Some(ty) => Some(self.bind_owned_signature_generics(ty, &generics)?),
                None => None,
            };
            names.push((
                name.clone(),
                ty.as_ref().map(|v| v.structural_type().clone()),
            ));
            params.push(ty);
        }
        let ret = match attribute.return_mode {
            ConstructorReturnMode::Doc => self.bind_owned_signature_generics(
                method.function_return().expect("function return"),
                &generics,
            )?,
            ConstructorReturnMode::SelfType | ConstructorReturnMode::Default => {
                if generics.is_empty() {
                    class.clone()
                } else {
                    OwnedValue::plain(LuaType::Generic(Arc::new(LuaGenericType::new(
                        id_for(class),
                        generics
                            .iter()
                            .cloned()
                            .map(|g| LuaType::TplRef(Arc::new(g)))
                            .collect(),
                    ))))
                }
            }
        };
        let metadata = LuaFunctionType::new(
            AsyncState::None,
            false,
            metadata.is_variadic(),
            names,
            ret.structural_type().clone(),
            Some(generics),
        );
        Ok(Some(OwnedValue::function(&metadata, params, ret)))
    }

    pub(super) fn call_operator_self(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.call_operator_self_inner(value, &mut Vec::new())
    }
    fn call_operator_self_inner(
        &mut self,
        value: &OwnedValue,
        visited: &mut Vec<LuaTypeDeclId>,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => {
                if visited.contains(id) {
                    return Ok(None);
                }
                visited.push(id.clone());
                let result = if let Some(def) = self.model.type_def_of(id) {
                    if def.kind == TypeDefKind::Alias {
                        if let Some(target) = self.model.alias_target(&def) {
                            self.call_operator_self_inner(&OwnedValue::plain(target), visited)?
                        } else {
                            None
                        }
                    } else if !def.call_overloads.is_empty()
                        || self
                            .model
                            .file_facts_of(def.file_id)
                            .is_some_and(|facts| facts.operator_of(&def.id, "call").is_some())
                    {
                        Some(value.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                visited.pop();
                Ok(result)
            }
            LuaType::Generic(generic) => {
                let id = generic.get_base_type_id();
                if visited.contains(&id) {
                    return Ok(None);
                }
                let Some(def) = self.model.type_def_of(&id) else {
                    return Ok(None);
                };
                if def.kind == TypeDefKind::Alias {
                    visited.push(id);
                    let expanded = self.expand_owned_aliases(value.clone())?;
                    let result = self.call_operator_self_inner(&expanded, visited)?;
                    visited.pop();
                    return Ok(result);
                }
                Ok((!def.call_overloads.is_empty()
                    || self
                        .model
                        .file_facts_of(def.file_id)
                        .is_some_and(|facts| facts.operator_of(&def.id, "call").is_some()))
                .then(|| value.clone()))
            }
            LuaType::Union(_) => {
                let mut values = Vec::new();
                for part in value.union_components().expect("union") {
                    if let Some(part) = self.call_operator_self_inner(&part, visited)?
                        && !values.contains(&part)
                    {
                        values.push(part);
                    }
                }
                if values.is_empty() {
                    Ok(None)
                } else {
                    OwnedValue::union_checked(values, || self.checkpoint()).map(Some)
                }
            }
            LuaType::Intersection(intersection) => {
                for (i, part) in intersection.get_types().iter().enumerate() {
                    if self
                        .call_operator_self_inner(&self.owned_child(value, i, part), visited)?
                        .is_some()
                    {
                        return Ok(Some(value.clone()));
                    }
                }
                Ok(None)
            }
            LuaType::Instance(instance) => Ok(self
                .call_operator_self_inner(
                    &self.owned_child(value, 0, instance.get_base()),
                    visited,
                )?
                .map(|_| value.clone())),
            _ => Ok(None),
        }
    }

    pub(super) fn callback_function(
        &mut self,
        value: OwnedValue,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let value = self.expand_owned_aliases(value)?;
        match value.structural_type() {
            LuaType::DocFunction(_) => Ok(Some(value)),
            LuaType::Ref(id) | LuaType::Def(id) => {
                let target = self
                    .model
                    .type_def_of(id)
                    .filter(|def| def.kind == TypeDefKind::Alias)
                    .and_then(|def| self.model.alias_target(&def));
                if let Some(target) = target {
                    self.callback_function(OwnedValue::plain(target))
                } else {
                    Ok(None)
                }
            }
            LuaType::Union(_) => {
                for part in value.union_components().expect("union") {
                    if let Some(function) = self.callback_function(part)? {
                        return Ok(Some(function));
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}
fn id_for(value: &OwnedValue) -> LuaTypeDeclId {
    match value.structural_type() {
        LuaType::Ref(id) | LuaType::Def(id) => id.clone(),
        _ => unreachable!(),
    }
}
