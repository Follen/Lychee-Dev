//! Owned finite-parser AST roots with iterative destruction. A left-associated
//! expression can be much deeper than the active parser-depth bound before
//! logical admission rejects it, including on a later syntax/control failure.
//! Never let such roots use the public AST's recursive derived Drop path.
use super::*;
use std::ops::{Deref, DerefMut};

pub(super) struct Owned<T: AstNode>(Option<Box<T>>);
impl<T: AstNode> Owned<T> {
    pub(super) fn new(value: Box<T>) -> Self {
        Self(Some(value))
    }
    pub(super) fn into_box(mut self) -> Box<T> {
        self.0.take().expect("owned AST root")
    }
}
impl<T: AstNode> Deref for Owned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.0.as_deref().expect("owned AST root")
    }
}
impl<T: AstNode> DerefMut for Owned<T> {
    fn deref_mut(&mut self) -> &mut T {
        self.0.as_deref_mut().expect("owned AST root")
    }
}
impl<T: AstNode> Drop for Owned<T> {
    fn drop(&mut self) {
        if let Some(value) = self.0.take() {
            let mut pending = vec![T::pending(value)];
            while let Some(node) = pending.pop() {
                match node {
                    Pending::Expr(value) => expression(value, &mut pending),
                    Pending::Query(value) => query(value, &mut pending),
                    Pending::Body(value) => body(value, &mut pending),
                    Pending::Select(value) => select(value, &mut pending),
                    Pending::Relation(value) => relation(value, &mut pending),
                }
            }
        }
    }
}

pub(super) enum Pending {
    Expr(Box<Expr>),
    Query(Box<Query>),
    Body(Box<SetExpr>),
    Select(Box<Select>),
    Relation(Box<TableFactor>),
}
pub(super) trait AstNode {
    fn pending(value: Box<Self>) -> Pending;
}
macro_rules! node {
    ($ty:ty, $kind:ident) => {
        impl AstNode for $ty {
            fn pending(value: Box<Self>) -> Pending {
                Pending::$kind(value)
            }
        }
    };
}
node!(Expr, Expr);
node!(Query, Query);
node!(SetExpr, Body);
node!(Select, Select);
node!(TableFactor, Relation);

