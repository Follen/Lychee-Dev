//! Owned query-sensitive diagnostic; query failures preserve caller control.
use super::super::{OwnedQuery, QueryResult};
use super::CheckContext;
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{DiagnosticCode, LuaType};
use emmylua_parser::{LuaAssignStat, LuaAstNode, LuaExpr, LuaIndexExpr, PathTrait};
impl<E> OwnedQuery<'_, '_, E> {
    pub(in crate::session::backend) fn check_need_check_nil(
        &mut self,
        context: &mut CheckContext<'_>,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let Some(tree) = self.model.syntax_tree() else {
            return Ok(());
        };
        for expr in tree.get_red_root().descendants().filter_map(LuaExpr::cast) {
            self.checkpoint()?;
            match expr {
                LuaExpr::CallExpr(call) => {
                    if call.has_safe_navigation() {
                        continue;
                    }
                    let Some(prefix) = call.get_prefix_expr() else {
                        continue;
                    };
                    let ty = self.nil_prefix_type(&prefix)?;
                    if ty.structural_type().is_nullable() {
                        context.add_diagnostic(
                            DiagnosticCode::NeedCheckNil,
                            prefix.get_range(),
                            t!(
                                "function `%{name}` may be nil",
                                name = prefix.syntax().text()
                            ),
                        );
                    }
                }
                LuaExpr::IndexExpr(index) => {
                    if index.is_safe_index() {
                        continue;
                    }
                    let Some(prefix) = index.get_prefix_expr() else {
                        continue;
                    };
                    let ty = self.nil_prefix_type(&prefix)?;
                    if ty.structural_type().is_nullable()
                        && !assign_rhs_asserts_lhs_prefix(&prefix, &index)
                    {
                        context.add_diagnostic(
                            DiagnosticCode::NeedCheckNil,
                            prefix.get_range(),
                            t!("%{name} may be nil", name = prefix.syntax().text()),
                        );
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }
    fn nil_prefix_type(&mut self, prefix: &LuaExpr) -> QueryResult<OwnedValue, E> {
        if let LuaExpr::NameExpr(name) = prefix
            && let Some(decl) = self.model.resolve_name(name.get_position())
        {
            return self.type_of_decl_at(&decl, name.get_position().into());
        }
        let ty = self.type_of_expr_at(prefix.get_syntax_id(), prefix.get_range().start().into())?;
        if !matches!(ty.structural_type(), LuaType::Unknown) {
            return Ok(ty);
        }
        let LuaExpr::IndexExpr(index) = prefix else {
            return Ok(ty);
        };
        let Some(inner) = index.get_prefix_expr() else {
            return Ok(ty);
        };
        let inner_ty = self.type_of_expr(inner.get_syntax_id())?;
        match inner_ty.structural_type() {
            LuaType::Array(array) => {
                let base = self.owned_child(&inner_ty, 0, array.get_base());
                if matches!(
                    base.structural_type(),
                    LuaType::DocFunction(_) | LuaType::Signature(_) | LuaType::Function
                ) {
                    Ok(base)
                } else if self.model.strict_array_index() || base.structural_type().is_nullable() {
                    Ok(OwnedValue::raw_union(vec![
                        base,
                        OwnedValue::plain(LuaType::Nil),
                    ]))
                } else {
                    Ok(base)
                }
            }
            LuaType::Union(_) => {
                let mut types = Vec::new();
                let mut found = false;
                for component in inner_ty.union_components().expect("union") {
                    self.checkpoint()?;
                    if let LuaType::Array(array) = component.structural_type() {
                        let base = self.owned_child(&component, 0, array.get_base());
                        found = true;
                        if self.model.strict_array_index() || base.structural_type().is_nullable() {
                            types.push(base);
                            types.push(OwnedValue::plain(LuaType::Nil));
                        } else {
                            types.push(base);
                        }
                    }
                }
                if found {
                    if types.len() == 1 {
                        Ok(types.pop().expect("len checked"))
                    } else {
                        Ok(OwnedValue::raw_union(types))
                    }
                } else {
                    Ok(ty)
                }
            }
            _ => Ok(ty),
        }
    }
}
fn assign_rhs_asserts_lhs_prefix(prefix: &LuaExpr, index_expr: &LuaIndexExpr) -> bool {
    let Some(assign) = index_expr.ancestors::<LuaAssignStat>().next() else {
        return false;
    };
    let (vars, exprs) = assign.get_var_and_expr_list();
    let index_range = index_expr.get_range();
    // The checked index must belong to the assignment LHS (the outer access of `res[1][1]`).
    if !vars
        .iter()
        .any(|var| var.get_range().contains_range(index_range))
    {
        return false;
    }
    let Some(prefix_path) = expr_access_path(prefix) else {
        return false;
    };
    exprs
        .iter()
        .any(|expr| expr_contains_asserted_path(expr, &prefix_path))
}

fn expr_contains_asserted_path(expr: &LuaExpr, expected_path: &str) -> bool {
    match expr {
        // Closure bodies do not run immediately during this assignment; asserts inside them cannot protect the current LHS access.
        LuaExpr::ClosureExpr(_) => false,
        LuaExpr::CallExpr(call_expr) => {
            if call_expr.is_assert()
                && call_expr
                    .get_args_list()
                    .and_then(|args| args.get_args().next())
                    .and_then(|arg| expr_access_path(&arg))
                    .as_deref()
                    == Some(expected_path)
            {
                return true;
            }
            if call_expr
                .get_prefix_expr()
                .is_some_and(|prefix| expr_contains_asserted_path(&prefix, expected_path))
            {
                return true;
            }
            call_expr.get_args_list().is_some_and(|args| {
                args.get_args()
                    .any(|arg| expr_contains_asserted_path(&arg, expected_path))
            })
        }
        LuaExpr::ParenExpr(paren_expr) => paren_expr
            .get_expr()
            .is_some_and(|inner| expr_contains_asserted_path(&inner, expected_path)),
        _ => expr
            .children::<LuaExpr>()
            .any(|child| expr_contains_asserted_path(&child, expected_path)),
    }
}

fn expr_access_path(expr: &LuaExpr) -> Option<String> {
    match expr {
        LuaExpr::NameExpr(name_expr) => name_expr.get_access_path().map(|path| path.to_string()),
        LuaExpr::IndexExpr(index_expr) => index_expr.get_access_path().map(|path| path.to_string()),
        LuaExpr::ParenExpr(paren_expr) => expr_access_path(&paren_expr.get_expr()?),
        _ => None,
    }
}
