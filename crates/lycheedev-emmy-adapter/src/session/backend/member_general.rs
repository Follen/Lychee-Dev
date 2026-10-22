//! Owned member queries: original cache, lookup/collection order and contextual
//! initializer projection, using public facts, syntax and type-shell APIs.
use super::{CacheEntry, OwnedMemberInfo, OwnedQuery, PendingMemo, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DeclKind, FileId, GenericTplId, InFiled, LuaMemberKey, LuaType, Member, MemberRef, SemanticId,
    TypeDef, TypeDefKind, VariadicType,
};
use emmylua_parser::{
    LuaAstNode, LuaExpr, LuaIndexKey, LuaLiteralExpr, LuaLiteralToken, LuaTableExpr, LuaTableField,
    NumberResult,
};
use std::{
    collections::{HashMap, HashSet},
    rc::Rc,
};

type Bindings = HashMap<GenericTplId, OwnedValue>;
type NameBindings = HashMap<String, OwnedValue>;
const MAX_MEMBER_INHERITANCE_DEPTH: usize = 16;

impl<E> OwnedQuery<'_, '_, E> {
    pub fn type_of_member(&mut self, member: &SemanticId) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let file = match member {
            SemanticId::Member(key) => key.file_id,
            _ => self.model.file_id(),
        };
        let key = (file, member.clone());
        let cached = self.members.borrow().get(&key).cloned();
        if let Some(cached) = cached {
            self.checkpoint()?;
            return Ok(match cached {
                CacheEntry::Ready(value) => value,
                CacheEntry::InProgress => None,
            });
        }
        self.members
            .borrow_mut()
            .insert(key.clone(), CacheEntry::InProgress);
        let pending = PendingMemo {
            memo: Rc::clone(&self.members),
            key: Some(key),
        };
        let value = self.member_value_body(member, file)?;
        self.checkpoint()?;
        pending.commit(value.clone());
        Ok(value)
    }

    fn member_value_body(
        &mut self,
        id: &SemanticId,
        file: FileId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        if file != self.model.file_id() {
            let mut foreign = OwnedQuery::new(self.analysis, self.session, file, &mut *self.check);
            foreign.contribution_order = self.contribution_order.clone();
            foreign.workspace_facts = std::sync::Arc::clone(&self.workspace_facts);
            foreign.flow_trees = Rc::clone(&self.flow_trees);
            foreign.initializer_selection = std::rc::Rc::clone(&self.initializer_selection);
            return foreign.type_of_member(id);
        }
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        let member = facts.member_by_id(id).cloned();
        if let Some(syntax) = member.as_ref().and_then(|member| member.value_syntax)
            && self.manages_initializer(syntax)?
        {
            return self.type_of_expr(syntax).map(Some);
        }
        if let Some(value) = self.member_enum_const(file, id)? {
            return Ok(Some(OwnedValue::plain(value)));
        }
        if let Some(member) = &member
            && !matches!(member.owner, SemanticId::TypeDef(_))
            && let Some(syntax) = member.value_syntax
        {
            let expr = self
                .model
                .syntax_tree_of(file)
                .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                .and_then(LuaExpr::cast);
            if let Some(expr) = expr
                && (self.member_is_initializer(member)? || matches!(expr, LuaExpr::CallExpr(_)))
            {
                let value = self.type_of_expr(syntax)?;
                if !matches!(value.structural_type(), LuaType::Unknown) {
                    return Ok(Some(value));
                }
            }
            // A selected session deliberately suppresses the old uncontextual
            // shell fallback for runtime assignments, including plain values.
            return Ok(Some(OwnedValue::plain(LuaType::Unknown)));
        }
        self.checkpoint()?;
        let Some(member) = member else {
            return Ok(Some(OwnedValue::plain(LuaType::Unknown)));
        };
        let mut ty = LuaType::Unknown;
        if let Some(module) = &member.module_path
            && let Some(file) = self.model.module_file_of(module.as_str())
        {
            ty = LuaType::ModuleRef(file);
        } else if let SemanticId::TypeDef(_) = &member.owner
            && let Some(syntax) = member.value_syntax
        {
            let generics = self
                .model
                .file_facts_of(file)
                .and_then(|facts| facts.type_def_by_id(&member.owner))
                .map(|def| def.generic_params.as_slice())
                .unwrap_or(&[]);
            ty = self.model.doc_type_lua_in(file, syntax, generics);
        }
        if matches!(ty, LuaType::Unknown) && member.is_method {
            ty = LuaType::Function;
        }
        self.checkpoint()?;
        let value = self.expand_aliases(OwnedValue::plain(ty))?;
        self.eval_owned_conditionals(value).map(Some)
    }

    fn member_enum_const(
        &mut self,
        file: FileId,
        id: &SemanticId,
    ) -> QueryResult<Option<LuaType>, E> {
        self.checkpoint()?;
        let result = (|| {
            let facts = self.model.file_facts_of(file)?;
            let member = facts.member_by_id(id)?;
            let SemanticId::Member(owner) = &member.owner else {
                return None;
            };
            let decl = facts.decl_by_value_range(owner.key_range)?;
            facts
                .type_def_by_owner_syntax(decl.owner_syntax?)
                .filter(|def| def.kind == TypeDefKind::Enum)?;
            let node = member
                .value_syntax?
                .to_node_from_root(&self.model.syntax_tree_of(file)?.get_red_root())?;
            let literal = LuaLiteralExpr::cast(node)?;
            match literal.get_literal()? {
                LuaLiteralToken::Number(number) => match number.get_number_value() {
                    NumberResult::Int(value) => Some(LuaType::IntegerConst(value)),
                    NumberResult::Uint(value) => Some(LuaType::IntegerConst(value as i64)),
                    _ => None,
                },
                _ => None,
            }
        })();
        self.checkpoint()?;
        Ok(result)
    }

    pub(super) fn member_is_initializer(&mut self, member: &Member) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let Some(value) = member.value_syntax else {
            return Ok(false);
        };
        match &member.owner {
            SemanticId::Decl(key) => {
                let node = self
                    .model
                    .file_facts_of(key.file_id)
                    .and_then(|facts| facts.decl_by_id(&member.owner))
                    .and_then(|decl| decl.value_expr_syntax)
                    .and_then(|syntax| {
                        self.model
                            .syntax_tree_of(key.file_id)
                            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                    });
                Ok(node
                    .and_then(LuaTableExpr::cast)
                    .is_some_and(|table| table.get_range().contains(value.get_range().start())))
            }
            SemanticId::Member(key) => {
                let Some(tree) = self.model.syntax_tree_of(key.file_id) else {
                    return Ok(false);
                };
                for node in tree.get_red_root().descendants() {
                    self.checkpoint()?;
                    if let Some(table) = LuaTableExpr::cast(node)
                        && table.get_range() == key.key_range
                        && table.get_range().contains(value.get_range().start())
                    {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }

    pub fn member_infos(&mut self, value: &OwnedValue) -> QueryResult<Vec<OwnedMemberInfo>, E> {
        self.checkpoint()?;
        if let Some(cached) = self.member_lists.get(value).cloned() {
            self.checkpoint()?;
            return Ok(cached);
        }
        let members = self.member_infos_uncached(value)?;
        self.checkpoint()?;
        self.member_lists.insert(value.clone(), members.clone());
        Ok(members)
    }

    fn member_infos_uncached(
        &mut self,
        value: &OwnedValue,
    ) -> QueryResult<Vec<OwnedMemberInfo>, E> {
        if let Some(members) = self.selected_members(value)? {
            return Ok(members);
        }
        let mut out = Vec::new();
        self.collect_owned_members(value, None, None, &mut Vec::new(), &mut out, None)?;
        Ok(dedup_members(out))
    }

    pub fn member_infos_with_key_all(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Vec<OwnedMemberInfo>, E> {
        if let Some(members) = self.selected_members(value)? {
            return Ok(members
                .into_iter()
                .filter(|member| &member.key == key)
                .collect());
        }
        let mut out = Vec::new();
        self.collect_owned_members(value, None, None, &mut Vec::new(), &mut out, Some(key))?;
        Ok(out)
    }

    pub fn member_infos_with_key(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Vec<OwnedMemberInfo>, E> {
        self.member_infos_with_key_all(value, key)
            .map(dedup_members)
    }

    pub fn member_type(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.member_info_uncached(value, key)
            .map(|info| info.map(|info| info.typ))
    }

    pub fn member_info(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        self.checkpoint()?;
        let cache_key = (value.clone(), key.clone());
        if let Some(info) = self.member_items.get(&cache_key).cloned() {
            self.checkpoint()?;
            return Ok(info);
        }
        let info = self.member_info_uncached(value, key)?;
        self.checkpoint()?;
        self.member_items.insert(cache_key, info.clone());
        Ok(info)
    }

    pub(super) fn member_info_uncached(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        if let Some(members) = self.selected_members(value)? {
            return Ok(members.into_iter().find(|info| &info.key == key));
        }
        self.member_info_impl_owned(value, key)
    }

    pub(super) fn table_metatable_type(
        &mut self,
        table: &InFiled<rowan::TextRange>,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        if table.file_id != self.model.file_id() {
            let mut foreign =
                OwnedQuery::new(self.analysis, self.session, table.file_id, &mut *self.check);
            foreign.contribution_order = self.contribution_order.clone();
            foreign.workspace_facts = std::sync::Arc::clone(&self.workspace_facts);
            foreign.flow_trees = Rc::clone(&self.flow_trees);
            foreign.initializer_selection = std::rc::Rc::clone(&self.initializer_selection);
            return foreign.table_metatable_type(table);
        }
        let calls = self
            .model
            .file_facts_of(table.file_id)
            .map(|facts| facts.setmetatable_calls().to_vec())
            .unwrap_or_default();
        for (range, first, metatable) in calls {
            self.checkpoint()?;
            let matches = if range == table.value {
                true
            } else {
                matches!(self.type_of_expr(first)?.structural_type(), LuaType::TableConst(found) if found.file_id == table.file_id && found.value == table.value)
            };
            if matches {
                return self.type_of_expr(metatable).map(Some);
            }
        }
        Ok(None)
    }

    pub(super) fn table_metatable_index_info(
        &mut self,
        table: &InFiled<rowan::TextRange>,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        let Some(value) = self.table_metatable_type(table)? else {
            return Ok(None);
        };
        self.member_info_uncached(&value, &LuaMemberKey::Name("__index".into()))
    }
}

fn synthetic_member(key: LuaMemberKey, typ: OwnedValue) -> OwnedMemberInfo {
    OwnedMemberInfo {
        key,
        typ,
        id: None,
        file_id: None,
        is_method: false,
    }
}
fn dedup_members(members: Vec<OwnedMemberInfo>) -> Vec<OwnedMemberInfo> {
    let mut seen = HashSet::new();
    members
        .into_iter()
        .filter(|member| seen.insert(member.key.clone()))
        .collect()
}
fn push_member(
    out: &mut Vec<OwnedMemberInfo>,
    filter: Option<&LuaMemberKey>,
    info: OwnedMemberInfo,
) {
    if filter.is_none_or(|key| key == &info.key) {
        out.push(info);
    }
}

impl<E> OwnedQuery<'_, '_, E> {
    fn owner_member_refs(
        &mut self,
        owner: &SemanticId,
        filter: Option<&LuaMemberKey>,
    ) -> QueryResult<Vec<MemberRef>, E> {
        self.checkpoint()?;
        let members = if let Some(LuaMemberKey::Name(name)) = filter {
            self.model.members_of_owner_named(owner, name.as_str())
        } else {
            self.model.members_of_owner(owner)
        };
        let mut out = Vec::new();
        for member in members.iter() {
            self.checkpoint()?;
            out.push(member.clone());
        }
        Ok(out)
    }

    fn owned_member_info_of(
        &mut self,
        reference: &MemberRef,
        bindings: Option<&Bindings>,
        names: Option<&NameBindings>,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        self.checkpoint()?;
        let Some(member) = self
            .model
            .file_facts_of(reference.file_id)
            .and_then(|facts| facts.member_by_id(&reference.id))
            .cloned()
        else {
            return Ok(None);
        };
        if let Some(version) = self.model.lua_version()
            && let Some(signature) = member.value_syntax.and_then(|syntax| {
                self.model
                    .file_facts_of(reference.file_id)
                    .and_then(|facts| facts.signature_by_closure(syntax))
            })
            && let Some(docs) = &signature.docs
            && !docs.versions.is_empty()
        {
            let mut visible = false;
            for condition in &docs.versions {
                self.checkpoint()?;
                if condition.check(&version) {
                    visible = true;
                    break;
                }
            }
            if !visible {
                return Ok(None);
            }
        }
        let Some(mut typ) = self.type_of_member(&reference.id)? else {
            return Ok(None);
        };
        if matches!(typ.structural_type(), LuaType::Unknown | LuaType::Function)
            && !self.model.is_std_file(reference.file_id)
            && !self.member_is_class_table(reference.file_id, &member)?
            && let Some(syntax) = member.value_syntax
        {
            if reference.file_id == self.model.file_id() {
                let expr = self.type_of_expr(syntax)?;
                if self.member_is_initializer(&member)? {
                    if let Ok(function) = self.signature_in(reference.file_id, syntax)? {
                        typ = function;
                    } else if !matches!(expr.structural_type(), LuaType::Unknown) {
                        typ = expr;
                    }
                } else if !matches!(expr.structural_type(), LuaType::Unknown) {
                    typ = expr;
                }
            } else if let Ok(function) = self.signature_in(reference.file_id, syntax)? {
                typ = function;
            }
        }
        if member.is_nullable
            && !typ.structural_type().is_nullable()
            && !matches!(
                typ.structural_type(),
                LuaType::Function
                    | LuaType::DocFunction(_)
                    | LuaType::Ref(_)
                    | LuaType::Def(_)
                    | LuaType::Generic(_)
                    | LuaType::Array(_)
                    | LuaType::Object(_)
            )
        {
            typ = OwnedValue::union_checked(vec![typ, OwnedValue::plain(LuaType::Nil)], || {
                self.checkpoint()
            })?;
        }
        typ = self.reassign_owned_function_generics(typ)?;
        if let Some(bindings) = bindings {
            typ = self.substitute_owned(typ, bindings)?;
        }
        if let Some(names) = names {
            typ = self.substitute_owned_names(typ, names)?;
        }
        typ = self.expand_aliases(typ)?;
        typ = self.eval_owned_conditionals(typ)?;
        Ok(Some(OwnedMemberInfo {
            key: member.key,
            typ,
            id: Some(member.id),
            file_id: Some(reference.file_id),
            is_method: member.is_method,
        }))
    }

    fn member_is_class_table(&mut self, file: FileId, member: &Member) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(false);
        };
        match &member.owner {
            SemanticId::Decl(_) => {
                let Some(decl) = facts.decl_by_id(&member.owner) else {
                    return Ok(false);
                };
                for def in &facts.type_defs {
                    self.checkpoint()?;
                    if def.name == decl.name && def.owner_syntax == decl.owner_syntax {
                        return Ok(true);
                    }
                }
            }
            SemanticId::Name(name) => {
                for def in &facts.type_defs {
                    self.checkpoint()?;
                    if def.name.as_str() == name.as_str() {
                        return Ok(true);
                    }
                }
            }
            SemanticId::Member(key) => {
                let mut decl = None;
                for candidate in &facts.decls {
                    self.checkpoint()?;
                    if candidate
                        .value_expr_syntax
                        .is_some_and(|syntax| syntax.get_range() == key.key_range)
                    {
                        decl = Some(candidate);
                        break;
                    }
                }
                if let Some(decl) = decl {
                    for def in &facts.type_defs {
                        self.checkpoint()?;
                        if def.name == decl.name && def.owner_syntax == decl.owner_syntax {
                            return Ok(true);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }

    fn find_owned_member_in_owner(
        &mut self,
        owner: &SemanticId,
        key: &LuaMemberKey,
        bindings: Option<&Bindings>,
        names: Option<&NameBindings>,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        for reference in self.owner_member_refs(owner, Some(key))? {
            self.checkpoint()?;
            if self
                .model
                .file_facts_of(reference.file_id)
                .and_then(|facts| facts.member_by_id(&reference.id))
                .is_some_and(|member| &member.key == key)
                && let Some(info) = self.owned_member_info_of(&reference, bindings, names)?
            {
                return Ok(Some(info));
            }
        }
        Ok(None)
    }

    fn direct_owned_member(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        if let Some(members) = self.selected_members(value)? {
            return Ok(members.into_iter().find(|info| &info.key == key));
        }
        match value.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => {
                let Some(def) = self.model.type_def_of(id) else {
                    return Ok(None);
                };
                if def.kind == TypeDefKind::Alias {
                    let mut current = value.clone();
                    let mut visited = HashSet::new();
                    loop {
                        self.checkpoint()?;
                        let (LuaType::Ref(id) | LuaType::Def(id)) = current.structural_type()
                        else {
                            return self.direct_owned_member(&current, key);
                        };
                        if !visited.insert(id.clone()) {
                            return Ok(None);
                        }
                        let Some(def) = self.model.type_def_of(id) else {
                            return Ok(None);
                        };
                        if def.kind != TypeDefKind::Alias {
                            return self.direct_owned_member(&current, key);
                        }
                        let Some(alias) = self.model.alias_target(&def) else {
                            return Ok(None);
                        };
                        current = OwnedValue::plain(alias);
                    }
                }
                let (bindings, names) = self.member_default_bindings(&def, false)?;
                self.find_owned_type_member(
                    &def,
                    key,
                    bindings.as_ref(),
                    names.as_ref(),
                    &mut Vec::new(),
                )
            }
            LuaType::Generic(generic) => {
                let Some(def) = self.model.type_def_of(&generic.get_base_type_id()) else {
                    return Ok(None);
                };
                if def.kind == TypeDefKind::Alias {
                    let expanded = self.expand_aliases(value.clone())?;
                    return self.direct_owned_member(&expanded, key);
                }
                let (bindings, names) = self.member_generic_bindings(value, &def);
                self.find_owned_type_member(
                    &def,
                    key,
                    Some(&bindings),
                    Some(&names),
                    &mut Vec::new(),
                )
            }
            LuaType::TableConst(table) => {
                let owner = SemanticId::member(table.file_id, table.value);
                if let Some(info) = self.find_owned_member_in_owner(&owner, key, None, None)? {
                    return Ok(Some(info));
                }
                let owners = self.table_decl_owners(table)?;
                for owner in owners {
                    if let Some(info) = self.find_owned_member_in_owner(&owner, key, None, None)? {
                        return Ok(Some(info));
                    }
                }
                Ok(None)
            }
            LuaType::Object(object) => Ok(object
                .get_fields()
                .get(key)
                .map(|ty| synthetic_member(key.clone(), self.owned_field(value, key, ty)))),
            LuaType::Array(array) => {
                self.direct_owned_member(&self.owned_child(value, 0, array.get_base()), key)
            }
            LuaType::String | LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
                self.find_owned_member_in_owner(&SemanticId::name("string".into()), key, None, None)
            }
            _ => Ok(None),
        }
    }

    fn member_default_bindings(
        &mut self,
        def: &TypeDef,
        reject_unknown: bool,
    ) -> QueryResult<(Option<Bindings>, Option<NameBindings>), E> {
        let mut bindings = Bindings::new();
        let mut names = NameBindings::new();
        for (index, param) in def.generic_params.iter().enumerate() {
            self.checkpoint()?;
            let Some(syntax) = param.default.or(param.constraint) else {
                return Ok((None, None));
            };
            let ty = self
                .model
                .doc_type_lua_in(def.file_id, syntax, &def.generic_params);
            self.checkpoint()?;
            if reject_unknown && matches!(ty, LuaType::Unknown) {
                return Ok((None, None));
            }
            let value = OwnedValue::plain(ty);
            bindings.insert(GenericTplId::Type(index as u32), value.clone());
            names.insert(param.name.to_string(), value);
        }
        Ok(if bindings.is_empty() {
            (None, None)
        } else {
            (Some(bindings), Some(names))
        })
    }

    fn member_generic_bindings(
        &self,
        value: &OwnedValue,
        def: &TypeDef,
    ) -> (Bindings, NameBindings) {
        let LuaType::Generic(generic) = value.structural_type() else {
            unreachable!("generic member bindings");
        };
        let mut bindings = Bindings::new();
        let mut names = NameBindings::new();
        for (index, ty) in generic.get_params().iter().enumerate() {
            let value = self.owned_child(value, index, ty);
            bindings.insert(GenericTplId::Type(index as u32), value.clone());
            if let Some(param) = def.generic_params.get(index) {
                names.insert(param.name.to_string(), value);
            }
        }
        (bindings, names)
    }

    fn table_decl_owners(
        &mut self,
        table: &InFiled<rowan::TextRange>,
    ) -> QueryResult<Vec<SemanticId>, E> {
        self.checkpoint()?;
        let Some(decl) = self
            .model
            .file_facts_of(table.file_id)
            .and_then(|facts| facts.decl_by_value_range(table.value))
        else {
            return Ok(Vec::new());
        };
        let mut owners = vec![decl.id.clone()];
        if matches!(decl.kind, DeclKind::Global) {
            owners.push(SemanticId::name(decl.name.clone()));
        }
        Ok(owners)
    }

    fn find_owned_type_member(
        &mut self,
        def: &TypeDef,
        key: &LuaMemberKey,
        bindings: Option<&Bindings>,
        names: Option<&NameBindings>,
        visited: &mut Vec<SemanticId>,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        self.checkpoint()?;
        if visited.contains(&def.id) || visited.len() >= MAX_MEMBER_INHERITANCE_DEPTH {
            return Ok(None);
        }
        visited.push(def.id.clone());
        if let Some(info) = self.find_owned_member_in_owner(&def.id, key, bindings, names)? {
            visited.pop();
            return Ok(Some(info));
        }
        for super_name in &def.super_names {
            self.checkpoint()?;
            if let Some(bound) = member_bound_parent(def, super_name.as_str(), bindings) {
                if let Some(info) = self.member_info_impl_owned(&bound, key)? {
                    visited.pop();
                    return Ok(Some(info));
                }
                continue;
            }
            if let Some(parent) = self
                .model
                .resolve_type_def_in(def.file_id, super_name.as_str())
                && let Some(info) =
                    self.find_owned_type_member(&parent, key, bindings, names, visited)?
            {
                visited.pop();
                return Ok(Some(info));
            }
        }
        for owner in self.type_runtime_owners(def)? {
            if owner != def.id
                && let Some(info) = self.find_owned_member_in_owner(&owner, key, bindings, names)?
            {
                visited.pop();
                return Ok(Some(info));
            }
        }
        visited.pop();
        Ok(None)
    }

    // This entry corresponds to upstream member_info_impl (one fewer initial
    // selected probe than member_info); positive selected probes still occur once.
    fn member_info_impl_owned(
        &mut self,
        value: &OwnedValue,
        key: &LuaMemberKey,
    ) -> QueryResult<Option<OwnedMemberInfo>, E> {
        // The fixed member_info -> impl -> direct sequence probes selected
        // membership at each producer. Only a positive probe emits a receipt.
        if let Some(members) = self.selected_members(value)? {
            return Ok(members.into_iter().find(|info| &info.key == key));
        }
        if let Some(info) = self.direct_owned_member(value, key)? {
            return Ok(Some(info));
        }
        if let LuaType::Intersection(types) = value.structural_type() {
            let mut distinct = Vec::new();
            let mut first = None;
            for (index, ty) in types.get_types().iter().enumerate() {
                let component = self.owned_child(value, index, ty);
                if let Some(info) = self.member_info_uncached(&component, key)? {
                    if !distinct.contains(&info.typ) {
                        distinct.push(info.typ.clone());
                    }
                    if first.is_none() {
                        first = Some(info);
                    }
                }
            }
            let Some(mut first) = first else {
                return Ok(None);
            };
            first.typ = if distinct.len() == 1 {
                distinct.remove(0)
            } else {
                OwnedValue::plain(LuaType::Never)
            };
            return Ok(Some(first));
        }
        if let Some(info) = self
            .member_infos_uncached(value)?
            .into_iter()
            .find(|info| &info.key == key)
        {
            return Ok(Some(info));
        }
        if let LuaType::TableConst(table) = value.structural_type()
            && let Some(index) = self.table_metatable_index_info(table)?
            && let Some(ret) = index.typ.function_return()
        {
            return Ok(Some(synthetic_member(key.clone(), ret)));
        }
        Ok(None)
    }

    fn collect_owned_members(
        &mut self,
        value: &OwnedValue,
        bindings: Option<&Bindings>,
        names: Option<&NameBindings>,
        visited: &mut Vec<SemanticId>,
        out: &mut Vec<OwnedMemberInfo>,
        filter: Option<&LuaMemberKey>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => {
                if let Some(def) = self.model.type_def_of(id) {
                    if def.kind == TypeDefKind::Alias {
                        if visited.contains(&def.id) {
                            return Ok(());
                        }
                        visited.push(def.id.clone());
                        if let Some(alias) = self.model.alias_target(&def) {
                            self.collect_owned_members(
                                &OwnedValue::plain(alias),
                                bindings,
                                names,
                                visited,
                                out,
                                filter,
                            )?;
                        }
                        visited.pop();
                        return Ok(());
                    }
                    if bindings.is_none() && !def.generic_params.is_empty() {
                        let (defaults, _) = self.member_default_bindings(&def, true)?;
                        if let Some(defaults) = defaults {
                            return self.collect_owned_type_members(
                                &def,
                                Some(&defaults),
                                names,
                                visited,
                                out,
                                filter,
                            );
                        }
                    }
                    self.collect_owned_type_members(&def, bindings, names, visited, out, filter)?;
                }
            }
            LuaType::Generic(generic) => {
                let def = self.model.type_def_of(&generic.get_base_type_id());
                if def
                    .as_ref()
                    .is_some_and(|def| def.kind == TypeDefKind::Alias)
                {
                    let expanded = self.expand_aliases(value.clone())?;
                    return self.collect_owned_members(&expanded, None, None, visited, out, filter);
                }
                let mut substitutions = Bindings::new();
                let mut named = NameBindings::new();
                for (index, ty) in generic.get_params().iter().enumerate() {
                    self.checkpoint()?;
                    let param = self.owned_child(value, index, ty);
                    substitutions.insert(GenericTplId::Type(index as u32), param.clone());
                    if let Some(param_def) =
                        def.as_ref().and_then(|def| def.generic_params.get(index))
                    {
                        named.insert(param_def.name.to_string(), param);
                    }
                }
                self.collect_owned_members(
                    &OwnedValue::plain(LuaType::Ref(generic.get_base_type_id())),
                    Some(&substitutions),
                    Some(&named),
                    visited,
                    out,
                    filter,
                )?;
            }
            LuaType::Array(array) => self.collect_owned_members(
                &self.owned_child(value, 0, array.get_base()),
                bindings,
                names,
                visited,
                out,
                filter,
            )?,
            LuaType::TableConst(table) => {
                let owner = SemanticId::member(table.file_id, table.value);
                for reference in self.owner_member_refs(&owner, filter)? {
                    if let Some(info) = self.owned_member_info_of(&reference, bindings, names)? {
                        if let Some(expanded) =
                            self.expand_table_member_pack(table, &reference, &info)?
                        {
                            for info in expanded {
                                push_member(out, filter, info);
                            }
                        } else {
                            push_member(out, filter, info);
                        }
                    }
                }
                for owner in self.table_decl_owners(table)? {
                    for reference in self.owner_member_refs(&owner, filter)? {
                        if let Some(info) =
                            self.owned_member_info_of(&reference, bindings, names)?
                        {
                            push_member(out, filter, info);
                        }
                    }
                }
            }
            LuaType::Object(object) => {
                if let Some(members) = self.selected_members(value)? {
                    for member in members {
                        push_member(out, filter, member);
                    }
                    return Ok(());
                }
                for (key, ty) in object.get_fields() {
                    self.checkpoint()?;
                    push_member(
                        out,
                        filter,
                        synthetic_member(key.clone(), self.owned_field(value, key, ty)),
                    );
                }
                for (index, (_, ty)) in object.get_index_access().iter().enumerate() {
                    self.checkpoint()?;
                    push_member(
                        out,
                        filter,
                        synthetic_member(
                            LuaMemberKey::Name("[index]".into()),
                            self.owned_index(value, index, false, ty),
                        ),
                    );
                }
            }
            LuaType::String | LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
                for reference in
                    self.owner_member_refs(&SemanticId::name("string".into()), filter)?
                {
                    if let Some(info) = self.owned_member_info_of(&reference, bindings, names)? {
                        push_member(out, filter, info);
                    }
                }
            }
            LuaType::Intersection(types) => {
                for (index, ty) in types.get_types().iter().enumerate() {
                    self.collect_owned_members(
                        &self.owned_child(value, index, ty),
                        bindings,
                        names,
                        visited,
                        out,
                        filter,
                    )?;
                }
            }
            LuaType::Union(_) => {
                for component in value.union_components().expect("union components") {
                    let mut members = Vec::new();
                    self.collect_owned_members(
                        &component,
                        bindings,
                        names,
                        visited,
                        &mut members,
                        filter,
                    )?;
                    for info in members {
                        self.checkpoint()?;
                        if let Some(existing) =
                            out.iter_mut().find(|existing| existing.key == info.key)
                        {
                            if existing.typ != info.typ {
                                existing.typ = OwnedValue::union_checked(
                                    vec![existing.typ.clone(), info.typ],
                                    || self.checkpoint(),
                                )?;
                            }
                        } else {
                            push_member(out, filter, info);
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn collect_owned_type_members(
        &mut self,
        def: &TypeDef,
        bindings: Option<&Bindings>,
        names: Option<&NameBindings>,
        visited: &mut Vec<SemanticId>,
        out: &mut Vec<OwnedMemberInfo>,
        filter: Option<&LuaMemberKey>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if visited.contains(&def.id) || visited.len() >= MAX_MEMBER_INHERITANCE_DEPTH {
            return Ok(());
        }
        visited.push(def.id.clone());
        for reference in self.owner_member_refs(&def.id, filter)? {
            if let Some(info) = self.owned_member_info_of(&reference, bindings, names)? {
                push_member(out, filter, info);
            }
        }
        for name in &def.super_names {
            self.checkpoint()?;
            if let Some(bound) = member_bound_parent(def, name.as_str(), bindings) {
                self.collect_owned_members(&bound, None, None, visited, out, filter)?;
                continue;
            }
            if let Some(parent) = self.model.resolve_type_def_in(def.file_id, name.as_str()) {
                self.collect_owned_type_members(&parent, bindings, names, visited, out, filter)?;
            }
        }
        for owner in self.type_runtime_owners(def)? {
            if owner == def.id {
                continue;
            }
            for reference in self.owner_member_refs(&owner, filter)? {
                if let Some(info) = self.owned_member_info_of(&reference, bindings, names)? {
                    push_member(out, filter, info);
                }
            }
        }
        // The fixed collection producer intentionally keeps its visited entries;
        // the direct find producer above pops instead. Preserve this distinction.
        Ok(())
    }

    fn expand_table_member_pack(
        &mut self,
        table: &InFiled<rowan::TextRange>,
        reference: &MemberRef,
        info: &OwnedMemberInfo,
    ) -> QueryResult<Option<Vec<OwnedMemberInfo>>, E> {
        let LuaType::Variadic(_) = info.typ.structural_type() else {
            return Ok(None);
        };
        let LuaMemberKey::Integer(start) = info.key else {
            return Ok(None);
        };
        self.checkpoint()?;
        let field = self
            .model
            .file_facts_of(reference.file_id)
            .and_then(|facts| facts.member_by_id(&reference.id))
            .and_then(|member| member.value_syntax)
            .and_then(|syntax| {
                self.model
                    .syntax_tree_of(reference.file_id)
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            })
            .and_then(|node| node.ancestors().find_map(LuaTableField::cast));
        let Some(field) = field else {
            return Ok(None);
        };
        let Some(table_expr) = field.syntax().parent().and_then(LuaTableExpr::cast) else {
            return Ok(None);
        };
        if table_expr.get_range() != table.value
            || field
                .get_field_key()
                .as_ref()
                .is_some_and(|key| !matches!(key, LuaIndexKey::Idx(_)))
        {
            return Ok(None);
        }
        let mut last = None;
        for field in table_expr.get_fields() {
            self.checkpoint()?;
            last = Some(field.get_range());
        }
        if last != Some(field.get_range()) {
            return Ok(None);
        }
        let mut slots = Vec::new();
        let LuaType::Variadic(variadic) = info.typ.structural_type() else {
            unreachable!();
        };
        match variadic.as_ref() {
            VariadicType::Base(base) => slots.push(self.owned_child(&info.typ, 0, base)),
            VariadicType::Multi(types) => {
                for (index, ty) in types.iter().enumerate() {
                    self.checkpoint()?;
                    let slot = self.owned_child(&info.typ, index, ty);
                    if let LuaType::Variadic(inner) = slot.structural_type() {
                        match inner.as_ref() {
                            VariadicType::Base(base) => {
                                slots.push(self.owned_child(&slot, 0, base))
                            }
                            VariadicType::Multi(inner) => {
                                for (index, ty) in inner.iter().enumerate() {
                                    slots.push(self.owned_child(&slot, index, ty));
                                }
                            }
                        }
                    } else {
                        slots.push(slot);
                    }
                }
            }
        }
        if slots.is_empty() {
            return Ok(None);
        }
        Ok(Some(
            slots
                .into_iter()
                .enumerate()
                .map(|(index, value)| {
                    synthetic_member(LuaMemberKey::Integer(start + index as i64), value)
                })
                .collect(),
        ))
    }
}

fn member_bound_parent(
    def: &TypeDef,
    name: &str,
    bindings: Option<&Bindings>,
) -> Option<OwnedValue> {
    let bindings = bindings?;
    for (index, param) in def.generic_params.iter().enumerate() {
        let param_name = param.name.as_str();
        if name == param_name || name.starts_with(&format!("{param_name}<")) {
            return bindings.get(&GenericTplId::Type(index as u32)).cloned();
        }
    }
    None
}

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn type_runtime_owners(&mut self, def: &TypeDef) -> QueryResult<Vec<SemanticId>, E> {
        self.checkpoint()?;
        let mut out = vec![def.id.clone()];
        if let SemanticId::TypeDef(key) = &def.id
            && let Some(primary) = self
                .model
                .type_defs_in_scope(key.scope, key.full_name.as_str())
                .iter()
                .next()
        {
            push_owner(&mut out, primary.id.clone());
        }
        // Reproduce WorkspaceDeclIndex::type_def_location: the first export
        // contribution for this scoped identity wins, not whichever duplicate
        // TypeDef the caller happened to resolve. The immutable intake supplies
        // the exact per-workspace contribution order.
        let order = self.owner_contribution_order()?;
        let mut location = None;
        for &file in order.iter() {
            self.checkpoint()?;
            if let Some(exported) = self
                .model
                .file_exports(file)
                .and_then(|exports| exports.types.iter().find(|exported| exported.id == def.id))
            {
                location = Some((exported.file_id, exported.name.clone()));
                break;
            }
        }
        let Some((file, bare_name)) = location else {
            return Ok(out);
        };
        // The public export contribution is precisely the index's per-file
        // runtime_values input; its first matching entry wins (or_insert).
        let runtime = self
            .model
            .file_exports(file)
            .and_then(|exports| {
                exports
                    .runtime_values
                    .iter()
                    .find(|(name, _)| name == &bare_name)
            })
            .map(|(_, id)| id.clone());
        if let Some(runtime) = runtime {
            if let Some(owner) = self.canonical_runtime_owner(&runtime)? {
                push_owner(&mut out, owner);
            }
            if let SemanticId::Decl(key) = &runtime
                && let Some(facts) = self.model.file_facts_of(key.file_id)
                && let Some(decl) = facts.decl_by_id(&runtime)
                && matches!(decl.kind, DeclKind::Global)
                && facts.is_meta
                && self.analysis.db.is_main_file(key.file_id)
            {
                push_owner(&mut out, SemanticId::name(decl.name.clone()));
            }
        }
        let associated = self
            .model
            .file_facts_of(file)
            .and_then(|facts| {
                facts.type_def_by_id(&def.id)?.owner_syntax.map(|syntax| {
                    facts
                        .decls_by_owner_syntax(syntax)
                        .map(|decl| decl.id.clone())
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        for id in associated {
            self.checkpoint()?;
            if let Some(owner) = self.canonical_runtime_owner(&id)? {
                push_owner(&mut out, owner);
            }
        }
        Ok(out)
    }

    pub(super) fn canonical_runtime_owner(
        &mut self,
        id: &SemanticId,
    ) -> QueryResult<Option<SemanticId>, E> {
        use emmylua_code_analysis::ModuleExport;
        self.checkpoint()?;
        let file = match id {
            SemanticId::Decl(key) => key.file_id,
            SemanticId::Member(key) => key.file_id,
            _ => return Ok(Some(id.clone())),
        };
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        let own_export = match &facts.module_export {
            ModuleExport::Decl { decl, .. } => decl == id,
            ModuleExport::Expr { value_syntax } => {
                matches!(id, SemanticId::Member(key) if key.key_range == value_syntax.get_range())
            }
            _ => false,
        };
        if own_export {
            return Ok(Some(id.clone()));
        }
        let alias_file = self
            .model
            .file_exports(file)
            .and_then(|exports| exports.aliases.iter().find(|alias| &alias.decl == id))
            .map(|alias| alias.module_file);
        if let Some(target) = alias_file {
            return Ok(self.model.file_facts_of(target).and_then(|facts| {
                match &facts.module_export {
                    ModuleExport::Decl { decl, .. } => Some(decl.clone()),
                    ModuleExport::Expr { value_syntax } => {
                        Some(SemanticId::member(target, value_syntax.get_range()))
                    }
                    _ => None,
                }
            }));
        }
        if let Some(decl) = facts.decl_by_id(id)
            && matches!(decl.kind, DeclKind::Global)
        {
            return Ok(Some(
                self.model
                    .global_decl(decl.name.as_str())
                    .unwrap_or_else(|| SemanticId::name(decl.name.clone())),
            ));
        }
        Ok(Some(id.clone()))
    }
}
fn push_owner(out: &mut Vec<SemanticId>, owner: SemanticId) {
    if !out.contains(&owner) {
        out.push(owner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{
        BindingContext, BindingDecision, BindingSite, BoundValue, OwnedBindingSession,
    };
    use emmylua_code_analysis::EmmyLuaAnalysis;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    struct Unmanaged(Arc<AtomicUsize>);
    impl BindingContext for Unmanaged {
        fn identity(&self) -> &str {
            "foreign-flow-cache-test"
        }
        fn resolve(&self, _: BindingSite) -> BindingDecision<BoundValue> {
            self.0.fetch_add(1, Ordering::Relaxed);
            BindingDecision::NotManaged
        }
    }

    #[test]
    fn foreign_member_and_metatable_queries_share_cfg_but_keep_fresh_expression_scopes() {
        let mut analysis = EmmyLuaAnalysis::new();
        let root = std::env::current_dir()
            .unwrap()
            .join("foreign-flow-cache-test");
        analysis.add_main_workspace(root.clone());
        let files = analysis.update_files_by_path(vec![
            (root.join("entry.lua"), Some("return 0".into())),
            (
                root.join("members.lua"),
                Some("local seed=1\nlocal t={first=seed,second=seed}\nreturn t".into()),
            ),
            (
                root.join("metatable.lua"),
                // The public fixed-engine index records declaration initializers.
                Some(
                    "local meta={}\nlocal t={}\nlocal applied=setmetatable(t,meta)\nreturn t"
                        .into(),
                ),
            ),
        ]);
        let member_model = analysis.semantic_model(files[1]);
        let facts = member_model.file_facts().unwrap();
        let members: Vec<_> = ["first", "second"]
            .map(|name| {
                facts
                    .members
                    .iter()
                    .find(|member| member.key == LuaMemberKey::Name(name.into()))
                    .unwrap()
                    .id
                    .clone()
            })
            .into_iter()
            .collect();
        let meta_model = analysis.semantic_model(files[2]);
        let meta_facts = meta_model.file_facts().unwrap();
        assert_eq!(meta_facts.setmetatable_calls().len(), 1);
        let table = |name: &str| {
            InFiled::new(
                files[2],
                meta_facts
                    .decls
                    .iter()
                    .find(|decl| decl.name.as_str() == name)
                    .unwrap()
                    .value_expr_syntax
                    .unwrap()
                    .get_range(),
            )
        };
        let (table, metatable) = (table("t"), table("meta"));
        let probes = Arc::new(AtomicUsize::new(0));
        let session =
            OwnedBindingSession::new_checked(Arc::new(Unmanaged(Arc::clone(&probes))), || {
                Ok::<_, ()>(())
            })
            .unwrap();
        let mut check = || Ok::<_, ()>(());
        let mut query =
            OwnedQuery::new_for_snapshot(&analysis, &session, files[0], &mut check).unwrap();
        assert!(query.flow_trees.borrow().is_empty());
        assert_eq!(
            query.type_of_member(&members[0]).unwrap().unwrap(),
            OwnedValue::plain(LuaType::IntegerConst(1))
        );
        let member_cfg = query
            .flow_trees
            .borrow()
            .get(&files[1])
            .cloned()
            .expect("foreign member publishes its one CFG");
        assert_eq!(
            query.type_of_member(&members[1]).unwrap().unwrap(),
            OwnedValue::plain(LuaType::IntegerConst(1))
        );
        assert!(Arc::ptr_eq(
            &member_cfg,
            query.flow_trees.borrow().get(&files[1]).unwrap()
        ));
        assert_eq!(query.flow_trees.borrow().len(), 1);
        let expected = OwnedValue::plain(LuaType::TableConst(metatable));
        assert_eq!(
            query.table_metatable_type(&table).unwrap(),
            Some(expected.clone())
        );
        let meta_cfg = query
            .flow_trees
            .borrow()
            .get(&files[2])
            .cloned()
            .expect("foreign metatable publishes its one CFG");
        let before = probes.load(Ordering::Relaxed);
        assert_eq!(query.table_metatable_type(&table).unwrap(), Some(expected));
        assert!(
            probes.load(Ordering::Relaxed) > before,
            "fresh model expression probes must not be cached with the CFG"
        );
        assert!(Arc::ptr_eq(
            &meta_cfg,
            query.flow_trees.borrow().get(&files[2]).unwrap()
        ));
        assert!(!Arc::ptr_eq(&member_cfg, &meta_cfg));
        assert_eq!(query.flow_trees.borrow().len(), 2);
        assert!(
            session
                .coverage_checked(|| Ok::<_, ()>(()))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn standalone_metatable_call_does_not_invent_an_unindexed_mutation() {
        let mut analysis = EmmyLuaAnalysis::new();
        let root = std::env::current_dir()
            .unwrap()
            .join("standalone-metatable-test");
        analysis.add_main_workspace(root.clone());
        let files = analysis.update_files_by_path(vec![
            (root.join("entry.lua"), Some("return 0".into())),
            (
                root.join("metatable.lua"),
                Some("local meta={}\nlocal t={}\nsetmetatable(t,meta)\nreturn t".into()),
            ),
        ]);
        let model = analysis.semantic_model(files[1]);
        let facts = model.file_facts().unwrap();
        // The fixed public facts and member helper have no standalone mutation
        // record or query-position state. Scanning the whole file here would
        // incorrectly invent effects before/after a call or in a nested scope.
        assert!(facts.setmetatable_calls().is_empty());
        let table_syntax = facts
            .decls
            .iter()
            .find(|decl| decl.name.as_str() == "t")
            .unwrap()
            .value_expr_syntax
            .unwrap();
        let table = InFiled::new(files[1], table_syntax.get_range());
        assert_eq!(
            model.type_of_expr(table_syntax),
            LuaType::TableConst(table.clone())
        );
        let probes = Arc::new(AtomicUsize::new(0));
        let session =
            OwnedBindingSession::new_checked(Arc::new(Unmanaged(Arc::clone(&probes))), || {
                Ok::<_, ()>(())
            })
            .unwrap();
        let mut check = || Ok::<_, ()>(());
        let mut query =
            OwnedQuery::new_for_snapshot(&analysis, &session, files[0], &mut check).unwrap();
        assert_eq!(query.table_metatable_type(&table).unwrap(), None);
        assert_eq!(query.table_metatable_type(&table).unwrap(), None);
        assert_eq!(probes.load(Ordering::Relaxed), 0);
        assert!(query.flow_trees.borrow().is_empty());
        assert!(
            session
                .coverage_checked(|| Ok::<_, ()>(()))
                .unwrap()
                .is_empty()
        );
    }
}
