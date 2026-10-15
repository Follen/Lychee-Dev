//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! check/checker/undefined_field.rs; Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    Decl, DeclKind, DiagnosticCode, FileId, InFiled, LuaType, LuaTypeDeclId, ModuleExport,
    SemanticModel, TypeDef, TypeDefKind, TypeScope, TypeVisibility,
};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaIndexExpr, LuaIndexKey, LuaSyntaxKind, LuaTokenKind};
use rowan::TextRange;

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_undefined_field(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        for index in root.descendants().filter_map(LuaIndexExpr::cast) {
            self.checkpoint()?;
            self.undefined_field_index(context, &index)?;
        }
        Ok(())
    }
    fn undefined_field_index(
        &mut self,
        context: &mut CheckContext<'_>,
        index: &LuaIndexExpr,
    ) -> QueryResult<(), E> {
        let Some(resolved) = self.resolve_owned_member(index)? else {
            return Ok(());
        };
        if resolved.member_id.is_some() {
            return Ok(());
        }
        let Some(prefix) = index.get_prefix_expr() else {
            return Ok(());
        };
        if let LuaExpr::NameExpr(name) = &prefix
            && name.get_name_text().is_some_and(|name| {
                matches!(
                    name.as_str(),
                    "table" | "string" | "math" | "io" | "os" | "coroutine" | "utf8" | "bit32"
                )
            })
        {
            return Ok(());
        }
        let prefix_ty = match &prefix {
            LuaExpr::NameExpr(name) => match self.model.resolve_name(name.get_position()) {
                Some(decl) => self.type_of_decl_at(&decl, name.get_position().into())?,
                None => self.type_of_expr(prefix.get_syntax_id())?,
            },
            _ => self.type_of_expr(prefix.get_syntax_id())?,
        };
        if is_invalid_prefix_type(prefix_ty.structural_type()) {
            return Ok(());
        }
        let Some(key) = index.get_index_key() else {
            return Ok(());
        };
        if self.undefined_valid_member(&prefix_ty, &prefix, index, &key)? {
            return Ok(());
        }
        context.add_diagnostic(
            DiagnosticCode::UndefinedField,
            key.get_range().unwrap_or_else(|| index.get_range()),
            t!("Undefined field: `%{name}`", name = key.get_path_part()),
        );
        Ok(())
    }
    fn undefined_valid_member(
        &mut self,
        ty: &OwnedValue,
        prefix: &LuaExpr,
        index: &LuaIndexExpr,
        key: &LuaIndexKey,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if let Some(expanded) = self.field_expand_alias(ty) {
            return self.undefined_valid_member(&expanded, prefix, index, key);
        }
        Ok(match ty.structural_type() {
            LuaType::TableConst(table) => {
                let Some((decl, def)) = table_binding(&self.model, table.file_id, table.value)
                else {
                    return Ok(!self.undefined_export_missing(ty, table, index, key)?);
                };
                if matches!(
                    def.name.as_str(),
                    "table" | "string" | "math" | "io" | "os" | "coroutine" | "utf8" | "bit32"
                ) {
                    return Ok(true);
                }
                match def.kind {
                    TypeDefKind::Enum => enum_table_access(&self.model, &decl, &def, key),
                    TypeDefKind::Class | TypeDefKind::Alias => self.undefined_named_member(
                        &OwnedValue::plain(class_lua_type(&def)),
                        prefix,
                        index,
                        key,
                    )?,
                }
            }
            LuaType::Array(_) => {
                let key_ty = self.undefined_key_type(key)?;
                numeric_key_expanded(&self.model, key_ty.structural_type())
                    || is_unknown_key(key_ty.structural_type())
                    || in_conditional_statement(index)
            }
            LuaType::Generic(generic) => {
                if generic.get_base_type_id().get_name() != "table" {
                    return self.undefined_named_member(ty, prefix, index, key);
                }
                let key_ty = self.undefined_key_type(key)?;
                let key_ty = self.undefined_resolve_generic(&key_ty);
                if is_unknown_key(key_ty.structural_type())
                    || is_unconstrained_generic_name(&self.model, key_ty.structural_type())
                {
                    return Ok(true);
                }
                let accepted = if let Some(table_key) = generic.get_params().first() {
                    let table_key = self.owned_child(ty, 0, table_key);
                    self.undefined_key_matches(&key_ty, &table_key)?
                } else {
                    false
                };
                accepted || in_conditional_statement(index)
            }
            LuaType::Ref(_) | LuaType::Def(_) => {
                self.undefined_named_member(ty, prefix, index, key)?
            }
            LuaType::Intersection(intersection) => {
                for (i, component) in intersection.get_types().iter().enumerate() {
                    let component = self.owned_child(ty, i, component);
                    if self.undefined_valid_member(&component, prefix, index, key)? {
                        return Ok(true);
                    }
                }
                false
            }
            LuaType::Union(_) => {
                for component in ty.union_components().expect("union") {
                    if self.undefined_valid_member(&component, prefix, index, key)? {
                        return Ok(true);
                    }
                }
                false
            }
            LuaType::Object(_) => {
                let key_ty = self.undefined_key_type(key)?;
                if is_unknown_key(key_ty.structural_type()) || matches!(key, LuaIndexKey::Expr(_)) {
                    return Ok(true);
                }
                if let Some(key) = self.export_member_key(key)? {
                    return Ok(self.member_info(ty, &key)?.is_some());
                }
                false
            }
            _ => true,
        })
    }
    fn undefined_named_member(
        &mut self,
        ty: &OwnedValue,
        prefix: &LuaExpr,
        index: &LuaIndexExpr,
        key: &LuaIndexKey,
    ) -> QueryResult<bool, E> {
        let ty = self.undefined_resolve_generic(ty);
        if is_unconstrained_generic_name(&self.model, ty.structural_type()) {
            return Ok(true);
        }
        if let Some(def) = named_type_def(&self.model, ty.structural_type())
            && is_mapped_alias(&self.model, &def)
        {
            return Ok(true);
        }
        if let Some(def) = named_type_def(&self.model, ty.structural_type())
            && def.kind == TypeDefKind::Enum
            && !prefix_directly_names_decl(&self.model, prefix)
        {
            return Ok(false);
        }
        let key_ty = self.undefined_key_type(key)?;
        if let Some(def) = named_type_def(&self.model, ty.structural_type())
            && has_index_signature(&self.model, &def, key_ty.structural_type())
        {
            return Ok(true);
        }
        if let Some(key) = self.export_member_key(key)?
            && self.member_info(&ty, &key)?.is_some()
        {
            return Ok(true);
        }
        if is_unknown_key(key_ty.structural_type()) {
            return Ok(true);
        }
        if matches!(key, LuaIndexKey::Expr(_))
            && (is_string_key(key_ty.structural_type())
                || is_enum_like_key(&self.model, key_ty.structural_type())
                || matches!(key_ty.structural_type(), LuaType::TableConst(_)))
        {
            return Ok(true);
        }
        Ok(in_conditional_statement(index) && has_bracket(index))
    }
    fn undefined_key_type(&mut self, key: &LuaIndexKey) -> QueryResult<OwnedValue, E> {
        match key {
            LuaIndexKey::Name(_) | LuaIndexKey::String(_) => Ok(OwnedValue::plain(LuaType::String)),
            LuaIndexKey::Integer(_) | LuaIndexKey::Idx(_) => {
                Ok(OwnedValue::plain(LuaType::Integer))
            }
            LuaIndexKey::Expr(expr) => self.type_of_expr(expr.get_syntax_id()),
        }
    }
    fn undefined_resolve_generic(&self, ty: &OwnedValue) -> OwnedValue {
        let id = match ty.structural_type() {
            LuaType::Ref(id) | LuaType::Def(id) => id,
            _ => return ty.clone(),
        };
        if named_type_def(&self.model, ty.structural_type()).is_some() {
            return ty.clone();
        }
        let Some(facts) = self.model.file_facts() else {
            return ty.clone();
        };
        if let Some((_, param)) = facts.generic_param_constraints(id.get_name()).next()
            && let Some(constraint) = param.constraint
        {
            return OwnedValue::plain(self.model.doc_type_lua(constraint));
        }
        ty.clone()
    }
    fn undefined_key_matches(
        &mut self,
        key: &OwnedValue,
        table_key: &OwnedValue,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if table_key.structural_type().is_unknown() || table_key.structural_type().is_any() {
            return Ok(true);
        }
        Ok(match table_key.structural_type() {
            LuaType::String | LuaType::StringConst(_) | LuaType::DocStringConst(_) => {
                is_string_key(key.structural_type())
            }
            LuaType::Integer
            | LuaType::Number
            | LuaType::IntegerConst(_)
            | LuaType::DocIntegerConst(_) => {
                numeric_key_expanded(&self.model, key.structural_type())
            }
            LuaType::Union(_) => {
                for member in table_key.union_components().expect("union") {
                    if self.undefined_key_matches(key, &member)? {
                        return Ok(true);
                    }
                }
                false
            }
            LuaType::Ref(_) | LuaType::Def(_) => self.type_check(key, table_key)?,
            _ => true,
        })
    }
    fn undefined_export_missing(
        &mut self,
        value: &OwnedValue,
        table: &InFiled<TextRange>,
        index: &LuaIndexExpr,
        key: &LuaIndexKey,
    ) -> QueryResult<bool, E> {
        let Some(facts) = self.model.file_facts_of(table.file_id) else {
            return Ok(false);
        };
        let ModuleExport::Decl { decl, .. } = &facts.module_export else {
            return Ok(false);
        };
        let Some(decl) = facts.decl_by_id(decl) else {
            return Ok(false);
        };
        if decl
            .value_expr_syntax
            .is_none_or(|syntax| syntax.get_range() != table.value)
        {
            return Ok(false);
        }
        if let Some(key) = self.export_member_key(key)? {
            return Ok(self.member_info(value, &key)?.is_none());
        }
        Ok(!in_conditional_statement(index) && false)
    }
}

