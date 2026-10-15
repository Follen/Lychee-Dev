//! Go 3.2.3 plans and actual query-scoped callback measurements.
//! Plain EXPLAIN binds schemas without evaluating or visiting source rows.
use super::*;
use std::borrow::Cow;

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

pub(super) fn prefix(sql: &str) -> Result<(bool, bool, Cow<'_, str>)> {
    use sqlparser::tokenizer::{Token, Tokenizer};
    let tokens = Tokenizer::new(&GenericDialect {}, sql)
        .tokenize_with_location()
        .map_err(|e| problem("relational.invalid_syntax", &e.to_string()))?;
    let mut significant = tokens
        .iter()
        .filter(|t| !matches!(t.token, Token::Whitespace(_)));
    let is_word = |token: &Token, expected: &str| matches!(token, Token::Word(w) if w.quote_style.is_none() && w.value.eq_ignore_ascii_case(expected));
    let Some(first) = significant.next().filter(|t| is_word(&t.token, "EXPLAIN")) else {
        return Ok((false, false, Cow::Borrowed(sql)));
    };
    let analyze = significant.next().filter(|t| is_word(&t.token, "ANALYZE"));
    // Mask only the recognized keywords, retaining comments and parser offsets.
    // Extra sqlparser EXPLAIN options still go through normal SELECT rejection.
    let mut masked = sql.to_owned();
    for token in std::iter::once(first).chain(analyze) {
        let location = token.span.start;
        let (mut line, mut column) = (1u64, 1u64);
        let mut offset = None;
        for (at, ch) in sql.char_indices() {
            if line == location.line && column == location.column {
                offset = Some(at);
                break;
            }
            if ch == '\n' {
                line += 1;
                column = 1;
            } else {
                column += 1;
            }
        }
        let offset = offset
            .ok_or_else(|| problem("relational.invalid_syntax", "Missing EXPLAIN keyword"))?;
        masked.replace_range(offset..offset + 7, "       ");
    }
    Ok((true, analyze.is_some(), Cow::Owned(masked)))
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
    query(program, 0, &mut plan, work)?;
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
fn expression(
    term: &Term,
    parent: usize,
    plan: &mut Explanation,
    work: &mut Work<'_>,
) -> Result<()> {
    work.spend()?;
    match term {
        Term::Unary(_, t) | Term::NullTest(t, _) | Term::Cast(t, _) => {
            expression(t, parent, plan, work)?
        }
        Term::Binary(a, _, b) | Term::Like(a, b, _) => {
            expression(a, parent, plan, work)?;
            expression(b, parent, plan, work)?;
        }
        Term::Between(a, b, c, _) => {
            expression(a, parent, plan, work)?;
            expression(b, parent, plan, work)?;
            expression(c, parent, plan, work)?;
        }
        Term::In(a, terms, _) => {
            expression(a, parent, plan, work)?;
            for t in terms {
                expression(t, parent, plan, work)?;
            }
        }
        Term::ScalarSubquery(body) | Term::Exists(body) => {
            let detail = if matches!(term, Term::Exists(_)) {
                "exists"
            } else {
                "subquery"
            };
            let child = add(plan, work, parent, "expression-subquery", detail)?;
            query(body, child, plan, work)?;
        }
        Term::InSubquery(input, body, _) => {
            let child = add(plan, work, parent, "expression-subquery", "operator")?;
            query(body, child, plan, work)?;
            expression(input, parent, plan, work)?;
        }
        Term::Call(_, terms) => {
            for t in terms {
                expression(t, parent, plan, work)?;
            }
        }
        Term::Aggregate(_, Some(t), _) => expression(t, parent, plan, work)?,
        Term::Case(base, branches, other) => {
            if let Some(t) = base {
                expression(t, parent, plan, work)?;
            }
            for (a, b) in branches {
                expression(a, parent, plan, work)?;
                expression(b, parent, plan, work)?;
            }
            if let Some(t) = other {
                expression(t, parent, plan, work)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn input(
    table: &RelationName,
    derived: Option<&Program>,
    parent: usize,
    plan: &mut Explanation,
    work: &mut Work<'_>,
) -> Result<()> {
    if let Some(body) = derived {
        query(body, parent, plan, work)?;
    } else {
        add(plan, work, parent, "input", &table.label())?;
    }
    Ok(())
}
fn query(
    program: &Program,
    parent: usize,
    plan: &mut Explanation,
    work: &mut Work<'_>,
) -> Result<()> {
    if program.branches.is_empty() {
        select(program, program, program, parent, plan, work)?;
    } else {
        // Rust's synthetic output SELECT is not in the Go syntax tree. CTEs
        // belong to the first branch; compound suffixes belong to the tail.
        let mut parent = parent;
        for (i, branch) in program.branches.iter().enumerate() {
            let bindings = if i == 0 { program } else { branch };
            let suffix = if i + 1 == program.branches.len() {
                program
            } else {
                branch
            };
            let id = select(branch, bindings, suffix, parent, plan, work)?;
            if i + 1 < program.branches.len() {
                parent = add(
                    plan,
                    work,
                    id,
                    "union-all",
                    "tail order/slice apply to compound result",
                )?;
            }
        }
    }
    Ok(())
}
fn select(
    program: &Program,
    bindings: &Program,
    suffix: &Program,
    parent: usize,
    plan: &mut Explanation,
    work: &mut Work<'_>,
) -> Result<usize> {
    let id = add(plan, work, parent, "select", "")?;
    for (name, body) in &bindings.bindings {
        let kind = if bindings.recursive && recursive::references(body, name) > 0 {
            "recursive-delta"
        } else {
            "materialized-cte"
        };
        let child = add(plan, work, id, kind, name)?;
        query(body, child, plan, work)?;
    }
    input(&program.table, program.derived.as_deref(), id, plan, work)?;
    for join in &program.joins {
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
        let child = add(plan, work, id, operation, &detail)?;
        input(&join.table, join.derived.as_deref(), child, plan, work)?;
        if let Some(t) = &join.condition {
            expression(t, child, plan, work)?;
        }
    }
    if let Some(t) = &program.filter {
        let child = add(plan, work, id, "filter", "")?;
        expression(t, child, plan, work)?;
    }
    let grouped = !program.groups.is_empty()
        || program.having.is_some()
        || program
            .projection
            .iter()
            .filter_map(|p| p.expr.as_ref())
            .any(aggregate::contains)
        || suffix.order.iter().any(|(t, _, _)| aggregate::contains(t));
    if grouped {
        add(plan, work, id, "group", "hash keys; streaming accumulators")?;
    }
    if let Some(t) = &program.having {
        expression(t, id, plan, work)?;
    }
    for p in &program.projection {
        if let Some(t) = &p.expr {
            expression(t, id, plan, work)?;
        }
    }
    if program.distinct {
        add(plan, work, id, "distinct", "typed value keys")?;
    }
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
        add(plan, work, id, "order", detail)?;
    }
    for (t, _, _) in &suffix.order {
        expression(t, id, plan, work)?;
    }
    if suffix.limit.is_some() || suffix.offset.is_some() {
        add(plan, work, id, "slice", "offset then limit")?;
    }
    Ok(id)
}
