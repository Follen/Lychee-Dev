//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! check/checker/check_field.rs; Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DiagnosticCode, LuaMemberKey, LuaType, SemanticId, SemanticModel, TypeDef, TypeDefKind,
    TypeScope, humanize_semantic_type,
};
use emmylua_parser::{
    LuaAssignStat, LuaAst, LuaAstNode, LuaAstToken, LuaCallExpr, LuaExpr, LuaLocalStat,
    LuaTableExpr, LuaVarExpr,
};

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_field(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for node in root.descendants().filter_map(LuaAst::cast) {
            self.checkpoint()?;
            match node {
                LuaAst::LuaLocalStat(stat) => self.field_local(context, &stat)?,
                LuaAst::LuaAssignStat(stat) => {
                    self.field_assign(context, &stat)?;
                    self.field_index_assign(context, &stat)?;
                }
                LuaAst::LuaCallExpr(call) => self.field_call_args(context, &call)?,
                _ => {}
            }
        }
        Ok(())
    }
    fn field_local(
        &mut self,
        context: &mut CheckContext<'_>,
        stat: &LuaLocalStat,
    ) -> QueryResult<(), E> {
        let names = stat.get_local_name_list().collect::<Vec<_>>();
        let values = stat.get_value_exprs().collect::<Vec<_>>();
        for (index, name) in names.iter().enumerate() {
            let Some(LuaExpr::TableExpr(table)) = values.get(index) else {
                continue;
            };
            let Some(decl) = self.model.decl_by_offset(name.get_range().start()) else {
                continue;
            };
            self.field_against(context, &decl, table)?;
        }
        Ok(())
    }
    fn field_assign(
        &mut self,
        context: &mut CheckContext<'_>,
        stat: &LuaAssignStat,
    ) -> QueryResult<(), E> {
        let (vars, values) = stat.get_var_and_expr_list();
        for (var, expr) in vars.iter().zip(values.iter()) {
            let LuaExpr::TableExpr(table) = expr else {
                continue;
            };
            let LuaVarExpr::NameExpr(name) = var else {
                continue;
            };
            let Some(decl) = self.model.resolve_name(name.get_position()) else {
                continue;
            };
            self.field_against(context, &decl, table)?;
        }
        Ok(())
    }
    fn field_index_assign(
        &mut self,
        context: &mut CheckContext<'_>,
        stat: &LuaAssignStat,
    ) -> QueryResult<(), E> {
        let (vars, _) = stat.get_var_and_expr_list();
        for var in vars {
            let LuaVarExpr::IndexExpr(index) = var else {
                continue;
            };
            let Some(prefix) = index.get_prefix_expr() else {
                continue;
            };
            let prefix_ty = self.type_of_expr(prefix.get_syntax_id())?;
            let Some(index_key) = index.get_index_key() else {
                continue;
            };
            let Some(key) = self.export_member_key(&index_key)? else {
                continue;
            };
            let prefix_ty = self.field_expand_alias(&prefix_ty).unwrap_or(prefix_ty);
            if let LuaType::Object(object) = prefix_ty.structural_type() {
                if object.get_field(&key).is_none()
                    && !object
                        .get_index_access()
                        .iter()
                        .any(|(key_ty, _)| index_key_type_accepts(key_ty, &key))
                {
                    context.add_diagnostic(DiagnosticCode::InjectField, index_key.get_range().unwrap_or_else(|| index.get_range()),
                        t!("Fields cannot be injected into the reference of `%{class}` for `%{field}`. ", class = humanize_semantic_type(&self.model, prefix_ty.structural_type()), field = index_key.get_path_part()));
                }
                continue;
            }
            let Some(target) = resolve_target(&self.model, prefix_ty.structural_type()) else {
                continue;
            };
            let Some(def) = find_type_def(&self.model, &target) else {
                continue;
            };
            if !def.super_names.is_empty()
                || self
                    .model
                    .members_of_owner(&target)
                    .iter()
                    .any(|member| member.name.starts_with('['))
                || is_mapped_alias(&self.model, &def)
            {
                continue;
            }
            if self.member_info(&prefix_ty, &key)?.is_some() {
                continue;
            }
            context.add_diagnostic(
                DiagnosticCode::InjectField,
                index_key.get_range().unwrap_or_else(|| index.get_range()),
                t!(
                    "Fields cannot be injected into the reference of `%{class}` for `%{field}`. ",
                    class = humanize_semantic_type(&self.model, prefix_ty.structural_type()),
                    field = index_key.get_path_part()
                ),
            );
        }
        Ok(())
    }
    // Only alias targets originate in pure doc projection; unchanged non-alias values retain carriers.
    pub(in crate::session::backend) fn field_expand_alias(
        &self,
        ty: &OwnedValue,
    ) -> Option<OwnedValue> {
        let mut current = ty.clone();
        let mut visited = Vec::new();
        loop {
            let id = match current.structural_type() {
                LuaType::Ref(id) | LuaType::Def(id) => id.clone(),
                _ => return (!visited.is_empty()).then_some(current),
            };
            let def = self.model.type_def_of(&id)?;
            if def.kind != TypeDefKind::Alias {
                return (!visited.is_empty()).then_some(current);
            }
            if visited.contains(&id) {
                return None;
            }
            visited.push(id);
            current = OwnedValue::plain(self.model.alias_target(&def)?);
        }
    }
    fn field_call_args(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
    ) -> QueryResult<(), E> {
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(());
        };
        let candidates = self.callable_candidates_cached(&callee)?;
        let Some(args) = call.get_args_list() else {
            return Ok(());
        };
        for candidate in &candidates {
            let Some(params) = candidate.function_parameters() else {
                continue;
            };
            for (index, arg) in args.get_args().enumerate() {
                let LuaExpr::TableExpr(table) = arg else {
                    continue;
                };
                let Some(Some(param)) = params.get(index) else {
                    continue;
                };
                self.field_against_type(context, param, &table)?;
            }
        }
        Ok(())
    }
    fn field_against(
        &mut self,
        context: &mut CheckContext<'_>,
        id: &SemanticId,
        table: &LuaTableExpr,
    ) -> QueryResult<(), E> {
        let Some(facts) = self.model.file_facts() else {
            return Ok(());
        };
        let Some(decl) = facts.decl_by_id(id) else {
            return Ok(());
        };
        let Some(syntax) = decl.doc_type_syntax else {
            return Ok(());
        };
        let target = OwnedValue::plain(self.model.doc_type_lua(syntax));
        self.field_against_type(context, &target, table)
    }
    fn field_against_type(
        &mut self,
        context: &mut CheckContext<'_>,
        target: &OwnedValue,
        table: &LuaTableExpr,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if let Some(expanded) = self.field_expand_alias(target) {
            return self.field_against_type(context, &expanded, table);
        }
        match target.structural_type() {
            LuaType::Intersection(intersection) => {
                for (index, component) in intersection.get_types().iter().enumerate() {
                    let component = self.owned_child(target, index, component);
                    self.field_against_type(context, &component, table)?;
                }
                return Ok(());
            }
            LuaType::Union(_) => {
                let count = table.get_fields_with_keys().len();
                for component in target.union_components().expect("union") {
                    if matches!(
                        component.structural_type(),
                        LuaType::Ref(_) | LuaType::Def(_) | LuaType::Generic(_)
                    ) && count > 0
                    {
                        return self.field_against_type(context, &component, table);
                    }
                    if matches!(component.structural_type(), LuaType::Array(_)) && count == 0 {
                        return Ok(());
                    }
                }
                return Ok(());
            }
            LuaType::Object(object) => {
                let fields = table.get_fields_with_keys().to_vec();
                let mut missing = Vec::new();
                for (key, ty) in object.get_fields() {
                    let name = match key {
                        LuaMemberKey::Name(n) => n.to_string(),
                        LuaMemberKey::Integer(i) => i.to_string(),
                        _ => continue,
                    };
                    if !fields.iter().any(|(_, key)| key.get_path_part() == name)
                        && !ty.is_nullable()
                    {
                        missing.push(name);
                    }
                }
                if !missing.is_empty() {
                    context.add_diagnostic(
                        DiagnosticCode::MissingFields,
                        table.get_range(),
                        format!(
                            "Missing required fields: {}",
                            missing
                                .iter()
                                .map(|name| format!("`{}`", name))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    );
                }
                return Ok(());
            }
            _ => {}
        }
        let Some(target_id) = resolve_target(&self.model, target.structural_type()) else {
            return Ok(());
        };
        let mut members = Vec::new();
        self.field_collect(&target_id, &mut Vec::new(), &mut members)?;
        if let Some(def) = find_type_def(&self.model, &target_id) {
            members.extend(
                object_super_fields(&self.model, &def)
                    .into_iter()
                    .map(|(name, ty)| (name, OwnedValue::plain(ty))),
            );
        }
        let fields = table
            .get_fields_with_keys()
            .into_iter()
            .map(|(_, key)| key.get_path_part())
            .collect::<Vec<_>>();
        for field in &fields {
            if !members.iter().any(|(name, _)| name == field) {
                context.add_diagnostic(
                    DiagnosticCode::InjectField,
                    table.get_range(),
                    format!("Field `{}` cannot be injected into the target type.", field),
                );
            }
        }
        let missing = members
            .iter()
            .filter(|(name, ty)| !fields.contains(name) && !is_optional(ty.structural_type()))
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            context.add_diagnostic(
                DiagnosticCode::MissingFields,
                table.get_range(),
                format!(
                    "Missing required fields: {}",
                    missing
                        .iter()
                        .map(|name| format!("`{}`", name))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
        Ok(())
    }
    fn field_collect(
        &mut self,
        id: &SemanticId,
        visited: &mut Vec<SemanticId>,
        out: &mut Vec<(String, OwnedValue)>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        if visited.contains(id) {
            return Ok(());
        }
        visited.push(id.clone());
        for member in self.model.members_of_owner(id).iter() {
            let name = member.name.to_string();
            let mut ty = self
                .type_of_member(&member.id)?
                .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
            if let LuaType::Ref(id) | LuaType::Def(id) = ty.structural_type()
                && let Some(def) = self.model.type_def_of(id)
                && let Some(alias) = self.model.alias_target(&def)
            {
                ty = OwnedValue::plain(alias);
            }
            if let Some(facts) = self.model.file_facts_of(member.file_id)
                && let Some(member) = facts.member_by_id(&member.id)
                && member.is_nullable
                && !ty.structural_type().is_nullable()
            {
                ty = OwnedValue::raw_union(vec![ty, OwnedValue::plain(LuaType::Nil)]);
            }
            if !out.iter().any(|(n, _)| n == &name) {
                out.push((name, ty));
            }
        }
        if let Some(def) = find_type_def(&self.model, id) {
            for name in &def.super_names {
                if let Some(super_def) = super_type_def(&self.model, name) {
                    self.field_collect(&super_def.id, visited, out)?;
                }
            }
        }
        Ok(())
    }
}

fn index_key_type_accepts(key_ty: &LuaType, member_key: &LuaMemberKey) -> bool {
    match member_key {
        LuaMemberKey::Name(_) => key_type_contains(key_ty, &LuaType::String),
        LuaMemberKey::Integer(_) => key_type_contains(key_ty, &LuaType::Integer),
        _ => true,
    }
}

fn key_type_contains(key_ty: &LuaType, expected: &LuaType) -> bool {
    match key_ty {
        LuaType::Any | LuaType::Unknown => true,
        LuaType::Union(union) => union
            .into_vec()
            .iter()
            .any(|ty| key_type_contains(&ty, expected)),
        _ => key_ty == expected || (expected == &LuaType::Integer && key_ty == &LuaType::Number),
    }
}

fn object_super_fields(
    semantic_model: &SemanticModel<'_>,
    def: &TypeDef,
) -> Vec<(String, LuaType)> {
    let Some(tree) = semantic_model.syntax_tree_of(def.file_id) else {
        return Vec::new();
    };
    let root = tree.get_red_root();
    let mut out = Vec::new();
    for class_tag in root
        .descendants()
        .filter_map(emmylua_parser::LuaDocTagClass::cast)
    {
        let Some(name_token) = class_tag.get_name_token() else {
            continue;
        };
        if name_token.syntax().text_range() != def.name_range {
            continue;
        }
        let Some(supers) = class_tag.get_supers() else {
            continue;
        };
        for super_ty in supers.get_types() {
            let emmylua_parser::LuaDocType::Object(object) = super_ty else {
                continue;
            };
            for field in object.get_fields() {
                let Some(key) = field.get_field_key() else {
                    continue;
                };
                let name = match &key {
                    emmylua_parser::LuaDocObjectFieldKey::Name(name) => {
                        name.get_name_text().to_string()
                    }
                    emmylua_parser::LuaDocObjectFieldKey::String(str) => str.get_value(),
                    _ => continue,
                };
                let ty = field
                    .get_type()
                    .map(|ty| semantic_model.doc_type_lua_rich_in(def.file_id, ty.get_syntax_id()))
                    .unwrap_or(LuaType::Unknown);
                if !out.iter().any(|(n, _)| n == &name) {
                    out.push((name, ty));
                }
            }
        }
    }
    out
}

fn is_mapped_alias(semantic_model: &SemanticModel<'_>, def: &TypeDef) -> bool {
    let Some(syntax) = def.alias_type else {
        return false;
    };
    let Some(tree) = semantic_model.syntax_tree_of(def.file_id) else {
        return false;
    };
    let Some(node) = syntax.to_node_from_root(&tree.get_red_root()) else {
        return false;
    };
    matches!(
        emmylua_parser::LuaDocType::cast(node),
        Some(emmylua_parser::LuaDocType::Mapped(_))
    )
}

fn resolve_target(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> Option<SemanticId> {
    match ty {
        LuaType::Ref(id) | LuaType::Def(id) => semantic_model.type_def_of(id).map(|def| def.id),
        LuaType::Generic(generic) => semantic_model
            .type_def_of(&generic.get_base_type_id())
            .map(|def| def.id),
        _ => None,
    }
}

fn find_type_def(semantic_model: &SemanticModel<'_>, type_def_id: &SemanticId) -> Option<TypeDef> {
    let SemanticId::TypeDef(key) = type_def_id else {
        return None;
    };
    let defs = semantic_model.type_defs_in_scope(key.scope, &key.full_name);
    defs.iter().next().cloned()
}

fn super_type_def(semantic_model: &SemanticModel<'_>, full_name: &str) -> Option<TypeDef> {
    semantic_model
        .type_defs_in_scope(TypeScope::Global, full_name)
        .iter()
        .next()
        .cloned()
}

fn is_optional(ty: &LuaType) -> bool {
    match ty {
        LuaType::Any | LuaType::Unknown | LuaType::Nil => true,
        LuaType::Union(union) => union.into_vec().iter().any(|t| matches!(t, LuaType::Nil)),
        _ => false,
    }
}