fn is_invalid_prefix_type(ty: &LuaType) -> bool {
    match ty {
        LuaType::Any
        | LuaType::Unknown
        | LuaType::Table
        | LuaType::Userdata
        | LuaType::Global
        | LuaType::String
        | LuaType::Integer
        | LuaType::Number
        | LuaType::Boolean
        | LuaType::Function
        | LuaType::Thread
        | LuaType::StringConst(_)
        | LuaType::IntegerConst(_)
        | LuaType::FloatConst(_)
        | LuaType::BooleanConst(_)
        | LuaType::DocStringConst(_)
        | LuaType::DocIntegerConst(_)
        | LuaType::DocBooleanConst(_) => true,
        LuaType::TplRef(tpl) => tpl.get_constraint().is_none(),
        LuaType::Instance(instance) => is_invalid_prefix_type(instance.get_base()),
        _ => false,
    }
}

fn enum_table_access(
    semantic_model: &SemanticModel<'_>,
    decl: &Decl,
    _def: &TypeDef,
    index_key: &LuaIndexKey,
) -> bool {
    // Dynamic keys (parameter enum type / expression) are allowed broadly.
    if matches!(index_key, LuaIndexKey::Expr(_)) {
        return true;
    }
    let key_text = index_key.get_path_part();
    let mut accepted: Vec<String> = Vec::new();
    for member_ref in semantic_model.members_of_owner(&decl.id).iter() {
        accepted.push(member_ref.name.to_string());
        if let Some(facts) = semantic_model.file_facts_of(member_ref.file_id)
            && let Some(member) = facts.member_by_id(&member_ref.id)
            && let Some(value_syntax) = member.value_syntax
            && let Some(node) = semantic_model
                .syntax_tree()
                .and_then(|tree| value_syntax.to_node_from_root(&tree.get_red_root()))
        {
            let raw = node.text().to_string();
            accepted.push(raw.trim_matches(['"', '\'']).to_string());
        }
    }
    accepted.iter().any(|text| text == &key_text)
}

