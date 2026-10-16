//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaDocType, LuaExpr, LuaLiteralToken};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_unnecessary_assert(
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
            if !call.is_assert() {
                continue;
            }
            let Some(args) = call.get_args_list() else {
                continue;
            };
            let Some(first) = args.get_args().next() else {
                continue;
            };
            let ty = if let Some(value) = self.binding_type_of_expr(first.get_syntax_id())? {
                value
            } else if let Some(ty) = doc_literal_type(&self.model, &first) {
                OwnedValue::plain(ty)
            } else {
                self.type_of_expr_at(first.get_syntax_id(), first.get_range().start().into())?
            };
            if ty.structural_type().is_always_truthy() {
                context.add_diagnostic(
                    DiagnosticCode::UnnecessaryAssert,
                    call.get_range(),
                    t!("Unnecessary assert: this expression is always truthy"),
                );
            } else if ty.structural_type().is_always_falsy() {
                context.add_diagnostic(
                    DiagnosticCode::UnnecessaryAssert,
                    call.get_range(),
                    t!("Impossible assert: this expression is always falsy; prefer `error()`"),
                );
            }
        }
        Ok(())
    }
}
fn doc_literal_type(
    semantic_model: &emmylua_code_analysis::SemanticModel<'_>,
    expr: &LuaExpr,
) -> Option<LuaType> {
    let LuaExpr::NameExpr(name_expr) = expr else {
        return None;
    };
    let decl_id = semantic_model.resolve_name(name_expr.get_position())?;
    let facts = semantic_model.file_facts()?;
    let decl = facts.decl_by_id(&decl_id)?;
    let syntax = decl.doc_type_syntax?;
    let tree = semantic_model.syntax_tree()?;
    let node = syntax.to_node_from_root(&tree.get_red_root())?;
    let LuaDocType::Literal(literal) = LuaDocType::cast(node)? else {
        return None;
    };
    match literal.get_literal()? {
        LuaLiteralToken::Bool(bool_token) => Some(LuaType::BooleanConst(bool_token.is_true())),
        LuaLiteralToken::Number(number_token) => match number_token.get_number_value() {
            emmylua_parser::NumberResult::Int(i) => Some(LuaType::IntegerConst(i)),
            _ => Some(LuaType::Number),
        },
        LuaLiteralToken::String(string_token) => Some(LuaType::StringConst(
            smol_str::SmolStr::new(string_token.get_value()).into(),
        )),
        LuaLiteralToken::Nil(_) => Some(LuaType::Nil),
        _ => None,
    }
}
