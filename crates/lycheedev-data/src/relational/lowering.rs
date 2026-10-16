//! Move-based, iterative scalar lowering. Public AST roots and incomplete Term
//! results both have iterative cleanup, including cancellation and rejection.
use super::*;
use ast_owner::Owned;

pub(super) enum QueryKind {
    Scalar,
    Exists(bool),
    In(bool),
}
pub(super) enum Build {
    Unary(UnaryOperator),
    Binary(BinaryOperator),
    Null(bool),
    Cast(Result<String>),
    Like(bool),
    Between(bool),
    In(usize, bool),
    Call(String, usize, bool, bool),
    Case(bool, usize, bool),
}
pub(super) enum Task {
    Visit(Owned<Expr>, usize, usize),
    Build(Build),
    QueryTerm(QueryKind, usize),
    Query(query_lowering::Task),
}

pub(super) fn term(
    expr: &mut Expr,
    parameters: &mut BTreeSet<String>,
    depth: usize,
    check: &dyn Fn() -> Result<()>,
) -> Result<Term> {
    let mut scopes = vec![BTreeSet::new()];
    let (value, _) = run(
        vec![Task::Visit(ast_owner::take_expr(expr), depth, 0)],
        &mut scopes,
        check,
    )?;
    parameters.extend(std::mem::take(&mut scopes[0]));
    value.ok_or_else(unsupported)
}
pub(super) fn query(
    query: &mut Query,
    depth: usize,
    check: &dyn Fn() -> Result<()>,
) -> Result<Program> {
    let mut scopes = vec![BTreeSet::new()];
    let (_, query) = run(
        vec![Task::Query(query_lowering::Task::new(
            ast_owner::take_query(query),
            depth,
            0,
        ))],
        &mut scopes,
        check,
    )?;
    query.ok_or_else(unsupported)
}
fn run(
    mut tasks: Vec<Task>,
    scopes: &mut Vec<BTreeSet<String>>,
    check: &dyn Fn() -> Result<()>,
) -> Result<(Option<Term>, Option<Program>)> {
    let mut values = Vec::new();
    let mut query_result = None;
    while let Some(task) = tasks.pop() {
        check()?;
        match task {
            Task::Visit(expr, depth, scope) => {
                if depth > 256 {
                    return Err(budget());
                }
                visit(
                    expr,
                    &mut scopes[scope],
                    depth,
                    scope,
                    &mut tasks,
                    &mut values,
                )?;
            }
            Task::Build(node) => build(node, &mut values)?,
            Task::QueryTerm(kind, scope) => {
                let query = query_result.take().ok_or_else(unsupported)?;
                scopes[scope].extend(query.parameters.iter().cloned());
                let query = std::sync::Arc::new(query);
                let value = match kind {
                    QueryKind::Scalar => Term::ScalarSubquery(query),
                    QueryKind::Exists(false) => Term::Exists(query),
                    QueryKind::Exists(true) => {
                        Term::Unary(UnaryOperator::Not, Box::new(Term::Exists(query)))
                    }
                    QueryKind::In(negated) => {
                        let input = pop(&mut values)?;
                        Term::InSubquery(Box::new(input), query, negated)
                    }
                };
                values.push(value);
            }
            Task::Query(task) => query_lowering::step(
                task,
                &mut tasks,
                &mut values,
                scopes,
                &mut query_result,
                check,
            )?,
        }
    }
    if values.len() > 1 {
        return Err(unsupported());
    }
    Ok((values.pop(), query_result))
}

