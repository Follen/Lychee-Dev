//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_parser::{LuaAstNode, LuaExpr, LuaIfStat};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_unnecessary_if(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for stat in tree
            .get_red_root()
            .descendants()
            .filter_map(LuaIfStat::cast)
        {
            if let Some(condition) = stat.get_condition_expr() {
                self.unnecessary_if_condition(context, &condition)?;
            }
            for clause in stat.get_else_if_clause_list() {
                if let Some(condition) = clause.get_condition_expr() {
                    self.unnecessary_if_condition(context, &condition)?;
                }
            }
        }
        Ok(())
    }
    fn unnecessary_if_condition(
        &mut self,
        context: &mut CheckContext<'_>,
        condition: &LuaExpr,
    ) -> QueryResult<(), E> {
        let ty = self.type_of_expr(condition.get_syntax_id())?;
        if ty.structural_type().is_always_truthy() {
            context.add_diagnostic(
                DiagnosticCode::UnnecessaryIf,
                condition.get_range(),
                t!("Unnecessary `if` statement: this condition is always truthy"),
            );
        } else if ty.structural_type().is_always_falsy() {
            context.add_diagnostic(
                DiagnosticCode::UnnecessaryIf,
                condition.get_range(),
                t!("Impossible `if` statement: this condition is always falsy"),
            );
        }
        Ok(())
    }
}
