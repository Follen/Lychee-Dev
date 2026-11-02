//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::SelectedProjectionBackend;
use emmylua_code_analysis::DiagnosticCode;
use emmylua_parser::{LuaAssignStat, LuaAstNode, LuaExpr, LuaVarExpr};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_readonly(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for assign in tree
            .get_red_root()
            .descendants()
            .filter_map(LuaAssignStat::cast)
        {
            let (vars, _) = assign.get_var_and_expr_list();
            for var in vars {
                self.checkpoint()?;
                match var {
                    LuaVarExpr::NameExpr(name) => {
                        let Some(id) = self.model.resolve_name(name.get_position()) else {
                            continue;
                        };
                        if self
                            .model
                            .file_facts()
                            .and_then(|f| f.decl_by_id(&id))
                            .is_some_and(|d| d.readonly)
                        {
                            context.add_diagnostic(
                                DiagnosticCode::ReadOnly,
                                name.get_range(),
                                t!("The variable is marked as readonly and cannot be assigned to."),
                            );
                        }
                    }
                    LuaVarExpr::IndexExpr(index) => {
                        let range = index.get_range();
                        if let Some(id) =
                            self.resolve_owned_member(&index)?.and_then(|r| r.member_id)
                        {
                            let file = self
                                .resolve_owned_member(&index)?
                                .and_then(|r| r.file_id)
                                .unwrap_or(self.model.file_id());
                            if self
                                .model
                                .file_facts_of(file)
                                .and_then(|f| f.member_by_id(&id))
                                .is_some_and(|m| m.readonly)
                            {
                                context.add_diagnostic(DiagnosticCode::ReadOnly,range,t!("The property is marked as readonly and cannot be assigned to."));
                            }
                        }
                        if let Some(prefix) = index.get_prefix_expr()
                            && readonly_prefix(&self.model, &prefix)
                        {
                            context.add_diagnostic(
                                DiagnosticCode::ReadOnly,
                                range,
                                t!("The variable is marked as readonly and cannot be assigned to."),
                            );
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
fn readonly_prefix(
    semantic_model: &emmylua_code_analysis::SemanticModel<'_>,
    expr: &LuaExpr,
) -> bool {
    match expr {
        LuaExpr::NameExpr(name_expr) => semantic_model
            .resolve_name(name_expr.get_position())
            .and_then(|decl_id| {
                semantic_model
                    .file_facts()
                    .and_then(|facts| facts.decl_by_id(&decl_id))
            })
            .is_some_and(|decl| decl.readonly),
        LuaExpr::IndexExpr(index_expr) => index_expr
            .get_prefix_expr()
            .is_some_and(|prefix| readonly_prefix(semantic_model, &prefix)),
        _ => false,
    }
}
