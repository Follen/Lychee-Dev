//! Adapted from EmmyLua Analyzer Rust 08b0873962e83bb8307e9a81052e3f41578fbc78,
//! check/checker/check_export.rs; Copyright (c) CppCXY, MIT (LICENSE-EmmyLua).
use super::CheckContext;
use crate::session::backend::{OwnedQuery, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{
    DiagnosticCode, FileFacts, FileId, InFiled, LuaMemberKey, LuaType, ModuleExport,
    humanize_semantic_type,
};
use emmylua_parser::{
    LuaAst, LuaAstNode, LuaCallExpr, LuaExpr, LuaIndexExpr, LuaIndexKey, LuaSyntaxKind, LuaVarExpr,
    NumberResult,
};
use std::collections::HashSet;

impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_export(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        let root = tree.get_red_root();
        let mut checked = HashSet::new();
        for node in root.descendants().filter_map(LuaAst::cast) {
            self.checkpoint()?;
            match node {
                LuaAst::LuaAssignStat(assign) => {
                    let (vars, _) = assign.get_var_and_expr_list();
                    for var in &vars {
                        if let LuaVarExpr::IndexExpr(index) = var {
                            checked.insert(index.get_syntax_id());
                            self.export_check_index(context, index, DiagnosticCode::InjectField)?;
                        }
                    }
                }
                LuaAst::LuaIndexExpr(index) => {
                    if !checked.contains(&index.get_syntax_id()) {
                        self.export_check_index(context, &index, DiagnosticCode::UndefinedField)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn export_check_index(
        &mut self,
        context: &mut CheckContext<'_>,
        index: &LuaIndexExpr,
        code: DiagnosticCode,
    ) -> QueryResult<(), E> {
        let Some(prefix) = index.get_prefix_expr() else {
            return Ok(());
        };
        // Inference intentionally precedes recognizing the require import.
        let prefix_ty = self.type_of_expr(prefix.get_syntax_id())?;
        // This checker is specifically about an imported module's export
        // surface. An authoritative selected value is not that old import,
        // even when its source spelling still names the original declaration.
        // General field/type checkers inspect the actual selected shape.
        if !prefix_ty.is_context_free() || self.binding_manages_call(prefix.get_syntax_id())? {
            return Ok(());
        }
        let Some(file) = self.export_import_module(&prefix)? else {
            return Ok(());
        };
        let Some(facts) = self.model.file_facts_of(file) else {
            return Ok(());
        };
        let Some(export) = export_table_type(file, facts) else {
            return Ok(());
        };
        let Some(index_key) = index.get_index_key() else {
            return Ok(());
        };
        let Some(key) = self.export_member_key(&index_key)? else {
            return Ok(());
        };
        if self
            .member_infos_with_key(&OwnedValue::plain(export), &key)?
            .into_iter()
            .any(|info| info.file_id.is_none_or(|found| found == file))
        {
            return Ok(());
        }
        let field = index_key.get_path_part();
        let range = index_key.get_range().unwrap_or_else(|| index.get_range());
        match code {
            DiagnosticCode::InjectField => context.add_diagnostic(
                code,
                range,
                t!(
                    "Fields cannot be injected into the reference of `%{class}` for `%{field}`. ",
                    class = humanize_semantic_type(&self.model, prefix_ty.structural_type()),
                    field = field
                ),
            ),
            DiagnosticCode::UndefinedField => context.add_diagnostic(
                code,
                range,
                t!("Undefined field `%{field}`. ", field = field),
            ),
            _ => {}
        }
        Ok(())
    }
    fn export_import_module(&mut self, prefix: &LuaExpr) -> QueryResult<Option<FileId>, E> {
        let call = match prefix {
            LuaExpr::CallExpr(call) if call.is_require() => call.clone(),
            LuaExpr::NameExpr(name) => {
                let call = (|| {
                    let id = self.model.resolve_name(name.get_position())?;
                    let decl = self.model.file_facts()?.decl_by_id(&id)?;
                    let syntax = decl.value_expr_syntax?;
                    let tree = self.model.syntax_tree()?;
                    let call = LuaCallExpr::cast(syntax.to_node_from_root(&tree.get_red_root())?)?;
                    call.is_require().then_some(call)
                })();
                let Some(call) = call else { return Ok(None) };
                call
            }
            _ => return Ok(None),
        };
        // The declaration can be unmanaged while its initializer was selected.
        // Do not recover the original module through that initializer's syntax.
        if self.binding_manages_call(call.get_syntax_id())? {
            return Ok(None);
        }
        let Some(arg) = call.get_args_list().and_then(|list| list.get_args().next()) else {
            return Ok(None);
        };
        let value = self.type_of_expr(arg.get_syntax_id())?;
        let name = match value.structural_type() {
            LuaType::StringConst(s) | LuaType::DocStringConst(s) => s.as_ref().to_string(),
            _ => return Ok(None),
        };
        Ok(self.model.module_file_of(&name))
    }
    pub(in crate::session::backend) fn export_member_key(
        &mut self,
        key: &LuaIndexKey,
    ) -> QueryResult<Option<LuaMemberKey>, E> {
        Ok(match key {
            LuaIndexKey::Name(name) => Some(LuaMemberKey::Name(name.get_name_text().into())),
            LuaIndexKey::String(s) => Some(LuaMemberKey::Name(s.get_value().into())),
            LuaIndexKey::Integer(i) => match i.get_number_value() {
                NumberResult::Int(i) => Some(LuaMemberKey::Integer(i)),
                _ => None,
            },
            LuaIndexKey::Idx(i) => Some(LuaMemberKey::Integer(*i as i64)),
            LuaIndexKey::Expr(expr) => {
                match self.type_of_expr(expr.get_syntax_id())?.structural_type() {
                    LuaType::StringConst(s) | LuaType::DocStringConst(s) => {
                        Some(LuaMemberKey::Name(s.as_ref().clone()))
                    }
                    LuaType::IntegerConst(i) | LuaType::DocIntegerConst(i) => {
                        Some(LuaMemberKey::Integer(*i))
                    }
                    _ => None,
                }
            }
        })
    }
}

fn export_table_type(module_file: FileId, facts: &FileFacts) -> Option<LuaType> {
    let syntax = match &facts.module_export {
        ModuleExport::Expr { value_syntax } => Some(*value_syntax),
        ModuleExport::Decl { decl, .. } => facts
            .decl_by_id(decl)
            .and_then(|decl| decl.value_expr_syntax),
        ModuleExport::Global { .. } | ModuleExport::None => None,
    }?;
    if !matches!(
        syntax.get_kind(),
        LuaSyntaxKind::TableArrayExpr
            | LuaSyntaxKind::TableObjectExpr
            | LuaSyntaxKind::TableEmptyExpr
    ) {
        return None;
    }
    Some(LuaType::TableConst(InFiled::new(
        module_file,
        syntax.get_range(),
    )))
}