fn pop(values: &mut Vec<Term>) -> Result<Term> {
    values.pop().ok_or_else(unsupported)
}
fn push(tasks: &mut Vec<Task>, expr: &mut Expr, depth: usize, scope: usize) {
    tasks.push(Task::Visit(ast_owner::take_expr(expr), depth + 1, scope));
}
#[inline(never)]
fn visit(
    mut expr: Owned<Expr>,
    parameters: &mut BTreeSet<String>,
    depth: usize,
    scope: usize,
    tasks: &mut Vec<Task>,
    values: &mut Vec<Term>,
) -> Result<()> {
    if matches!(
        &*expr,
        Expr::InSubquery { .. } | Expr::Subquery(_) | Expr::Exists { .. }
    ) {
        let (query, input, exists) = take_query_expression(&mut expr);
        let kind = if let Some((_, negated)) = &input {
            QueryKind::In(*negated)
        } else if let Some(negated) = exists {
            QueryKind::Exists(negated)
        } else {
            QueryKind::Scalar
        };
        tasks.push(Task::QueryTerm(kind, scope));
        tasks.push(Task::Query(query_lowering::Task::new(
            query,
            depth + 1,
            usize::MAX,
        )));
        if let Some((input, _)) = input {
            tasks.push(Task::Visit(input, depth + 1, scope));
        }
        return Ok(());
    }
    let leaf = match &mut *expr {
        Expr::Wildcard(_) => Some(Term::Star(String::new())),
        Expr::QualifiedWildcard(name, _) => {
            let [ObjectNamePart::Identifier(name)] = name.0.as_slice() else {
                return Err(unsupported());
            };
            Some(Term::Star(syntax::identifier(name)?.to_owned()))
        }
        Expr::Identifier(name) => Some(Term::Column(ColumnRef::new(
            syntax::identifier(name)?.to_owned(),
        ))),
        Expr::CompoundIdentifier(names) if names.len() == 2 => {
            Some(Term::Column(ColumnRef::qualified(
                syntax::identifier(&names[0])?.to_owned(),
                syntax::identifier(&names[1])?.to_owned(),
            )))
        }
        Expr::Value(value) => Some(match &value.value {
            Value::Null => Term::Value(Scalar::Null),
            Value::Boolean(value) => Term::Value(Scalar::Bool(*value)),
            Value::Number(value, _) => Term::Number(value.clone()),
            Value::SingleQuotedString(value) => Term::Value(Scalar::String(value.clone())),
            Value::Placeholder(name) if name.starts_with(':') && name.len() > 1 => {
                parameters.insert(name[1..].into());
                Term::Parameter(name[1..].into())
            }
            _ => return Err(unsupported()),
        }),
        Expr::Nested(child) => {
            push(tasks, child, depth, scope);
            None
        }
        Expr::UnaryOp { op, expr }
            if matches!(
                op,
                UnaryOperator::Plus | UnaryOperator::Minus | UnaryOperator::Not
            ) =>
        {
            tasks.push(Task::Build(Build::Unary(*op)));
            push(tasks, expr, depth, scope);
            None
        }
        Expr::BinaryOp { left, op, right }
            if matches!(
                op,
                BinaryOperator::Plus
                    | BinaryOperator::Minus
                    | BinaryOperator::Multiply
                    | BinaryOperator::Divide
                    | BinaryOperator::Modulo
                    | BinaryOperator::Eq
                    | BinaryOperator::NotEq
                    | BinaryOperator::Lt
                    | BinaryOperator::LtEq
                    | BinaryOperator::Gt
                    | BinaryOperator::GtEq
                    | BinaryOperator::And
                    | BinaryOperator::Or
                    | BinaryOperator::StringConcat
            ) =>
        {
            tasks.push(Task::Build(Build::Binary(op.clone())));
            push(tasks, right, depth, scope);
            push(tasks, left, depth, scope);
            None
        }
        Expr::IsNull(child) => {
            tasks.push(Task::Build(Build::Null(false)));
            push(tasks, child, depth, scope);
            None
        }
        Expr::IsNotNull(child) => {
            tasks.push(Task::Build(Build::Null(true)));
            push(tasks, child, depth, scope);
            None
        }
        Expr::Cast {
            kind: CastKind::Cast,
            expr,
            data_type,
            format: None,
        } => {
            tasks.push(Task::Build(Build::Cast(scalar::cast_name(data_type))));
            push(tasks, expr, depth, scope);
            None
        }
        Expr::Like {
            negated,
            any: false,
            expr,
            pattern,
            escape_char: None,
        } => {
            tasks.push(Task::Build(Build::Like(*negated)));
            push(tasks, pattern, depth, scope);
            push(tasks, expr, depth, scope);
            None
        }
        Expr::Between {
            expr,
            negated,
            low,
            high,
        } => {
            tasks.push(Task::Build(Build::Between(*negated)));
            push(tasks, high, depth, scope);
            push(tasks, low, depth, scope);
            push(tasks, expr, depth, scope);
            None
        }
        Expr::InList {
            expr,
            list,
            negated,
        } => {
            tasks.push(Task::Build(Build::In(list.len(), *negated)));
            for child in list.iter_mut().rev() {
                push(tasks, child, depth, scope);
            }
            push(tasks, expr, depth, scope);
            None
        }
        Expr::Function(function) => {
            if function.over.is_some()
                || function.filter.is_some()
                || function.null_treatment.is_some()
                || !matches!(function.parameters, FunctionArguments::None)
                || !function.within_group.is_empty()
            {
                return Err(unsupported());
            }
            let FunctionArguments::List(args) = &mut function.args else {
                return Err(unsupported());
            };
            if !args.clauses.is_empty() {
                return Err(unsupported());
            }
            let name = scalar::name(&function.name)?;
            if matches!(args.duplicate_treatment, Some(DuplicateTreatment::All)) {
                return Err(problem(
                    "relational.invalid_syntax",
                    "Unexpected ALL in function arguments",
                ));
            }
            let distinct = matches!(args.duplicate_treatment, Some(DuplicateTreatment::Distinct));
            let aggregate = ["COUNT", "MIN", "MAX", "SUM", "AVG"].contains(&name.as_str());
            tasks.push(Task::Build(Build::Call(
                name,
                args.args.len(),
                distinct,
                aggregate,
            )));
            for arg in args.args.iter_mut().rev() {
                match arg {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => {
                        push(tasks, expr, depth, scope)
                    }
                    FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => {
                        tasks.push(Task::Visit(
                            Owned::new(Box::new(Expr::Wildcard(Default::default()))),
                            depth + 1,
                            scope,
                        ));
                    }
                    FunctionArg::Unnamed(FunctionArgExpr::QualifiedWildcard(name)) => {
                        tasks.push(Task::Visit(
                            Owned::new(Box::new(Expr::QualifiedWildcard(
                                name.clone(),
                                Default::default(),
                            ))),
                            depth + 1,
                            scope,
                        ));
                    }
                    _ => return Err(unsupported()),
                }
            }
            None
        }
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => {
            tasks.push(Task::Build(Build::Case(
                operand.is_some(),
                conditions.len(),
                else_result.is_some(),
            )));
            if let Some(child) = else_result {
                push(tasks, child, depth, scope);
            }
            for pair in conditions.iter_mut().rev() {
                push(tasks, &mut pair.result, depth, scope);
                push(tasks, &mut pair.condition, depth, scope);
            }
            if let Some(child) = operand {
                push(tasks, child, depth, scope);
            }
            None
        }
        _ => return Err(unsupported()),
    };
    if let Some(leaf) = leaf {
        values.push(leaf);
    }
    Ok(())
}
fn build(node: Build, values: &mut Vec<Term>) -> Result<()> {
    let value = match node {
        Build::Unary(op) => Term::Unary(op, Box::new(pop(values)?)),
        Build::Null(negated) => Term::NullTest(Box::new(pop(values)?), negated),
        Build::Cast(kind) => Term::Cast(Box::new(pop(values)?), kind?),
        Build::Binary(op) => {
            let right = pop(values)?;
            Term::Binary(Box::new(pop(values)?), op, Box::new(right))
        }
        Build::Like(negated) => {
            let right = pop(values)?;
            Term::Like(Box::new(pop(values)?), Box::new(right), negated)
        }
        Build::Between(negated) => {
            let high = pop(values)?;
            let low = pop(values)?;
            Term::Between(
                Box::new(pop(values)?),
                Box::new(low),
                Box::new(high),
                negated,
            )
        }
        Build::In(count, negated) => {
            let start = values.len().checked_sub(count).ok_or_else(unsupported)?;
            let list = values.split_off(start);
            Term::In(Box::new(pop(values)?), list, negated)
        }
        Build::Call(name, count, distinct, aggregate) => {
            let start = values.len().checked_sub(count).ok_or_else(unsupported)?;
            let args = values.split_off(start);
            if aggregate {
                Term::Aggregate(name, args, distinct)
            } else {
                Term::Call(name, args, distinct)
            }
        }
        Build::Case(operand, count, otherwise) => {
            let fallback = if otherwise {
                pop(values)?
            } else {
                Term::Value(Scalar::Null)
            };
            let start = values
                .len()
                .checked_sub(count * 2)
                .ok_or_else(unsupported)?;
            let mut pairs = values.split_off(start).into_iter();
            let mut conditions = Vec::with_capacity(count);
            while let Some(condition) = pairs.next() {
                conditions.push((condition, pairs.next().ok_or_else(unsupported)?));
            }
            let operand = if operand {
                Some(Box::new(pop(values)?))
            } else {
                None
            };
            Term::Case(operand, conditions, Some(Box::new(fallback)))
        }
    };
    values.push(value);
    Ok(())
}