fn table_binding(
    semantic_model: &SemanticModel<'_>,
    file_id: FileId,
    range: TextRange,
) -> Option<(Decl, TypeDef)> {
    let facts = semantic_model.file_facts_of(file_id)?;
    let decl = facts.decl_by_value_range(range)?;
    let def = facts
        .type_defs_named_case_insensitive(decl.name.as_str())
        .next()
        .or_else(|| {
            // `---@enum K3` + `local apiAlias = {...}`: when names differ, associate with the nearest preceding type definition by position.
            facts.nearest_type_def_before(decl.name_range.start())
        })?
        .clone();
    Some((decl.clone(), def))
}

fn class_lua_type(def: &TypeDef) -> LuaType {
    match def.visibility {
        TypeVisibility::Public => LuaType::Ref(LuaTypeDeclId::global(&def.full_name)),
        _ => LuaType::Def(LuaTypeDeclId::file(def.file_id, &def.full_name)),
    }
}

fn is_unknown_key(ty: &LuaType) -> bool {
    matches!(
        ty,
        LuaType::Any | LuaType::Unknown | LuaType::Never | LuaType::Table
    ) || matches!(ty, LuaType::Generic(_))
        || matches!(ty, LuaType::TplRef(tpl) if tpl.get_constraint().is_none())
}

