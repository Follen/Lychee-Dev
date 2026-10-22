//! Expected method signatures retain the original inline-doc / own-overload /
//! owner-field producer priority, including repeated-field last-wins behavior.
use super::*;
use emmylua_code_analysis::{LuaTypeDeclId, SemanticId, TypeDefKind, TypeScope};

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn expected_member_param_for_closure(
        &mut self,
        closure: LuaSyntaxId,
        index: usize,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let is_method = self
            .model
            .file_facts()
            .and_then(|facts| facts.signature_by_closure(closure))
            .is_some_and(|signature| signature.is_method);
        let Some(functions) = self.expected_member_signatures_for_closure(closure)? else {
            return Ok(None);
        };
        let mut types = Vec::new();
        for function in functions {
            self.checkpoint()?;
            let LuaType::DocFunction(metadata) = function.structural_type() else {
                continue;
            };
            let has_self = metadata
                .get_params()
                .first()
                .is_some_and(|(name, ty)| name == "self" || matches!(ty, Some(LuaType::SelfInfer)));
            let expected = index + usize::from(is_method && has_self);
            if let Some(value) = function
                .function_parameters()
                .and_then(|params| params.get(expected).cloned().flatten())
                && !types.contains(&value)
            {
                types.push(value);
            }
        }
        if types.is_empty() {
            Ok(None)
        } else {
            OwnedValue::union_checked(types, || self.checkpoint()).map(Some)
        }
    }

    pub(in crate::session::backend) fn expected_member_signatures_for_closure(
        &mut self,
        closure: LuaSyntaxId,
    ) -> QueryResult<Option<Vec<OwnedValue>>, E> {
        self.checkpoint()?;
        let Some(facts) = self.model.file_facts() else {
            return Ok(None);
        };
        let Some(member) = facts
            .members
            .iter()
            .find(|member| member.value_syntax == Some(closure))
        else {
            return Ok(None);
        };
        let file = self.model.file_id();
        if let Some(syntax) = member.doc_type_syntax {
            let doc = OwnedValue::plain(self.model.doc_type_lua_rich_in(file, syntax));
            let mut doc = self.expand_aliases(doc)?;
            let mut visited = Vec::new();
            loop {
                self.checkpoint()?;
                let (LuaType::Ref(id) | LuaType::Def(id)) = doc.structural_type() else {
                    break;
                };
                let id = id.clone();
                if visited.contains(&id) {
                    break;
                }
                let Some(def) = self.model.type_def_of(&id) else {
                    break;
                };
                if def.kind != TypeDefKind::Alias {
                    break;
                }
                let Some(target) = self.model.alias_target(&def) else {
                    break;
                };
                visited.push(id);
                doc = self.expand_aliases(OwnedValue::plain(target))?;
            }
            let functions = self.signature_candidates(doc)?;
            if !functions.is_empty() {
                return Ok(Some(functions));
            }
        }
        if let Some(docs) = facts
            .signature_by_closure(closure)
            .and_then(|signature| signature.docs.as_ref())
            .filter(|docs| !docs.overloads.is_empty())
        {
            let mut functions = Vec::new();
            for syntax in &docs.overloads {
                self.checkpoint()?;
                let ty = self.model.doc_type_lua_rich_in(file, *syntax);
                self.checkpoint()?;
                if matches!(ty, LuaType::DocFunction(_)) {
                    functions.push(OwnedValue::plain(ty));
                }
            }
            if !functions.is_empty() {
                return Ok(Some(functions));
            }
        }
        let owner = member.owner.clone();
        let key = member.key.clone();
        let owner_type = match &owner {
            SemanticId::Decl(id) => {
                let Some(mut value) = self.type_of_decl(&owner)? else {
                    return Ok(None);
                };
                if !matches!(
                    value.structural_type(),
                    LuaType::Ref(_) | LuaType::Def(_) | LuaType::Generic(_)
                ) && let Some(facts) = self.model.file_facts_of(id.file_id)
                    && let Some(info) = facts.decl_by_id(&owner)
                    && let Some(def) = info
                        .owner_syntax
                        .and_then(|syntax| facts.type_def_by_owner_syntax(syntax))
                {
                    value = OwnedValue::plain(self.model.type_def_ref(def));
                }
                value
            }
            SemanticId::TypeDef(_) => {
                let Some(value) = self.signature_owner_type(&owner)? else {
                    return Ok(None);
                };
                value
            }
            SemanticId::Name(_) => {
                let Some(owner) = self.model.resolve_owner(&owner) else {
                    return Ok(None);
                };
                let Some(value) = self.signature_owner_type(&owner)? else {
                    return Ok(None);
                };
                value
            }
            SemanticId::Member(table) => {
                let Some(decl) = facts.decl_by_value_range(table.key_range) else {
                    return Ok(None);
                };
                let Some(value) = self.type_of_decl(&decl.id)? else {
                    return Ok(None);
                };
                value
            }
            _ => return Ok(None),
        };
        let infos = self.member_infos_with_key_all(&owner_type, &key)?;
        let mut out = Vec::new();
        for info in infos {
            self.checkpoint()?;
            let value = match info.typ.structural_type() {
                LuaType::Ref(id) | LuaType::Def(id)
                    if self
                        .model
                        .type_def_of(id)
                        .is_some_and(|def| def.kind == TypeDefKind::Alias) =>
                {
                    let Some(def) = self.model.type_def_of(id) else {
                        return Ok(None);
                    };
                    match self.model.alias_target(&def) {
                        Some(target) => self.expand_aliases(OwnedValue::plain(target))?,
                        None => info.typ,
                    }
                }
                _ => self.expand_aliases(info.typ)?,
            };
            out.extend(self.signature_candidates(value)?);
        }
        if out.len() > 1 {
            out = vec![out.pop().expect("nonempty overload candidates")];
        }
        Ok(Some(out))
    }

    fn signature_candidates(&mut self, value: OwnedValue) -> QueryResult<Vec<OwnedValue>, E> {
        self.checkpoint()?;
        let mut out = Vec::new();
        if matches!(value.structural_type(), LuaType::DocFunction(_)) {
            out.push(value);
        } else if let Some(parts) = value.union_components() {
            for part in parts {
                self.checkpoint()?;
                if matches!(part.structural_type(), LuaType::DocFunction(_)) {
                    out.push(part);
                }
            }
        }
        Ok(out)
    }

    fn signature_owner_type(&mut self, owner: &SemanticId) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        match owner {
            SemanticId::Decl(_) => self.type_of_decl(owner),
            SemanticId::TypeDef(def) => {
                let id = match &def.scope {
                    TypeScope::Global => LuaTypeDeclId::global(&def.full_name),
                    TypeScope::Internal(workspace) => {
                        LuaTypeDeclId::internal(*workspace, &def.full_name)
                    }
                    TypeScope::File(file) => LuaTypeDeclId::file(*file, &def.full_name),
                };
                Ok(self
                    .model
                    .type_def_of(&id)
                    .map(|def| OwnedValue::plain(self.model.type_def_ref(&def))))
            }
            _ => Ok(None),
        }
    }

    pub(in crate::session::backend) fn method_owner_type(
        &mut self,
        closure: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        let Some(member) = self
            .model
            .file_facts()
            .and_then(|facts| facts.member_by_value_syntax(closure))
        else {
            return Ok(None);
        };
        let mut owner = member.owner.clone();
        if matches!(owner, SemanticId::Name(_)) {
            let Some(resolved) = self.model.resolve_owner(&owner) else {
                return Ok(None);
            };
            owner = resolved;
        }
        let Some(mut value) = self.signature_owner_type(&owner)? else {
            return Ok(None);
        };
        if let SemanticId::Decl(id) = &owner
            && let Some(facts) = self.model.file_facts_of(id.file_id)
            && let Some(decl) = facts.decl_by_id(&owner)
            && let Some(def) = facts
                .type_defs
                .iter()
                .find(|def| def.owner_syntax.is_some() && def.owner_syntax == decl.owner_syntax)
        {
            value = OwnedValue::plain(self.model.type_def_ref(def));
        }
        self.checkpoint()?;
        Ok(Some(value))
    }
}
