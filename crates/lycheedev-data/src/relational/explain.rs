//! Go 3.2.3 plans and actual query-scoped callback measurements.
//! Plain EXPLAIN binds schemas without evaluating or visiting source rows.
use super::*;

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct PlanStep {
    pub id: usize,
    pub parent: usize,
    pub operation: &'static str,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Explanation {
    pub output_columns: Vec<String>,
    pub steps: Vec<PlanStep>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measured: Option<ExecutionMeasurement>,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct MeasuredTable {
    pub catalog: String,
    pub name: String,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ScanMeasurement {
    pub table: MeasuredTable,
    pub calls: u64,
    pub rows: u64,
    #[serde(skip_serializing_if = "is_zero")]
    pub lookups: u64,
}
fn is_zero(value: &u64) -> bool {
    *value == 0
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionMeasurement {
    pub scans: Vec<ScanMeasurement>,
    pub work_units: u64,
    pub charged_bytes: usize,
    pub output_rows: usize,
}

pub(super) fn prefix(tokens: &mut Vec<sqlparser::tokenizer::TokenWithSpan>) -> (bool, bool) {
    use sqlparser::tokenizer::Token;
    let is_word = |token: &Token, expected: &str| {
        matches!(token, Token::Word(w)
        if w.quote_style.is_none() && equal_fold(&w.value, expected))
    };
    if !tokens.first().is_some_and(|t| is_word(&t.token, "EXPLAIN")) {
        return (false, false);
    }
    let analyze = tokens.get(1).is_some_and(|t| is_word(&t.token, "ANALYZE"));
    // Original-source admission and spans are already complete. Remove just
    // these tokens, without masking Unicode or scanning a second SQL string.
    tokens.drain(..if analyze { 2 } else { 1 });
    (true, analyze)
}

pub(super) fn build(
    program: &Program,
    output_columns: Vec<String>,
    work: &mut Work<'_>,
) -> Result<Explanation> {
    let mut plan = Explanation {
        output_columns,
        steps: vec![],
        measured: None,
    };
    walk(program, &mut plan, work)?;
    Ok(plan)
}
fn add(
    plan: &mut Explanation,
    work: &mut Work<'_>,
    parent: usize,
    operation: &'static str,
    detail: &str,
) -> Result<usize> {
    work.spend()?;
    work.retain(64 + operation.len() + detail.len())?;
    let id = plan.steps.len() + 1;
    plan.steps.push(PlanStep {
        id,
        parent,
        operation,
        detail: detail.into(),
    });
    Ok(id)
}

enum Task<'a> {
    Query(&'a Program, usize),
    Chain(&'a Program, usize, usize),
    NextBranch(&'a Program, usize, usize),
    Select(
        &'a Program,
        &'a Program,
        &'a Program,
        usize,
        Option<(&'a Program, usize)>,
    ),
    Cte(&'a str, &'a Program, bool, usize),
    Input(&'a RelationName, Option<&'a Program>, usize),
    Join(&'a joins::Link, usize),
    Filter(&'a Term, usize),
    Expression(&'a Term, usize),
    Wildcard,
    Add(usize, &'static str, &'static str),
}
fn walk(program: &Program, plan: &mut Explanation, work: &mut Work<'_>) -> Result<()> {
    let mut pending = vec![Task::Query(program, 0)];
    let mut children = Vec::new();
    while let Some(task) = pending.pop() {
        match task {
            Task::Query(program, parent) => {
                pending.push(if program.branches.is_empty() {
                    Task::Select(program, program, program, parent, None)
                } else {
                    Task::Chain(program, 0, parent)
                });
            }
            Task::Chain(program, index, parent) => {
                let branch = &program.branches[index];
                let bindings = if index == 0 { program } else { branch };
                let suffix = if index + 1 == program.branches.len() {
                    program
                } else {
                    branch
                };
                pending.push(Task::Select(
                    branch,
                    bindings,
                    suffix,
                    parent,
                    Some((program, index)),
                ));
            }
            Task::NextBranch(program, index, id) => {
                if index + 1 < program.branches.len() {
                    let parent = add(
                        plan,
                        work,
                        id,
                        "union-all",
                        "tail order/slice apply to compound result",
                    )?;
                    pending.push(Task::Chain(program, index + 1, parent));
                }
            }
            Task::Select(program, bindings, suffix, parent, chain) => {
                let id = add(plan, work, parent, "select", "")?;
                let grouped = !program.groups.is_empty()
                    || program.having.is_some()
                    || program
                        .projection
                        .iter()
                        .filter_map(|p| p.expr.as_ref())
                        .any(aggregate::contains)
                    || suffix.order.iter().any(|(t, _, _)| aggregate::contains(t));
                if let Some((program, index)) = chain {
                    pending.push(Task::NextBranch(program, index, id));
                }
                if suffix.limit.is_some() || suffix.offset.is_some() {
                    pending.push(Task::Add(id, "slice", "offset then limit"));
                }
                pending.extend(
                    suffix
                        .order
                        .iter()
                        .rev()
                        .map(|(t, _, _)| Task::Expression(t, id)),
                );
                if !suffix.order.is_empty() {
                    let detail = if !grouped
                        && !program.distinct
                        && suffix.limit.is_some()
                        && suffix.order.len() == 1
                    {
                        "bounded stable Top-K when offset+limit <= 1000000; otherwise stable merge sort; validates complete input"
                    } else {
                        "stable merge sort"
                    };
                    pending.push(Task::Add(id, "order", detail));
                }
                if program.distinct {
                    pending.push(Task::Add(id, "distinct", "typed value keys"));
                }
                pending.extend(program.projection.iter().rev().map(|p| {
                    p.expr
                        .as_ref()
                        .map_or(Task::Wildcard, |t| Task::Expression(t, id))
                }));
                if let Some(t) = &program.having {
                    pending.push(Task::Expression(t, id));
                }
                if grouped {
                    pending.push(Task::Add(id, "group", "hash keys; streaming accumulators"));
                }
                if let Some(t) = &program.filter {
                    pending.push(Task::Filter(t, id));
                }
                pending.extend(program.joins.iter().rev().map(|join| Task::Join(join, id)));
                pending.push(Task::Input(&program.table, program.derived.as_deref(), id));
                pending.extend(
                    bindings
                        .bindings
                        .iter()
                        .rev()
                        .map(|(name, body)| Task::Cte(name, body, bindings.recursive, id)),
                );
            }
            Task::Cte(name, body, recursive, parent) => {
                let kind = if recursive && recursive::references(body, name) > 0 {
                    "recursive-delta"
                } else {
                    "materialized-cte"
                };
                let child = add(plan, work, parent, kind, name)?;
                pending.push(Task::Query(body, child));
            }
            Task::Input(table, derived, parent) => {
                if let Some(body) = derived {
                    pending.push(Task::Query(body, parent));
                } else {
                    add(plan, work, parent, "input", &table.label())?;
                }
            }
            Task::Join(join, parent) => {
                let kind = if join.condition.is_none() {
                    "cross"
                } else if join.left {
                    "left"
                } else {
                    "inner"
                };
                let equality = matches!(
                    &join.condition,
                    Some(Term::Binary(_, BinaryOperator::Eq, _))
                );
                let (operation, detail) = if equality {
                    (
                        "adaptive-equality-join",
                        format!("{kind}; bounded hash index when eligible; nested-loop fallback"),
                    )
                } else {
                    ("nested-loop-join", format!("{kind}; materialize right"))
                };
                let child = add(plan, work, parent, operation, &detail)?;
                if let Some(t) = &join.condition {
                    pending.push(Task::Expression(t, child));
                }
                pending.push(Task::Input(&join.table, join.derived.as_deref(), child));
            }
            Task::Filter(term, parent) => {
                let child = add(plan, work, parent, "filter", "")?;
                pending.push(Task::Expression(term, child));
            }
            Task::Expression(term, parent) => {
                work.spend()?;
                match term {
                    Term::ScalarSubquery(body) | Term::Exists(body) => {
                        let detail = if matches!(term, Term::Exists(_)) {
                            "exists"
                        } else {
                            "subquery"
                        };
                        let child = add(plan, work, parent, "expression-subquery", detail)?;
                        pending.push(Task::Query(body, child));
                    }
                    Term::InSubquery(input, body, _) => {
                        let child = add(plan, work, parent, "expression-subquery", "operator")?;
                        pending.push(Task::Expression(input, parent));
                        pending.push(Task::Query(body, child));
                    }
                    _ => {
                        term_tree::children(term, &mut children);
                        pending.extend(
                            children
                                .drain(..)
                                .map(|term| Task::Expression(term, parent)),
                        );
                    }
                }
            }
            Task::Wildcard => work.spend()?,
            Task::Add(parent, operation, detail) => {
                add(plan, work, parent, operation, detail)?;
            }
        }
    }
    Ok(())
}
