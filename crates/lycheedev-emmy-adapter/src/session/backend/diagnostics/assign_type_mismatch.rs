//! Adapted from fixed Emmy assign_type_mismatch.rs; Copyright (c) CppCXY, MIT.
//! Annotation contracts remain separate from selected initializer values.
mod table;
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    Decl, DiagnosticCode, LuaMemberKey, LuaType, LuaTypeNode, SemanticId, SemanticModel, TypeDef,
    TypeDefKind, TypeScope, humanize_semantic_type,
};
use emmylua_parser::{
    BinaryOperator, LuaAssignStat, LuaAst, LuaAstNode, LuaCallExpr, LuaClosureExpr, LuaDocType,
    LuaExpr, LuaIndexExpr, LuaIndexKey, LuaLiteralToken, LuaLocalStat, LuaTableExpr, LuaVarExpr,
    NumberResult,
};
use std::collections::{HashMap, HashSet};

impl<E> OwnedQuery<'_, '_, E> {
    fn assignment_mismatch(
        &mut self,
        context: &mut CheckContext<'_>,
        range: rowan::TextRange,
        target: &OwnedValue,
        value: &OwnedValue,
    ) -> QueryResult<(), E> {
        let reason = match self.check_assign_type_detail(value, target)? {
            Err(
                crate::session::backend::relation::TypeCheckFailReason::TypeNotMatchWithReason(
                    reason,
                ),
            ) => Some(reason),
            _ => None,
        };
        let message = match reason {
            Some(reason) => t!(
                "expected `%{target}` but found `%{value}`. %{reason}",
                target = humanize_semantic_type(context.semantic_model, target.structural_type()),
                value = humanize_semantic_type(context.semantic_model, value.structural_type()),
                reason = reason
            ),
            None => t!(
                "expected `%{target}` but found `%{value}`",
                target = humanize_semantic_type(context.semantic_model, target.structural_type()),
                value = humanize_semantic_type(context.semantic_model, value.structural_type())
            ),
        };
        context.add_diagnostic(DiagnosticCode::AssignTypeMismatch, range, message);
        Ok(())
    }