#[inline(never)]
fn expression(value: Box<Expr>, pending: &mut Vec<Pending>) {
    match *value {
        Expr::Nested(expr)
        | Expr::UnaryOp { expr, .. }
        | Expr::Cast { expr, .. }
        | Expr::IsNull(expr)
        | Expr::IsNotNull(expr) => pending.push(Pending::Expr(expr)),
        Expr::BinaryOp { left, right, .. } => {
            pending.extend([Pending::Expr(left), Pending::Expr(right)]);
        }
        Expr::Like {
            expr,
            pattern,
            escape_char,
            ..
        } => {
            pending.extend([Pending::Expr(expr), Pending::Expr(pattern)]);
            pending.extend(escape_char.map(Pending::Expr));
        }
        Expr::Between {
            expr, low, high, ..
        } => {
            pending.extend([Pending::Expr(expr), Pending::Expr(low), Pending::Expr(high)]);
        }
        Expr::InList { expr, list, .. } => {
            pending.push(Pending::Expr(expr));
            pending.extend(list.into_iter().map(|expr| Pending::Expr(Box::new(expr))));
        }
        Expr::InSubquery { expr, subquery, .. } => {
            pending.extend([Pending::Expr(expr), Pending::Query(subquery)]);
        }
        Expr::Subquery(query)
        | Expr::Exists {
            subquery: query, ..
        } => {
            pending.push(Pending::Query(query));
        }
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            pending.extend(operand.into_iter().chain(else_result).map(Pending::Expr));
            for CaseWhen { condition, result } in conditions {
                pending.extend([
                    Pending::Expr(Box::new(condition)),
                    Pending::Expr(Box::new(result)),
                ]);
            }
        }
        Expr::Function(function) => {
            if let FunctionArguments::List(arguments) = function.args {
                for argument in arguments.args {
                    if let FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) = argument {
                        pending.push(Pending::Expr(Box::new(expr)));
                    }
                }
            }
        }
        // All other variants emitted by the finite grammar are leaves. This
        // owner is not an entrypoint for arbitrary foreign sqlparser trees.
        _ => {}
    }
}
#[inline(never)]
fn query(value: Box<Query>, pending: &mut Vec<Pending>) {
    if let Some(with) = value.with {
        pending.extend(
            with.cte_tables
                .into_iter()
                .map(|cte| Pending::Query(cte.query)),
        );
    }
    pending.push(Pending::Body(value.body));
    if let Some(OrderBy {
        kind: OrderByKind::Expressions(order),
        ..
    }) = value.order_by
    {
        pending.extend(
            order
                .into_iter()
                .map(|order| Pending::Expr(Box::new(order.expr))),
        );
    }
    if let Some(LimitClause::LimitOffset {
        limit,
        offset,
        limit_by,
    }) = value.limit_clause
    {
        pending.extend(limit.into_iter().map(|expr| Pending::Expr(Box::new(expr))));
        pending.extend(
            offset
                .into_iter()
                .map(|offset| Pending::Expr(Box::new(offset.value))),
        );
        pending.extend(
            limit_by
                .into_iter()
                .map(|expr| Pending::Expr(Box::new(expr))),
        );
    }
}
#[inline(never)]
fn body(value: Box<SetExpr>, pending: &mut Vec<Pending>) {
    match *value {
        SetExpr::Select(select) => pending.push(Pending::Select(select)),
        SetExpr::Query(query) => pending.push(Pending::Query(query)),
        SetExpr::SetOperation { left, right, .. } => {
            pending.extend([Pending::Body(left), Pending::Body(right)]);
        }
        _ => {}
    }
}
#[inline(never)]
fn select(value: Box<Select>, pending: &mut Vec<Pending>) {
    for item in value.projection {
        match item {
            SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                pending.push(Pending::Expr(Box::new(expr)));
            }
            _ => {}
        }
    }
    for table in value.from {
        pending.push(Pending::Relation(Box::new(table.relation)));
        for join in table.joins {
            pending.push(Pending::Relation(Box::new(join.relation)));
            let constraint = match join.join_operator {
                JoinOperator::Join(c)
                | JoinOperator::Inner(c)
                | JoinOperator::Left(c)
                | JoinOperator::LeftOuter(c)
                | JoinOperator::CrossJoin(c) => Some(c),
                _ => None,
            };
            if let Some(JoinConstraint::On(expr)) = constraint {
                pending.push(Pending::Expr(Box::new(expr)));
            }
        }
    }
    pending.extend(
        value
            .selection
            .into_iter()
            .chain(value.having)
            .map(|expr| Pending::Expr(Box::new(expr))),
    );
    if let GroupByExpr::Expressions(groups, _) = value.group_by {
        pending.extend(groups.into_iter().map(|expr| Pending::Expr(Box::new(expr))));
    }
}
#[inline(never)]
fn relation(value: Box<TableFactor>, pending: &mut Vec<Pending>) {
    if let TableFactor::Derived { subquery, .. } = *value {
        pending.push(Pending::Query(subquery));
    }
}

/// A child is detached only into another iterative owner. The inert body has
/// no expressions/queries and cannot conceal a consumer or recurse on Drop.
#[inline(never)]
pub(super) fn take_body(query: &mut Query) -> Box<SetExpr> {
    std::mem::replace(
        &mut query.body,
        Box::new(SetExpr::Values(Values {
            explicit_row: false,
            value_keyword: false,
            rows: Vec::new(),
        })),
    )
}
#[inline(never)]
pub(super) fn suffix_owner(
    order_by: Option<OrderBy>,
    limit_clause: Option<LimitClause>,
) -> Owned<Query> {
    Owned::new(Box::new(Query {
        with: None,
        body: Box::new(SetExpr::Values(Values {
            explicit_row: false,
            value_keyword: false,
            rows: Vec::new(),
        })),
        order_by,
        limit_clause,
        fetch: None,
        locks: Vec::new(),
        for_clause: None,
        settings: None,
        format_clause: None,
        pipe_operators: Vec::new(),
    }))
}

pub(super) fn replace_with(query: &mut Query, with: Option<With>) {
    if let Some(old) = std::mem::replace(&mut query.with, with) {
        for cte in old.cte_tables {
            drop(Owned::new(cte.query));
        }
    }
}
pub(super) fn replace_order(query: &mut Query, order: Option<OrderBy>) {
    if let Some(OrderBy {
        kind: OrderByKind::Expressions(old),
        ..
    }) = std::mem::replace(&mut query.order_by, order)
    {
        for expr in old {
            drop(Owned::new(Box::new(expr.expr)));
        }
    }
}

#[inline(never)]
pub(super) fn take_expr(expr: &mut Expr) -> Owned<Expr> {
    Owned::new(Box::new(std::mem::replace(
        expr,
        Expr::Value(Value::Null.with_empty_span()),
    )))
}

#[inline(never)]
pub(super) fn take_query(query: &mut Query) -> Owned<Query> {
    let mut empty = suffix_owner(None, None);
    std::mem::swap(query, &mut empty);
    empty
}
