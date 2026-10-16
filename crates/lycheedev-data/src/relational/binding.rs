//! Schema binding uses borrowed-node continuations. Wide lists are resumed one
//! child at a time, retaining Go's first error and exact bind-node work charges.
use super::*;

#[derive(Clone, Copy)]
struct Mode {
    aggregates: bool,
    clear_group: bool,
}
enum Task<'a> {
    Node(&'a Term, Mode, bool),
    List(&'a [Term], Mode),
    Cases(&'a [(Term, Term)], Option<&'a Term>, Mode),
}
#[cfg(test)]
pub(super) fn bind(
    term: &Term,
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    allow_aggregates: bool,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<()> {
    let input_context = subquery::Context {
        group: None,
        scope: subquery::Scope {
            allowed: None,
            ..context.scope
        },
        ..*context
    };
    let mut tasks = task_stack::Stack::new(work.shared)?;
    let mut root = Some(Task::Node(
        term,
        Mode {
            aggregates: allow_aggregates,
            clear_group: false,
        },
        false,
    ));
    while let Some(task) = root.take().or_else(|| tasks.pop()) {
        let (term, mode, argument) = match task {
            Task::List(values, mode) => {
                if let Some((first, rest)) = values.split_first() {
                    tasks.push(Task::List(rest, mode))?;
                    tasks.push(Task::Node(first, mode, true))?;
                }
                continue;
            }
            Task::Cases(cases, fallback, mode) => {
                if let Some(((condition, result), rest)) = cases.split_first() {
                    tasks.push(Task::Cases(rest, fallback, mode))?;
                    tasks.push(Task::Node(result, mode, true))?;
                    tasks.push(Task::Node(condition, mode, true))?;
                } else if let Some(fallback) = fallback {
                    tasks.push(Task::Node(fallback, mode, true))?;
                }
                continue;
            }
            Task::Node(term, mode, argument) => (term, mode, argument),
        };
        if argument && matches!(term, Term::Star(_)) {
            return Err(type_error());
        }
        work.spend()?;
        if let Some(request) = step(
            term,
            mode,
            &mut tasks,
            columns,
            alias,
            params,
            context,
            &input_context,
            work,
        )? {
            let context = if request.mode.clear_group {
                &input_context
            } else {
                context
            };
            if request.any {
                subquery::execute_any(&request.query, &[], columns, alias, params, context, work)?;
            } else {
                subquery::execute(&request.query, &[], columns, alias, params, context, work)?;
            }
            if let Some(input) = request.input {
                tasks.push(Task::Node(input, request.mode, true))?;
            }
        }
    }
    Ok(())
}

struct Request<'a> {
    query: std::sync::Arc<Program>,
    any: bool,
    mode: Mode,
    input: Option<&'a Term>,
}
#[inline(never)]
fn step<'a>(
    term: &'a Term,
    mode: Mode,
    tasks: &mut task_stack::Stack<'_, Task<'a>>,
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    context: &subquery::Context<'_>,
    input_context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<Option<Request<'a>>> {
    let context = if mode.clear_group {
        input_context
    } else {
        context
    };
    match term {
        Term::Ordinal(index) if *index >= columns.len() => return Err(binding()),
        Term::Column(name) => {
            subquery::resolve(columns, alias, name, context.scope.outer)?;
        }
        Term::Parameter(name) if !params.contains_key(name) => return Err(binding()),
        Term::Unary(_, input) | Term::NullTest(input, _) => {
            tasks.push(Task::Node(input, mode, true))?
        }
        Term::Binary(left, _, right) | Term::Like(left, right, _) => {
            tasks.push(Task::Node(right, mode, true))?;
            tasks.push(Task::Node(left, mode, true))?;
        }
        Term::Between(input, low, high, _) => {
            tasks.push(Task::Node(high, mode, true))?;
            tasks.push(Task::Node(low, mode, true))?;
            tasks.push(Task::Node(input, mode, true))?;
        }
        Term::In(input, list, _) => {
            tasks.push(Task::List(list, mode))?;
            tasks.push(Task::Node(input, mode, true))?;
        }
        Term::InSubquery(input, query, _) => {
            if context.scope.callback.is_none() {
                return Err(unsupported());
            }
            if work.planning {
                return Ok(Some(Request {
                    query: query.clone(),
                    any: false,
                    mode,
                    input: Some(input),
                }));
            }
            tasks.push(Task::Node(input, mode, true))?;
        }
        Term::ScalarSubquery(query) | Term::Exists(query) => {
            if context.scope.callback.is_none() {
                return Err(unsupported());
            }
            if work.planning {
                return Ok(Some(Request {
                    query: query.clone(),
                    any: matches!(term, Term::Exists(_)),
                    mode,
                    input: None,
                }));
            }
        }
        Term::Cast(input, kind) => {
            scalar::cast_kind(kind)?;
            tasks.push(Task::Node(input, mode, true))?;
        }
        Term::Aggregate(name, args, distinct) => {
            if !mode.aggregates {
                return Err(type_error());
            }
            if let Some(input) = aggregate::input(name, args, *distinct)? {
                tasks.push(Task::Node(
                    input,
                    Mode {
                        aggregates: false,
                        clear_group: true,
                    },
                    true,
                ))?;
            }
        }
        Term::Call(name, args, distinct) => {
            if *distinct {
                return Err(type_error());
            }
            check_arity(name, args.len())?;
            tasks.push(Task::List(args, mode))?;
        }
        Term::Case(input, cases, fallback) => {
            tasks.push(Task::Cases(cases, fallback.as_deref(), mode))?;
            if let Some(input) = input {
                tasks.push(Task::Node(input, mode, true))?;
            }
        }
        _ => {}
    }
    Ok(None)
}

