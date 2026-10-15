use super::*;
use crate::session::BindingDecision;
use emmylua_code_analysis::{
    DocGenericParam, FileId, GenericTpl, LuaIntersectionType, LuaStringTplType, LuaTypeDeclId,
};
use emmylua_parser::{LuaDocType, LuaTypeBinaryOperator, LuaTypeUnaryOperator};
use std::{collections::HashMap, sync::Arc};
pub(in crate::session::backend) type OwnerConstraints = HashMap<String, (usize, OwnedValue)>;
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn access_owner_constraints(
        &mut self,
        callee: &LuaExpr,
    ) -> QueryResult<OwnerConstraints, E> {
        self.checkpoint()?;
        let managed = !matches!(
            self.session
                .context()
                .resolve(self.site(callee.get_syntax_id())),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        let mut out = HashMap::new();
        if managed {
            return Ok(out);
        }
        let Some(index) = LuaIndexExpr::cast(callee.syntax().clone()) else {
            return Ok(out);
        };
        let Some(resolved) = self.resolve_owned_member(&index)? else {
            return Ok(out);
        };
        let (Some(id), Some(file)) = (resolved.member_id, resolved.file_id) else {
            return Ok(out);
        };
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(out);
        };
        let Some(member) = facts.member_by_id(&id) else {
            return Ok(out);
        };
        let def = match &member.owner {
            SemanticId::TypeDef(_) => facts.type_def_by_id(&member.owner),
            SemanticId::Decl(_) => facts
                .decl_by_id(&member.owner)
                .and_then(|d| d.owner_syntax)
                .and_then(|s| facts.type_def_by_owner_syntax(s)),
            _ => None,
        };
        let Some(def) = def else { return Ok(out) };
        for (index, param) in def.generic_params.iter().enumerate() {
            self.checkpoint()?;
            if let Some(syntax) = param.constraint {
                let ty = self.constraint_doc_type(
                    def.file_id,
                    syntax,
                    &def.generic_params,
                    &HashMap::new(),
                )?;
                out.insert(param.name.to_string(), (index, ty));
            }
        }
        Ok(out)
    }
    pub(super) fn access_apply_owner_generics(
        &mut self,
        function: &OwnedValue,
        constraints: &OwnerConstraints,
    ) -> QueryResult<OwnedValue, E> {
        if constraints.is_empty() {
            return Ok(function.clone());
        }
        let LuaType::DocFunction(meta) = function.structural_type() else {
            unreachable!("function")
        };
        let mut params = Vec::new();
        for param in function.function_parameters().expect("parameters") {
            params.push(match param {
                Some(p) => Some(self.access_bind_owner_generic(p, constraints)?),
                None => None,
            });
        }
        Ok(OwnedValue::function(
            meta,
            params,
            function.function_return().expect("return"),
        ))
    }
    fn access_bind_owner_generic(
        &mut self,
        value: OwnedValue,
        constraints: &OwnerConstraints,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        match value.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => Ok(constraints
                .get(id.get_name())
                .map(|(index, constraint)| {
                    OwnedValue::plain(LuaType::TplRef(Arc::new(GenericTpl::new(
                        GenericTplId::Type(*index as u32),
                        id.get_name().into(),
                        Some(constraint.structural_type().clone()),
                        None,
                        false,
                        None,
                    ))))
                })
                .unwrap_or(value)),
            LuaType::StrTplRef(tpl) => Ok(constraints
                .get(tpl.get_name())
                .map(|(_, constraint)| {
                    OwnedValue::plain(LuaType::StrTplRef(Arc::new(LuaStringTplType::new(
                        tpl.get_prefix(),
                        tpl.get_name(),
                        tpl.get_tpl_id(),
                        tpl.get_suffix(),
                        Some(constraint.structural_type().clone()),
                    ))))
                })
                .unwrap_or(value)),
            LuaType::Union(_) => {
                let mut members = Vec::new();
                for member in value.union_components().expect("union") {
                    members.push(self.access_bind_owner_generic(member, constraints)?);
                }
                Ok(OwnedValue::raw_union(members))
            }
            _ => Ok(value),
        }
    }
    pub(in crate::session::backend) fn constraint_doc_type(
        &mut self,
        file: FileId,
        syntax: LuaSyntaxId,
        generics: &[DocGenericParam],
        substitutions: &HashMap<String, OwnedValue>,
    ) -> QueryResult<OwnedValue, E> {
        self.checkpoint()?;
        let doc = self
            .model
            .syntax_tree_of(file)
            .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaDocType::cast);
        let fallback = |query: &mut Self| {
            query.checkpoint()?;
            Ok(OwnedValue::plain(
                query.model.doc_type_lua_rich_in(file, syntax),
            ))
        };
        let Some(doc) = doc else {
            return fallback(self);
        };
        match doc {
            LuaDocType::Name(name) => {
                let Some(name) = name.get_name_text() else {
                    return fallback(self);
                };
                if let Some(bound) = substitutions.get(&name) {
                    return Ok(bound.clone());
                }
                if let Some(index) = generics.iter().position(|g| g.name == name) {
                    return Ok(OwnedValue::plain(LuaType::TplRef(Arc::new(
                        GenericTpl::new(
                            GenericTplId::Type(index as u32),
                            name.into(),
                            None,
                            None,
                            false,
                            None,
                        ),
                    ))));
                }
                fallback(self)
            }
            LuaDocType::Unary(unary) => {
                if !unary
                    .get_op_token()
                    .is_some_and(|op| op.get_op() == LuaTypeUnaryOperator::Keyof)
                {
                    return fallback(self);
                }
                let Some(LuaDocType::Name(name)) = unary.get_type() else {
                    return fallback(self);
                };
                let Some(name) = name.get_name_text() else {
                    return fallback(self);
                };
                if let Some(bound) = substitutions.get(&name) {
                    let (LuaType::Ref(id) | LuaType::Def(id)) = bound.structural_type() else {
                        return Ok(OwnedValue::plain(LuaType::Unknown));
                    };
                    return match self.model.type_def_of(id) {
                        Some(def) => self.constraint_keyof(&def),
                        None => Ok(OwnedValue::plain(LuaType::Unknown)),
                    };
                }
                if let Some(index) = generics.iter().position(|g| g.name == name) {
                    return Ok(OwnedValue::plain(LuaType::TplRef(Arc::new(
                        GenericTpl::new(
                            GenericTplId::Type(index as u32),
                            name.into(),
                            None,
                            None,
                            false,
                            None,
                        ),
                    ))));
                }
                match self.model.resolve_type_def_in(file, &name) {
                    Some(def) => self.constraint_keyof(&def),
                    None => fallback(self),
                }
            }
            LuaDocType::Binary(binary) => {
                let Some((left, right)) = binary.get_types() else {
                    return fallback(self);
                };
                let left =
                    self.constraint_doc_type(file, left.get_syntax_id(), generics, substitutions)?;
                let right =
                    self.constraint_doc_type(file, right.get_syntax_id(), generics, substitutions)?;
                match binary.get_op_token().map(|op| op.get_op()) {
                    Some(LuaTypeBinaryOperator::Intersection) => {
                        Ok(OwnedValue::with_structural_children(
                            LuaType::Intersection(Arc::new(LuaIntersectionType::new(vec![
                                left.structural_type().clone(),
                                right.structural_type().clone(),
                            ]))),
                            vec![right, left],
                        ))
                    }
                    Some(LuaTypeBinaryOperator::Union) => {
                        let mut types = Vec::new();
                        for ty in [left, right] {
                            types.extend(ty.union_components().unwrap_or_else(|| vec![ty]));
                        }
                        Ok(OwnedValue::raw_union(types))
                    }
                    _ => fallback(self),
                }
            }
            LuaDocType::Conditional(conditional) => {
                let Some((condition, yes, no)) = conditional.get_types() else {
                    return fallback(self);
                };
                let mut take_true = false;
                if let LuaDocType::Binary(binary) = condition
                    && let Some((left, right)) = binary.get_types()
                    && binary
                        .get_op_token()
                        .is_some_and(|op| op.get_op() == LuaTypeBinaryOperator::Extends)
                {
                    let left = self.constraint_doc_type(
                        file,
                        left.get_syntax_id(),
                        generics,
                        substitutions,
                    )?;
                    let right = self.constraint_doc_type(
                        file,
                        right.get_syntax_id(),
                        generics,
                        substitutions,
                    )?;
                    take_true = self.constraint_compatible_owned(&left, &right)?;
                }
                self.constraint_doc_type(
                    file,
                    if take_true { yes } else { no }.get_syntax_id(),
                    generics,
                    substitutions,
                )
            }
            _ => fallback(self),
        }
    }
    fn constraint_keyof(&mut self, def: &TypeDef) -> QueryResult<OwnedValue, E> {
        let mut keys = Vec::new();
        self.constraint_key_names(def, &mut keys, &mut Vec::new())?;
        Ok(if keys.is_empty() {
            OwnedValue::plain(LuaType::Unknown)
        } else {
            OwnedValue::raw_union(
                keys.into_iter()
                    .map(|key| {
                        OwnedValue::plain(LuaType::StringConst(smol_str::SmolStr::new(key).into()))
                    })
                    .collect(),
            )
        })
    }
    fn constraint_key_names(
        &mut self,
        def: &TypeDef,
        keys: &mut Vec<String>,
        visited: &mut Vec<String>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if visited.iter().any(|n| n == def.full_name.as_str()) {
            return Ok(());
        }
        visited.push(def.full_name.to_string());
        for member in self.model.members_of_owner(&def.id) {
            self.checkpoint()?;
            if !keys.iter().any(|k| k == member.name.as_str()) {
                keys.push(member.name.to_string());
            }
        }
        for name in &def.super_names {
            if let Some(def) = self.model.resolve_type_def_in(def.file_id, name) {
                self.constraint_key_names(&def, keys, visited)?;
            }
        }
        Ok(())
    }
    pub(in crate::session::backend) fn constraint_compatible_owned(
        &mut self,
        actual: &OwnedValue,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if actual == target {
            return Ok(true);
        }
        if let (LuaType::StringConst(a), LuaType::StringConst(b)) =
            (actual.structural_type(), target.structural_type())
        {
            return Ok(a == b);
        }
        if let Some(members) = actual.union_components() {
            for member in members {
                if !self.constraint_compatible_owned(&member, target)? {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        if let Some(members) = target.union_components() {
            for member in members {
                if self.constraint_compatible_owned(actual, &member)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        self.type_check(actual, target)
    }
}
