//! Reconstruct the Go retrieve()/expression() stack from the retained AST.
//! Parentheses count here even though lowering erases them. Left-associative
//! infix chains share an expression frame; only their RHS recursively enters.
//! This is admission of an already parsed tree, not a second SQL interpreter.
use super::*;

pub(super) fn validate(
    query: &Query,
    tokens: &[sqlparser::tokenizer::TokenWithSpan],
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    use sqlparser::tokenizer::Token;
    // sqlparser's projection wildcard fast path erases exactly one (*) pair.
    // Recover that parser frame from the unchanged token span, without
    // interpreting identifiers or replaying the query parser.
    let wildcard_frame = |options: &WildcardAdditionalOptions| -> usize {
        let location = options.wildcard_token.0.span.start;
        let Ok(at) = tokens.binary_search_by_key(&location, |token| token.span.start) else {
            return 0;
        };
        usize::from(
            at > 0
                && tokens[at - 1].token == Token::LParen
                && tokens
                    .get(at + 1)
                    .is_some_and(|token| token.token == Token::RParen),
        )
    };
    enum Node<'a> {
        Query(&'a Query),
        Select(&'a Select),
        Expr(&'a Expr),
        Relation(&'a TableFactor),
        Leaf,
    }
    let mut pending = vec![(Node::Query(query), 1usize)];
    while let Some((node, depth)) = pending.pop() {
        check()?;
        if depth > 128 {
            return Err(budget());
        }
        match node {
            Node::Query(query) => {
                if let Some(with) = &query.with {
                    for cte in &with.cte_tables {
                        pending.push((Node::Query(&cte.query), depth + 1));
                    }
                }
                // sqlparser builds left-associated set nodes. Go UNION is a
                // right-linked chain, whose still-active retrieve frames grow.
                let mut bodies = vec![query.body.as_ref()];
                let mut branches = Vec::new();
                while let Some(body) = bodies.pop() {
                    match body {
                        SetExpr::SetOperation { left, right, .. } => {
                            bodies.push(right);
                            bodies.push(left);
                        }
                        _ => branches.push(body),
                    }
                }
                let tail = depth + branches.len().saturating_sub(1);
                for (i, body) in branches.into_iter().enumerate() {
                    match body {
                        SetExpr::Select(select) => pending.push((Node::Select(select), depth + i)),
                        // Query wrappers are created by the retrieval shell for
                        // Go branches, not source parentheses or extra frames.
                        SetExpr::Query(query) => pending.push((Node::Query(query), depth + i)),
                        _ => {}
                    }
                }
                if let Some(OrderBy {
                    kind: OrderByKind::Expressions(order),
                    ..
                }) = &query.order_by
                {
                    pending.extend(order.iter().map(|o| (Node::Expr(&o.expr), tail + 1)));
                }
                // count() consumes its literal/parameter without expression().
            }
            Node::Select(select) => {
                for projection in &select.projection {
                    match projection {
                        SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                            pending.push((Node::Expr(expr), depth + 1))
                        }
                        SelectItem::Wildcard(options) => {
                            pending.push((Node::Leaf, depth + 1 + wildcard_frame(options)))
                        }
                        _ => pending.push((Node::Leaf, depth + 1)),
                    }
                }
                for table in &select.from {
                    pending.push((Node::Relation(&table.relation), depth));
                    for join in &table.joins {
                        pending.push((Node::Relation(&join.relation), depth));
                        let condition = match &join.join_operator {
                            JoinOperator::Join(c)
                            | JoinOperator::Inner(c)
                            | JoinOperator::Left(c)
                            | JoinOperator::LeftOuter(c) => Some(c),
                            _ => None,
                        };
                        if let Some(JoinConstraint::On(expr)) = condition {
                            pending.push((Node::Expr(expr), depth + 1));
                        }
                    }
                }
                pending.extend(
                    select
                        .selection
                        .iter()
                        .chain(select.having.iter())
                        .map(|e| (Node::Expr(e), depth + 1)),
                );
                if let GroupByExpr::Expressions(expressions, _) = &select.group_by {
                    pending.extend(expressions.iter().map(|e| (Node::Expr(e), depth + 1)));
                }
            }
            Node::Relation(TableFactor::Derived { subquery, .. }) => {
                pending.push((Node::Query(subquery), depth + 1))
            }
            Node::Relation(_) | Node::Leaf => {}
            Node::Expr(expr) => match expr {
                Expr::Nested(expr) | Expr::UnaryOp { expr, .. } | Expr::Cast { expr, .. } => {
                    pending.push((Node::Expr(expr), depth + 1))
                }
                Expr::BinaryOp { left, right, .. }
                | Expr::Like {
                    expr: left,
                    pattern: right,
                    ..
                } => {
                    pending.push((Node::Expr(left), depth));
                    pending.push((Node::Expr(right), depth + 1));
                }
                Expr::IsNull(expr) | Expr::IsNotNull(expr) => {
                    pending.push((Node::Expr(expr), depth))
                }
                Expr::Between {
                    expr, low, high, ..
                } => {
                    pending.push((Node::Expr(expr), depth));
                    pending.push((Node::Expr(low), depth + 1));
                    pending.push((Node::Expr(high), depth + 1));
                }
                Expr::InList { expr, list, .. } => {
                    pending.push((Node::Expr(expr), depth));
                    pending.extend(list.iter().map(|e| (Node::Expr(e), depth + 1)));
                }
                Expr::InSubquery { expr, subquery, .. } => {
                    pending.push((Node::Expr(expr), depth));
                    pending.push((Node::Query(subquery), depth + 1));
                }
                Expr::Subquery(query) => pending.push((Node::Query(query), depth + 1)),
                Expr::Exists { subquery, negated } => {
                    pending.push((Node::Query(subquery), depth + 1 + usize::from(*negated)))
                }
                Expr::Function(function) => {
                    if let FunctionArguments::List(arguments) = &function.args {
                        for argument in &arguments.args {
                            match argument {
                                FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => {
                                    pending.push((Node::Expr(expr), depth + 1))
                                }
                                _ => pending.push((Node::Leaf, depth + 1)),
                            }
                        }
                    }
                }
                Expr::Case {
                    operand,
                    conditions,
                    else_result,
                    ..
                } => {
                    pending.extend(
                        operand
                            .iter()
                            .chain(else_result.iter())
                            .map(|e| (Node::Expr(e), depth + 1)),
                    );
                    for case in conditions {
                        pending.push((Node::Expr(&case.condition), depth + 1));
                        pending.push((Node::Expr(&case.result), depth + 1));
                    }
                }
                _ => {}
            },
        }
    }
    Ok(())
}