    pub(in crate::session::backend) fn check_assign_type_mismatch(
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
                LuaAst::LuaLocalStat(stat) => self.assignment_local(context, &stat)?,
                LuaAst::LuaAssignStat(stat) => self.assignment_assign(context, &stat)?,
                LuaAst::LuaTableExpr(table) => self.assignment_inline_table(context, &table)?,
                LuaAst::LuaCallExpr(call) => self.assignment_call_args(context, &call)?,
                _ => {}
            }
        }
        Ok(())
    }

    fn assignment_call_overload(
        &mut self,
        call: &LuaCallExpr,
        target: &OwnedValue,
    ) -> QueryResult<bool, E> {
        let Some(LuaExpr::NameExpr(name)) = call.get_prefix_expr() else {
            return Ok(false);
        };
        if self.binding_manages_call(call.get_syntax_id())?
            || self.binding_manages(name.get_syntax_id())?
        {
            return Ok(false);
        }
        let Some(id @ SemanticId::Decl(_)) = self.model.resolve_name(name.get_position()) else {
            return Ok(false);
        };
        if self.owner_initializer_managed(&id)? {
            return Ok(false);
        }
        let SemanticId::Decl(key) = &id else {
            unreachable!()
        };
        let Some(facts) = self.model.file_facts_of(key.file_id) else {
            return Ok(false);
        };
        let Some(decl) = facts.decl_by_id(&SemanticId::Decl(key.clone())) else {
            return Ok(false);
        };
        let Some(closure) = decl.value_expr_syntax else {
            return Ok(false);
        };
        let Some(docs) = facts
            .signature_by_closure(closure)
            .and_then(|signature| signature.docs.as_ref())
        else {
            return Ok(false);
        };
        let count = call.get_args_count().unwrap_or(0);
        for syntax in &docs.overloads {
            self.checkpoint()?;
            let Some(tree) = self.model.syntax_tree_of(key.file_id) else {
                continue;
            };
            let Some(node) = syntax.to_node_from_root(&tree.get_red_root()) else {
                continue;
            };
            let Some(LuaDocType::Func(function)) = LuaDocType::cast(node) else {
                continue;
            };
            if function.get_params().count() != count {
                continue;
            }
            let Some(returns) = function.get_return_type_list() else {
                continue;
            };
            let Some(ret) = returns
                .get_return_type_list()
                .next()
                .and_then(|ret| ret.get_name_and_type().1)
            else {
                continue;
            };
            let ret = OwnedValue::plain(
                self.model
                    .doc_type_lua_rich_in(key.file_id, ret.get_syntax_id()),
            );
            if self.type_check_assign(&ret, target)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn assignment_string_template(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
    ) -> QueryResult<bool, E> {
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(false);
        };
        if self.binding_manages_call(call.get_syntax_id())?
            || self.binding_manages(callee.get_syntax_id())?
        {
            return Ok(false);
        }
        let LuaExpr::NameExpr(name) = &callee else {
            return Ok(false);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(false);
        };
        let Some(signature) = self.access_decl_signature(&decl)? else {
            return Ok(false);
        };
        let LuaType::DocFunction(metadata) = signature.structural_type() else {
            return Ok(false);
        };
        let constraints: Vec<_> = metadata
            .get_generic_params()
            .iter()
            .filter_map(|param| {
                param.get_constraint().map(|constraint| {
                    (
                        param.get_name().to_string(),
                        OwnedValue::plain(constraint.clone()),
                    )
                })
            })
            .collect();
        if constraints.is_empty() {
            return Ok(false);
        }
        let Some(args) = call.get_args_list() else {
            return Ok(false);
        };
        for (index, arg) in args.get_args().enumerate() {
            self.checkpoint()?;
            let Some((_, Some(param))) = metadata.get_params().get(index) else {
                continue;
            };
            if !str_tpl_contains_tpl(param) {
                continue;
            }
            if self.binding_manages(arg.get_syntax_id())? {
                let _ = self.type_of_expr(arg.get_syntax_id())?;
                continue;
            }
            let LuaExpr::LiteralExpr(literal) = &arg else {
                continue;
            };
            let Some(LuaLiteralToken::String(token)) = literal.get_literal() else {
                continue;
            };
            let name = token.get_value();
            let resolved = self
                .model
                .resolve_type_def(&name)
                .map(|def| self.model.type_def_ref(&def))
                .or_else(|| {
                    self.model.file_facts().and_then(|facts| {
                        facts
                            .type_defs
                            .iter()
                            .find(|def| def.name.to_lowercase() == name.to_lowercase())
                            .map(|def| self.model.type_def_ref(def))
                    })
                });
            let Some(resolved) = resolved else {
                continue;
            };
            let resolved = OwnedValue::plain(resolved);
            for (name, constraint) in &constraints {
                if str_tpl_uses_name(param, name)
                    && !self.type_check_assign(&resolved, constraint)?
                {
                    self.assignment_mismatch(context, arg.get_range(), constraint, &resolved)?;
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    fn assignment_call_args(
        &mut self,
        context: &mut CheckContext<'_>,
        call: &LuaCallExpr,
    ) -> QueryResult<(), E> {
        if call.get_prefix_expr().is_none() {
            return Ok(());
        }
        let analysis = self.call_site_analysis(call)?;
        if analysis.candidates.is_empty() {
            return Ok(());
        }
        let Some(args) = call.get_args_list() else {
            return Ok(());
        };
        for candidate in &analysis.candidates {
            let Some(params) = candidate.function_parameters() else {
                continue;
            };
            for (index, arg) in args.get_args().enumerate() {
                self.checkpoint()?;
                let Some(Some(param)) = params.get(index) else {
                    break;
                };
                let LuaExpr::TableExpr(table) = arg else {
                    continue;
                };
                if self.binding_manages(table.get_syntax_id())? {
                    let _ = self.type_of_expr(table.get_syntax_id())?;
                    continue;
                }
                if is_structured_table_target(&self.model, param.structural_type()) {
                    self.assignment_table_fields(context, param, &table)?;
                }
            }
        }
        Ok(())
    }

    fn assignment_inline_table(
        &mut self,
        context: &mut CheckContext<'_>,
        table: &LuaTableExpr,
    ) -> QueryResult<(), E> {
        if self.binding_manages(table.get_syntax_id())? {
            let _ = self.type_of_expr(table.get_syntax_id())?;
            return Ok(());
        }
        let owner = SemanticId::member(self.model.file_id(), table.get_range());
        for reference in self.model.members_of_owner(&owner).iter() {
            self.checkpoint()?;
            let Some(facts) = self.model.file_facts_of(reference.file_id) else {
                continue;
            };
            let Some(member) = facts.member_by_id(&reference.id) else {
                continue;
            };
            let Some(syntax) = member.doc_type_syntax else {
                continue;
            };
            let Some(value) = member
                .value_syntax
                .and_then(|syntax| {
                    self.model
                        .syntax_tree()
                        .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
                })
                .and_then(LuaExpr::cast)
            else {
                continue;
            };
            let target = OwnedValue::plain(self.model.doc_type_lua(syntax));
            let actual = self.type_of_expr(value.get_syntax_id())?;
            if matches!(
                actual.structural_type(),
                LuaType::Unknown | LuaType::Any | LuaType::Never | LuaType::Nil
            ) || self.type_check(&actual, &target)?
            {
                continue;
            }
            self.assignment_mismatch(context, value.get_range(), &target, &actual)?;
        }
        Ok(())
    }

    fn assignment_local(
        &mut self,
        context: &mut CheckContext<'_>,
        stat: &LuaLocalStat,
    ) -> QueryResult<(), E> {
        let names: Vec<_> = stat.get_local_name_list().collect();
        let values: Vec<_> = stat.get_value_exprs().collect();
        for (index, name) in names.iter().enumerate() {
            self.checkpoint()?;
            let Some(value) = values.get(index) else {
                break;
            };
            let Some(id) = self.model.decl_by_offset(name.get_range().start()) else {
                continue;
            };
            let Some(facts) = self.model.file_facts() else {
                continue;
            };
            let Some(decl) = facts.decl_by_id(&id) else {
                continue;
            };
            if let LuaExpr::CallExpr(call) = value
                && self.assignment_string_template(context, call)?
            {
                continue;
            }
            if decl.doc_type_syntax.is_none() {
                if let Some(target) = class_contract_for_decl(&self.model, decl) {
                    if let LuaExpr::CallExpr(call) = value
                        && !self.binding_manages_call(call.get_syntax_id())?
                        && let Some(callee) = call.get_prefix_expr()
                        && !self.binding_manages(callee.get_syntax_id())?
                        && is_setmetatable_call(call)
                    {
                        continue;
                    }
                    self.assignment_value_against(context, &OwnedValue::plain(target), value)?;
                }
                continue;
            }
            let target = match facts.decl_by_id(&id).and_then(|decl| decl.doc_type_syntax) {
                Some(syntax) => Some(OwnedValue::plain(self.model.doc_type_lua_rich(syntax))),
                None => self.type_of_decl(&id)?,
            };
            if let Some(target) = target {
                self.assignment_value_against(context, &target, value)?;
            }
        }
        Ok(())
    }

    fn assignment_assign(
        &mut self,
        context: &mut CheckContext<'_>,
        stat: &LuaAssignStat,
    ) -> QueryResult<(), E> {
        let (vars, exprs) = stat.get_var_and_expr_list();
        for (var, expr) in vars.iter().zip(exprs.iter()) {
            self.checkpoint()?;
            match var {
                LuaVarExpr::NameExpr(name) => {
                    let Some(decl) = self.model.resolve_name(name.get_position()) else {
                        if name.get_text() == "_"
                            && let Some(target) = preceding_doc_type_target(&self.model, stat)
                        {
                            self.assignment_value_against(
                                context,
                                &OwnedValue::plain(target),
                                expr,
                            )?;
                        }
                        continue;
                    };
                    let target =
                        self.type_of_decl_assign_target_at(&decl, name.get_position().into())?;
                    self.assignment_value_against(context, &target, expr)?;
                }
                LuaVarExpr::IndexExpr(index) => {
                    let Some(resolved) = self.resolve_owned_member(index)? else {
                        self.assignment_self_member(context, index, expr)?;
                        continue;
                    };
                    let Some(id) = resolved.member_id else {
                        self.assignment_self_member(context, index, expr)?;
                        continue;
                    };
                    let owner_is_self = self.model.file_facts().and_then(|facts| facts.member_by_id(&id)).is_some_and(|member| matches!(member.owner, SemanticId::Name(ref name) if name.as_str() == "self"));
                    if owner_is_self {
                        self.assignment_self_member(context, index, expr)?;
                        continue;
                    }
                    self.assignment_member(context, &id, index.get_range().start().into(), expr)?;
                }
            }
        }
        Ok(())
    }

    fn assignment_self_member(
        &mut self,
        context: &mut CheckContext<'_>,
        index: &LuaIndexExpr,
        value: &LuaExpr,
    ) -> QueryResult<(), E> {
        let Some(LuaExpr::NameExpr(name)) = index.get_prefix_expr() else {
            return Ok(());
        };
        if name.get_text() != "self" {
            return Ok(());
        }
        let key = index
            .get_index_key()
            .map(|key| key.get_path_part())
            .unwrap_or_default();
        let Some(closure) = index.ancestors::<LuaClosureExpr>().next() else {
            return Ok(());
        };
        let Some(facts) = self.model.file_facts() else {
            return Ok(());
        };
        let methods: Vec<_> = facts
            .members
            .iter()
            .filter(|member| member.value_syntax == Some(closure.get_syntax_id()))
            .collect();
        let Some(method) = methods.into_iter().find(|member| member.is_method) else {
            return Ok(());
        };
        let owner = self
            .type_of_decl(&method.owner)?
            .unwrap_or_else(|| OwnedValue::plain(LuaType::Unknown));
        let target = if let Some(member) = facts.members.iter().find(|member| {
            member.owner == method.owner
                && member.key.name() == Some(key.as_str())
                && member.value_syntax != Some(value.get_syntax_id())
        }) {
            Some(self.type_of_member_at(&member.id, index.get_range().start().into())?)
        } else {
            self.member_type(&owner, &LuaMemberKey::Name(key.into()))?
        };
        if let Some(target) = target {
            self.assignment_value_against(context, &target, value)?;
        }
        Ok(())
    }

    fn assignment_member(
        &mut self,
        context: &mut CheckContext<'_>,
        id: &SemanticId,
        offset: u32,
        value: &LuaExpr,
    ) -> QueryResult<(), E> {
        let mut target = self.type_of_member_at(id, offset)?;
        if let Some(member) = self
            .model
            .file_facts()
            .and_then(|facts| facts.member_by_id(id))
            && member.value_syntax == Some(value.get_syntax_id())
        {
            let SemanticId::Decl(owner) = &member.owner else {
                return self.assignment_value_target(context, &target, value);
            };
            let prefix =
                self.type_of_decl_assign_target_at(&SemanticId::Decl(owner.clone()), offset)?;
            let key = member.key.clone();
            let infos = self.member_infos_with_key(&prefix, &key)?;
            let declared = match infos.into_iter().find(|info| info.id.as_ref() != Some(id)) {
                Some(info) => Some(info.typ),
                None => self.member_type(&prefix, &key)?,
            };
            if let Some(declared) = declared {
                target = declared;
            }
        }
        let actual =
            self.type_of_expr_at(value.get_syntax_id(), value.get_range().start().into())?;
        if matches!(actual.structural_type(), LuaType::Nil)
            || matches!(target.structural_type(), LuaType::Nil)
        {
            return Ok(());
        }
        self.assignment_value_target(context, &target, value)
    }

    fn assignment_value_target(
        &mut self,
        context: &mut CheckContext<'_>,
        target: &OwnedValue,
        value: &LuaExpr,
    ) -> QueryResult<(), E> {
        if target.structural_type().contains_tpl_node() {
            return Ok(());
        }
        self.assignment_value_against(context, target, value)
    }

    fn assignment_value_against(
        &mut self,
        context: &mut CheckContext<'_>,
        target: &OwnedValue,
        value: &LuaExpr,
    ) -> QueryResult<(), E> {
        if !self.binding_manages(value.get_syntax_id())?
            && let LuaExpr::TableExpr(table) = value
        {
            if matches!(
                target.structural_type(),
                LuaType::Table | LuaType::TableConst(_) | LuaType::TableGeneric(_)
            ) {
                return Ok(());
            }
            if let LuaType::Generic(generic) = target.structural_type()
                && generic.get_base_type_id().get_name() == "table"
            {
                return Ok(());
            }
            if is_structured_table_target(&self.model, target.structural_type()) {
                self.assignment_table_fields(context, target, table)?;
            } else {
                let actual =
                    self.type_of_expr_at(value.get_syntax_id(), value.get_range().start().into())?;
                if matches!(
                    actual.structural_type(),
                    LuaType::Unknown | LuaType::Any | LuaType::Never
                ) {
                    return Ok(());
                }
                if matches!(target.structural_type(), LuaType::Tuple(_))
                    && self.type_check_assign(&actual, target)?
                {
                    return Ok(());
                }
                self.assignment_mismatch(context, value.get_range(), target, &actual)?;
            }
            return Ok(());
        }
        if let LuaExpr::CallExpr(call) = value
            && let Some(callee) = call.get_prefix_expr()
            && !self.binding_manages_call(call.get_syntax_id())?
            && !self.binding_manages(callee.get_syntax_id())?
        {
            for candidate in self.callable_candidates_cached(&callee)? {
                if let Some(ret) = candidate.function_return()
                    && self.type_check_assign(&ret, target)?
                {
                    return Ok(());
                }
            }
            if self.assignment_call_overload(call, target)? {
                return Ok(());
            }
        }
        let actual = self.assignment_value_type(value)?;
        if !self.binding_manages(value.get_syntax_id())?
            && matches!(target.structural_type(), LuaType::Integer)
            && matches!(actual.structural_type(), LuaType::Number)
            && is_numeric_binary_expr(value)
        {
            return Ok(());
        }
        if let LuaType::Union(union) = actual.structural_type() {
            let mut any = false;
            for (index, component) in union.into_vec().iter().enumerate() {
                if self.type_check_assign(&self.owned_child(&actual, index, component), target)? {
                    any = true;
                    break;
                }
            }
            if any && self.assignment_guard_narrowed(value)? {
                return Ok(());
            }
        }
        if matches!(
            actual.structural_type(),
            LuaType::Unknown | LuaType::Any | LuaType::Never
        ) || self.type_check_assign(&actual, target)?
        {
            return Ok(());
        }
        self.assignment_mismatch(context, value.get_range(), target, &actual)
    }

    fn assignment_guard_narrowed(&mut self, value: &LuaExpr) -> QueryResult<bool, E> {
        if self.binding_manages(value.get_syntax_id())? {
            return Ok(false);
        }
        let LuaExpr::NameExpr(name) = value else {
            return Ok(false);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(false);
        };
        let Some(facts) = self.model.file_facts() else {
            return Ok(false);
        };
        let Some(decl) = facts.decl_by_id(&decl) else {
            return Ok(false);
        };
        if decl.multi_return_index.is_none() {
            return Ok(false);
        }
        let Some(LuaExpr::CallExpr(call)) = decl
            .value_expr_syntax
            .and_then(|syntax| {
                self.model
                    .syntax_tree()
                    .and_then(|tree| syntax.to_node_from_root(&tree.get_red_root()))
            })
            .and_then(LuaExpr::cast)
        else {
            return Ok(false);
        };
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(false);
        };
        if self.binding_manages_call(call.get_syntax_id())?
            || self.binding_manages(callee.get_syntax_id())?
        {
            return Ok(false);
        }
        let LuaExpr::NameExpr(name) = callee else {
            return Ok(false);
        };
        let Some(decl) = self.model.resolve_name(name.get_position()) else {
            return Ok(false);
        };
        if self.access_decl_signature(&decl)?.is_none() {
            return Ok(false);
        }
        let SemanticId::Decl(key) = decl else {
            return Ok(false);
        };
        let Some(facts) = self.model.file_facts_of(key.file_id) else {
            return Ok(false);
        };
        let Some(decl) = facts.decl_by_id(&SemanticId::Decl(key)) else {
            return Ok(false);
        };
        Ok(decl
            .value_expr_syntax
            .and_then(|syntax| facts.signature_by_closure(syntax))
            .and_then(|signature| signature.docs.as_ref())
            .is_some_and(|docs| !docs.return_overloads.is_empty()))
    }

    fn assignment_value_type(&mut self, value: &LuaExpr) -> QueryResult<OwnedValue, E> {
        let ty = self.type_of_expr_at(value.get_syntax_id(), value.get_range().start().into())?;
        if matches!(ty.structural_type(), LuaType::Function)
            && let LuaExpr::NameExpr(name) = value
            && let Some(decl) = self.model.resolve_name(name.get_position())
            && let Some(function) = self.access_decl_signature(&decl)?
        {
            return Ok(function);
        }
        let (LuaType::Ref(id) | LuaType::Def(id)) = ty.structural_type() else {
            return Ok(ty);
        };
        if self.model.type_def_of(id).is_some() {
            return Ok(ty);
        }
        let Some(signatures) = self.model.signatures() else {
            return Ok(ty);
        };
        for signature in signatures {
            self.checkpoint()?;
            let Some(docs) = signature.docs.as_ref() else {
                continue;
            };
            if let Some(param) = docs
                .generic_params
                .iter()
                .find(|param| param.name.as_str() == id.get_name())
                && let Some(syntax) = param.constraint
            {
                let constraint = self.model.doc_type_lua_rich(syntax);
                if !matches!(constraint, LuaType::Unknown) {
                    return Ok(OwnedValue::plain(constraint));
                }
            }
        }
        Ok(ty)
    }
}
fn str_tpl_contains_tpl(ty: &LuaType) -> bool {
    match ty {
        LuaType::StrTplRef(_) => true,
        LuaType::Union(union) => union.into_vec().iter().any(str_tpl_contains_tpl),
        _ => false,
    }
}

fn str_tpl_uses_name(ty: &LuaType, name: &str) -> bool {
    match ty {
        LuaType::StrTplRef(tpl) => tpl.get_name() == name,
        LuaType::Union(union) => union
            .into_vec()
            .iter()
            .any(|component| str_tpl_uses_name(component, name)),
        _ => false,
    }
}

/// Table literals in call arguments: check each field against the callee parameter type (`f({ t = "" })`).

fn is_setmetatable_call(call: &LuaCallExpr) -> bool {
    call.get_prefix_expr().is_some_and(|prefix| match prefix {
        LuaExpr::NameExpr(name_expr) => {
            name_expr.get_name_text().as_deref() == Some("setmetatable")
        }
        LuaExpr::IndexExpr(index_expr) => index_expr
            .get_index_name_token()
            .is_some_and(|name| name.text() == "setmetatable"),
        _ => false,
    })
}

fn is_structured_table_target(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> bool {
    match ty {
        LuaType::Array(_) | LuaType::Object(_) => true,
        LuaType::Union(union) => union
            .into_vec()
            .iter()
            .any(|component| is_structured_table_target(semantic_model, component)),
        LuaType::Generic(generic) => {
            generic.get_base_type_id().get_name() == "array"
                || is_structured_table_target(
                    semantic_model,
                    &LuaType::Ref(generic.get_base_type_id()),
                )
        }
        LuaType::Ref(_) | LuaType::Def(_) => {
            let Some(def) = named_def(semantic_model, ty) else {
                return false;
            };
            match def.kind {
                TypeDefKind::Class => true,
                TypeDefKind::Alias => semantic_model
                    .alias_target(&def)
                    .is_some_and(|target| is_structured_table_target(semantic_model, &target)),
                TypeDefKind::Enum => false,
            }
        }
        _ => false,
    }
}

fn is_numeric_binary_expr(expr: &LuaExpr) -> bool {
    match expr {
        LuaExpr::BinaryExpr(binary) => binary.get_op_token().is_some_and(|op| {
            matches!(
                op.get_op(),
                BinaryOperator::OpAdd
                    | BinaryOperator::OpSub
                    | BinaryOperator::OpMul
                    | BinaryOperator::OpIDiv
                    | BinaryOperator::OpDiv
                    | BinaryOperator::OpMod
            )
        }),
        LuaExpr::ParenExpr(paren) => paren
            .get_expr()
            .is_some_and(|inner| is_numeric_binary_expr(&inner)),
        _ => false,
    }
}

/// `local ok, result = pick(...)` plus `@return_overload` guard: pass the result union when it matches the target component.

fn index_key_to_member_key(
    _semantic_model: &SemanticModel<'_>,
    key: &LuaIndexKey,
) -> Option<LuaMemberKey> {
    match key {
        LuaIndexKey::Name(_) | LuaIndexKey::String(_) => {
            Some(LuaMemberKey::Name(key.get_path_part().into()))
        }
        LuaIndexKey::Integer(_) | LuaIndexKey::Idx(_) => {
            let value = match key {
                LuaIndexKey::Integer(token) => Some(token.get_number_value()),
                LuaIndexKey::Idx(_) => key
                    .get_path_part()
                    .parse::<i64>()
                    .ok()
                    .map(NumberResult::Int),
                _ => None,
            };
            match value {
                Some(NumberResult::Int(i)) => Some(LuaMemberKey::Integer(i)),
                Some(NumberResult::Uint(u)) => Some(LuaMemberKey::Integer(u as i64)),
                _ => None,
            }
        }
        LuaIndexKey::Expr(_) => None,
    }
}

fn class_contract_for_decl(semantic_model: &SemanticModel<'_>, decl: &Decl) -> Option<LuaType> {
    let def = facts_type_def_for_decl(semantic_model, decl)?;
    if def.kind == TypeDefKind::Enum {
        return None;
    }
    Some(semantic_model.type_def_ref(&def))
}

fn facts_type_def_for_decl(semantic_model: &SemanticModel<'_>, decl: &Decl) -> Option<TypeDef> {
    let facts = semantic_model.file_facts()?;
    facts
        .type_defs
        .iter()
        .find(|def| def.owner_syntax.is_some() && def.owner_syntax == decl.owner_syntax)
        .cloned()
}

fn named_def(semantic_model: &SemanticModel<'_>, ty: &LuaType) -> Option<TypeDef> {
    let id = match ty {
        LuaType::Ref(id) | LuaType::Def(id) => id,
        LuaType::Generic(generic) => {
            return named_def(semantic_model, &LuaType::Ref(generic.get_base_type_id()));
        }
        _ => return None,
    };
    semantic_model.type_def_of(id)
}

/// The `---@type C` comment immediately before an assignment statement -> target type (`_ = c` scenario).
fn preceding_doc_type_target(
    semantic_model: &SemanticModel<'_>,
    stat: &LuaAssignStat,
) -> Option<LuaType> {
    let tree = semantic_model.syntax_tree()?;
    let mut before = String::new();
    for item in tree.get_red_root().descendants_with_tokens() {
        let Some(token) = item.into_token() else {
            continue;
        };
        if token.text_range().end() <= stat.get_range().start() {
            before.push_str(token.text());
        }
    }
    let mark = before.rfind("@type")?;
    let rest = &before[mark + 5..];
    let name: String = rest
        .trim()
        .chars()
        .take_while(|c| c.is_alphanumeric() || matches!(c, '.' | '_'))
        .collect();
    if name.is_empty() {
        return None;
    }
    Some(semantic_model.type_from_name(&name))
}
