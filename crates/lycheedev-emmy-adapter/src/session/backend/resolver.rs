//! Member-reference resolution. Selected values never cross a raw LuaType result.
use super::{OwnedQuery, QueryResult};
use crate::session::{
    BindingCoverage, BindingDecision, OwnedValue, SelectedProjectionBackend, SelectedProjector,
};
use emmylua_code_analysis::{
    DeclKind, FileId, LuaMemberKey, LuaType, ModuleExport, OwnerId, SemanticId, TypeDefKind,
    TypeScope,
};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaIndexExpr, LuaIndexKey, LuaSyntaxId, VisibilityKind};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct OwnedResolvedMember {
    pub(super) binding_token: Option<crate::session::SessionToken>,
    pub member_id: Option<SemanticId>,
    pub file_id: Option<FileId>,
    pub owner: SemanticId,
    pub name: String,
    pub member_type: Option<OwnedValue>,
    pub visibility: Option<VisibilityKind>,
    pub is_method: bool,
}
impl<E> OwnedQuery<'_, '_, E> {
    pub fn resolve_owned_member(
        &mut self,
        index: &LuaIndexExpr,
    ) -> QueryResult<Option<OwnedResolvedMember>, E> {
        self.checkpoint()?;
        let key = (self.model.file_id(), index.get_syntax_id());
        if let Some(result) = self.resolved_members.get(&key).cloned() {
            self.checkpoint()?;
            return Ok(result);
        }
        let result = self.resolve_owned_member_body(index)?;
        self.checkpoint()?;
        self.resolved_members.insert(key, result.clone());
        Ok(result)
    }
    fn resolve_owned_member_body(
        &mut self,
        index: &LuaIndexExpr,
    ) -> QueryResult<Option<OwnedResolvedMember>, E> {
        if self.resolution_manages(index.get_syntax_id())? {
            self.resolution_unavailable(index, "managed_index_declaration_unavailable")?;
            return Ok(None);
        }
        if let Some(prefix) = index.get_prefix_expr() {
            let ty = self.type_of_expr(prefix.get_syntax_id())?;
            if let Some(members) = self.selected_members(&ty)? {
                let Some(key) = index.get_index_key() else {
                    return Ok(None);
                };
                if !matches!(key, LuaIndexKey::Name(_) | LuaIndexKey::String(_)) {
                    self.resolution_unavailable(index, "frozen_member_key_unavailable")?;
                    return Ok(None);
                }
                let name = key.get_path_part();
                let Some(member) = members
                    .into_iter()
                    .find(|member| member.key == LuaMemberKey::Name(name.clone().into()))
                else {
                    return Ok(None);
                };
                let Some((owner, _)) = self.raw_owned_member_reference(index)? else {
                    return Ok(None);
                };
                return Ok(Some(OwnedResolvedMember {
                    binding_token: Some(self.session.token.clone()),
                    member_id: member.id,
                    file_id: member.file_id,
                    owner,
                    name: name.to_string(),
                    member_type: Some(member.typ),
                    visibility: None,
                    is_method: member.is_method,
                }));
            }
            if self.resolution_manages(prefix.get_syntax_id())?
                || ty.structural_type().is_unknown()
                || !ty.is_context_free()
            {
                self.resolution_unavailable(index, "managed_member_declaration_unavailable")?;
                return Ok(None);
            }
        }
        let Some((owner, name)) = self.raw_owned_member_reference(index)? else {
            return Ok(None);
        };
        let same_name = self
            .model
            .file_facts()
            .map(|facts| {
                facts
                    .members_of_owner_named(&owner, &name)
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let prefer_typed = same_name
            .iter()
            .all(|member| member.doc_type_syntax.is_none())
            && (self.owned_prefix_has_named_member(index, &name)?
                || self.owned_require_has_member(index, &name)?);
        if !prefer_typed
            && let Some(member) = same_name
                .iter()
                .find(|member| member.doc_type_syntax.is_some())
                .or_else(|| same_name.first())
        {
            return Ok(Some(self.assemble_owned_member(
                Some(member.id.clone()),
                Some(self.model.file_id()),
                owner,
                name,
            )));
        }
        if let Some(facts) = self.model.file_facts()
            && let Some(LuaExpr::NameExpr(prefix)) = index.get_prefix_expr()
            && let Some(text) = prefix.get_name_text()
            && let Some(def) = facts.type_def_by_name(&text)
            && let Some(member) = facts.field_members_of_type(&def.id, &name.clone().into())
        {
            return Ok(Some(self.assemble_owned_member(
                Some(member.id.clone()),
                Some(self.model.file_id()),
                owner,
                name,
            )));
        }
        let prefix = match index.get_prefix_expr() {
            Some(prefix) => Some(self.owned_prefix_resolution_type(&prefix)?),
            None => None,
        };
        if let Some(prefix) = &prefix {
            let id = match prefix.structural_type() {
                LuaType::Ref(id) | LuaType::Def(id) => Some(id),
                LuaType::Generic(generic) => Some(generic.get_base_type_id_ref()),
                LuaType::TplRef(tpl) => match tpl.get_constraint() {
                    Some(LuaType::Ref(id) | LuaType::Def(id)) => Some(id),
                    Some(LuaType::Generic(generic)) => Some(generic.get_base_type_id_ref()),
                    _ => None,
                },
                _ => None,
            };
            if let Some(def) = id.and_then(|id| self.model.type_def_of(id)) {
                let owners = if def.kind == TypeDefKind::Class {
                    self.type_runtime_owners(&def)?
                } else {
                    vec![def.id.clone()]
                };
                for candidate in owners {
                    if let Some(member) =
                        self.model.members_of_owner_named(&candidate, &name).first()
                    {
                        return Ok(Some(self.assemble_owned_member(
                            Some(member.id.clone()),
                            Some(member.file_id),
                            owner,
                            name,
                        )));
                    }
                    if let Some((member, file)) =
                        self.owned_implicit_self_member(&candidate, &name, false)?
                    {
                        return Ok(Some(self.assemble_owned_member(
                            Some(member),
                            file,
                            owner,
                            name,
                        )));
                    }
                }
                if def.kind == TypeDefKind::Alias
                    && let Some(info) =
                        self.member_info(prefix, &LuaMemberKey::Name(name.clone().into()))?
                    && let Some(member) = info.id
                {
                    let file = member_id_file(&member);
                    return Ok(Some(self.assemble_owned_member(
                        Some(member),
                        file,
                        owner,
                        name,
                    )));
                }
            }
        }
        if let Some(prefix) = &prefix
            && let LuaType::TableConst(table) = prefix.structural_type()
        {
            let mut owners = vec![SemanticId::member(table.file_id, table.value)];
            if let Some(decl) = self
                .model
                .file_facts_of(table.file_id)
                .and_then(|facts| facts.decl_by_value_range(table.value))
            {
                owners.push(decl.id.clone());
            }
            for candidate in owners {
                if let Some(member) = self.model.members_of_owner_named(&candidate, &name).first() {
                    return Ok(Some(self.assemble_owned_member(
                        Some(member.id.clone()),
                        Some(member.file_id),
                        owner,
                        name,
                    )));
                }
                if let Some((member, file)) =
                    self.owned_implicit_self_member(&candidate, &name, true)?
                {
                    return Ok(Some(self.assemble_owned_member(
                        Some(member),
                        file,
                        owner,
                        name,
                    )));
                }
            }
        }
        let old_owner = self.model.resolve_owner(&owner);
        let contribution_order = self.owner_contribution_order()?;
        let mut cross = self.owned_resolve_owner_ids(&owner, &contribution_order)?;
        push_resolution_owner(&mut cross, owner.clone());
        if let Some(module) = self.owned_require_owner(index)? {
            push_resolution_owner(&mut cross, module);
        }
        let mut best = None;
        let mut discovery = 0usize;
        for candidate in cross {
            let owner_rank = u8::from(old_owner.as_ref() == Some(&candidate));
            for member in self.model.members_of_owner(&candidate).iter() {
                self.checkpoint()?;
                if member.name.as_str() != name {
                    continue;
                }
                let mut visibility = 1u8;
                let mut documented = 1u8;
                if let Some(facts) = self
                    .model
                    .file_facts_of(member.file_id)
                    .and_then(|facts| facts.member_by_id(&member.id))
                {
                    visibility = u8::from(facts.visibility == VisibilityKind::Public);
                    documented = u8::from(facts.doc_type_syntax.is_none());
                }
                let rank = (owner_rank, visibility, documented, discovery);
                discovery += 1;
                if best.as_ref().is_none_or(|(previous, _)| rank < *previous) {
                    best = Some((rank, member.clone()));
                }
            }
        }
        if let Some((_, member)) = best {
            return Ok(Some(self.assemble_owned_member(
                Some(member.id),
                Some(member.file_id),
                owner,
                name,
            )));
        }
        if let Some(prefix) = &prefix {
            let id = match prefix.structural_type() {
                LuaType::Ref(id) | LuaType::Def(id) => Some(id),
                LuaType::Generic(generic) => Some(generic.get_base_type_id_ref()),
                _ => None,
            };
            if id
                .and_then(|id| self.model.type_def_of(id))
                .is_some_and(|def| def.kind == TypeDefKind::Class)
                && let Some(info) =
                    self.member_info(prefix, &LuaMemberKey::Name(name.clone().into()))?
                && let (Some(id), Some(file)) = (info.id, info.file_id)
            {
                return Ok(Some(self.assemble_owned_member(
                    Some(id),
                    Some(file),
                    owner,
                    name,
                )));
            }
        }
        Ok(Some(self.assemble_owned_member(None, None, owner, name)))
    }
    fn assemble_owned_member(
        &self,
        member_id: Option<SemanticId>,
        file_id: Option<FileId>,
        owner: SemanticId,
        name: String,
    ) -> OwnedResolvedMember {
        let member = member_id.as_ref().zip(file_id).and_then(|(id, file)| {
            self.model
                .file_facts_of(file)
                .and_then(|facts| facts.member_by_id(id).map(|member| (facts, member)))
        });
        let visibility = member.map(|(_, member)| member.visibility);
        let is_method = member
            .and_then(|(facts, member)| facts.signature_by_closure(member.value_syntax?))
            .is_some_and(|signature| signature.is_method);
        OwnedResolvedMember {
            binding_token: None,
            member_id,
            file_id,
            owner,
            name,
            member_type: None,
            visibility,
            is_method,
        }
    }
    fn resolution_manages(&mut self, syntax: LuaSyntaxId) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let managed = !matches!(
            self.session.context().resolve(self.site(syntax)),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        Ok(managed)
    }
    fn resolution_unavailable(
        &mut self,
        index: &LuaIndexExpr,
        reason: &'static str,
    ) -> QueryResult<(), E> {
        self.session.record(
            BindingCoverage {
                site: self.site(index.get_syntax_id()),
                operation: "resolve_member",
                state: "unavailable",
                proof: String::new(),
                reason: Some(reason.into()),
                parameter: None,
                parameter_index: None,
                argument: None,
            },
            || self.checkpoint(),
        )
    }
    fn raw_owned_member_reference(
        &mut self,
        index: &LuaIndexExpr,
    ) -> QueryResult<Option<(SemanticId, String)>, E> {
        self.checkpoint()?;
        let Some(key) = index.get_index_key() else {
            return Ok(None);
        };
        let Some(mut current) = index.get_prefix_expr() else {
            return Ok(None);
        };
        let mut segments = Vec::new();
        loop {
            self.checkpoint()?;
            current = match current {
                LuaExpr::ParenExpr(paren) => {
                    let Some(expr) = paren.get_expr() else {
                        return Ok(None);
                    };
                    expr
                }
                LuaExpr::IndexExpr(parent) => {
                    let Some(key) = parent.get_index_key() else {
                        return Ok(None);
                    };
                    segments.push(key.get_path_part());
                    let Some(prefix) = parent.get_prefix_expr() else {
                        return Ok(None);
                    };
                    prefix
                }
                other => {
                    current = other;
                    break;
                }
            };
        }
        let LuaExpr::NameExpr(name) = current else {
            return Ok(None);
        };
        let Some(text) = name.get_name_text() else {
            return Ok(None);
        };
        let Some(facts) = self.model.file_facts() else {
            return Ok(None);
        };
        let owner = if text != "_ENV"
            && text != "_G"
            && let Some(decl) = facts.find_visible_decl_before_offset(&text, name.get_position())
            && decl.kind != DeclKind::Global
        {
            decl.id.clone()
        } else {
            SemanticId::name(text.into())
        };
        let owner = if let SemanticId::Name(root) = &owner {
            let mut path = root.to_string();
            for segment in segments.iter().rev() {
                path.push('.');
                path.push_str(segment);
            }
            SemanticId::name(path.into())
        } else {
            owner
        };
        Ok(Some((owner, key.get_path_part().to_string())))
    }
    pub(super) fn owned_prefix_resolution_type(
        &mut self,
        expr: &LuaExpr,
    ) -> QueryResult<OwnedValue, E> {
        let projector = SelectedProjector::new(self.analysis, self.session);
        if let Some(value) = projector.project_selected(self.site(expr.get_syntax_id()), self)? {
            return Ok(value);
        }
        if let LuaExpr::NameExpr(name) = expr
            && let Some(decl) = self.model.resolve_name(name.get_position())
        {
            let mut value = self
                .type_of_decl(&decl)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            let parameter = self
                .model
                .file_facts()
                .and_then(|facts| {
                    facts
                        .decl_by_id(&decl)
                        .and_then(|decl| (decl.kind == DeclKind::Param).then_some(decl))
                })
                .and_then(|decl| {
                    self.model
                        .file_facts()?
                        .signature_and_param_index_of_decl(decl)
                })
                .map(|(signature, index)| (signature.closure_syntax, index));
            if let Some((closure, index)) = parameter {
                if value.structural_type().is_unknown()
                    && let Some(ty) = self.parameter_type(closure, index)?
                {
                    value = ty;
                }
                return self.attach_generic_constraints(value, closure);
            }
            return Ok(value);
        }
        self.type_of_expr(expr.get_syntax_id())
    }
    fn owned_prefix_has_named_member(
        &mut self,
        index: &LuaIndexExpr,
        name: &str,
    ) -> QueryResult<bool, E> {
        let Some(prefix) = index.get_prefix_expr() else {
            return Ok(false);
        };
        let prefix = self.owned_prefix_resolution_type(&prefix)?;
        let id = match prefix.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => Some(id),
            LuaType::Generic(generic) => Some(generic.get_base_type_id_ref()),
            _ => None,
        };
        Ok(id
            .and_then(|id| self.model.type_def_of(id))
            .is_some_and(|def| {
                self.model
                    .members_of_owner(&def.id)
                    .iter()
                    .any(|member| member.name.as_str() == name)
            }))
    }
    fn owned_implicit_self_member(
        &mut self,
        owner: &SemanticId,
        name: &str,
        require_method: bool,
    ) -> QueryResult<Option<(SemanticId, Option<FileId>)>, E> {
        for method in self.model.members_of_owner(owner).iter() {
            self.checkpoint()?;
            let Some(facts) = self.model.file_facts_of(method.file_id) else {
                continue;
            };
            let Some(method) = facts.member_by_id(&method.id) else {
                continue;
            };
            if require_method && !method.is_method {
                continue;
            }
            let Some(closure) = method.value_syntax else {
                continue;
            };
            let Some(self_decl) = facts
                .decls
                .iter()
                .find(|decl| decl.name == "self" && decl.owner_syntax == Some(closure))
            else {
                continue;
            };
            if let Some(member) = facts.members_of_owner_named(&self_decl.id, name).next() {
                return Ok(Some((member.id.clone(), member_id_file(&member.id))));
            }
        }
        Ok(None)
    }
    fn owned_require_has_member(
        &mut self,
        index: &LuaIndexExpr,
        name: &str,
    ) -> QueryResult<bool, E> {
        Ok(self.owned_require_owner(index)?.is_some_and(|owner| {
            self.model
                .members_of_owner(&owner)
                .iter()
                .any(|member| member.name.as_str() == name)
        }))
    }
    fn owned_require_owner(&mut self, index: &LuaIndexExpr) -> QueryResult<Option<SemanticId>, E> {
        let Some(prefix) = index.get_prefix_expr() else {
            return Ok(None);
        };
        self.owned_require_member_owner(&prefix, 0)
    }
    fn owned_require_member_owner(
        &mut self,
        expr: &LuaExpr,
        depth: usize,
    ) -> QueryResult<Option<SemanticId>, E> {
        self.checkpoint()?;
        if depth > 8 {
            return Ok(None);
        }
        match expr {
            LuaExpr::ParenExpr(paren) => match paren.get_expr() {
                Some(expr) => self.owned_require_member_owner(&expr, depth + 1),
                None => Ok(None),
            },
            LuaExpr::IndexExpr(index) => {
                let Some(prefix) = index.get_prefix_expr() else {
                    return Ok(None);
                };
                let Some(parent) = self.owned_require_member_owner(&prefix, depth + 1)? else {
                    return Ok(None);
                };
                let Some(key) = index.get_index_key() else {
                    return Ok(None);
                };
                let members = self
                    .model
                    .members_of_owner_named(&parent, &key.get_path_part());
                let Some(member) = members.first() else {
                    return Ok(None);
                };
                let value = self
                    .model
                    .file_facts_of(member.file_id)
                    .and_then(|facts| facts.member_by_id(&member.id))
                    .and_then(|member| member.value_syntax)
                    .and_then(|syntax| {
                        self.model
                            .syntax_tree_of(member.file_id)
                            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                    })
                    .and_then(LuaExpr::cast);
                let owner = match value {
                    Some(LuaExpr::TableExpr(table)) => {
                        Some(SemanticId::member(member.file_id, table.get_range()))
                    }
                    Some(LuaExpr::NameExpr(name)) => name.get_name_text().and_then(|text| {
                        self.model
                            .file_facts_of(member.file_id)?
                            .find_visible_decl_before_offset(&text, name.get_position())
                            .map(|decl| decl.id.clone())
                    }),
                    _ => None,
                };
                Ok(Some(owner.unwrap_or_else(|| member.id.clone())))
            }
            _ => Ok(self
                .owned_require_file(expr, 0)?
                .and_then(|file| self.resolution_module_owner(file))),
        }
    }
    fn owned_require_file(
        &mut self,
        expr: &LuaExpr,
        depth: usize,
    ) -> QueryResult<Option<FileId>, E> {
        self.checkpoint()?;
        if depth > 8 {
            return Ok(None);
        }
        match expr {
            LuaExpr::ParenExpr(paren) => match paren.get_expr() {
                Some(expr) => self.owned_require_file(&expr, depth + 1),
                None => Ok(None),
            },
            LuaExpr::NameExpr(name) => {
                let value = self
                    .model
                    .resolve_name(name.get_position())
                    .and_then(|id| self.model.file_facts()?.decl_by_id(&id)?.value_expr_syntax)
                    .and_then(|syntax| {
                        self.model
                            .syntax_tree()
                            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                    })
                    .and_then(LuaExpr::cast);
                match value {
                    Some(expr) => self.owned_require_file(&expr, depth + 1),
                    None => Ok(None),
                }
            }
            LuaExpr::CallExpr(call) => {
                if !matches!(call.get_prefix_expr(), Some(LuaExpr::NameExpr(name)) if name.get_name_text().as_deref() == Some("require"))
                {
                    return Ok(None);
                }
                let Some(arg) = call.get_args_list().and_then(|args| args.get_args().next()) else {
                    return Ok(None);
                };
                let value = self.type_of_expr(arg.get_syntax_id())?;
                Ok(match value.structural_type() {
                    LuaType::StringConst(name) | LuaType::DocStringConst(name) => {
                        self.model.module_file_of(name.as_str())
                    }
                    _ => None,
                })
            }
            _ => Ok(None),
        }
    }
    fn resolution_module_owner(&self, file: FileId) -> Option<SemanticId> {
        match &self.model.file_facts_of(file)?.module_export {
            ModuleExport::Decl { decl, .. } => Some(decl.clone()),
            ModuleExport::Expr { value_syntax } => {
                Some(SemanticId::member(file, value_syntax.get_range()))
            }
            _ => None,
        }
    }
    fn resolution_canonical_identity(
        &mut self,
        owner: &SemanticId,
    ) -> QueryResult<Option<OwnerId>, E> {
        self.checkpoint()?;
        let file = match owner {
            SemanticId::Name(name) => return Ok(Some(OwnerId::Global(name.as_str().into()))),
            SemanticId::TypeDef(key) => {
                return Ok(Some(OwnerId::Type(key.scope, key.full_name.clone())));
            }
            SemanticId::Signature(_) => return Ok(Some(OwnerId::Concrete(owner.clone()))),
            SemanticId::Decl(key) => key.file_id,
            SemanticId::Member(key) => key.file_id,
        };
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(None);
        };
        let own_export = match &facts.module_export {
            ModuleExport::Decl { decl, .. } => decl == owner,
            ModuleExport::Expr { value_syntax } => {
                matches!(owner, SemanticId::Member(key) if key.key_range == value_syntax.get_range())
            }
            _ => false,
        };
        if own_export {
            return Ok(Some(OwnerId::Module(file)));
        }
        if let Some(alias) = self
            .model
            .file_exports(file)
            .and_then(|exports| exports.aliases.iter().find(|alias| &alias.decl == owner))
        {
            return Ok(Some(alias.owner_id()));
        }
        Ok(Some(match owner {
            SemanticId::Decl(key) => match facts.decl_by_id(owner) {
                Some(decl) if decl.kind == DeclKind::Global => OwnerId::Global(decl.name.clone()),
                _ => OwnerId::Local(key.file_id, key.name_range),
            },
            SemanticId::Member(key) => OwnerId::Table(key.file_id, key.key_range),
            _ => unreachable!("identity handled above"),
        }))
    }
    fn resolution_identity_to_raw(&self, owner: &OwnerId) -> Option<SemanticId> {
        match owner {
            OwnerId::Type(scope, name) => self
                .model
                .type_defs_in_scope(*scope, name.as_str())
                .first()
                .map(|def| def.id.clone()),
            OwnerId::Global(name) => Some(
                self.model
                    .global_decl(name.as_str())
                    .unwrap_or_else(|| SemanticId::name(name.clone())),
            ),
            OwnerId::Module(file) => self.resolution_module_owner(*file),
            OwnerId::Local(file, range) => Some(SemanticId::decl(*file, *range)),
            OwnerId::Table(file, range) => Some(SemanticId::member(*file, *range)),
            OwnerId::Concrete(id) => Some(id.clone()),
        }
    }
    pub(super) fn owned_resolve_owner_ids(
        &mut self,
        owner: &SemanticId,
        contribution_order: &Arc<[FileId]>,
    ) -> QueryResult<Vec<SemanticId>, E> {
        let mut out = Vec::new();
        for id in self.owned_owner_identity_set(owner, contribution_order)? {
            self.checkpoint()?;
            if let Some(raw) = self.resolution_identity_to_raw(&id) {
                push_resolution_owner(&mut out, raw);
            }
        }
        Ok(out)
    }
    fn owned_owner_identity_set(
        &mut self,
        owner: &SemanticId,
        order: &Arc<[FileId]>,
    ) -> QueryResult<Vec<OwnerId>, E> {
        self.checkpoint()?;
        let mut out = Vec::new();
        if let Some(identity) = self.resolution_canonical_identity(owner)? {
            push_owner_identity(&mut out, identity);
        }
        match owner {
            SemanticId::Name(name) => {
                for def in self
                    .model
                    .type_defs_in_scope(TypeScope::Global, name.as_str())
                    .iter()
                {
                    if let SemanticId::TypeDef(key) = &def.id {
                        push_owner_identity(
                            &mut out,
                            OwnerId::Type(key.scope, key.full_name.clone()),
                        );
                    }
                    self.push_owned_type_identities(&def.id, order, &mut out)?;
                }
                if let Some(global) = self.model.global_decl(name.as_str())
                    && let Some(identity) = self.resolution_canonical_identity(&global)?
                {
                    push_owner_identity(&mut out, identity);
                }
                for runtime in self.owned_runtime_declarations_named(name.as_str(), order)? {
                    if let Some(identity) = self.resolution_canonical_identity(&runtime)? {
                        push_owner_identity(&mut out, identity);
                    }
                    if let SemanticId::Decl(key) = &runtime
                        && let Some(facts) = self.model.file_facts_of(key.file_id)
                        && let Some(decl) = facts.decl_by_id(&runtime)
                        && let Some(syntax) = decl.owner_syntax
                    {
                        for def in facts.type_defs_by_owner_syntax(syntax).filter(|def| {
                            matches!(def.kind, TypeDefKind::Class | TypeDefKind::Enum)
                        }) {
                            if let SemanticId::TypeDef(key) = &def.id {
                                push_owner_identity(
                                    &mut out,
                                    OwnerId::Type(key.scope, key.full_name.clone()),
                                );
                            }
                        }
                    }
                }
                if let Some(dot) = name.rfind('.') {
                    let head = SemanticId::name(name[..dot].into());
                    let tail = &name[dot + 1..];
                    for head in self.owned_owner_identity_set(&head, order)? {
                        // Canonical buckets differ from raw public owner buckets,
                        // especially Global(Name) and require-alias module owners.
                        let mut seen = Vec::new();
                        for &file in order.iter() {
                            self.checkpoint()?;
                            if let Some(exports) = self.model.file_exports(file) {
                                for member in &exports.members {
                                    self.checkpoint()?;
                                    if member.owner_id == head
                                        && member.key.to_path() == tail
                                        && !seen.contains(&(member.file_id, member.member.clone()))
                                    {
                                        seen.push((member.file_id, member.member.clone()));
                                        if let Some(identity) =
                                            self.resolution_canonical_identity(&member.member)?
                                        {
                                            push_owner_identity(&mut out, identity);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            SemanticId::TypeDef(key) => {
                push_owner_identity(&mut out, OwnerId::Type(key.scope, key.full_name.clone()));
                self.push_owned_type_identities(owner, order, &mut out)?;
            }
            SemanticId::Decl(key) => {
                if let Some(facts) = self.model.file_facts_of(key.file_id)
                    && let Some(decl) = facts.decl_by_id(owner)
                    && let Some(syntax) = decl.owner_syntax
                {
                    for def in facts
                        .type_defs_by_owner_syntax(syntax)
                        .filter(|def| matches!(def.kind, TypeDefKind::Class | TypeDefKind::Enum))
                    {
                        if let SemanticId::TypeDef(key) = &def.id {
                            push_owner_identity(
                                &mut out,
                                OwnerId::Type(key.scope, key.full_name.clone()),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
        Ok(out)
    }
    fn push_owned_type_identities(
        &mut self,
        id: &SemanticId,
        order: &Arc<[FileId]>,
        out: &mut Vec<OwnerId>,
    ) -> QueryResult<(), E> {
        for &file in order.iter() {
            self.checkpoint()?;
            let Some(exports) = self.model.file_exports(file) else {
                continue;
            };
            let Some(def) = exports.types.iter().find(|def| &def.id == id) else {
                continue;
            };
            if let Some((_, runtime)) = exports
                .runtime_values
                .iter()
                .find(|(name, _)| name == &def.name)
            {
                if let Some(identity) = self.resolution_canonical_identity(runtime)? {
                    push_owner_identity(out, identity);
                }
                if let SemanticId::Decl(key) = runtime
                    && let Some(facts) = self.model.file_facts_of(key.file_id)
                    && let Some(decl) = facts.decl_by_id(runtime)
                    && decl.kind == DeclKind::Global
                    && facts.is_meta
                    && self.analysis.db.is_main_file(key.file_id)
                {
                    push_owner_identity(
                        out,
                        OwnerId::Concrete(SemanticId::name(decl.name.clone())),
                    );
                }
            }
            if let Some(facts) = self.model.file_facts_of(def.file_id)
                && let Some(def) = facts.type_def_by_id(id)
                && let Some(syntax) = def.owner_syntax
            {
                for decl in facts.decls_by_owner_syntax(syntax) {
                    if let Some(identity) = self.resolution_canonical_identity(&decl.id)? {
                        push_owner_identity(out, identity);
                    }
                }
            }
            break;
        }
        Ok(())
    }
    fn owned_runtime_declarations_named(
        &mut self,
        name: &str,
        contribution_order: &Arc<[FileId]>,
    ) -> QueryResult<Vec<SemanticId>, E> {
        // The admitted snapshot supplies the original declaration-contribution
        // order, already grouped by upstream workspace lookup priority. This
        // consumer cannot infer ingestion history from an arbitrary Analysis.
        let mut declarations = Vec::new();
        for &file in contribution_order.iter() {
            self.checkpoint()?;
            if let Some(exports) = self.model.file_exports(file) {
                for (runtime_name, runtime) in &exports.runtime_values {
                    self.checkpoint()?;
                    if runtime_name.as_str() == name {
                        declarations.push(runtime.clone());
                    }
                }
            }
        }
        Ok(declarations)
    }
}
fn push_resolution_owner(out: &mut Vec<SemanticId>, owner: SemanticId) {
    if !out.contains(&owner) {
        out.push(owner);
    }
}
fn member_id_file(id: &SemanticId) -> Option<FileId> {
    match id {
        SemanticId::Member(key) => Some(key.file_id),
        _ => None,
    }
}

fn push_owner_identity(out: &mut Vec<OwnerId>, id: OwnerId) {
    if !out.contains(&id) {
        out.push(id);
    }
}
