//! Read-only typed SQL evaluation. Integer lexemes/providers retain all 64 bits;
//! parsing does not authorize writes, implicit casts or external I/O.
use crate::hotfix::remote::{equal_fold, identifier_lower};
use crate::{fault, table};
use lycheedev_core::Result;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use serde::Serialize;
use sqlparser::{ast::*, dialect::GenericDialect, parser::Parser};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering as AtomicOrdering},
    time::{Duration, Instant},
};
mod aggregate;
mod compound;
mod explain;
pub use explain::{ExecutionMeasurement, Explanation, MeasuredTable, PlanStep, ScanMeasurement};
mod identifier_categories;
mod identifiers;
mod lexical;
mod parser_admission;
pub(crate) use identifiers::SourceTableUse;
use identifiers::{ColumnOwners, ColumnRef, RelationName};
mod joins;
mod provider;
pub(crate) use provider::{EqualityOutcome, PreparedSource, RootResolver, RowProvider, RowYield};
mod recursive;
mod results;
mod scalar;
pub(crate) use scalar::text as scalar_text;
mod selection;
mod subquery;
mod syntax;
mod topk;
#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(untagged)]
pub enum Scalar {
    Null,
    Int(i64),
    UInt(u64),
    Float(f64),
    Bool(bool),
    String(String),
}
fn problem(code: &str, message: &str) -> lycheedev_core::ToolkitError {
    fault(code, 4, message)
}
fn unsupported() -> lycheedev_core::ToolkitError {
    fault(
        "relational.unsupported",
        3,
        "This SQL construct has not completed Rust evaluation parity",
    )
}
fn numeric() -> lycheedev_core::ToolkitError {
    problem(
        "relational.numeric_range",
        "SQL number exceeds supported finite 64-bit range",
    )
}
fn type_error() -> lycheedev_core::ToolkitError {
    problem(
        "relational.type_mismatch",
        "SQL scalar types are incompatible",
    )
}
fn binding() -> lycheedev_core::ToolkitError {
    problem(
        "relational.invalid_binding",
        "SQL table, column or parameter binding is invalid or ambiguous",
    )
}
fn budget() -> lycheedev_core::ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "SQL work/memory/deadline budget exceeded",
    )
}
fn integer(value: i128) -> Result<Scalar> {
    if let Ok(v) = i64::try_from(value) {
        Ok(Scalar::Int(v))
    } else if let Ok(v) = u64::try_from(value) {
        Ok(Scalar::UInt(v))
    } else {
        Err(numeric())
    }
}
pub(crate) fn literal(text: &str) -> Result<Scalar> {
    if text.len() > 1024 {
        return Err(numeric());
    }
    if !scalar::decimal_literal(text) {
        return Err(type_error());
    }
    if text.contains(['.', 'e', 'E']) {
        let v = text.parse::<f64>().map_err(|_| numeric())?;
        if !v.is_finite() {
            return Err(numeric());
        }
        Ok(Scalar::Float(v))
    } else {
        text.parse::<i128>()
            .map_err(|_| numeric())
            .and_then(integer)
    }
}
fn int(v: &Scalar) -> Option<i128> {
    match v {
        Scalar::Int(n) => Some(*n as i128),
        Scalar::UInt(n) => Some(*n as i128),
        _ => None,
    }
}
fn number(v: &Scalar) -> Result<f64> {
    match v {
        Scalar::Int(n) => Ok(*n as f64),
        Scalar::UInt(n) => Ok(*n as f64),
        Scalar::Float(n) if n.is_finite() => Ok(*n),
        _ => Err(type_error()),
    }
}
fn compare_integer_float(n: i128, f: f64) -> Result<Ordering> {
    if !f.is_finite() {
        return Err(numeric());
    }
    if f >= i128::MAX as f64 {
        return Ok(Ordering::Less);
    }
    if f <= i128::MIN as f64 {
        return Ok(Ordering::Greater);
    }
    let truncated = f.trunc() as i128;
    let order = n.cmp(&truncated);
    if order != Ordering::Equal {
        return Ok(order);
    }
    Ok(if f > f.trunc() {
        Ordering::Less
    } else if f < f.trunc() {
        Ordering::Greater
    } else {
        Ordering::Equal
    })
}
fn compare(a: &Scalar, b: &Scalar) -> Result<Ordering> {
    match (a, b) {
        (Scalar::String(a), Scalar::String(b)) => Ok(a.cmp(b)),
        (Scalar::Bool(a), Scalar::Bool(b)) => Ok(a.cmp(b)),
        (Scalar::Float(a), Scalar::Float(b)) => a.partial_cmp(b).ok_or_else(numeric),
        (a, Scalar::Float(b)) if int(a).is_some() => compare_integer_float(int(a).unwrap(), *b),
        (Scalar::Float(a), b) if int(b).is_some() => {
            Ok(compare_integer_float(int(b).unwrap(), *a)?.reverse())
        }
        (a, b) if int(a).is_some() && int(b).is_some() => Ok(int(a).cmp(&int(b))),
        _ => Err(type_error()),
    }
}
fn truth(v: &Scalar) -> Result<Option<bool>> {
    match v {
        Scalar::Null => Ok(None),
        Scalar::Bool(v) => Ok(Some(*v)),
        _ => Err(type_error()),
    }
}
fn arithmetic(a: &Scalar, op: &BinaryOperator, b: &Scalar) -> Result<Scalar> {
    if matches!(a, Scalar::Null) || matches!(b, Scalar::Null) {
        return Ok(Scalar::Null);
    }
    if let (Some(a), Some(b)) = (int(a), int(b)) {
        let n = match op {
            BinaryOperator::Plus => a.checked_add(b),
            BinaryOperator::Minus => a.checked_sub(b),
            BinaryOperator::Multiply => a.checked_mul(b),
            BinaryOperator::Modulo if b == 0 => return Ok(Scalar::Null),
            BinaryOperator::Modulo => a.checked_rem(b),
            BinaryOperator::Divide => return rational_arithmetic(&integer(a)?, op, &integer(b)?),
            _ => return Err(unsupported()),
        };
        return integer(n.ok_or_else(numeric)?);
    }
    rational_arithmetic(a, op, b)
}
fn rational(value: &Scalar) -> Result<BigRational> {
    if let Some(n) = int(value) {
        return Ok(BigRational::from_integer(n.into()));
    }
    if let Scalar::Float(f) = value {
        return BigRational::from_float(*f).ok_or_else(numeric);
    }
    Err(type_error())
}
fn rational_arithmetic(a: &Scalar, op: &BinaryOperator, b: &Scalar) -> Result<Scalar> {
    let (a, b) = (rational(a)?, rational(b)?);
    // Every operand is first canonicalized to i64/u64/finite f64. A binary
    // f64 needs at most 1075 denominator bits; arithmetic never retains a
    // rational between SQL nodes. Guard both inputs and output explicitly.
    let bounded = |n: &BigRational| n.numer().bits() <= 4096 && n.denom().bits() <= 4096;
    if !bounded(&a) || !bounded(&b) {
        return Err(budget());
    }
    if matches!(op, BinaryOperator::Divide | BinaryOperator::Modulo) && b.is_zero() {
        return Ok(Scalar::Null);
    }
    let result = match op {
        BinaryOperator::Plus => a + b,
        BinaryOperator::Minus => a - b,
        BinaryOperator::Multiply => a * b,
        BinaryOperator::Divide => a / b,
        BinaryOperator::Modulo => {
            let whole = (&a / &b).to_integer();
            a - BigRational::from_integer(whole) * b
        }
        _ => return Err(unsupported()),
    };
    if !bounded(&result) {
        return Err(budget());
    }
    let f = result
        .to_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(numeric)?;
    Ok(Scalar::Float(f))
}
#[derive(Clone, Debug, PartialEq)]
enum Term {
    Value(Scalar),
    Number(String),
    // Internal compound output projection. Never produced by the SQL parser.
    Ordinal(usize),
    Column(ColumnRef),
    Parameter(String),
    Unary(UnaryOperator, Box<Term>),
    Binary(Box<Term>, BinaryOperator, Box<Term>),
    NullTest(Box<Term>, bool),
    Cast(Box<Term>, String),
    Like(Box<Term>, Box<Term>, bool),
    Between(Box<Term>, Box<Term>, Box<Term>, bool),
    In(Box<Term>, Vec<Term>, bool),
    InSubquery(Box<Term>, std::sync::Arc<Program>, bool),
    ScalarSubquery(std::sync::Arc<Program>),
    Exists(std::sync::Arc<Program>),
    // Keep the complete parsed call shape until real schema binding. Invalid
    // arity, DISTINCT and star qualifiers must not hide a resolver failure.
    Star(String),
    Call(String, Vec<Term>, bool),
    Aggregate(String, Vec<Term>, bool),
    Case(Option<Box<Term>>, Vec<(Term, Term)>, Option<Box<Term>>),
}
fn term(expr: &Expr, parameters: &mut BTreeSet<String>, depth: usize) -> Result<Term> {
    if depth > 256 {
        return Err(budget());
    }
    let mut nested = |e: &Expr| term(e, parameters, depth + 1);
    Ok(match expr {
        Expr::Wildcard(_) => Term::Star(String::new()),
        Expr::QualifiedWildcard(name, _) => {
            let [ObjectNamePart::Identifier(name)] = name.0.as_slice() else {
                return Err(unsupported());
            };
            Term::Star(syntax::identifier(name)?.to_owned())
        }
        Expr::Identifier(name) => {
            Term::Column(ColumnRef::new(syntax::identifier(name)?.to_owned()))
        }
        Expr::CompoundIdentifier(names) if names.len() == 2 => Term::Column(ColumnRef::qualified(
            syntax::identifier(&names[0])?.to_owned(),
            syntax::identifier(&names[1])?.to_owned(),
        )),
        Expr::Value(v) => match &v.value {
            Value::Null => Term::Value(Scalar::Null),
            Value::Boolean(v) => Term::Value(Scalar::Bool(*v)),
            Value::Number(n, _) => Term::Number(n.clone()),
            Value::SingleQuotedString(v) => Term::Value(Scalar::String(v.clone())),
            Value::Placeholder(name) if name.starts_with(':') && name.len() > 1 => {
                parameters.insert(name[1..].into());
                Term::Parameter(name[1..].into())
            }
            _ => return Err(unsupported()),
        },
        Expr::Nested(e) => return nested(e),
        Expr::UnaryOp { op, expr }
            if matches!(
                op,
                UnaryOperator::Plus | UnaryOperator::Minus | UnaryOperator::Not
            ) =>
        {
            Term::Unary(*op, Box::new(nested(expr)?))
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
            Term::Binary(
                Box::new(nested(left)?),
                op.clone(),
                Box::new(nested(right)?),
            )
        }
        Expr::Cast {
            kind: CastKind::Cast,
            expr,
            data_type,
            format: None,
        } => Term::Cast(Box::new(nested(expr)?), scalar::cast_name(data_type)?),
        Expr::Like {
            negated,
            any: false,
            expr,
            pattern,
            escape_char: None,
        } => Term::Like(
            Box::new(nested(expr)?),
            Box::new(nested(pattern)?),
            *negated,
        ),
        Expr::IsNull(e) => Term::NullTest(Box::new(nested(e)?), false),
        Expr::IsNotNull(e) => Term::NullTest(Box::new(nested(e)?), true),
        Expr::Between {
            expr,
            negated,
            low,
            high,
        } => Term::Between(
            Box::new(nested(expr)?),
            Box::new(nested(low)?),
            Box::new(nested(high)?),
            *negated,
        ),
        Expr::InList {
            expr,
            list,
            negated,
        } => Term::In(
            Box::new(nested(expr)?),
            list.iter().map(&mut nested).collect::<Result<_>>()?,
            *negated,
        ),
        Expr::InSubquery {
            expr,
            subquery,
            negated,
        } => {
            let input = nested(expr)?;
            let query = Program::compile_query(*subquery.clone(), depth + 1)?;
            parameters.extend(query.parameters.iter().cloned());
            Term::InSubquery(Box::new(input), std::sync::Arc::new(query), *negated)
        }
        Expr::Subquery(subquery) | Expr::Exists { subquery, .. } => {
            let query = Program::compile_query(*subquery.clone(), depth + 1)?;
            parameters.extend(query.parameters.iter().cloned());
            let query = std::sync::Arc::new(query);
            match expr {
                Expr::Exists { negated, .. } => {
                    let exists = Term::Exists(query);
                    if *negated {
                        Term::Unary(UnaryOperator::Not, Box::new(exists))
                    } else {
                        exists
                    }
                }
                _ => Term::ScalarSubquery(query),
            }
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
            let FunctionArguments::List(args) = &function.args else {
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
            let mut terms = Vec::with_capacity(args.args.len());
            for arg in &args.args {
                terms.push(match arg {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => nested(expr)?,
                    FunctionArg::Unnamed(FunctionArgExpr::Wildcard) => Term::Star(String::new()),
                    FunctionArg::Unnamed(FunctionArgExpr::QualifiedWildcard(name)) => {
                        let [ObjectNamePart::Identifier(name)] = name.0.as_slice() else {
                            return Err(unsupported());
                        };
                        Term::Star(syntax::identifier(name)?.to_owned())
                    }
                    _ => return Err(unsupported()),
                });
            }
            if ["COUNT", "MIN", "MAX", "SUM", "AVG"].contains(&name.as_str()) {
                Term::Aggregate(name, terms, distinct)
            } else {
                Term::Call(name, terms, distinct)
            }
        }
        Expr::Case {
            operand,
            conditions,
            else_result,
            ..
        } => Term::Case(
            operand
                .as_ref()
                .map(|e| nested(e).map(Box::new))
                .transpose()?,
            conditions
                .iter()
                .map(|pair| Ok((nested(&pair.condition)?, nested(&pair.result)?)))
                .collect::<Result<_>>()?,
            Some(
                else_result
                    .as_ref()
                    .map(|e| nested(e))
                    .transpose()?
                    .map_or_else(|| Box::new(Term::Value(Scalar::Null)), Box::new),
            ),
        ),
        _ => return Err(unsupported()),
    })
}
fn check_arity(name: &str, count: usize) -> Result<()> {
    let valid = match name {
        "COALESCE" => count > 0,
        "LOWER" | "UPPER" | "LENGTH" => count == 1,
        "NULLIF" | "HAS_FLAG" | "BIT_AND" => count == 2,
        _ => return Err(unsupported()),
    };
    if valid { Ok(()) } else { Err(type_error()) }
}
fn bind(
    term: &Term,
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<()> {
    bind_with_aggregates(term, columns, alias, params, true, context, work)
}
fn bind_with_aggregates(
    term: &Term,
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    allow_aggregates: bool,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<()> {
    work.spend()?;
    match term {
        Term::Ordinal(index) if *index >= columns.len() => return Err(binding()),
        Term::Column(name) => {
            subquery::resolve(columns, alias, name, context.scope.outer)?;
        }
        Term::Parameter(name) if !params.contains_key(name) => return Err(binding()),
        Term::Unary(_, t) | Term::NullTest(t, _) => {
            bind_argument(t, columns, alias, params, allow_aggregates, context, work)?
        }
        Term::Binary(a, _, b) | Term::Like(a, b, _) => {
            bind_argument(a, columns, alias, params, allow_aggregates, context, work)?;
            bind_argument(b, columns, alias, params, allow_aggregates, context, work)?;
        }
        Term::Between(a, b, c, _) => {
            for t in [a, b, c] {
                bind_argument(t, columns, alias, params, allow_aggregates, context, work)?;
            }
        }
        Term::In(t, list, _) => {
            bind_argument(t, columns, alias, params, allow_aggregates, context, work)?;
            for t in list {
                bind_argument(t, columns, alias, params, allow_aggregates, context, work)?;
            }
        }
        Term::InSubquery(input, query, _) => {
            if context.scope.callback.is_none() {
                return Err(unsupported());
            }
            // Go binds the nested schema before this operator's arguments,
            // even for an unreachable branch or an empty outer input.
            if work.planning {
                subquery::execute(query, &[], columns, alias, params, context, work)?;
            }
            bind_argument(
                input,
                columns,
                alias,
                params,
                allow_aggregates,
                context,
                work,
            )?;
        }
        Term::ScalarSubquery(query) | Term::Exists(query) => {
            if context.scope.callback.is_none() {
                return Err(unsupported());
            }
            if work.planning {
                if matches!(term, Term::Exists(_)) {
                    subquery::execute_any(query, &[], columns, alias, params, context, work)?;
                } else {
                    subquery::execute(query, &[], columns, alias, params, context, work)?;
                }
            }
        }
        Term::Cast(input, kind) => {
            scalar::cast_kind(kind)?;
            bind_argument(
                input,
                columns,
                alias,
                params,
                allow_aggregates,
                context,
                work,
            )?;
        }
        Term::Aggregate(name, args, distinct) => {
            if !allow_aggregates {
                return Err(type_error());
            }
            if let Some(input) = aggregate::input(name, args, *distinct)? {
                let input_context = subquery::Context {
                    group: None,
                    scope: subquery::Scope {
                        allowed: None,
                        ..context.scope
                    },
                    ..*context
                };
                bind_argument(input, columns, alias, params, false, &input_context, work)?;
            }
        }
        Term::Call(name, list, distinct) => {
            if *distinct {
                return Err(type_error());
            }
            check_arity(name, list.len())?;
            for t in list {
                bind_argument(t, columns, alias, params, allow_aggregates, context, work)?;
            }
        }
        Term::Case(operand, cases, fallback) => {
            if let Some(t) = operand {
                bind_argument(t, columns, alias, params, allow_aggregates, context, work)?;
            }
            for (a, b) in cases {
                bind_argument(a, columns, alias, params, allow_aggregates, context, work)?;
                bind_argument(b, columns, alias, params, allow_aggregates, context, work)?;
            }
            if let Some(t) = fallback {
                bind_argument(t, columns, alias, params, allow_aggregates, context, work)?;
            }
        }
        _ => {}
    }
    Ok(())
}
// Go's loop checks star children before calling bindTerm, so rejecting one
// consumes the parent node but no child unit. A root star is handled by the
// projection expander; COUNT(*) bypasses this helper after shape validation.
fn bind_argument(
    term: &Term,
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    allow_aggregates: bool,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<()> {
    if matches!(term, Term::Star(_)) {
        return Err(type_error());
    }
    bind_with_aggregates(
        term,
        columns,
        alias,
        params,
        allow_aggregates,
        context,
        work,
    )
}

fn column(columns: &[String], alias: ColumnOwners<'_>, name: &ColumnRef) -> Result<usize> {
    let mut found = columns.iter().enumerate().filter(|(i, candidate)| {
        (name.qualifier.is_empty() || equal_fold(&name.qualifier, alias.owner(*i)))
            && equal_fold(candidate, &name.name)
    });
    let index = found
        .next()
        .map(|(i, _)| i)
        .ok_or_else(|| missing_column(columns, alias, name))?;
    if found.next().is_some() {
        return Err(binding());
    }
    Ok(index)
}
fn missing_column(
    columns: &[String],
    alias: ColumnOwners<'_>,
    name: &ColumnRef,
) -> lycheedev_core::ToolkitError {
    let mut candidates = Vec::<(usize, String)>::new();
    for (i, candidate) in columns.iter().enumerate() {
        let owner = alias.owner(i);
        if !name.qualifier.is_empty() && !equal_fold(&name.qualifier, owner) {
            continue;
        }
        let distance = column_distance(&name.name, candidate);
        if distance > 3 {
            continue;
        }
        let label = if owner.is_empty() {
            candidate.to_owned()
        } else {
            format!("{owner}.{candidate}")
        };
        let item = (distance, label);
        let at = candidates.partition_point(|candidate| candidate <= &item);
        if at < 8 {
            candidates.insert(at, item);
            candidates.truncate(8);
        }
    }
    let candidates: Vec<_> = candidates.into_iter().map(|(_, name)| name).collect();
    let label = name.label();
    let mut error = binding();
    error.message = format!(
        "relational.unresolved_binding: column {label}; candidates: {}",
        candidates.join(", ")
    );
    error.details = serde_json::json!({"column":label,"candidates":candidates});
    error
}
fn column_distance(a: &str, b: &str) -> usize {
    let mut x = ['\0'; 256];
    let mut y = ['\0'; 256];
    let mut nx = 0;
    let mut ny = 0;
    for c in a.chars() {
        if nx == 256 {
            return 4;
        }
        x[nx] = crate::unicode_simple::map(c, false);
        nx += 1;
    }
    for c in b.chars() {
        if ny == 256 {
            return 4;
        }
        y[ny] = crate::unicode_simple::map(c, false);
        ny += 1;
    }
    let mut previous = [0usize; 257];
    let mut next = [0usize; 257];
    for (i, v) in previous.iter_mut().enumerate().take(ny + 1) {
        *v = i;
    }
    for (i, a) in x.iter().enumerate().take(nx) {
        next[0] = i + 1;
        for (j, b) in y.iter().enumerate().take(ny) {
            next[j + 1] = (previous[j + 1] + 1)
                .min(next[j] + 1)
                .min(previous[j] + usize::from(a != b));
        }
        std::mem::swap(&mut previous, &mut next);
    }
    previous[ny]
}
#[derive(Clone, Copy)]
pub struct Limits {
    pub work: u64,
    pub memory_bytes: usize,
    pub deadline: Instant,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            work: 5_000_000,
            memory_bytes: 64 << 20,
            deadline: Instant::now() + Duration::from_secs(180),
        }
    }
}
struct Work<'a> {
    left: u64,
    bytes: usize,
    deadline: Instant,
    cancel: Option<&'a AtomicBool>,
    shared: Option<&'a crate::resource::Budget>,
    planning: bool,
    // Only temporary Rust allocations, never Go logical retention/work.
    temporaries: Vec<crate::resource::RetainedReservation<'a>>,
    // Only explicitly prepared physical inputs may use equality selection.
    // Materialized CTE/derived/effective/meta sources never inherit a capability.
    selection_sources: Vec<(&'a Source, usize)>,
    providers: &'a [PreparedSource<'a>],
    resolver: Option<&'a dyn RootResolver>,
    roots: BTreeMap<String, &'a Source>,
    measure: bool,
    scans: Vec<(&'a Source, explain::ScanMeasurement)>,
}
impl<'a> Work<'a> {
    fn measure_source(&mut self, name: &RelationName, source: &'a Source) -> Result<()> {
        if !self.measure {
            return Ok(());
        }
        let table = explain::MeasuredTable {
            catalog: identifier_lower(name.table_use().catalog),
            name: identifier_lower(&name.name),
        };
        if self.scans.iter().any(|(_, scan)| scan.table == table) {
            return Ok(());
        }
        if let Some(shared) = self.shared {
            shared
                .safety_only()
                .charge(crate::resource::Cost::retained(
                    256 + table.catalog.len() + table.name.len(),
                ))?;
        }
        self.scans.push((
            source,
            explain::ScanMeasurement {
                table,
                calls: 0,
                rows: 0,
                lookups: 0,
            },
        ));
        Ok(())
    }
    fn measure_root(&mut self, name: &RelationName) -> Result<()> {
        if let Some(source) = self.roots.get(&name.physical_key()).copied() {
            self.measure_source(name, source)?;
        }
        Ok(())
    }
    fn resolve_root(&mut self, name: &RelationName) -> Result<&'a Source> {
        if name.name.starts_with('\0') {
            return Err(binding());
        }
        let key = name.physical_key();
        if let Some(source) = self.roots.get(&key) {
            return Ok(*source);
        }
        let resolver = self.resolver.ok_or_else(binding)?;
        self.consume(0)?;
        let source = resolver.resolve(name.table_use())?;
        // Go execute.go's cached resolver retains its immutable source schema
        // once, before validating/using it in the real preflight. This is the
        // original executor allowance, never a new Work or query replay.
        let bytes = source.columns.iter().try_fold(
            128usize
                .checked_add(source.columns.len().checked_mul(32).ok_or_else(budget)?)
                .ok_or_else(budget)?,
            |bytes, name| bytes.checked_add(name.len()).ok_or_else(budget),
        )?;
        self.retain(bytes)?;
        if let Some(shared) = self.shared {
            shared
                .safety_only()
                .charge(crate::resource::Cost::retained(key.len() + 96))?;
        }
        self.measure_source(name, source)?;
        self.roots.insert(key, source);
        Ok(source)
    }
}
impl Work<'_> {
    fn temporary(&mut self, bytes: usize) -> Result<()> {
        self.consume(0)?;
        if let Some(shared) = self.shared {
            if self.temporaries.len() == self.temporaries.capacity() {
                let capacity = self
                    .temporaries
                    .capacity()
                    .max(2)
                    .checked_mul(2)
                    .ok_or_else(budget)?;
                self.safety_retain(
                    (capacity - self.temporaries.capacity())
                        .checked_mul(
                            std::mem::size_of::<crate::resource::RetainedReservation<'_>>(),
                        )
                        .ok_or_else(budget)?,
                )?;
                self.temporaries
                    .try_reserve_exact(capacity - self.temporaries.len())
                    .map_err(|_| budget())?;
            }
            self.temporaries.push(shared.reserve_retained(bytes)?);
        }
        Ok(())
    }
    fn temporary_scope<T>(&mut self, work: impl FnOnce(&mut Self) -> Result<T>) -> Result<T> {
        let mark = self.temporaries.len();
        let result = work(self);
        self.temporaries.truncate(mark);
        result
    }

    fn safety_retain(&mut self, bytes: usize) -> Result<()> {
        self.consume(0)?;
        if let Some(shared) = self.shared {
            shared
                .safety_only()
                .charge(crate::resource::Cost::retained(bytes))?;
        }
        Ok(())
    }
    fn scan_index(&self, source: &Source) -> Option<usize> {
        self.scans
            .iter()
            .position(|(root, _)| std::ptr::eq(*root, source))
    }
    fn scan_call(&mut self, index: Option<usize>, lookup: bool) {
        if let Some(index) = index {
            self.scans[index].1.calls += 1;
            self.scans[index].1.lookups += u64::from(lookup);
        }
    }
    fn scan_row(&mut self, index: Option<usize>) {
        if let Some(index) = index {
            self.scans[index].1.rows += 1;
        }
    }
    fn measured(&mut self, limits: &Limits, output_rows: usize) -> explain::ExecutionMeasurement {
        let mut scans: Vec<_> = std::mem::take(&mut self.scans)
            .into_iter()
            .map(|(_, scan)| scan)
            .collect();
        scans.sort_by(|a, b| a.table.cmp(&b.table));
        explain::ExecutionMeasurement {
            scans,
            work_units: limits.work - self.left,
            charged_bytes: limits.memory_bytes - self.bytes,
            output_rows,
        }
    }
    fn spend(&mut self) -> Result<()> {
        self.consume(1)
    }
    fn consume(&mut self, amount: usize) -> Result<()> {
        if let Some(shared) = self.shared {
            shared.charge(crate::resource::Cost::work(amount))?;
        }
        if self.cancel.is_some_and(|v| v.load(AtomicOrdering::Acquire)) {
            return Err(fault("operation.cancelled", 7, "SQL query cancelled"));
        }
        if self.left < amount as u64 || Instant::now() >= self.deadline {
            return Err(budget());
        }
        self.left -= amount as u64;
        Ok(())
    }
    fn retain(&mut self, bytes: usize) -> Result<()> {
        if let Some(shared) = self.shared {
            shared.charge(crate::resource::Cost::retained(bytes))?;
        }
        self.bytes = self.bytes.checked_sub(bytes).ok_or_else(budget)?;
        Ok(())
    }
    fn clone_scalar(&mut self, value: &Scalar) -> Result<Scalar> {
        if let Scalar::Float(value) = value
            && !value.is_finite()
        {
            return Err(numeric());
        }
        if let Scalar::String(text) = value {
            // Go scalars share immutable strings; row/group ownership has its
            // own logical charge. Only Rust's necessary clone is additional.
            self.temporary(text.len())?;
        }
        Ok(value.clone())
    }
}
fn case_text(text: &str, upper: bool, work: &mut Work<'_>) -> Result<String> {
    if text.len() > 1 << 20 {
        return Err(budget());
    }
    work.consume(text.len())?;
    // Count the exact UTF8 destination before allocating, rather than guessing
    // a full-case expansion. The Go baseline uses Unicode17 simple mappings.
    let mut bytes = 0usize;
    for (i, c) in text.chars().enumerate() {
        if i % 4096 == 0 {
            work.consume(0)?;
        }
        bytes = bytes
            .checked_add(crate::unicode_simple::map(c, upper).len_utf8())
            .ok_or_else(budget)?;
    }
    if bytes > 1 << 20 {
        return Err(budget());
    }
    work.temporary(bytes)?;
    let mut output = String::with_capacity(bytes);
    for (i, c) in text.chars().enumerate() {
        if i % 4096 == 0 {
            work.consume(0)?;
        }
        output.push(crate::unicode_simple::map(c, upper));
    }
    Ok(output)
}
fn eval(
    t: &Term,
    row: &[Scalar],
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
) -> Result<Scalar> {
    let sources = BTreeMap::new();
    let context = subquery::Context::new(&sources, None);
    eval_scoped(t, row, columns, alias, params, &context, work)
}
fn eval_scoped(
    t: &Term,
    row: &[Scalar],
    columns: &[String],
    alias: ColumnOwners<'_>,
    params: &BTreeMap<String, Scalar>,
    context: &subquery::Context<'_>,
    work: &mut Work<'_>,
) -> Result<Scalar> {
    work.spend()?;
    let mut value = |t: &Term| eval_scoped(t, row, columns, alias, params, context, work);
    match t {
        // Only a rewritten grouped expression can evaluate an aggregate.
        // Go's raw scalar evaluator rejects aggregate names and star nodes.
        Term::Aggregate(..) | Term::Star(_) => Err(unsupported()),
        Term::Value(v) => work.clone_scalar(v),
        Term::Number(n) => literal(n),
        Term::Ordinal(index) => work.clone_scalar(row.get(*index).ok_or_else(binding)?),
        Term::Column(name) => match subquery::resolve(columns, alias, name, context.scope.outer)? {
            subquery::Position::Local(index) => {
                work.clone_scalar(row.get(index).ok_or_else(binding)?)
            }
            subquery::Position::Outer(frame, index) => work.clone_scalar(frame.value(index)?),
        },
        Term::Parameter(name) => work.clone_scalar(params.get(name).ok_or_else(binding)?),
        Term::NullTest(t, negated) => Ok(Scalar::Bool(matches!(value(t)?, Scalar::Null) ^ negated)),
        Term::Unary(op, t) => {
            let v = value(t)?;
            if v == Scalar::Null {
                return Ok(v);
            }
            match op {
                UnaryOperator::Plus if int(&v).is_some() => Ok(v),
                UnaryOperator::Minus if int(&v).is_some() => integer(-int(&v).unwrap()),
                UnaryOperator::Plus => Ok(Scalar::Float(number(&v)?)),
                UnaryOperator::Minus => Ok(Scalar::Float(-number(&v)?)),
                UnaryOperator::Not => {
                    Ok(truth(&v)?.map(|b| Scalar::Bool(!b)).unwrap_or(Scalar::Null))
                }
                _ => Err(unsupported()),
            }
        }
        Term::Binary(a, op, b) => {
            let a = value(a)?;
            if matches!(op, BinaryOperator::And | BinaryOperator::Or) {
                let b = value(b)?;
                let a = truth(&a)?;
                let b = truth(&b)?;
                return Ok(match op {
                    BinaryOperator::And if a == Some(false) || b == Some(false) => {
                        Scalar::Bool(false)
                    }
                    BinaryOperator::Or if a == Some(true) || b == Some(true) => Scalar::Bool(true),
                    _ => a
                        .zip(b)
                        .map(|(a, b)| {
                            Scalar::Bool(if op == &BinaryOperator::And {
                                a && b
                            } else {
                                a || b
                            })
                        })
                        .unwrap_or(Scalar::Null),
                });
            }
            let b = value(b)?;
            if matches!(a, Scalar::Null) || matches!(b, Scalar::Null) {
                return Ok(Scalar::Null);
            }
            match op {
                BinaryOperator::Eq
                | BinaryOperator::NotEq
                | BinaryOperator::Lt
                | BinaryOperator::LtEq
                | BinaryOperator::Gt
                | BinaryOperator::GtEq => {
                    let c = compare(&a, &b)?;
                    Ok(Scalar::Bool(match op {
                        BinaryOperator::Eq => c == Ordering::Equal,
                        BinaryOperator::NotEq => c != Ordering::Equal,
                        BinaryOperator::Lt => c == Ordering::Less,
                        BinaryOperator::LtEq => c != Ordering::Greater,
                        BinaryOperator::Gt => c == Ordering::Greater,
                        _ => c != Ordering::Less,
                    }))
                }
                BinaryOperator::StringConcat => {
                    let (Scalar::String(a), Scalar::String(b)) = (a, b) else {
                        return Err(type_error());
                    };
                    let bytes = a.len().checked_add(b.len()).ok_or_else(budget)?;
                    if bytes > 1 << 20 {
                        return Err(budget());
                    }
                    work.temporary(bytes)?;
                    let mut output = String::with_capacity(bytes);
                    output.push_str(&a);
                    output.push_str(&b);
                    Ok(Scalar::String(output))
                }
                _ => {
                    // BigRational operands/results are bounded to4096bits;
                    // reserve a conservative temporary representation up front.
                    if matches!(a, Scalar::Float(_))
                        || matches!(b, Scalar::Float(_))
                        || op == &BinaryOperator::Divide
                    {
                        work.temporary(8 << 10)?;
                    }
                    arithmetic(&a, op, &b)
                }
            }
        }
        Term::Between(t, low, high, negated) => {
            let (v, l, h) = (value(t)?, value(low)?, value(high)?);
            let low = if v == Scalar::Null || l == Scalar::Null {
                None
            } else {
                Some(compare(&v, &l)? != Ordering::Less)
            };
            let high = if v == Scalar::Null || h == Scalar::Null {
                None
            } else {
                Some(compare(&v, &h)? != Ordering::Greater)
            };
            Ok(if low == Some(false) || high == Some(false) {
                Scalar::Bool(*negated)
            } else if low == Some(true) && high == Some(true) {
                Scalar::Bool(!negated)
            } else {
                Scalar::Null
            })
        }
        Term::In(t, list, negated) => {
            let v = value(t)?;
            let mut null = false;
            let mut matched = false;
            for t in list {
                let other = value(t)?;
                if v == Scalar::Null || other == Scalar::Null {
                    null = true;
                } else if compare(&v, &other)? == Ordering::Equal {
                    matched = true;
                }
            }
            Ok(if matched {
                Scalar::Bool(!negated)
            } else if null {
                Scalar::Null
            } else {
                Scalar::Bool(*negated)
            })
        }
        Term::InSubquery(input, query, negated) => {
            let left = value(input)?;
            let result = subquery::execute(query, row, columns, alias, params, context, work)?;
            let mut null = false;
            let mut matched = false;
            // Materialize the complete RHS first, then compare every row.
            // A previous match does not hide a later incompatible scalar.
            for row in result.rows {
                work.spend()?;
                let other = row.first().ok_or_else(subquery::cardinality)?;
                if left == Scalar::Null || other == &Scalar::Null {
                    null = true;
                } else if compare(&left, other)? == Ordering::Equal {
                    matched = true;
                }
            }
            Ok(if matched {
                Scalar::Bool(!negated)
            } else if null {
                Scalar::Null
            } else {
                Scalar::Bool(*negated)
            })
        }
        Term::Cast(input, kind) => {
            let input = value(input)?;
            scalar::cast(input, kind, work)
        }
        Term::Like(input, pattern, negated) => {
            let input = value(input)?;
            let pattern = value(pattern)?;
            scalar::like(&input, &pattern, *negated, work)
        }
        Term::ScalarSubquery(query) => {
            let result = subquery::execute(query, row, columns, alias, params, context, work)?;
            if result.rows.len() > 1 {
                return Err(subquery::cardinality());
            }
            // The nested executor already owns and canonicalizes its scalar.
            // Move it out rather than cloning/recharging a returned string.
            Ok(result
                .rows
                .into_iter()
                .next()
                .and_then(|row| row.into_iter().next())
                .unwrap_or(Scalar::Null))
        }
        Term::Exists(query) => {
            // Go completes the query, even after the first row. Later provider,
            // type, cancellation and budget failures cannot be hidden by EXISTS.
            let result = subquery::execute_any(query, row, columns, alias, params, context, work)?;
            Ok(Scalar::Bool(!result.rows.is_empty()))
        }
        Term::Case(operand, cases, fallback) => {
            let operand = operand.as_ref().map(|v| value(v)).transpose()?;
            for (test, result) in cases {
                let test = value(test)?;
                let yes = if let Some(operand) = &operand {
                    operand != &Scalar::Null
                        && test != Scalar::Null
                        && compare(operand, &test)? == Ordering::Equal
                } else {
                    truth(&test)? == Some(true)
                };
                if yes {
                    return value(result);
                }
            }
            fallback
                .as_ref()
                .map(|v| value(v))
                .transpose()
                .map(|v| v.unwrap_or(Scalar::Null))
        }
        Term::Call(name, args, distinct) => {
            // Preserve the raw scalar evaluator's error independently of the
            // public binder, which rejects scalar DISTINCT with ErrType.
            if *distinct {
                return Err(unsupported());
            }
            check_arity(name, args.len())?;
            if name == "COALESCE" {
                if args.is_empty() {
                    return Err(type_error());
                }
                for arg in args {
                    let v = value(arg)?;
                    if v != Scalar::Null {
                        return Ok(v);
                    }
                }
                return Ok(Scalar::Null);
            }
            if matches!(name.as_str(), "HAS_FLAG" | "BIT_AND") {
                // Go validates each argument before evaluating the next. A
                // first NULL suppresses the second; a first bad type wins over
                // a later NULL, provider failure or numeric-range error.
                let mut bits = [0u64; 2];
                for (index, arg) in args.iter().enumerate() {
                    let scalar = value(arg)?;
                    if scalar == Scalar::Null {
                        return Ok(Scalar::Null);
                    }
                    bits[index] = int(&scalar).ok_or_else(type_error)? as u64;
                }
                return Ok(if name == "HAS_FLAG" {
                    Scalar::Bool(bits[0] & bits[1] == bits[1])
                } else {
                    Scalar::UInt(bits[0] & bits[1])
                });
            }
            let left = value(&args[0])?;
            if name == "NULLIF" {
                let right = value(&args[1])?;
                return Ok(
                    if left != Scalar::Null
                        && right != Scalar::Null
                        && compare(&left, &right)? == Ordering::Equal
                    {
                        Scalar::Null
                    } else {
                        left
                    },
                );
            }
            if left == Scalar::Null {
                return Ok(Scalar::Null);
            }
            let Scalar::String(text) = left else {
                return Err(type_error());
            };
            match name.as_str() {
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
    }
}
pub struct Source {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Scalar>>,
    pub complete: bool,
}
impl Source {
    pub fn from_wdc(
        table: &table::Table<'_>,
        limits: Limits,
        cancel: Option<&AtomicBool>,
    ) -> Result<Self> {
        Self::from_wdc_budgeted(table, limits, cancel, None)
    }
    pub fn from_wdc_budgeted(
        table: &table::Table<'_>,
        limits: Limits,
        cancel: Option<&AtomicBool>,
        shared: Option<&crate::resource::Budget>,
    ) -> Result<Self> {
        Self::from_wdc_mode(table, limits, cancel, shared, false)
    }
    pub(crate) fn schema_from_wdc_budgeted(
        table: &table::Table<'_>,
        limits: Limits,
        cancel: Option<&AtomicBool>,
        shared: Option<&crate::resource::Budget>,
    ) -> Result<Self> {
        Self::from_wdc_mode(table, limits, cancel, shared, true)
    }
    fn from_wdc_mode(
        table: &table::Table<'_>,
        limits: Limits,
        cancel: Option<&AtomicBool>,
        shared: Option<&crate::resource::Budget>,
        schema_only: bool,
    ) -> Result<Self> {
        let mut columns = vec![];
        for field in table.columns() {
            if field.array {
                for i in 0..field.elements {
                    columns.push(format!("{}[{i}]", field.name));
                }
            } else {
                columns.push(field.name.clone());
            }
            if columns.len() > 4096 {
                return Err(budget());
            }
        }
        let mut rows = vec![];
        let mut work = Work {
            left: limits.work,
            bytes: limits.memory_bytes,
            deadline: limits.deadline,
            cancel,
            shared,
            planning: false,
            temporaries: Vec::new(),
            selection_sources: Vec::new(),
            providers: &[],
            resolver: None,
            roots: BTreeMap::new(),
            measure: false,
            scans: Vec::new(),
        };
        work.consume(0)?;
        if schema_only {
            return Ok(Self {
                columns,
                rows,
                complete: table.coverage()["complete"] == true,
            });
        }
        for id in table.ids() {
            work.spend()?;
            let mut raw = table.row(id, 1 << 20)?;
            let mut row = vec![];
            let mut push = |value: table::Value| -> Result<()> {
                row.push(match value {
                    table::Value::Null => Scalar::Null,
                    table::Value::Int(v) => Scalar::Int(v),
                    table::Value::UInt(v) => Scalar::UInt(v),
                    table::Value::Float(v) if v.is_finite() => Scalar::Float(v),
                    table::Value::Float32(v) if v.is_finite() => Scalar::Float(v as f64),
                    table::Value::String(v) => Scalar::String(v),
                    _ => return Err(type_error()),
                });
                Ok(())
            };
            for field in table.columns() {
                let value = raw.remove(&field.name).ok_or_else(binding)?;
                if field.array {
                    let table::Value::Array(values) = value else {
                        return Err(type_error());
                    };
                    if values.len() != field.elements as usize {
                        return Err(binding());
                    }
                    for value in values {
                        push(value)?;
                    }
                } else {
                    push(value)?;
                }
            }
            work.retain(row_cost(&row)?)?;
            rows.push(row);
        }
        Ok(Self {
            columns,
            rows,
            complete: table.coverage()["complete"] == true,
        })
    }
}
fn row_cost(row: &[Scalar]) -> Result<usize> {
    let mut cost = row
        .len()
        .checked_mul(32)
        .and_then(|n| n.checked_add(24))
        .ok_or_else(budget)?;
    for v in row {
        match v {
            Scalar::Float(f) if !f.is_finite() => return Err(numeric()),
            Scalar::String(s) => {
                cost = cost.checked_add(s.len()).ok_or_else(budget)?;
            }
            _ => {}
        }
    }
    Ok(cost)
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectionMode {
    NamedSource,
    CompoundOrdinals,
}
#[derive(Clone, Debug, PartialEq)]
struct Projection {
    expr: Option<Term>,
    name: String,
}
type QuerySuffix = (Vec<(Term, bool, Option<bool>)>, Option<Term>, Option<Term>);
#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    explain: bool,
    analyze: bool,
    recursive: bool,
    bindings: Vec<(String, Program)>,
    branches: Vec<Program>,
    derived: Option<Box<Program>>,
    table: RelationName,
    alias: String,
    projection: Vec<Projection>,
    filter: Option<Term>,
    order: Vec<(Term, bool, Option<bool>)>,
    parameters: BTreeSet<String>,
    limit: Option<Term>,
    offset: Option<Term>,
    distinct: bool,
    joins: Vec<joins::Link>,
    groups: Vec<Term>,
    having: Option<Term>,
}
#[derive(Debug)]
pub struct QueryResult {
    pub plan: Option<Explanation>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Scalar>>,
    pub complete: bool,
}
impl Serialize for QueryResult {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut output =
            serializer.serialize_struct("Result", if self.plan.is_some() { 3 } else { 2 })?;
        output.serialize_field("columns", &self.plan.is_none().then_some(&self.columns))?;
        output.serialize_field("rows", &self.plan.is_none().then_some(&self.rows))?;
        if let Some(plan) = &self.plan {
            output.serialize_field("plan", plan)?;
        }
        output.end()
    }
}
impl QueryResult {
    pub(crate) fn public(&self) -> serde_json::Value {
        if let Some(plan) = &self.plan {
            serde_json::json!({"columns":null,"rows":null,"plan":plan})
        } else {
            serde_json::json!({"columns":self.columns,"rows":self.rows})
        }
    }
}
impl Program {
    fn terms(&self) -> impl Iterator<Item = &Term> {
        self.projection
            .iter()
            .filter_map(|projection| projection.expr.as_ref())
            .chain(self.joins.iter().filter_map(|join| join.condition.as_ref()))
            .chain(self.groups.iter())
            .chain(self.order.iter().map(|(term, _, _)| term))
            .chain(self.filter.iter())
            .chain(self.having.iter())
            .chain(self.limit.iter())
            .chain(self.offset.iter())
    }
    pub fn schema_only(&self) -> bool {
        self.explain
    }
    pub fn compile(sql: &str) -> Result<Self> {
        let mut tokens = lexical::tokens(sql)?;
        let (explain, analyze) = explain::prefix(&mut tokens);
        // sqlparser's implementation guards count more frames than Go's
        // retrieve/expression stack. Its independent ceiling stays bounded;
        // the Go syntax admission below uses the actual logical structure.
        let mut parser = Parser::new(&syntax::QueryDialect)
            .with_recursion_limit(256)
            .with_tokens_with_locations(tokens.clone());
        let query = match parser.parse_query() {
            Ok(query) => query,
            Err(error) => {
                compound::validate_all_keyword(
                    sql,
                    &tokens,
                    parser.get_current_index().saturating_add(1),
                )?;
                return Err(match error {
                    sqlparser::parser::ParserError::RecursionLimitExceeded => budget(),
                    error => problem("relational.invalid_syntax", &error.to_string()),
                });
            }
        };
        parser.consume_token(&sqlparser::tokenizer::Token::SemiColon);
        if parser.peek_token().token != sqlparser::tokenizer::Token::EOF {
            return Err(problem(
                "relational.invalid_syntax",
                "Expected end of retrieval",
            ));
        }
        // Check UNION's retained token after normal left-to-right parsing, so
        // a later missing ALL cannot conceal an earlier malformed expression.
        compound::validate_all_keyword(sql, &tokens, usize::MAX)?;
        parser_admission::validate(&query, &tokens)?;
        let mut program = Self::compile_query(*query, 0)?;
        subquery::validate_nodes(&program)?;
        program.explain = explain;
        program.analyze = analyze;
        Ok(program)
    }
    fn compile_query(mut query: Query, depth: usize) -> Result<Self> {
        if depth > 256 {
            return Err(budget());
        }
        let mut bindings = vec![];
        let mut recursive = false;
        if let Some(with) = query.with.take() {
            recursive = with.recursive;
            let mut names = BTreeSet::new();
            for cte in with.cte_tables {
                if cte.from.is_some() || cte.materialized.is_some() || !cte.alias.columns.is_empty()
                {
                    return Err(unsupported());
                }
                let name = syntax::identifier(&cte.alias.name)?.to_owned();
                if !names.insert(identifier_lower(&name)) {
                    return Err(problem("relational.invalid_syntax", "Duplicate CTE name"));
                }
                bindings.push((name, Self::compile_query(*cte.query, depth + 1)?));
            }
        }
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
            let bodies = compound::branches(*query.body.clone())?;
            let mut branches = Vec::with_capacity(bodies.len());
            for body in bodies {
                let mut branch = query.clone();
                branch.body = Box::new(body);
                branch.order_by = None;
                branch.limit_clause = None;
                branches.push(Self::compile_query(branch, depth)?);
            }
            let mut parameters = BTreeSet::new();
            for body in branches.iter().chain(bindings.iter().map(|(_, b)| b)) {
                parameters.extend(body.parameters.iter().cloned());
            }
            let (order, limit, offset) =
                Self::suffix(query.order_by, query.limit_clause, &mut parameters, depth)?;
            return Ok(Self {
                explain: false,
                analyze: false,
                recursive,
                bindings,
                branches,
                derived: None,
                table: RelationName::internal("\0union:output"),
                alias: String::new(),
                projection: vec![Projection {
                    expr: None,
                    name: String::new(),
                }],
                filter: None,
                order,
                parameters,
                limit,
                offset,
                distinct: false,
                joins: vec![],
                groups: vec![],
                having: None,
            });
        }
        let SetExpr::Select(select) = *query.body else {
            return Err(unsupported());
        };
        // Fixed Go 3.2 compiler requires FROM after every SELECT projection.
        // sqlparser accepts source-free SELECT, but that is a syntax error in
        // our public language rather than an unimplemented Rust expression.
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
        let GroupByExpr::Expressions(group_expressions, modifiers) = &select.group_by else {
            return Err(unsupported());
        };
        if !modifiers.is_empty() {
            return Err(unsupported());
        }
        let (table, alias, derived) =
            joins::relation(&select.from[0].relation, "\0derived:base", depth)?;
        let mut parameters = BTreeSet::new();
        let joins = select.from[0]
            .joins
            .iter()
            .enumerate()
            .map(|(i, j)| joins::compile(j, &mut parameters, i, depth))
            .collect::<Result<Vec<_>>>()?;
        let mut projection = vec![];
        for item in &select.projection {
            projection.push(match item {
                SelectItem::UnnamedExpr(e) => Projection {
                    expr: Some(term(e, &mut parameters, depth)?),
                    name: String::new(),
                },
                SelectItem::ExprWithAlias { expr, alias } => Projection {
                    expr: Some(term(expr, &mut parameters, depth)?),
                    name: syntax::identifier(alias)?.to_owned(),
                },
                SelectItem::Wildcard(options)
                    if options == &WildcardAdditionalOptions::default() =>
                {
                    Projection {
                        expr: None,
                        name: String::new(),
                    }
                }
                SelectItem::QualifiedWildcard(
                    SelectItemQualifiedWildcardKind::ObjectName(name),
                    options,
                ) if options == &WildcardAdditionalOptions::default() => Projection {
                    expr: None,
                    name: match name.0.as_slice() {
                        [ObjectNamePart::Identifier(name)] => syntax::identifier(name)?.to_owned(),
                        _ => return Err(unsupported()),
                    },
                },
                _ => return Err(unsupported()),
            });
        }
        if projection.is_empty() {
            return Err(budget());
        }
        let groups = group_expressions
            .iter()
            .map(|e| term(e, &mut parameters, depth))
            .collect::<Result<Vec<_>>>()?;
        let having = select
            .having
            .as_ref()
            .map(|e| term(e, &mut parameters, depth))
            .transpose()?;
        let filter = select
            .selection
            .as_ref()
            .map(|e| term(e, &mut parameters, depth))
            .transpose()?;
        let (order, limit, offset) =
            Self::suffix(query.order_by, query.limit_clause, &mut parameters, depth)?;
        let distinct = matches!(select.distinct, Some(Distinct::Distinct));
        if select.distinct.is_some() && !distinct {
            return Err(unsupported());
        }
        for body in bindings
            .iter()
            .map(|(_, b)| b)
            .chain(derived.as_deref())
            .chain(joins.iter().filter_map(|j| j.derived.as_deref()))
        {
            parameters.extend(body.parameters.iter().cloned());
        }
        Ok(Self {
            explain: false,
            analyze: false,
            recursive,
            bindings,
            branches: vec![],
            derived,
            table,
            alias,
            projection,
            filter,
            order,
            parameters,
            limit,
            offset,
            distinct,
            joins,
            groups,
            having,
        })
    }
    fn suffix(
        order_by: Option<OrderBy>,
        limit_clause: Option<LimitClause>,
        parameters: &mut BTreeSet<String>,
        depth: usize,
    ) -> Result<QuerySuffix> {
        let mut order = vec![];
        if let Some(order_by) = order_by {
            if order_by.interpolate.is_some() {
                return Err(unsupported());
            }
            let OrderByKind::Expressions(expressions) = order_by.kind else {
                return Err(unsupported());
            };
            for e in expressions {
                if e.with_fill.is_some() {
                    return Err(unsupported());
                }
                let descending = matches!(e.options.sort, Some(OrderBySort::Desc));
                if matches!(e.options.sort, Some(OrderBySort::Using(_))) {
                    return Err(unsupported());
                }
                order.push((
                    term(&e.expr, parameters, depth)?,
                    descending,
                    e.options.nulls_first,
                ));
            }
        }
        let (mut limit, mut offset) = (None, None);
        if let Some(clause) = limit_clause {
            let LimitClause::LimitOffset {
                limit: l,
                offset: o,
                limit_by,
            } = clause
            else {
                return Err(problem(
                    "relational.invalid_syntax",
                    "Expected LIMIT count then OFFSET count",
                ));
            };
            if !limit_by.is_empty() {
                return Err(problem(
                    "relational.invalid_syntax",
                    "Unexpected BY after LIMIT",
                ));
            }
            if let (Some(limit), Some(offset)) = (&l, &o) {
                let limit = limit.span().start;
                let offset = offset.value.span().start;
                if (offset.line, offset.column) < (limit.line, limit.column) {
                    return Err(problem(
                        "relational.invalid_syntax",
                        "LIMIT must precede OFFSET",
                    ));
                }
            }
            if o.as_ref()
                .is_some_and(|o| !matches!(o.rows, OffsetRows::None))
            {
                return Err(problem(
                    "relational.invalid_syntax",
                    "Unexpected ROW/ROWS after OFFSET",
                ));
            }
            let count = |expr: &Expr, parameters: &mut BTreeSet<String>| {
                // Test the original AST before term() erases parentheses.
                match expr {
                    Expr::Value(value) => match &value.value {
                        Value::Number(value, false)
                            if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => {}
                        Value::Placeholder(value) if value.starts_with(':') && value.len() > 1 => {}
                        _ => {
                            return Err(problem(
                                "relational.invalid_syntax",
                                "Expected integer count or named parameter",
                            ));
                        }
                    },
                    _ => {
                        return Err(problem(
                            "relational.invalid_syntax",
                            "Expected integer count or named parameter",
                        ));
                    }
                }
                term(expr, parameters, depth)
            };
            limit = l.as_ref().map(|e| count(e, parameters)).transpose()?;
            offset = o
                .as_ref()
                .map(|e| count(&e.value, parameters))
                .transpose()?;
        }
        Ok((order, limit, offset))
    }
    pub fn source_tables(&self) -> Vec<&str> {
        self.source_table_uses()
            .into_iter()
            .map(|table| table.key)
            .collect()
    }
    pub(crate) fn source_table_uses(&self) -> Vec<SourceTableUse<'_>> {
        let mut tables = vec![];
        compound::dependencies(self, &BTreeSet::new(), &mut tables);
        tables.sort_by_cached_key(|name| {
            (
                format!(
                    "{}.{}",
                    identifier_lower(name.catalog.as_deref().unwrap_or("static")),
                    identifier_lower(&name.name)
                ),
                name.physical_key(),
            )
        });
        tables.into_iter().map(RelationName::table_use).collect()
    }
    pub(crate) fn validate_parameters<T>(&self, parameters: &BTreeMap<String, T>) -> Result<()> {
        if self.parameters.len() != parameters.len()
            || self
                .parameters
                .iter()
                .any(|name| !parameters.contains_key(name))
        {
            return Err(binding());
        }
        Ok(())
    }
    pub fn execute(
        &self,
        sources: &BTreeMap<String, Source>,
        parameters: &BTreeMap<String, Scalar>,
        limits: Limits,
        cancel: Option<&AtomicBool>,
    ) -> Result<QueryResult> {
        self.execute_budgeted(sources, parameters, limits, cancel, None)
    }
    pub fn execute_budgeted(
        &self,
        sources: &BTreeMap<String, Source>,
        parameters: &BTreeMap<String, Scalar>,
        limits: Limits,
        cancel: Option<&AtomicBool>,
        shared: Option<&crate::resource::Budget>,
    ) -> Result<QueryResult> {
        self.execute_prepared_budgeted(
            sources,
            parameters,
            limits,
            cancel,
            shared,
            &BTreeMap::new(),
        )
    }
    /// Only the physical source preparer can grant an identity-column lookup.
    /// This preserves scan semantics for arbitrary public Source values and
    /// for materialized CTEs, derived tables, effective overlays and metadata.
    pub(crate) fn execute_prepared_budgeted<'a>(
        &self,
        sources: &'a BTreeMap<String, Source>,
        parameters: &BTreeMap<String, Scalar>,
        limits: Limits,
        cancel: Option<&'a AtomicBool>,
        shared: Option<&'a crate::resource::Budget>,
        equal_columns: &BTreeMap<String, String>,
    ) -> Result<QueryResult> {
        if let Some(shared) = shared {
            shared.check()?;
        }
        if cancel.is_some_and(|cancel| cancel.load(AtomicOrdering::Acquire)) {
            return Err(fault("operation.cancelled", 7, "SQL query cancelled"));
        }
        if limits.work == 0 || limits.memory_bytes == 0 || limits.memory_bytes > 1 << 30 {
            return Err(budget());
        }
        let resolver = provider::MapResolver::Owned(sources);
        let mut work = Work {
            left: limits.work,
            bytes: limits.memory_bytes,
            deadline: limits.deadline,
            cancel,
            shared,
            planning: true,
            temporaries: Vec::new(),
            selection_sources: Vec::new(),
            providers: &[],
            resolver: Some(&resolver),
            roots: BTreeMap::new(),
            measure: self.analyze,
            scans: Vec::new(),
        };
        work.consume(0)?;
        if self
            .parameters
            .iter()
            .any(|name| !parameters.contains_key(name))
        {
            return Err(binding());
        }
        for name in &self.parameters {
            row_cost(std::slice::from_ref(&parameters[name]))?;
        }
        work.safety_retain(
            equal_columns
                .len()
                .checked_mul(std::mem::size_of::<(&Source, usize)>())
                .ok_or_else(budget)?,
        )?;
        work.selection_sources = Vec::with_capacity(equal_columns.len());
        for (name, field) in equal_columns {
            let name = RelationName::from_key(name)?;
            let source = resolver.resolve(name.table_use())?;
            let index = column(
                &source.columns,
                ColumnOwners::Single(""),
                &ColumnRef::new(field.clone()),
            )?;
            work.selection_sources.push((source, index));
        }
        let inputs = BTreeMap::new();
        self.execute_and_explain(&inputs, parameters, limits, &mut work)
    }

    /// Query-scoped lazy roots. Owners remain in the caller; lexical computed
    /// Sources cannot inherit these capabilities by name or schema.
    pub(crate) fn execute_providers_budgeted<'a>(
        &self,
        sources: &BTreeMap<String, &'a Source>,
        providers: &'a [PreparedSource<'a>],
        parameters: &BTreeMap<String, Scalar>,
        limits: Limits,
        cancel: Option<&'a AtomicBool>,
        shared: Option<&'a crate::resource::Budget>,
    ) -> Result<QueryResult> {
        if let Some(shared) = shared {
            shared.check()?;
        }
        if cancel.is_some_and(|cancel| cancel.load(AtomicOrdering::Acquire)) {
            return Err(fault("operation.cancelled", 7, "SQL query cancelled"));
        }
        if limits.work == 0 || limits.memory_bytes == 0 || limits.memory_bytes > 1 << 30 {
            return Err(budget());
        }
        let resolver = provider::MapResolver::Borrowed(sources);
        let mut work = Work {
            left: limits.work,
            bytes: limits.memory_bytes,
            deadline: limits.deadline,
            cancel,
            shared,
            planning: true,
            temporaries: Vec::new(),
            selection_sources: Vec::new(),
            providers,
            resolver: Some(&resolver),
            roots: BTreeMap::new(),
            measure: self.analyze,
            scans: Vec::new(),
        };
        work.consume(0)?;
        if self
            .parameters
            .iter()
            .any(|name| !parameters.contains_key(name))
        {
            return Err(binding());
        }
        for name in &self.parameters {
            row_cost(std::slice::from_ref(&parameters[name]))?;
        }
        for (i, entry) in providers.iter().enumerate() {
            if !sources
                .values()
                .any(|source| std::ptr::eq(*source, entry.schema))
                || providers[..i]
                    .iter()
                    .any(|other| std::ptr::eq(other.schema, entry.schema))
            {
                return Err(binding());
            }
        }
        let inputs = BTreeMap::new();
        self.execute_and_explain(&inputs, parameters, limits, &mut work)
    }

    /// Resolve physical roots lazily in the actual schema traversal, sharing
    /// immutable schemas and the original executor allowance with execution.
    pub(crate) fn execute_resolved_budgeted<'a>(
        &self,
        resolver: &'a dyn RootResolver,
        parameters: &BTreeMap<String, Scalar>,
        limits: Limits,
        cancel: Option<&'a AtomicBool>,
        shared: Option<&'a crate::resource::Budget>,
    ) -> Result<QueryResult> {
        if let Some(shared) = shared {
            shared.check()?;
        }
        if cancel.is_some_and(|cancel| cancel.load(AtomicOrdering::Acquire)) {
            return Err(fault("operation.cancelled", 7, "SQL query cancelled"));
        }
        if limits.work == 0 || limits.memory_bytes == 0 || limits.memory_bytes > 1 << 30 {
            return Err(budget());
        }
        let mut work = Work {
            left: limits.work,
            bytes: limits.memory_bytes,
            deadline: limits.deadline,
            cancel,
            shared,
            planning: true,
            temporaries: Vec::new(),
            selection_sources: Vec::new(),
            providers: &[],
            resolver: Some(resolver),
            roots: BTreeMap::new(),
            measure: self.analyze,
            scans: Vec::new(),
        };
        work.consume(0)?;
        if self
            .parameters
            .iter()
            .any(|name| !parameters.contains_key(name))
        {
            return Err(binding());
        }
        for name in &self.parameters {
            row_cost(std::slice::from_ref(&parameters[name]))?;
        }
        let inputs = BTreeMap::new();
        self.execute_and_explain(&inputs, parameters, limits, &mut work)
    }

    fn execute_and_explain(
        &self,
        inputs: &BTreeMap<String, &Source>,
        parameters: &BTreeMap<String, Scalar>,
        limits: Limits,
        work: &mut Work<'_>,
    ) -> Result<QueryResult> {
        let bound = self.execute_scoped(inputs, parameters, work)?;
        work.planning = false;
        let mut plan = if self.explain {
            Some(explain::build(self, bound.columns, work)?)
        } else {
            None
        };
        if self.explain && !self.analyze {
            return Ok(QueryResult {
                plan,
                columns: vec![],
                rows: vec![],
                complete: bound.complete,
            });
        }
        let mut result = self.execute_scoped(inputs, parameters, work)?;
        result.complete &= bound.complete;
        if let Some(explanation) = &mut plan {
            explanation.measured = Some(work.measured(&limits, result.rows.len()));
            return Ok(QueryResult {
                plan,
                columns: vec![],
                rows: vec![],
                complete: result.complete,
            });
        }
        Ok(result)
    }
    fn execute_scoped(
        &self,
        sources: &BTreeMap<String, &Source>,
        parameters: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
    ) -> Result<QueryResult> {
        self.execute_scoped_with(sources, subquery::Scope::root(), parameters, work)
    }
    fn execute_scoped_with(
        &self,
        sources: &BTreeMap<String, &Source>,
        scope: subquery::Scope<'_>,
        parameters: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
    ) -> Result<QueryResult> {
        work.spend()?;
        compound::execute(self, sources, scope, parameters, work)
    }
    fn execute_select(
        &self,
        sources: &BTreeMap<String, &Source>,
        scope: subquery::Scope<'_>,
        parameters: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
    ) -> Result<QueryResult> {
        self.execute_select_mode(
            sources,
            scope,
            parameters,
            work,
            ProjectionMode::NamedSource,
        )
    }
    fn execute_select_mode(
        &self,
        sources: &BTreeMap<String, &Source>,
        scope: subquery::Scope<'_>,
        parameters: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
        projection_mode: ProjectionMode,
    ) -> Result<QueryResult> {
        let complete = std::cell::Cell::new(true);
        let mut context = subquery::Context {
            scope: if projection_mode == ProjectionMode::NamedSource {
                scope.select(sources)
            } else {
                scope
            },
            group: None,
            complete: Some(&complete),
        };
        // Acquire each input exactly once, in relation order. The local vector
        // owns derived results until the complete synchronous query returns.
        let prepared_inputs = if projection_mode == ProjectionMode::NamedSource {
            Some(joins::inputs(self, sources, scope, parameters, work)?)
        } else {
            None
        };
        let base_source = if let Some(inputs) = &prepared_inputs {
            inputs[0].source()
        } else {
            compound::source(sources, &self.table, work)?
        };
        let joined = if self.joins.is_empty() {
            None
        } else {
            Some(joins::prepare_scoped(
                prepared_inputs.as_deref().ok_or_else(binding)?,
                &self.alias,
                &self.joins,
                parameters,
                work,
                self,
                &context,
            )?)
        };
        let ordinal_owners: Vec<String> = if projection_mode == ProjectionMode::CompoundOrdinals {
            let count = base_source.columns.len();
            if let Some(shared) = work.shared {
                let bytes = count
                    .checked_mul(std::mem::size_of::<String>() + 20)
                    .ok_or_else(budget)?;
                shared
                    .safety_only()
                    .charge(crate::resource::Cost::retained(bytes))?;
            }
            (0..count).map(|index| index.to_string()).collect()
        } else {
            Vec::new()
        };
        let alias = if let Some(joined) = &joined {
            ColumnOwners::PerColumn(&joined.owners)
        } else if projection_mode == ProjectionMode::CompoundOrdinals {
            ColumnOwners::PerColumn(&ordinal_owners)
        } else {
            ColumnOwners::Single(&self.alias)
        };
        let source = joined.as_ref().map(|p| &p.source).unwrap_or(base_source);
        if projection_mode == ProjectionMode::NamedSource {
            context.scope.fields = subquery::Fields {
                columns: &source.columns,
                alias,
            };
        }
        let mut ordering_terms = self.order.clone();
        let mut columns = vec![];
        let mut outputs = vec![];
        for p in &self.projection {
            if let Some(t) = &p.expr
                && !matches!(t, Term::Star(_))
            {
                bind(t, &source.columns, alias, parameters, &context, work)?;
                outputs.push(t.clone());
                columns.push(if p.name.is_empty() {
                    if let Term::Column(name) = t {
                        subquery::display_column(&source.columns, alias, name, context.scope.outer)?
                            .unwrap_or_else(|| format!("column_{}", columns.len() + 1))
                    } else {
                        format!("column_{}", columns.len() + 1)
                    }
                } else {
                    p.name.clone()
                });
            } else {
                let requested = if let Some(Term::Star(qualifier)) = &p.expr {
                    if !p.name.is_empty() {
                        return Err(binding());
                    }
                    qualifier
                } else {
                    &p.name
                };
                let mut selected = 0usize;
                for (ordinal, column) in source.columns.iter().enumerate() {
                    if !requested.is_empty() {
                        let qualifier = alias.owner(ordinal);
                        if !equal_fold(qualifier, requested) {
                            continue;
                        }
                    }
                    selected += 1;
                    outputs.push(if projection_mode == ProjectionMode::CompoundOrdinals {
                        // Go's synthetic UNION projection is a bound column,
                        // not a user wildcard, and pays one bind node.
                        work.spend()?;
                        Term::Ordinal(ordinal)
                    } else {
                        Term::Column(ColumnRef::qualified(alias.owner(ordinal), column.clone()))
                    });
                    columns.push(column.clone());
                }
                if selected == 0 {
                    return Err(binding());
                }
            }
        }
        if let Some(t) = &self.filter {
            bind_with_aggregates(t, &source.columns, alias, parameters, false, &context, work)?;
        }
        if let Some(t) = &self.having {
            bind(t, &source.columns, alias, parameters, &context, work)?;
        }
        for key in &self.groups {
            bind_with_aggregates(
                key,
                &source.columns,
                alias,
                parameters,
                false,
                &context,
                work,
            )?;
        }
        for (t, _, _) in &mut ordering_terms {
            // Go resolves a bare ORDER BY output name before binding against
            // source fields. Quoted dots remain part of that single name.
            let output = if let Term::Column(name) = &*t
                && name.qualifier.is_empty()
            {
                let mut matches = columns
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| equal_fold(candidate, &name.name));
                let found = matches.next().map(|(index, _)| index);
                if matches.next().is_some() {
                    return Err(binding());
                }
                found
            } else {
                None
            };
            if let Some(index) = output {
                *t = outputs[index].clone();
            }
            bind(t, &source.columns, alias, parameters, &context, work)?;
        }
        let count = |term: &Option<Term>, default: usize, work: &mut Work<'_>| -> Result<usize> {
            let Some(t) = term else {
                return Ok(default);
            };
            work.temporary_scope(|work| {
                let v = eval_scoped(
                    t,
                    &[],
                    &[],
                    ColumnOwners::Single(""),
                    parameters,
                    &context,
                    work,
                )?;
                let n = int(&v).filter(|n| *n >= 0).ok_or_else(type_error)?;
                usize::try_from(n).map_err(|_| budget())
            })
        };
        count(&self.limit, usize::MAX, work)?;
        count(&self.offset, 0, work)?;
        let is_grouped = !self.groups.is_empty()
            || self.having.is_some()
            || outputs.iter().any(aggregate::contains)
            || ordering_terms
                .iter()
                .any(|(term, _, _)| aggregate::contains(term));
        let group_mapping = if is_grouped {
            Some(subquery::group_mapping(
                &self.groups,
                &source.columns,
                alias,
                work,
            )?)
        } else {
            None
        };
        let group_context = subquery::Context {
            group: group_mapping
                .as_deref()
                .map(|mapping| subquery::Correlation {
                    columns: &source.columns,
                    alias,
                    mapping,
                }),
            ..context
        };
        let mut grouping = if is_grouped {
            Some(aggregate::Grouping::new(
                &self.groups,
                &mut outputs,
                &mut ordering_terms,
                self.having.as_ref(),
                &columns,
                parameters,
                &group_context,
                work,
            )?)
        } else {
            None
        };
        if work.planning {
            return Ok(QueryResult {
                plan: None,
                columns,
                rows: vec![],
                complete: source.complete && complete.get(),
            });
        }
        let selected = if joined.is_none()
            && provider::find(source, work).is_some()
            && provider::can_project(self, &source.columns, alias)
        {
            Some(provider::selected(self, &source.columns, alias, work)?)
        } else {
            None
        };
        let mut bounded =
            if !is_grouped && !self.distinct && self.limit.is_some() && self.order.len() <= 1 {
                let limit = count(&self.limit, 0, work)?;
                let offset = count(&self.offset, 0, work)?;
                topk::TopRows::new(self, false, limit, offset)
            } else {
                None
            };
        // Rust owns scalar vectors and copied strings; these temporary owners
        // do not add Go logical result-retention or work charges.
        work.safety_retain(
            (outputs.len() + ordering_terms.len())
                .checked_mul(std::mem::size_of::<Scalar>())
                .ok_or_else(budget)?,
        )?;
        let group_columns = if let Some(grouping) = &grouping {
            work.safety_retain(
                grouping
                    .columns()
                    .iter()
                    .map(|name| name.len() + std::mem::size_of::<String>())
                    .sum(),
            )?;
            Some(grouping.columns().to_vec())
        } else {
            None
        };
        let output_columns = group_columns.as_deref().unwrap_or(&source.columns);
        let output_alias = if is_grouped {
            ColumnOwners::Single("")
        } else {
            alias
        };
        let output_context = if is_grouped { &group_context } else { &context };
        let mut result: Vec<results::Output> = vec![];
        let mut project_row = |row: &[Scalar], work: &mut Work<'_>| -> Result<()> {
            let projected = outputs
                .iter()
                .map(|term| {
                    eval_scoped(
                        term,
                        row,
                        output_columns,
                        output_alias,
                        parameters,
                        output_context,
                        work,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let ordering = ordering_terms
                .iter()
                .map(|(term, _, _)| {
                    eval_scoped(
                        term,
                        row,
                        output_columns,
                        output_alias,
                        parameters,
                        output_context,
                        work,
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            let output = (projected, ordering);
            if let Some(bounded) = &mut bounded {
                bounded.add(output, work)?;
            } else {
                work.retain(results::output_cost(&output, 64)?)?;
                result.push(output);
            }
            Ok(())
        };
        {
            let mut consume = |row: &[Scalar], work: &mut Work<'_>| -> Result<()> {
                work.temporary_scope(|work| {
                    work.spend()?;
                    if row.len() != source.columns.len() {
                        return Err(binding());
                    }
                    if let Some(term) = &self.filter
                        && truth(&eval_scoped(
                            term,
                            row,
                            &source.columns,
                            alias,
                            parameters,
                            &context,
                            work,
                        )?)? != Some(true)
                    {
                        return Ok(());
                    }
                    if let Some(grouping) = &mut grouping {
                        grouping.add(row, &source.columns, alias, parameters, &context, work)
                    } else {
                        project_row(row, work)
                    }
                })
            };
            if let Some(plan) = &joined {
                plan.visit(parameters, &context, work, &mut consume)?;
            } else {
                selection::visit(
                    source,
                    selected.as_deref(),
                    self.filter.as_ref(),
                    alias,
                    parameters,
                    work,
                    &mut |row, work| {
                        if row.len() != source.columns.len() {
                            return Err(binding());
                        }
                        // executeSelect always visits joinRow(0), even without
                        // joins. A compound tail invokes executeRows directly.
                        if projection_mode == ProjectionMode::NamedSource {
                            work.spend()?;
                        }
                        consume(row, work)
                    },
                )?;
            }
        }
        if let Some(grouping) = grouping {
            grouping.finish(parameters, &group_context, work, &mut project_row)?;
        }
        if let Some(bounded) = bounded {
            result = bounded.finish(work)?;
        }
        // Go evaluates pagination again after the complete input, then
        // finishes DISTINCT/order/slice. Do not reuse an earlier result or
        // move this expression evaluation ahead of a provider's late failure.
        let limit = count(&self.limit, usize::MAX, work)?;
        let offset = count(&self.offset, 0, work)?;
        Ok(QueryResult {
            plan: None,
            columns,
            rows: results::finish(result, self.distinct, &ordering_terms, limit, offset, work)?,
            complete: source.complete && complete.get(),
        })
    }
}

#[cfg(test)]
#[path = "relational/identifier_tests.rs"]
mod identifier_tests;

#[cfg(test)]
#[path = "relational/resolver_tests.rs"]
mod resolver_tests;

#[cfg(test)]
#[path = "relational/execution_contract_tests.rs"]
mod execution_contract_tests;

#[cfg(test)]
#[path = "relational/function_binding_tests.rs"]
mod function_binding_tests;

#[cfg(test)]
mod identifier_grammar_tests;
