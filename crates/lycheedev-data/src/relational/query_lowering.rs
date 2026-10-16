//! Query and expression lowering share one heap continuation stack. The frame
//! owns the remaining public AST and partial Program, so errors/cancellation
//! use their iterative owners without retaining recursive native callers.
use super::*;
use ast_owner::Owned;
use lowering::Task as Next;
use std::collections::VecDeque;

pub(super) struct Task(Box<Frame>);
struct Frame {
    query: Owned<Query>,
    output: Program,
    depth: usize,
    scope: usize,
    names: BTreeSet<String>,
    stage: Stage,
}
enum Stage {
    Start,
    Cte(usize),
    CteDone(String, usize),
    Body,
    Branch(VecDeque<Owned<Query>>),
    BranchDone(VecDeque<Owned<Query>>),
    Base,
    BaseDone(RelationName, String),
    Join(usize),
    JoinDerived(usize, RelationName, String),
    JoinCondition(usize),
    JoinDone(usize),
    Projection(usize),
    ProjectionDone(usize),
    Group(usize),
    GroupDone(usize),
    Having,
    HavingDone,
    Filter,
    FilterDone,
    OrderBegin,
    Order(usize),
    OrderDone(usize, bool, Option<bool>),
    Counts,
    Finish,
}
impl Task {
    pub(super) fn new(query: Owned<Query>, depth: usize, scope: usize) -> Self {
        Self(Box::new(Frame {
            query,
            depth,
            scope,
            names: BTreeSet::new(),
            stage: Stage::Start,
            output: Program {
                explain: false,
                analyze: false,
                recursive: false,
                bindings: vec![],
                branches: vec![],
                derived: None,
                table: RelationName::internal("\0pending"),
                alias: String::new(),
                projection: vec![],
                filter: None,
                order: vec![],
                parameters: BTreeSet::new(),
                limit: None,
                offset: None,
                distinct: false,
                joins: vec![],
                groups: vec![],
                having: None,
            },
        }))
    }
}
fn resume(mut frame: Box<Frame>, stage: Stage, tasks: &mut Vec<Next>) {
    frame.stage = stage;
    tasks.push(Next::Query(Task(frame)));
}
fn child(
    frame: Box<Frame>,
    stage: Stage,
    query: Owned<Query>,
    depth: usize,
    tasks: &mut Vec<Next>,
) {
    resume(frame, stage, tasks);
    tasks.push(Next::Query(Task::new(query, depth, usize::MAX)));
}
fn expression(frame: Box<Frame>, stage: Stage, expr: Owned<Expr>, tasks: &mut Vec<Next>) {
    let (depth, scope) = (frame.depth, frame.scope);
    resume(frame, stage, tasks);
    tasks.push(Next::Visit(expr, depth, scope));
}
fn select(frame: &mut Frame) -> Result<&mut Select> {
    select_body(&mut frame.query.body)
}
fn term_result(values: &mut Vec<Term>) -> Result<Term> {
    values.pop().ok_or_else(unsupported)
}
fn query_result(result: &mut Option<Program>) -> Result<Program> {
    result.take().ok_or_else(unsupported)
}

