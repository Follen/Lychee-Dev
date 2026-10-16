//! VM member indexing with owned value identity and inherited execution state.
use super::{ExpressionValue, OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend, SelectedProjector};
use emmylua_code_analysis::{DeclKind, LuaMemberKey, LuaType, SemanticId, TypeDefKind};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaSyntaxId};
use std::{cell::RefCell, rc::Rc};

struct ComputingMember {
    environment: Rc<RefCell<super::environment::Environment>>,
    member: SemanticId,
}
impl Drop for ComputingMember {
    fn drop(&mut self) {
        self.environment.borrow_mut().computing.remove(&self.member);
    }
}
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn owned_expr_inherit(&mut self, syntax: LuaSyntaxId) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        if self
            .model
            .syntax_tree()
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaExpr::cast)
            .is_none()
        {
            return Ok(OwnedValue::plain(LuaType::Unknown));
        }
        let environment = self.environment();
        let old = environment.borrow();
        let parameters = old.parameters.clone();
        let computing = old.computing.clone();
        let callback_depth = old.callback_depth;
        drop(old);
        let _scope = self.enter_execution(parameters);
        let environment = self.environment();
        environment.borrow_mut().computing = computing;
        environment.borrow_mut().callback_depth = callback_depth;
        let projector = SelectedProjector::new(self.analysis, self.session);
        projector.evaluate_expression(self.site(syntax), self)
    }
    pub(super) fn index_member_general(
        &mut self,
        mut owner: ExpressionValue<OwnedValue>,
        key: &LuaMemberKey,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        let receiver = Some(owner.value.clone());
        if owner.value.structural_type().is_any() {
            return Ok(index_plain(LuaType::Any));
        }
        if let LuaType::TplRef(tpl) = owner.value.structural_type()
            && let Some(constraint) = tpl.get_constraint()
        {
            owner.value = OwnedValue::plain(constraint.clone());
        } else if let LuaType::Ref(id) = owner.value.structural_type() {
            if let Some(syntax) = self.model.file_facts().and_then(|facts| {
                facts
                    .generic_param_constraints(id.get_name())
                    .find_map(|(_, param)| param.constraint)
            }) {
                owner.value = OwnedValue::plain(
                    self.model
                        .doc_type_lua_rich_in(self.model.file_id(), syntax),
                );
            }
        }
        let mut owners = Vec::new();
        if let LuaType::TableConst(table) = owner.value.structural_type() {
            owners.push(SemanticId::member(table.file_id, table.value));
        }
        if let Some(id) = &owner.owner
            && !owners.contains(id)
        {
            owners.push(id.clone());
        }
        if let Some(id @ SemanticId::Decl(key)) = &owner.owner
            && let Some(decl) = self
                .model
                .file_facts_of(key.file_id)
                .and_then(|facts| facts.decl_by_id(id))
            && decl.kind == DeclKind::Global
        {
            let name = SemanticId::name(decl.name.clone());
            if !owners.contains(&name) {
                owners.push(name);
            }
        }
        if let Some(id @ SemanticId::Decl(key)) = &owner.owner
            && let Some(facts) = self.model.file_facts_of(key.file_id)
            && let Some(decl) = facts.decl_by_id(id)
            && let Some(syntax) = decl.owner_syntax
            && let Some(def) = facts
                .type_def_by_owner_syntax(syntax)
                .filter(|def| matches!(def.kind, TypeDefKind::Class | TypeDefKind::Enum))
            && !owners.contains(&def.id)
        {
            owners.push(def.id.clone());
        }
        if let LuaType::Ref(id) | LuaType::Def(id) = owner.value.structural_type()
            && let Some(def) = self.model.type_def_of(id)
            && let Some(id) = &owner.owner
            && self.index_owner_is_class_table(id, def.name.as_str())
        {
            let name = SemanticId::name(def.name.clone());
            if !owners.contains(&name) {
                owners.push(name);
            }
        }
        let type_has_member = matches!(
            owner.value.structural_type(),
            LuaType::Ref(_) | LuaType::Def(_) | LuaType::Generic(_)
        ) && self.member_type(&owner.value, key)?.is_some();
        for owner_id in owners {
            self.checkpoint()?;
            if type_has_member {
                continue;
            }
            let members = self.model.members_of_owner(&owner_id);
            let member = members.into_iter().find(|member| {
                self.index_member_key_matches(member.file_id, &member.id, member.name.as_str(), key)
            });
            let Some(member) = member else {
                continue;
            };
            let environment = self.environment();
            if !environment.borrow_mut().computing.insert(member.id.clone()) {
                return Ok(index_plain(LuaType::Unknown));
            }
            let guard = ComputingMember {
                environment,
                member: member.id.clone(),
            };
            let mut ty = self
                .type_of_member(&member.id)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            if matches!(owner.value.structural_type(), LuaType::TableConst(_))
                && let Some(member_def) = self
                    .model
                    .file_facts_of(member.file_id)
                    .and_then(|facts| facts.member_by_id(&member.id))
                && matches!(&member_def.key, LuaMemberKey::Name(name) if name.is_empty())
                && let Some(syntax) = member_def.value_syntax
            {
                let expr = self.owned_expr_inherit(syntax)?;
                if !expr.structural_type().is_unknown() {
                    ty = expr;
                }
            }
            if ty.structural_type().is_unknown()
                && let Some(fallback) = self.member_type(&owner.value, key)?
            {
                ty = fallback;
            }
            if matches!(ty.structural_type(), LuaType::Unknown | LuaType::Function)
                && let Some(syntax) = self
                    .model
                    .file_facts_of(member.file_id)
                    .and_then(|facts| facts.member_by_id(&member.id))
                    .and_then(|member| member.value_syntax)
            {
                let expr = self.owned_expr_inherit(syntax)?;
                if !expr.structural_type().is_unknown() {
                    ty = expr;
                }
            }
            drop(guard);
            return Ok(ExpressionValue {
                value: ty,
                owner: Some(member.id.clone()),
                closure_syntax: None,
                receiver,
            });
        }
        let fallback = self.member_type(&owner.value, key)?;
        let candidates = self.callable_candidates_from_prefix(&owner.value, key, None)?;
        if candidates.len() > 1 {
            return Ok(ExpressionValue {
                value: OwnedValue::union_checked(candidates, || self.checkpoint())?,
                owner: None,
                closure_syntax: None,
                receiver,
            });
        }
        let std_global = matches!(&owner.owner, Some(SemanticId::Decl(key)) if self.model.is_std_file(key.file_id));
        let broad_function = fallback
            .as_ref()
            .is_some_and(|ty| matches!(ty.structural_type(), LuaType::Function));
        let needs_member_owner = broad_function && std_global;
        let needs_method_owner = self
            .member_info_uncached(&owner.value, key)?
            .and_then(|info| info.id)
            .is_some()
            && (fallback
                .as_ref()
                .is_some_and(|ty| matches!(ty.structural_type(), LuaType::Signature(_)))
                || (broad_function && !std_global));
        if let Some(ty) = &fallback
            && !ty.structural_type().is_unknown()
            && !needs_member_owner
            && !needs_method_owner
        {
            if broad_function
                && let Some(info) = self.member_info(&owner.value, key)?
                && let (Some(member_id), Some(file)) = (info.id, info.file_id)
                && let Some(facts) = self.model.file_facts_of(file)
                && let Some(member) = facts.member_by_id(&member_id)
                && let Some(syntax) = member.value_syntax
                && facts
                    .signature_by_closure(syntax)
                    .and_then(|signature| signature.docs.as_ref())
                    .is_some_and(|docs| !docs.generic_params.is_empty())
            {
                if let Ok(fun) = self.signature_in(file, syntax)? {
                    return Ok(ExpressionValue {
                        value: fun,
                        owner: Some(member_id),
                        closure_syntax: None,
                        receiver,
                    });
                }
                let expr = self.owned_expr_inherit(syntax)?;
                if !expr.structural_type().is_unknown() {
                    return Ok(ExpressionValue {
                        value: expr,
                        owner: Some(member_id),
                        closure_syntax: None,
                        receiver,
                    });
                }
            }
            return Ok(ExpressionValue {
                value: ty.clone(),
                owner: None,
                closure_syntax: None,
                receiver,
            });
        }
        if needs_method_owner
            && let Some(info) = self.member_info(&owner.value, key)?
            && let Some(member) = info.id
        {
            return Ok(ExpressionValue {
                value: fallback.unwrap_or_else(|| OwnedValue::plain(LuaType::Function)),
                owner: Some(member),
                closure_syntax: None,
                receiver,
            });
        }
        if matches!(owner.value.structural_type(), LuaType::TableConst(_))
            && let Some(id @ SemanticId::Decl(decl_key)) = &owner.owner
            && let Some(decl) = self
                .model
                .file_facts_of(decl_key.file_id)
                .and_then(|facts| facts.decl_by_id(id))
        {
            let name_owner = SemanticId::name(decl.name.clone());
            if let Some(member) =
                self.model
                    .members_of_owner(&name_owner)
                    .into_iter()
                    .find(|member| {
                        self.index_member_key_matches(
                            member.file_id,
                            &member.id,
                            member.name.as_str(),
                            key,
                        )
                    })
            {
                let mut ty = self
                    .type_of_member(&member.id)?
                    .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
                if matches!(ty.structural_type(), LuaType::Unknown | LuaType::Function)
                    && let Some(syntax) = self
                        .model
                        .file_facts_of(member.file_id)
                        .and_then(|facts| facts.member_by_id(&member.id))
                        .and_then(|member| member.value_syntax)
                {
                    ty = self.owned_expr_inherit(syntax)?;
                }
                return Ok(ExpressionValue {
                    value: ty,
                    owner: Some(member.id.clone()),
                    closure_syntax: None,
                    receiver,
                });
            }
        }
        if let Some(value) = fallback {
            return Ok(ExpressionValue {
                value,
                owner: None,
                closure_syntax: None,
                receiver,
            });
        }
        let known = match owner.value.structural_type() {
            LuaType::TableConst(table) => {
                !self
                    .model
                    .members_of_owner(&SemanticId::member(table.file_id, table.value))
                    .is_empty()
                    || self
                        .model
                        .file_facts_of(table.file_id)
                        .and_then(|facts| facts.decl_by_value_range(table.value))
                        .is_some_and(|decl| !self.model.members_of_owner(&decl.id).is_empty())
            }
            LuaType::Ref(_) | LuaType::Def(_) | LuaType::Object(_) => true,
            _ => false,
        };
        Ok(index_plain(if known {
            LuaType::Nil
        } else {
            LuaType::Unknown
        }))
    }
    fn index_owner_is_class_table(&self, owner: &SemanticId, name: &str) -> bool {
        match owner {
            SemanticId::Name(found) => found.as_str() == name,
            SemanticId::Decl(key) => self
                .model
                .file_facts_of(key.file_id)
                .and_then(|facts| facts.decl_by_id(owner))
                .is_some_and(|decl| decl.name.as_str() == name),
            _ => false,
        }
    }
    fn index_member_key_matches(
        &self,
        file: emmylua_code_analysis::FileId,
        id: &SemanticId,
        name: &str,
        key: &LuaMemberKey,
    ) -> bool {
        match key {
            LuaMemberKey::Name(found) => found.as_str() == name,
            LuaMemberKey::Integer(_) => self
                .model
                .file_facts_of(file)
                .and_then(|facts| facts.member_by_id(id))
                .is_some_and(|member| &member.key == key),
            _ => false,
        }
    }
    pub(super) fn index_dynamic_general(
        &mut self,
        owner: ExpressionValue<OwnedValue>,
        key: ExpressionValue<OwnedValue>,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        match key.value.structural_type() {
            LuaType::StringConst(name) | LuaType::DocStringConst(name) => {
                self.index_member_general(owner, &LuaMemberKey::Name(name.as_str().into()))
            }
            LuaType::IntegerConst(index) | LuaType::DocIntegerConst(index) => {
                if let LuaType::Array(array) = owner.value.structural_type() {
                    return self.index_array_result(self.owned_child(
                        &owner.value,
                        0,
                        array.get_base(),
                    ));
                }
                self.index_member_general(owner, &LuaMemberKey::Integer(*index))
            }
            LuaType::Union(_) => {
                let mut types = Vec::new();
                let mut any_string = false;
                let mut any_numeric = false;
                for component in key.value.union_components().expect("union components") {
                    self.checkpoint()?;
                    let result = match component.structural_type() {
                        LuaType::StringConst(name) | LuaType::DocStringConst(name) => {
                            any_string = true;
                            Some(
                                self.index_member_general(
                                    clone_index_value(&owner),
                                    &LuaMemberKey::Name(name.as_str().into()),
                                )?
                                .value,
                            )
                        }
                        LuaType::IntegerConst(index) | LuaType::DocIntegerConst(index) => {
                            any_numeric = true;
                            Some(
                                self.index_member_general(
                                    clone_index_value(&owner),
                                    &LuaMemberKey::Integer(*index),
                                )?
                                .value,
                            )
                        }
                        LuaType::Number | LuaType::Integer | LuaType::FloatConst(_) => {
                            any_numeric = true;
                            None
                        }
                        LuaType::Nil => None,
                        _ => return Ok(index_plain(LuaType::Unknown)),
                    };
                    if let Some(result) = result
                        && !result.structural_type().is_unknown()
                        && !types.contains(&result)
                    {
                        types.push(result);
                    }
                }
                if let LuaType::Array(array) = owner.value.structural_type() {
                    let base = self.owned_child(&owner.value, 0, array.get_base());
                    if any_numeric
                        && !types
                            .iter()
                            .any(|ty| matches!(ty.structural_type(), LuaType::Nil) || ty == &base)
                    {
                        types.push(base);
                        if self.model.strict_array_index() {
                            types.push(OwnedValue::plain(LuaType::Nil));
                        }
                    }
                }
                if (any_string || any_numeric) && !types.is_empty() {
                    self.index_union_result(types)
                } else {
                    Ok(index_plain(LuaType::Unknown))
                }
            }
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(index_plain(LuaType::Unknown));
                };
                if def.kind == TypeDefKind::Enum {
                    let Some(enum_owner) = self
                        .model
                        .file_facts_of(def.file_id)
                        .and_then(|facts| facts.decl_by_owner_syntax(def.owner_syntax?))
                        .and_then(|decl| decl.value_expr_syntax)
                        .map(|syntax| SemanticId::member(def.file_id, syntax.get_range()))
                    else {
                        return Ok(index_plain(LuaType::Unknown));
                    };
                    let mut types = Vec::new();
                    for member in self.model.members_of_owner(&enum_owner).iter() {
                        let result = self
                            .index_member_general(
                                clone_index_value(&owner),
                                &LuaMemberKey::Name(member.name.clone()),
                            )?
                            .value;
                        if !result.structural_type().is_unknown() && !types.contains(&result) {
                            types.push(result);
                        }
                    }
                    if types.is_empty() {
                        Ok(index_plain(LuaType::Unknown))
                    } else {
                        self.index_union_result(types)
                    }
                } else if let Some(alias) = self.model.alias_target(&def) {
                    self.index_dynamic_general(owner, index_plain(alias))
                } else {
                    Ok(index_plain(LuaType::Unknown))
                }
            }
            LuaType::String => self.index_dynamic_string_general(owner),
            LuaType::Number | LuaType::Integer | LuaType::FloatConst(_) => {
                if let LuaType::Array(array) = owner.value.structural_type() {
                    return self.index_array_result(self.owned_child(
                        &owner.value,
                        0,
                        array.get_base(),
                    ));
                }
                if matches!(owner.value.structural_type(), LuaType::TableConst(_)) {
                    let mut types = Vec::new();
                    for info in self.member_infos(&owner.value)? {
                        if matches!(info.key, LuaMemberKey::Integer(_))
                            && !types.contains(&info.typ)
                        {
                            types.push(info.typ);
                        }
                    }
                    if !types.contains(&OwnedValue::plain(LuaType::Nil)) {
                        types.push(OwnedValue::plain(LuaType::Nil));
                    }
                    return self.index_union_result(types);
                }
                if let Some(components) = owner.value.union_components() {
                    let mut types = Vec::new();
                    for component in components {
                        let component = ExpressionValue {
                            value: component,
                            owner: owner.owner.clone(),
                            closure_syntax: None,
                            receiver: owner.receiver.clone(),
                        };
                        let result = self
                            .index_dynamic_general(component, clone_index_value(&key))?
                            .value;
                        if !result.structural_type().is_unknown() && !types.contains(&result) {
                            types.push(result);
                        }
                    }
                    return if types.is_empty() {
                        Ok(index_plain(LuaType::Unknown))
                    } else {
                        self.index_union_result(types)
                    };
                }
                Ok(index_plain(LuaType::Unknown))
            }
            _ => Ok(index_plain(LuaType::Unknown)),
        }
    }
    fn index_dynamic_string_general(
        &mut self,
        owner: ExpressionValue<OwnedValue>,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        if !matches!(owner.value.structural_type(), LuaType::TableConst(_)) {
            return Ok(index_plain(LuaType::Unknown));
        }
        let mut named = Vec::new();
        let mut empty = Vec::new();
        for info in self.member_infos(&owner.value)? {
            self.checkpoint()?;
            let LuaMemberKey::Name(name) = info.key else {
                continue;
            };
            let out = if name.is_empty() {
                &mut empty
            } else {
                &mut named
            };
            if !out.contains(&info.typ) {
                out.push(info.typ);
            }
        }
        if !named.is_empty() {
            if !named.contains(&OwnedValue::plain(LuaType::Nil)) {
                named.push(OwnedValue::plain(LuaType::Nil));
            }
            self.index_union_result(named)
        } else if !empty.is_empty() {
            self.index_union_result(empty)
        } else {
            Ok(index_plain(LuaType::Nil))
        }
    }
    fn index_array_result(
        &mut self,
        base: OwnedValue,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        if self.model.strict_array_index() {
            self.index_union_result(vec![base, OwnedValue::plain(LuaType::Nil)])
        } else {
            Ok(ExpressionValue::plain(base))
        }
    }
    fn index_union_result(
        &mut self,
        values: Vec<OwnedValue>,
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        OwnedValue::union_checked(values, || self.checkpoint()).map(ExpressionValue::plain)
    }
}
fn index_plain(ty: LuaType) -> ExpressionValue<OwnedValue> {
    ExpressionValue::plain(OwnedValue::plain(ty))
}
fn clone_index_value(value: &ExpressionValue<OwnedValue>) -> ExpressionValue<OwnedValue> {
    ExpressionValue {
        value: value.value.clone(),
        owner: value.owner.clone(),
        closure_syntax: value.closure_syntax,
        receiver: value.receiver.clone(),
    }
}
