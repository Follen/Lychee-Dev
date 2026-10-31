//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::SelectedProjectionBackend;
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_parser::{LuaAstNode, LuaCallExpr};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_require_module_visibility(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for call in tree
            .get_red_root()
            .descendants()
            .filter_map(LuaCallExpr::cast)
        {
            self.checkpoint()?;
            if !call.is_require() {
                continue;
            }
            let Some(args) = call.get_args_list() else {
                continue;
            };
            let Some(arg) = args.get_args().next() else {
                continue;
            };
            let ty = self.type_of_expr(arg.get_syntax_id())?;
            let module_path = match ty.structural_type() {
                LuaType::StringConst(s) | LuaType::DocStringConst(s) => s.to_string(),
                _ => continue,
            };
            let Some(file) = self.model.module_file_of(&module_path) else {
                continue;
            };
            let Some(facts) = self.model.file_facts_of(file) else {
                continue;
            };
            if facts.module_visibility != emmylua_code_analysis::ModuleVisibility::Public {
                context.add_diagnostic(
                    DiagnosticCode::RequireModuleNotVisible,
                    arg.get_range(),
                    t!(
                        "module '%{module}' visibility is not `public`",
                        module = module_path
                    ),
                );
            }
        }
        Ok(())
    }
}