pub(super) fn step(
    task: Task,
    tasks: &mut Vec<Next>,
    values: &mut Vec<Term>,
    scopes: &mut Vec<BTreeSet<String>>,
    result: &mut Option<Program>,
    check: &dyn Fn() -> Result<()>,
) -> Result<()> {
    let Task(mut frame) = task;
    let stage = std::mem::replace(&mut frame.stage, Stage::Finish);
    match stage {
        Stage::Start => {
            if frame.depth > 256 {
                return Err(budget());
            }
            if frame.scope == usize::MAX {
                frame.scope = scopes.len();
                scopes.push(BTreeSet::new());
            }
            frame.output.recursive = frame.query.with.as_ref().is_some_and(|with| with.recursive);
            resume(frame, Stage::Cte(0), tasks);
        }
        Stage::Cte(index) => {
            let Some(cte) = frame
                .query
                .with
                .as_mut()
                .and_then(|with| with.cte_tables.get_mut(index))
            else {
                resume(frame, Stage::Body, tasks);
                return Ok(());
            };
            check()?;
            if cte.from.is_some() || cte.materialized.is_some() || !cte.alias.columns.is_empty() {
                return Err(unsupported());
            }
            let name = syntax::identifier(&cte.alias.name)?.to_owned();
            if !frame.names.insert(identifier_lower(&name)) {
                return Err(problem("relational.invalid_syntax", "Duplicate CTE name"));
            }
            let query = ast_owner::take_query(&mut cte.query);
            let depth = frame.depth + 1;
            child(frame, Stage::CteDone(name, index), query, depth, tasks);
        }
        Stage::CteDone(name, index) => {
            frame.output.bindings.push((name, query_result(result)?));
            resume(frame, Stage::Cte(index + 1), tasks);
        }
        Stage::Body => {
            let query = &mut *frame.query;
            if query.fetch.is_some()
                || !query.locks.is_empty()
                || query.for_clause.is_some()
                || query.settings.is_some()
                || query.format_clause.is_some()
                || !query.pipe_operators.is_empty()
            {
                return Err(unsupported());
            }
            if matches!(*query.body, SetExpr::SetOperation { .. }) {
                let mut bodies = compound::branches(ast_owner::take_body(query), check)?;
                let tail = bodies.last_mut().ok_or_else(unsupported)?;
                let order = query.order_by.take().or_else(|| tail.order_by.take());
                let limit = query
                    .limit_clause
                    .take()
                    .or_else(|| tail.limit_clause.take());
                ast_owner::replace_order(query, order);
                query.limit_clause = limit;
                frame.output.table = RelationName::internal("\0union:output");
                frame.output.projection.push(Projection {
                    expr: None,
                    name: String::new(),
                });
                resume(frame, Stage::Branch(bodies.into()), tasks);
            } else {
                let select = select_body(&mut query.body)?;
                if select.from.is_empty() {
                    return Err(problem("relational.invalid_syntax", "expected FROM"));
                }
                if select.from.len() != 1
                    || select.into.is_some()
                    || select.top.is_some()
                    || select.prewhere.is_some()
                    || select.qualify.is_some()
                    || !select.named_window.is_empty()
                    || !select.lateral_views.is_empty()
                    || !select.connect_by.is_empty()
                    || !select.cluster_by.is_empty()
                    || !select.distribute_by.is_empty()
                    || !select.sort_by.is_empty()
                    || select.value_table_mode.is_some()
                    || select.select_modifiers.is_some()
                    || select.exclude.is_some()
                    || !select.optimizer_hints.is_empty()
                {
                    return Err(unsupported());
                }
                let GroupByExpr::Expressions(_, modifiers) = &select.group_by else {
                    return Err(unsupported());
                };
                if !modifiers.is_empty() {
                    return Err(unsupported());
                }
                resume(frame, Stage::Base, tasks);
            }
        }
        Stage::Branch(mut bodies) => {
            if let Some(body) = bodies.pop_front() {
                check()?;
                let depth = frame.depth;
                child(frame, Stage::BranchDone(bodies), body, depth, tasks);
            } else {
                for body in frame
                    .output
                    .branches
                    .iter()
                    .chain(frame.output.bindings.iter().map(|(_, b)| b))
                {
                    scopes[frame.scope].extend(body.parameters.iter().cloned());
                }
                resume(frame, Stage::OrderBegin, tasks);
            }
        }
        Stage::BranchDone(bodies) => {
            frame.output.branches.push(query_result(result)?);
            resume(frame, Stage::Branch(bodies), tasks);
        }
        Stage::Base => {
            let (table, alias, query) = joins::lower_relation(
                &mut select(&mut frame)?.from[0].relation,
                "\0derived:base",
                check,
            )?;
            if let Some(query) = query {
                let depth = frame.depth + 1;
                child(frame, Stage::BaseDone(table, alias), query, depth, tasks);
            } else {
                frame.output.table = table;
                frame.output.alias = alias;
                resume(frame, Stage::Join(0), tasks);
            }
        }
        Stage::BaseDone(table, alias) => {
            frame.output.table = table;
            frame.output.alias = alias;
            frame.output.derived = Some(Box::new(query_result(result)?));
            resume(frame, Stage::Join(0), tasks);
        }
        Stage::Join(index) => {
            let Some(join) = select(&mut frame)?.from[0].joins.get_mut(index) else {
                resume(frame, Stage::Projection(0), tasks);
                return Ok(());
            };
            check()?;
            let (table, alias, query) = joins::lower_relation(
                &mut join.relation,
                &format!("\0derived:join:{index}"),
                check,
            )?;
            if let Some(query) = query {
                let depth = frame.depth + 1;
                child(
                    frame,
                    Stage::JoinDerived(index, table, alias),
                    query,
                    depth,
                    tasks,
                );
            } else {
                frame.output.joins.push(joins::Link {
                    table,
                    alias,
                    derived: None,
                    left: false,
                    condition: None,
                });
                resume(frame, Stage::JoinCondition(index), tasks);
            }
        }
        Stage::JoinDerived(index, table, alias) => {
            frame.output.joins.push(joins::Link {
                table,
                alias,
                derived: Some(Box::new(query_result(result)?)),
                left: false,
                condition: None,
            });
            resume(frame, Stage::JoinCondition(index), tasks);
        }
        Stage::JoinCondition(index) => {
            let operator = &mut select(&mut frame)?.from[0].joins[index].join_operator;
            let (left, constraint) = match operator {
                JoinOperator::Join(c) | JoinOperator::Inner(c) => (false, c),
                JoinOperator::Left(c) | JoinOperator::LeftOuter(c) => (true, c),
                JoinOperator::CrossJoin(JoinConstraint::None) => {
                    resume(frame, Stage::Join(index + 1), tasks);
                    return Ok(());
                }
                _ => return Err(unsupported()),
            };
            let JoinConstraint::On(on) = constraint else {
                return Err(unsupported());
            };
            let expr = ast_owner::take_expr(on);
            frame.output.joins[index].left = left;
            expression(frame, Stage::JoinDone(index), expr, tasks);
        }
        Stage::JoinDone(index) => {
            frame.output.joins[index].condition = Some(term_result(values)?);
            resume(frame, Stage::Join(index + 1), tasks);
        }
        Stage::Projection(index) => {
            let Some(item) = select(&mut frame)?.projection.get_mut(index) else {
                if frame.output.projection.is_empty() {
                    return Err(budget());
                }
                resume(frame, Stage::Group(0), tasks);
                return Ok(());
            };
            check()?;
            match item {
                SelectItem::UnnamedExpr(expr) | SelectItem::ExprWithAlias { expr, .. } => {
                    let expr = ast_owner::take_expr(expr);
                    expression(frame, Stage::ProjectionDone(index), expr, tasks);
                }
                SelectItem::Wildcard(options)
                    if *options == WildcardAdditionalOptions::default() =>
                {
                    frame.output.projection.push(Projection {
                        expr: None,
                        name: String::new(),
                    });
                    resume(frame, Stage::Projection(index + 1), tasks);
                }
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    options,
                ) if *options == WildcardAdditionalOptions::default() => {
                    let name = match name.0.as_slice() {
                        [ObjectNamePart::Identifier(name)] => syntax::identifier(name)?.to_owned(),
                        _ => return Err(unsupported()),
                    };
                    frame
                        .output
                        .projection
                        .push(Projection { expr: None, name });
                    resume(frame, Stage::Projection(index + 1), tasks);
                }
                _ => return Err(unsupported()),
            }
        }
        Stage::ProjectionDone(index) => {
            let expr = term_result(values)?;
            let name = match &select(&mut frame)?.projection[index] {
                SelectItem::ExprWithAlias { alias, .. } => syntax::identifier(alias)?.to_owned(),
                _ => String::new(),
            };
            frame.output.projection.push(Projection {
                expr: Some(expr),
                name,
            });
            resume(frame, Stage::Projection(index + 1), tasks);
        }
        Stage::Group(index) => {
            let GroupByExpr::Expressions(groups, _) = &mut select(&mut frame)?.group_by else {
                return Err(unsupported());
            };
            if let Some(expr) = groups.get_mut(index) {
                let expr = ast_owner::take_expr(expr);
                expression(frame, Stage::GroupDone(index), expr, tasks);
            } else {
                resume(frame, Stage::Having, tasks);
            }
        }
        Stage::GroupDone(index) => {
            frame.output.groups.push(term_result(values)?);
            resume(frame, Stage::Group(index + 1), tasks);
        }
        Stage::Having => {
            if let Some(expr) = select(&mut frame)?.having.as_mut() {
                let expr = ast_owner::take_expr(expr);
                expression(frame, Stage::HavingDone, expr, tasks);
            } else {
                resume(frame, Stage::Filter, tasks);
            }
        }
        Stage::HavingDone => {
            frame.output.having = Some(term_result(values)?);
            resume(frame, Stage::Filter, tasks);
        }
        Stage::Filter => {
            if let Some(expr) = select(&mut frame)?.selection.as_mut() {
                let expr = ast_owner::take_expr(expr);
                expression(frame, Stage::FilterDone, expr, tasks);
            } else {
                resume(frame, Stage::OrderBegin, tasks);
            }
        }
        Stage::FilterDone => {
            frame.output.filter = Some(term_result(values)?);
            resume(frame, Stage::OrderBegin, tasks);
        }
        Stage::OrderBegin => {
            check()?;
            if let Some(order) = &frame.query.order_by {
                if order.interpolate.is_some() || !matches!(order.kind, OrderByKind::Expressions(_))
                {
                    return Err(unsupported());
                }
            }
            resume(frame, Stage::Order(0), tasks);
        }
        Stage::Order(index) => {
            let order = frame
                .query
                .order_by
                .as_mut()
                .and_then(|order| match &mut order.kind {
                    OrderByKind::Expressions(items) => items.get_mut(index),
                    _ => None,
                });
            if let Some(order) = order {
                check()?;
                if order.with_fill.is_some()
                    || matches!(order.options.sort, Some(OrderBySort::Using(_)))
                {
                    return Err(unsupported());
                }
                let descending = matches!(order.options.sort, Some(OrderBySort::Desc));
                let nulls = order.options.nulls_first;
                let expr = ast_owner::take_expr(&mut order.expr);
                expression(
                    frame,
                    Stage::OrderDone(index, descending, nulls),
                    expr,
                    tasks,
                );
            } else {
                resume(frame, Stage::Counts, tasks);
            }
        }
        Stage::OrderDone(index, descending, nulls) => {
            frame
                .output
                .order
                .push((term_result(values)?, descending, nulls));
            resume(frame, Stage::Order(index + 1), tasks);
        }
        Stage::Counts => {
            // Count grammar admits only a literal or parameter. Reuse the
            // suffix validator; this constant-depth call cannot enter a query.
            let (_, limit, offset) = Program::suffix(
                &mut None,
                &mut frame.query.limit_clause,
                &mut scopes[frame.scope],
                frame.depth,
                check,
            )?;
            frame.output.limit = limit;
            frame.output.offset = offset;
            if frame.output.branches.is_empty() {
                let select = select(&mut frame)?;
                let distinct = matches!(select.distinct, Some(Distinct::Distinct));
                if select.distinct.is_some() && !distinct {
                    return Err(unsupported());
                }
                frame.output.distinct = distinct;
            }
            resume(frame, Stage::Finish, tasks);
        }
        Stage::Finish => {
            for body in frame
                .output
                .bindings
                .iter()
                .map(|(_, b)| b)
                .chain(frame.output.derived.as_deref())
                .chain(
                    frame
                        .output
                        .joins
                        .iter()
                        .filter_map(|j| j.derived.as_deref()),
                )
            {
                scopes[frame.scope].extend(body.parameters.iter().cloned());
            }
            frame.output.parameters = std::mem::take(&mut scopes[frame.scope]);
            let Frame { output, .. } = *frame;
            *result = Some(output);
        }
    }
    Ok(())
}
