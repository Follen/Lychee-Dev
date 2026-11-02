//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! check/checker/duplicate_field.rs; Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
use super::CheckContext;
use crate::session::SelectedProjectionBackend;
use crate::session::backend::{OwnedQuery, QueryResult};
use emmylua_code_analysis::{DiagnosticCode, FileId, LuaType, Member, SemanticId};
use emmylua_parser::{
    LuaAssignStat, LuaAstNode, LuaCallExpr, LuaClosureExpr, LuaExpr, LuaSyntaxNode, LuaVarExpr,
};
use std::collections::HashMap;
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_duplicate_field(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(facts) = self.model.file_facts() else {
            return Ok(());
        };
        if let Some(tree) = self.model.syntax_tree() {
            self.duplicate_cross_file_assign(context, &tree.get_red_root())?;
        }
        let mut by_owner: HashMap<&SemanticId, Vec<&Member>> = HashMap::new();
        for member in &facts.members {
            self.checkpoint()?;
            by_owner.entry(&member.owner).or_default().push(member);
        }
        for (owner, members) in by_owner {
            let mut by_key: HashMap<String, Vec<&&Member>> = HashMap::new();
            for member in &members {
                by_key.entry(member.key.to_path()).or_default().push(member);
            }
            let is_type = matches!(owner, SemanticId::TypeDef(_));
            for (name, dupes) in by_key {
                self.checkpoint()?;
                if dupes.len() <= 1 {
                    continue;
                }
                let mut all_function = true;
                for member in &dupes {
                    if !self.duplicate_is_function_like(member)? {
                        all_function = false;
                        break;
                    }
                }
                if (is_type && all_function) || (!is_type && !all_function) {
                    continue;
                }
                let code = if is_type {
                    DiagnosticCode::DuplicateDocField
                } else {
                    DiagnosticCode::DuplicateSetField
                };
                for member in dupes {
                    if let Some(range) = member.id.member_key_range() {
                        context.add_diagnostic(
                            code,
                            range,
                            t!("Duplicate field `%{name}`.", name = name),
                        );
                    }
                }
            }
        }
        Ok(())
    }
    fn duplicate_cross_file_assign(
        &mut self,
        context: &mut CheckContext<'_>,
        root: &LuaSyntaxNode,
    ) -> QueryResult<(), E> {
        let current = self.model.file_id();
        for assign in root.descendants().filter_map(LuaAssignStat::cast) {
            let (vars, exprs) = assign.get_var_and_expr_list();
            for (index, var) in vars.iter().enumerate() {
                self.checkpoint()?;
                let LuaVarExpr::IndexExpr(expr) = var else {
                    continue;
                };
                let Some(prefix) = expr.get_prefix_expr() else {
                    continue;
                };
                let Some(resolved) = self.resolve_owned_member(expr)? else {
                    continue;
                };
                let file = self
                    .duplicate_required_module(&prefix)?
                    .or_else(|| resolved.file_id.filter(|file| *file != current));
                let Some(file) = file else { continue };
                if file == current {
                    continue;
                }
                let Some(facts) = self.model.file_facts_of(file) else {
                    continue;
                };
                if !facts
                    .members_named(resolved.name.as_str())
                    .any(|m| m.value_syntax.is_some())
                {
                    continue;
                }
                let Some(rhs) = exprs.get(index) else {
                    continue;
                };
                let value = self.type_of_expr(rhs.get_syntax_id())?;
                if !matches!(
                    value.structural_type(),
                    LuaType::DocFunction(_) | LuaType::Signature(_) | LuaType::Function
                ) {
                    continue;
                }
                let Some(range) = expr.get_index_key().and_then(|k| k.get_range()) else {
                    continue;
                };
                context.add_diagnostic(
                    DiagnosticCode::DuplicateSetField,
                    range,
                    t!("Duplicate field `%{name}`.", name = resolved.name),
                );
            }
        }
        Ok(())
    }
    fn duplicate_required_module(&mut self, prefix: &LuaExpr) -> QueryResult<Option<FileId>, E> {
        let arg = (|| {
            let LuaExpr::NameExpr(name) = prefix else {
                return None;
            };
            let id = self.model.resolve_name(name.get_position())?;
            let decl = self.model.file_facts()?.decl_by_id(&id)?;
            let syntax = decl.value_expr_syntax?;
            let tree = self.model.syntax_tree()?;
            let call = LuaCallExpr::cast(syntax.to_node_from_root(&tree.get_red_root())?)?;
            call.get_args_list()?.get_args().next()
        })();
        let Some(arg) = arg else { return Ok(None) };
        let value = self.type_of_expr(arg.get_syntax_id())?;
        let name = match value.structural_type() {
            LuaType::StringConst(s) | LuaType::DocStringConst(s) => s.as_ref().to_string(),
            _ => return Ok(None),
        };
        Ok(self.model.module_file_of(&name))
    }
    fn duplicate_is_function_like(&mut self, member: &Member) -> QueryResult<bool, E> {
        if let Some(syntax) = member.value_syntax
            && let Some(tree) = self.model.syntax_tree()
            && let Some(node) = syntax.to_node_from_root(&tree.get_red_root())
            && LuaClosureExpr::cast(node).is_some()
        {
            return Ok(true);
        }
        Ok(self.type_of_member(&member.id)?.is_some_and(|value| {
            matches!(
                value.structural_type(),
                LuaType::DocFunction(_) | LuaType::Signature(_) | LuaType::Function
            )
        }))
    }
}