pub(super) async fn bind_async<'a, 'w: 'a>(
    term: &Term,
    fields: &execution::Fields<'a>,
    allow_aggregates: bool,
    context: &execution::Context<'a>,
    control: &execution::Control<'a, 'w>,
) -> Result<()> {
    let input_context = execution::Context {
        group: None,
        scope: execution::Scope {
            allowed: None,
            ..context.scope.clone()
        },
        ..context.clone()
    };
    let mut tasks = task_stack::Stack::new(control.shared_budget())?;
    let mut root = Some(Task::Node(
        term,
        Mode {
            aggregates: allow_aggregates,
            clear_group: false,
        },
        false,
    ));
    while let Some(task) = root.take().or_else(|| tasks.pop()) {
        let (term, mode, argument) = match task {
            Task::List(values, mode) => {
                if let Some((first, rest)) = values.split_first() {
                    tasks.push(Task::List(rest, mode))?;
                    tasks.push(Task::Node(first, mode, true))?;
                }
                continue;
            }
            Task::Cases(cases, fallback, mode) => {
                if let Some(((condition, result), rest)) = cases.split_first() {
                    tasks.push(Task::Cases(rest, fallback, mode))?;
                    tasks.push(Task::Node(result, mode, true))?;
                    tasks.push(Task::Node(condition, mode, true))?;
                } else if let Some(fallback) = fallback {
                    tasks.push(Task::Node(fallback, mode, true))?;
                }
                continue;
            }
            Task::Node(term, mode, argument) => (term, mode, argument),
        };
        if argument && matches!(term, Term::Star(_)) {
            return Err(type_error());
        }
        control.work(|work| work.spend())?;
        if let Some(request) = control.work(|work| {
            let fields = fields.view();
            step(
                term,
                mode,
                &mut tasks,
                fields.columns,
                fields.alias,
                control.parameters(),
                &context.view(),
                &input_context.view(),
                work,
            )
        })? {
            let context = if request.mode.clear_group {
                &input_context
            } else {
                context
            };
            control
                .subquery(
                    request.query,
                    execution::Row::Empty,
                    fields.clone(),
                    context,
                    request.any,
                )
                .await?;
            if let Some(input) = request.input {
                tasks.push(Task::Node(input, request.mode, true))?;
            }
        }
    }
    Ok(())
}
