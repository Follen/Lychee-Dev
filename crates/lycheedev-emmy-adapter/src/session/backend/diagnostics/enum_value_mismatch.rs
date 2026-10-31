//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::SelectedProjectionBackend;
use emmylua_code_analysis::TypeDefKind;
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_parser::{BinaryOperator, LuaAst, LuaAstNode, LuaExpr, LuaLiteralToken};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_enum_value_mismatch(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for node in tree.get_red_root().descendants().filter_map(LuaAst::cast) {
            self.checkpoint()?;
            let condition = match node {
                LuaAst::LuaIfStat(stat) => stat.get_condition_expr(),
                LuaAst::LuaElseIfClauseStat(stat) => stat.get_condition_expr(),
                _ => None,
            };
            let Some(LuaExpr::BinaryExpr(binary)) = condition else {
                continue;
            };
            let Some(op) = binary.get_op_token() else {
                continue;
            };
            if !matches!(op.get_op(), BinaryOperator::OpEq | BinaryOperator::OpNe) {
                continue;
            }
            let Some((left, right)) = binary.get_exprs() else {
                continue;
            };
            let left_ty = self.type_of_expr(left.get_syntax_id())?;
            let right_ty = self.type_of_expr(right.get_syntax_id())?;
            check_pair(context, &self.model, &right, left_ty.structural_type());
            check_pair(context, &self.model, &left, right_ty.structural_type());
        }
        Ok(())
    }
}
fn check_pair(
    context: &mut CheckContext<'_>,
    semantic_model: &emmylua_code_analysis::SemanticModel<'_>,
    value_expr: &LuaExpr,
    enum_type: &LuaType,
) {
    let (LuaType::Ref(id) | LuaType::Def(id)) = enum_type else {
        return;
    };
    let Some(def) = semantic_model.type_def_of(id) else {
        return;
    };
    if def.kind != TypeDefKind::Enum {
        return;
    }
    // Constant literal text.
    let Some(value_text) = literal_text(value_expr) else {
        return;
    };
    // Enum value text set: `@field` (owner=TypeDef) plus runtime table fields (`local Status = {...}`, owner=Decl).
    let tree = semantic_model.syntax_tree().expect("tree");
    let root = tree.get_red_root();
    let mut member_refs = semantic_model.members_of_owner(&def.id).as_slice().to_vec();
    if let Some(facts) = semantic_model.file_facts_of(def.file_id)
        && let Some(decl) = facts.decl_named(def.name.as_str())
    {
        member_refs.extend(semantic_model.members_of_owner(&decl.id).iter().cloned());
    }
    let enum_value_texts: Vec<String> = member_refs
        .iter()
        .filter_map(|member_ref| {
            let facts = semantic_model.file_facts_of(member_ref.file_id)?;
            let member = facts.member_by_id(&member_ref.id)?;
            let value_syntax = member.value_syntax?;
            let node = value_syntax.to_node_from_root(&root)?;
            // Runtime member: LuaExpr literal; `@field`: doc type literal node.
            if let Some(text) = LuaExpr::cast(node.clone()).and_then(|expr| literal_text(&expr)) {
                return Some(text);
            }
            let raw = node.text().to_string();
            Some(raw.trim_matches(['"', '\'']).to_string())
        })
        .collect();
    if enum_value_texts.iter().any(|text| text == &value_text) {
        return;
    }
    context.add_diagnostic(
        DiagnosticCode::EnumValueMismatch,
        value_expr.get_range(),
        t!(
            "Value '%{value}' does not match any enum value. Expected one of: %{enum_values}",
            value = value_text,
            enum_values = enum_value_texts.join(", ")
        ),
    );
}

/// Text of a literal expression (number / string / boolean).
fn literal_text(expr: &LuaExpr) -> Option<String> {
    let LuaExpr::LiteralExpr(lit) = expr else {
        return None;
    };
    match lit.get_literal()? {
        LuaLiteralToken::Number(number) => Some(format!("{}", number.get_number_value())),
        LuaLiteralToken::String(str) => Some(str.get_value()),
        LuaLiteralToken::Bool(bool) => Some(bool.is_true().to_string()),
        LuaLiteralToken::Nil(_) => Some("nil".to_string()),
        _ => None,
    }
}
