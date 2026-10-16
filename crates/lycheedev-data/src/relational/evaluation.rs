//! Borrowed-node scalar evaluation with explicit value continuations. Eager
//! operators, selective CASE/COALESCE, and nested-query completion retain their
//! original order; original work/control and refundable heap admission apply.
use super::*;

enum Task<'a> {
    Eval(&'a Term),
    Unary(UnaryOperator),
    Null(bool),
    Cast(&'a str),
    Binary(&'a BinaryOperator),
    Between(bool),
    Like(bool),
    InStart(&'a [Term], bool),
    InItem(Scalar, &'a [Term], bool, bool, bool),
    InQuery(&'a Program, bool),
    CaseStart(&'a [(Term, Term)], Option<&'a Term>, bool),
    CaseNext(Option<Scalar>, &'a [(Term, Term)], Option<&'a Term>),
    CaseTest(
        Option<Scalar>,
        &'a Term,
        &'a [(Term, Term)],
        Option<&'a Term>,
    ),
    Coalesce(&'a [Term]),
    BitFirst(bool, &'a Term),
    BitSecond(bool, u64),
    Call(&'a str),
    NullIf,
}

pub(super) fn eval(
    term: &Term,
    row: &[Scalar],
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<Scalar> {
    let mut tasks = task_stack::Stack::new(work.shared)?;
    let mut values = task_stack::Stack::new(work.shared)?;
    let mut root = Some(Task::Eval(term));
    while let Some(task) = root.take().or_else(|| tasks.pop()) {
        if let Some(request) = step(
            task,
            &mut tasks,
            &mut values,
            row,
            columns,
            alias,
            params,
            context,
            work,
        )? {
            // The large scalar step frame has returned before entering another
            // query through the provider's synchronous callback boundary.
            let result = match &request.kind {
                QueryKind::Exists => subquery::execute_any(
                    request.query,
                    row,
                    columns,
                    alias,
                    params,
                    context,
                    work,
                )?,
                _ => subquery::execute(request.query, row, columns, alias, params, context, work)?,
            };
            values.push(finish_query(request.kind, result, work)?)?;
        }
    }
    if values.len() != 1 {
        return Err(unsupported());
    }
    pop(&mut values)
}
enum QueryKind {
    Scalar,
    Exists,
    In(Scalar, bool),
}
struct Request<'a> {
    query: &'a Program,
    kind: QueryKind,
}
#[inline(never)]
fn step<'a>(
    task: Task<'a>,
    tasks: &mut task_stack::Stack<'_, Task<'a>>,
    values: &mut task_stack::Stack<'_, Scalar>,
    row: &[Scalar],
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<Option<Request<'a>>> {
    match task {
        Task::Eval(term) => {
            work.spend()?;
            match term {
                Term::Aggregate(..) | Term::Star(_) => return Err(unsupported()),
                Term::Value(value) => values.push(work.clone_scalar(value)?)?,
                Term::Number(number) => values.push(literal(number)?)?,
                Term::Ordinal(index) => {
                    values.push(work.clone_scalar(row.get(*index).ok_or_else(binding)?)?)?
                }
                Term::Column(name) => {
                    let value = match subquery::resolve(columns, alias, name, context.scope.outer)?
                    {
                        subquery::Position::Local(index) => row.get(index).ok_or_else(binding)?,
                        subquery::Position::Outer(frame, index) => frame.value(index)?,
                    };
                    values.push(work.clone_scalar(value)?)?;
                }
                Term::Parameter(name) => {
                    values.push(work.clone_scalar(params.get(name).ok_or_else(binding)?)?)?
                }
                Term::Unary(op, input) => {
                    tasks.push(Task::Unary(*op))?;
                    tasks.push(Task::Eval(input))?;
                }
                Term::NullTest(input, negated) => {
                    tasks.push(Task::Null(*negated))?;
                    tasks.push(Task::Eval(input))?;
                }
                Term::Cast(input, kind) => {
                    tasks.push(Task::Cast(kind))?;
                    tasks.push(Task::Eval(input))?;
                }
                Term::Binary(left, op, right) => {
                    tasks.push(Task::Binary(op))?;
                    tasks.push(Task::Eval(right))?;
                    tasks.push(Task::Eval(left))?;
                }
                Term::Like(input, pattern, negated) => {
                    tasks.push(Task::Like(*negated))?;
                    tasks.push(Task::Eval(pattern))?;
                    tasks.push(Task::Eval(input))?;
                }
                Term::Between(input, low, high, negated) => {
                    tasks.push(Task::Between(*negated))?;
                    tasks.push(Task::Eval(high))?;
                    tasks.push(Task::Eval(low))?;
                    tasks.push(Task::Eval(input))?;
                }
                Term::In(input, list, negated) => {
                    tasks.push(Task::InStart(list, *negated))?;
                    tasks.push(Task::Eval(input))?;
                }
                Term::InSubquery(input, query, negated) => {
                    tasks.push(Task::InQuery(query, *negated))?;
                    tasks.push(Task::Eval(input))?;
                }
                Term::ScalarSubquery(query) => {
                    return Ok(Some(Request {
                        query,
                        kind: QueryKind::Scalar,
                    }));
                }
                Term::Exists(query) => {
                    return Ok(Some(Request {
                        query,
                        kind: QueryKind::Exists,
                    }));
                }
                Term::Case(input, cases, fallback) => {
                    tasks.push(Task::CaseStart(cases, fallback.as_deref(), input.is_some()))?;
                    if let Some(input) = input {
                        tasks.push(Task::Eval(input))?;
                    }
                }
                Term::Call(name, args, distinct) => {
                    if *distinct {
                        return Err(unsupported());
                    }
                    check_arity(name, args.len())?;
                    match name.as_str() {
                        "COALESCE" => {
                            let (first, rest) = args.split_first().ok_or_else(type_error)?;
                            tasks.push(Task::Coalesce(rest))?;
                            tasks.push(Task::Eval(first))?;
                        }
                        "HAS_FLAG" | "BIT_AND" => {
                            tasks.push(Task::BitFirst(name == "HAS_FLAG", &args[1]))?;
                            tasks.push(Task::Eval(&args[0]))?;
                        }
                        "NULLIF" => {
                            tasks.push(Task::NullIf)?;
                            tasks.push(Task::Eval(&args[1]))?;
                            tasks.push(Task::Eval(&args[0]))?;
                        }
                        _ => {
                            tasks.push(Task::Call(name))?;
                            tasks.push(Task::Eval(&args[0]))?;
                        }
                    }
                }
            }
        }
        Task::Unary(op) => {
            let value = pop(values)?;
            values.push(unary(op, value)?)?;
        }
        Task::Null(negated) => {
            let value = pop(values)?;
            values.push(Scalar::Bool(matches!(value, Scalar::Null) ^ negated))?;
        }
        Task::Cast(kind) => {
            let value = pop(values)?;
            values.push(scalar::cast(value, kind, work)?)?;
        }
        Task::Binary(op) => {
            let right = pop(values)?;
            let left = pop(values)?;
            values.push(binary(left, op, right, work)?)?;
        }
        Task::Like(negated) => {
            let pattern = pop(values)?;
            let input = pop(values)?;
            values.push(scalar::like(&input, &pattern, negated, work)?)?;
        }
        Task::Between(negated) => {
            let high = pop(values)?;
            let low = pop(values)?;
            let input = pop(values)?;
            let low = if input == Scalar::Null || low == Scalar::Null {
                None
            } else {
                Some(compare(&input, &low)? != Ordering::Less)
            };
            let high = if input == Scalar::Null || high == Scalar::Null {
                None
            } else {
                Some(compare(&input, &high)? != Ordering::Greater)
            };
            values.push(if low == Some(false) || high == Some(false) {
                Scalar::Bool(negated)
            } else if low == Some(true) && high == Some(true) {
                Scalar::Bool(!negated)
            } else {
                Scalar::Null
            })?;
        }
        Task::InStart(list, negated) => {
            let input = pop(values)?;
            if let Some((first, rest)) = list.split_first() {
                tasks.push(Task::InItem(input, rest, negated, false, false))?;
                tasks.push(Task::Eval(first))?;
            } else {
                values.push(Scalar::Bool(negated))?;
            }
        }
        Task::InItem(input, list, negated, mut null, mut matched) => {
            let other = pop(values)?;
            if input == Scalar::Null || other == Scalar::Null {
                null = true;
            } else if compare(&input, &other)? == Ordering::Equal {
                matched = true;
            }
            if let Some((first, rest)) = list.split_first() {
                tasks.push(Task::InItem(input, rest, negated, null, matched))?;
                tasks.push(Task::Eval(first))?;
            } else {
                values.push(in_result(negated, null, matched))?;
            }
        }
        Task::InQuery(query, negated) => {
            let input = pop(values)?;
            return Ok(Some(Request {
                query,
                kind: QueryKind::In(input, negated),
            }));
        }
        Task::CaseStart(cases, fallback, input) => {
            let input = if input { Some(pop(values)?) } else { None };
            tasks.push(Task::CaseNext(input, cases, fallback))?;
        }
        Task::CaseNext(input, cases, fallback) => {
            if let Some(((condition, result), rest)) = cases.split_first() {
                tasks.push(Task::CaseTest(input, result, rest, fallback))?;
                tasks.push(Task::Eval(condition))?;
            } else if let Some(fallback) = fallback {
                tasks.push(Task::Eval(fallback))?;
            } else {
                values.push(Scalar::Null)?;
            }
        }
        Task::CaseTest(input, result, rest, fallback) => {
            let condition = pop(values)?;
            let yes = if let Some(input) = &input {
                input != &Scalar::Null
                    && condition != Scalar::Null
                    && compare(input, &condition)? == Ordering::Equal
            } else {
                truth(&condition)? == Some(true)
            };
            if yes {
                tasks.push(Task::Eval(result))?;
            } else {
                tasks.push(Task::CaseNext(input, rest, fallback))?;
            }
        }
        Task::Coalesce(rest) => {
            let value = pop(values)?;
            if value != Scalar::Null {
                values.push(value)?;
            } else if let Some((first, rest)) = rest.split_first() {
                tasks.push(Task::Coalesce(rest))?;
                tasks.push(Task::Eval(first))?;
            } else {
                values.push(Scalar::Null)?;
            }
        }
        Task::BitFirst(flag, right) => {
            let value = pop(values)?;
            if value == Scalar::Null {
                values.push(Scalar::Null)?;
            } else {
                let bits = int(&value).ok_or_else(type_error)? as u64;
                tasks.push(Task::BitSecond(flag, bits))?;
                tasks.push(Task::Eval(right))?;
            }
        }
        Task::BitSecond(flag, left) => {
            let value = pop(values)?;
            if value == Scalar::Null {
                values.push(Scalar::Null)?;
            } else {
                let right = int(&value).ok_or_else(type_error)? as u64;
                values.push(if flag {
                    Scalar::Bool(left & right == right)
                } else {
                    Scalar::UInt(left & right)
                })?;
            }
        }
        Task::NullIf => {
            let right = pop(values)?;
            let left = pop(values)?;
            values.push(
                if left != Scalar::Null
                    && right != Scalar::Null
                    && compare(&left, &right)? == Ordering::Equal
                {
                    Scalar::Null
                } else {
                    left
                },
            )?;
        }
        Task::Call(name) => {
            let value = pop(values)?;
            values.push(call(name, value, work)?)?;
        }
    }
    Ok(None)
}
fn finish_query(kind: QueryKind, result: QueryResult, work: &mut Work<'_>) -> Result<Scalar> {
    match kind {
        QueryKind::Scalar => {
            if result.rows.len() > 1 {
                return Err(subquery::cardinality());
            }
            Ok(result
                .rows
                .into_iter()
                .next()
                .and_then(|row| row.into_iter().next())
                .unwrap_or(Scalar::Null))
        }
        QueryKind::Exists => Ok(Scalar::Bool(!result.rows.is_empty())),
        QueryKind::In(input, negated) => {
            let mut null = false;
            let mut matched = false;
            for row in result.rows {
                work.spend()?;
                let other = row.first().ok_or_else(subquery::cardinality)?;
                if input == Scalar::Null || other == &Scalar::Null {
                    null = true;
                } else if compare(&input, other)? == Ordering::Equal {
                    matched = true;
                }
            }
            Ok(in_result(negated, null, matched))
        }
    }
}

fn pop(values: &mut task_stack::Stack<'_, Scalar>) -> Result<Scalar> {
    values.pop().ok_or_else(unsupported)
}
fn in_result(negated: bool, null: bool, matched: bool) -> Scalar {
    if matched {
        Scalar::Bool(!negated)
    } else if null {
        Scalar::Null
    } else {
        Scalar::Bool(negated)
    }
}
fn unary(op: UnaryOperator, value: Scalar) -> Result<Scalar> {
    if value == Scalar::Null {
        return Ok(value);
    }
    match op {
        UnaryOperator::Plus if int(&value).is_some() => Ok(value),
        UnaryOperator::Minus if int(&value).is_some() => integer(-int(&value).unwrap()),
        UnaryOperator::Plus => Ok(Scalar::Float(number(&value)?)),
        UnaryOperator::Minus => Ok(Scalar::Float(-number(&value)?)),
        UnaryOperator::Not => Ok(truth(&value)?
            .map(|value| Scalar::Bool(!value))
            .unwrap_or(Scalar::Null)),
        _ => Err(unsupported()),
    }
}
fn binary(left: Scalar, op: &BinaryOperator, right: Scalar, work: &mut Work<'_>) -> Result<Scalar> {
    if matches!(op, BinaryOperator::And | BinaryOperator::Or) {
        let left = truth(&left)?;
        let right = truth(&right)?;
        return Ok(match op {
            BinaryOperator::And if left == Some(false) || right == Some(false) => {
                Scalar::Bool(false)
            }
            BinaryOperator::Or if left == Some(true) || right == Some(true) => Scalar::Bool(true),
            _ => left
                .zip(right)
                .map(|(left, right)| {
                    Scalar::Bool(if op == &BinaryOperator::And {
                        left && right
                    } else {
                        left || right
                    })
                })
                .unwrap_or(Scalar::Null),
        });
    }
    if left == Scalar::Null || right == Scalar::Null {
        return Ok(Scalar::Null);
    }
    match op {
        BinaryOperator::Eq
        | BinaryOperator::NotEq
        | BinaryOperator::Lt
        | BinaryOperator::LtEq
        | BinaryOperator::Gt
        | BinaryOperator::GtEq => {
            let order = compare(&left, &right)?;
            Ok(Scalar::Bool(match op {
                BinaryOperator::Eq => order == Ordering::Equal,
                BinaryOperator::NotEq => order != Ordering::Equal,
                BinaryOperator::Lt => order == Ordering::Less,
                BinaryOperator::LtEq => order != Ordering::Greater,
                BinaryOperator::Gt => order == Ordering::Greater,
                _ => order != Ordering::Less,
            }))
        }
        BinaryOperator::StringConcat => {
            let (Scalar::String(left), Scalar::String(right)) = (left, right) else {
                return Err(type_error());
            };
            let bytes = left.len().checked_add(right.len()).ok_or_else(budget)?;
            if bytes > 1 << 20 {
                return Err(budget());
            }
            work.temporary(bytes)?;
            let mut output = String::with_capacity(bytes);
            output.push_str(&left);
            output.push_str(&right);
            Ok(Scalar::String(output))
        }
        _ => {
            if matches!(left, Scalar::Float(_))
                || matches!(right, Scalar::Float(_))
                || op == &BinaryOperator::Divide
            {
                work.temporary(8 << 10)?;
            }
            arithmetic(&left, op, &right)
        }
    }
}
fn call(name: &str, value: Scalar, work: &mut Work<'_>) -> Result<Scalar> {
    if value == Scalar::Null {
        return Ok(value);
    }
    let Scalar::String(text) = value else {
        return Err(type_error());
    };
    match name {
        "LOWER" => Ok(Scalar::String(case_text(&text, false, work)?)),
        "UPPER" => Ok(Scalar::String(case_text(&text, true, work)?)),
        "LENGTH" => {
            if text.len() > 1 << 20 {
                return Err(budget());
            }
            work.consume(text.len())?;
            Ok(Scalar::Int(text.chars().count() as i64))
        }
        _ => Err(unsupported()),
    }
}

#[cfg(test)]
pub(super) fn binary_stack_bytes() -> usize {
    4 * (std::mem::size_of::<Task<'_>>() + std::mem::size_of::<Scalar>())
}