fn is_string_key(ty: &LuaType) -> bool {
    match ty {
        LuaType::String
        | LuaType::StringConst(_)
        | LuaType::DocStringConst(_)
        | LuaType::StrTplRef(_) => true,
        LuaType::Union(union) => union.into_vec().iter().any(is_string_key),
        _ => false,
    }
}

fn numeric_key(ty: &LuaType) -> bool {
    match ty {
        LuaType::Number
        | LuaType::Integer
        | LuaType::IntegerConst(_)
        | LuaType::FloatConst(_)
        | LuaType::DocIntegerConst(_) => true,
        LuaType::Union(union) => union.into_vec().iter().any(numeric_key),
        LuaType::MultiLineUnion(union) => match union.to_union() {
            LuaType::Union(union) => union.into_vec().iter().any(numeric_key),
            _ => false,
        },
        LuaType::Ref(_) | LuaType::Def(_) => false,
        _ => false,
    }
}

fn numeric_key_expanded(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> bool {
    numeric_key_expanded_inner(semantic_model, ty, &mut Vec::new())
}

fn numeric_key_expanded_inner(
    semantic_model: &SemanticModel<'_>,
    ty: &LuaType,
    visited: &mut Vec<LuaTypeDeclId>,
) -> bool {
    if numeric_key(ty) {
        return true;
    }
    let Some(def) = named_type_def(semantic_model, ty) else {
        return false;
    };
    let id = match ty {
        LuaType::Ref(id) | LuaType::Def(id) => id,
        _ => return false,
    };
    if visited.contains(id) {
        return false;
    }
    visited.push(id.clone());
    match def.kind {
        TypeDefKind::Alias => semantic_model
            .alias_target(&def)
            .is_some_and(|target| numeric_key_expanded_inner(semantic_model, &target, visited)),
        TypeDefKind::Class => def.super_names.iter().any(|super_name| {
            matches!(super_name.as_str(), "integer" | "number" | "int")
                || semantic_model
                    .type_defs_in_scope(TypeScope::Global, super_name.as_str())
                    .into_iter()
                    .next()
                    .is_some_and(|super_def| {
                        matches!(super_def.name.as_str(), "integer" | "number" | "int")
                    })
        }),
        TypeDefKind::Enum => false,
    }
}

fn has_index_signature(
    semantic_model: &SemanticModel<'_>,
    def: &TypeDef,
    key_ty: &LuaType,
) -> bool {
    for member_ref in semantic_model.members_of_owner(&def.id).iter() {
        let Some(facts) = semantic_model.file_facts_of(member_ref.file_id) else {
            continue;
        };
        let Some(member) = facts.member_by_id(&member_ref.id) else {
            continue;
        };
        match member.key.to_path().as_str() {
            "string" => {
                if is_string_key(key_ty) {
                    return true;
                }
            }
            "integer" | "number" => {
                if numeric_key_expanded(semantic_model, key_ty) {
                    return true;
                }
            }
            "any" | "unknown" => return true,
            _ => {}
        }
    }
    false
}

fn is_unconstrained_generic_name(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> bool {
    let id = match ty {
        LuaType::Ref(id) | LuaType::Def(id) => id,
        _ => return false,
    };
    if named_type_def(semantic_model, ty).is_some() {
        return false;
    }
    let name = id.get_name();
    semantic_model
        .file_facts()
        .is_some_and(|facts| facts.is_unconstrained_generic_name(name))
}

fn named_type_def(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> Option<TypeDef> {
    let id = match ty {
        LuaType::Ref(id) | LuaType::Def(id) => id,
        LuaType::Generic(generic) => {
            return named_type_def(semantic_model, &LuaType::Ref(generic.get_base_type_id()));
        }
        _ => return None,
    };
    semantic_model.type_def_of(id)
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

fn prefix_directly_names_decl(semantic_model: &SemanticModel<'_>, prefix: &LuaExpr) -> bool {
    let LuaExpr::NameExpr(name_expr) = prefix else {
        return false;
    };
    let Some(decl_id) = semantic_model.resolve_name(name_expr.get_position()) else {
        return false;
    };
    semantic_model
        .file_facts()
        .and_then(|facts| facts.decl_by_id(&decl_id))
        .is_some_and(|decl| {
            decl.doc_type_syntax.is_none()
                && matches!(decl.kind, DeclKind::Global | DeclKind::Local { .. })
                && decl.value_expr_syntax.is_some_and(|syntax| {
                    let Some(tree) = semantic_model.syntax_tree_of(decl.file_id) else {
                        return false;
                    };
                    let Some(node) = syntax.to_node_from_root(&tree.get_red_root()) else {
                        return false;
                    };
                    matches!(LuaExpr::cast(node), Some(LuaExpr::TableExpr(_)))
                })
        })
}

fn is_enum_like_key(semantic_model: &SemanticModel<'_>, key_ty: &LuaType) -> bool {
    is_enum_like_key_inner(semantic_model, key_ty, &mut Vec::new())
}

fn is_enum_like_key_inner(
    semantic_model: &SemanticModel<'_>,
    key_ty: &LuaType,
    visited: &mut Vec<LuaTypeDeclId>,
) -> bool {
    match key_ty {
        LuaType::Ref(_) | LuaType::Def(_) => {
            let Some(def) = named_type_def(semantic_model, key_ty) else {
                return false;
            };
            let id = match key_ty {
                LuaType::Ref(id) | LuaType::Def(id) => id,
                _ => return false,
            };
            if visited.contains(id) {
                return false;
            }
            visited.push(id.clone());
            match def.kind {
                TypeDefKind::Enum => true,
                TypeDefKind::Alias => semantic_model
                    .alias_target(&def)
                    .is_some_and(|target| is_enum_like_key_inner(semantic_model, &target, visited)),
                TypeDefKind::Class => false,
            }
        }
        LuaType::Union(union) => union
            .into_vec()
            .iter()
            .any(|component| is_enum_like_key_inner(semantic_model, component, visited)),
        _ => false,
    }
}

fn has_bracket(index_expr: &LuaIndexExpr) -> bool {
    index_expr
        .syntax()
        .children_with_tokens()
        .any(|child| child.kind() == LuaTokenKind::TkLeftBracket.into())
}

fn in_conditional_statement(index_expr: &LuaIndexExpr) -> bool {
    let range = index_expr.get_range();
    for ancestor in index_expr.syntax().ancestors() {
        let kind: LuaSyntaxKind = ancestor.kind().into();
        match kind {
            LuaSyntaxKind::IfStat => {
                if let Some(condition) = emmylua_parser::LuaIfStat::cast(ancestor)
                    .and_then(|stat| stat.get_condition_expr())
                    && condition.get_range().contains_range(range)
                {
                    return true;
                }
            }
            LuaSyntaxKind::WhileStat => {
                if let Some(condition) = emmylua_parser::LuaWhileStat::cast(ancestor)
                    .and_then(|stat| stat.get_condition_expr())
                    && condition.get_range().contains_range(range)
                {
                    return true;
                }
            }
            LuaSyntaxKind::ElseIfClauseStat => {
                if let Some(condition) = emmylua_parser::LuaElseIfClauseStat::cast(ancestor)
                    .and_then(|stat| stat.get_condition_expr())
                    && condition.get_range().contains_range(range)
                {
                    return true;
                }
            }
            LuaSyntaxKind::RepeatStat => {
                if let Some(condition) = emmylua_parser::LuaRepeatStat::cast(ancestor)
                    .and_then(|stat| stat.get_condition_expr())
                    && condition.get_range().contains_range(range)
                {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}
