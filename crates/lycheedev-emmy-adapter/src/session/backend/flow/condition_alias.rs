use super::*;
use emmylua_parser::{BinaryOperator, LuaLiteralToken, LuaNameExpr, UnaryOperator};
impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn array_len_narrowing(
        &mut self,
        decl: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        let mut expr = expr.clone();
        let mut truthy = truthy;
        loop {
            self.checkpoint()?;
            match expr {
                LuaExpr::ParenExpr(paren) => {
                    let Some(inner) = paren.get_expr() else {
                        return Ok(None);
                    };
                    expr = inner;
                }
                LuaExpr::UnaryExpr(unary)
                    if unary
                        .get_op_token()
                        .is_some_and(|op| op.get_op() == UnaryOperator::OpNot) =>
                {
                    let Some(inner) = unary.get_expr() else {
                        return Ok(None);
                    };
                    expr = inner;
                    truthy = !truthy;
                }
                _ => break,
            }
        }
        let LuaExpr::BinaryExpr(binary) = expr else {
            return Ok(None);
        };
        let (Some(op), Some((left, right))) = (binary.get_op_token(), binary.get_exprs()) else {
            return Ok(None);
        };
        let (unary, literal) = match (left, right) {
            (LuaExpr::UnaryExpr(unary), LuaExpr::LiteralExpr(literal))
            | (LuaExpr::LiteralExpr(literal), LuaExpr::UnaryExpr(unary)) => (unary, literal),
            _ => return Ok(None),
        };
        if !unary
            .get_op_token()
            .is_some_and(|op| op.get_op() == UnaryOperator::OpLen)
        {
            return Ok(None);
        }
        let Some(LuaExpr::NameExpr(name)) = unary.get_expr() else {
            return Ok(None);
        };
        if self.model.resolve_name(name.get_position()).as_ref() != Some(decl) {
            return Ok(None);
        }
        let bound = match guards::literal_type(&literal) {
            Some(LuaType::IntegerConst(value) | LuaType::DocIntegerConst(value)) => value,
            _ => return Ok(None),
        };
        Ok(match op.get_op() {
            BinaryOperator::OpEq | BinaryOperator::OpLe if truthy => {
                Some(Narrowing::ArrayLen(bound))
            }
            BinaryOperator::OpLe => Some(
                bound
                    .checked_add(1)
                    .map(Narrowing::ArrayMinLen)
                    .unwrap_or_else(|| Narrowing::Replace(OwnedValue::plain(LuaType::Never))),
            ),
            BinaryOperator::OpLt if truthy => bound.checked_sub(1).map(Narrowing::ArrayLen),
            BinaryOperator::OpLt => Some(Narrowing::ArrayMinLen(bound)),
            BinaryOperator::OpGe if truthy => Some(Narrowing::ArrayMinLen(bound)),
            BinaryOperator::OpGe => bound.checked_sub(1).map(Narrowing::ArrayLen),
            BinaryOperator::OpGt if truthy => bound.checked_add(1).map(Narrowing::ArrayMinLen),
            BinaryOperator::OpGt => Some(Narrowing::ArrayLen(bound)),
            _ => None,
        })
    }
    pub(super) fn local_initializer(&self, name: &LuaNameExpr) -> Option<LuaExpr> {
        let decl = self.model.resolve_name(name.get_position())?;
        let SemanticId::Decl(key) = &decl else {
            return None;
        };
        let syntax = self
            .model
            .file_facts_of(key.file_id)?
            .decl_by_id(&decl)?
            .value_expr_syntax?;
        LuaExpr::cast(
            syntax.to_node_from_root(&self.model.syntax_tree_of(key.file_id)?.get_red_root())?,
        )
    }
    pub(super) fn alias_narrowing(
        &mut self,
        decl: &SemanticId,
        expr: &LuaExpr,
        truthy: bool,
        prior: bool,
    ) -> QueryResult<Option<Narrowing>, E> {
        self.checkpoint()?;
        if let LuaExpr::BinaryExpr(binary) = expr {
            let (Some(op), Some((left, right))) = (binary.get_op_token(), binary.get_exprs())
            else {
                return Ok(None);
            };
            if matches!(op.get_op(), BinaryOperator::OpEq | BinaryOperator::OpNe) {
                let (name, literal) = match (left, right) {
                    (LuaExpr::NameExpr(name), LuaExpr::LiteralExpr(literal))
                    | (LuaExpr::LiteralExpr(literal), LuaExpr::NameExpr(name)) => (name, literal),
                    _ => return Ok(None),
                };
                if let Some(LuaExpr::CallExpr(call)) = self.local_initializer(&name)
                    && let Some(LuaExpr::NameExpr(callee)) = call.get_prefix_expr()
                    && callee.get_name_text().as_deref() == Some("type")
                    && let Some(LuaExpr::NameExpr(arg)) =
                        call.get_args_list().and_then(|args| args.get_args().next())
                    && self.model.resolve_name(arg.get_position()).as_ref() == Some(decl)
                    && let Some(LuaLiteralToken::String(value)) = literal.get_literal()
                {
                    return Ok(guards::primitive(&value.get_value()).map(|primitive| {
                        Narrowing::Primitive {
                            primitive,
                            keep: if op.get_op() == BinaryOperator::OpEq {
                                truthy
                            } else {
                                !truthy
                            },
                        }
                    }));
                }
            }
        }
        let LuaExpr::NameExpr(name) = expr else {
            return Ok(None);
        };
        let Some(initializer) = self.local_initializer(name) else {
            return Ok(None);
        };
        if matches!(initializer, LuaExpr::NameExpr(_)) {
            return Ok(None);
        }
        self.direct_decl_narrowing(decl, &initializer, truthy, prior)
    }
}
