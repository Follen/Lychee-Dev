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
mod ast_owner;
mod binding;
mod compound;
mod diagnostic;
mod evaluation;
mod execution;
mod execution_query;
mod execution_scan;
mod execution_select;
mod explain;
mod expression_parser;
pub use explain::{ExecutionMeasurement, Explanation, MeasuredTable, PlanStep, ScanMeasurement};
#[cfg(test)]
mod compiler_trace;
mod identifier_categories;
mod identifiers;
mod lexical;
mod lowering;
mod parser_admission;
mod query_lowering;
mod retrieval;
pub(crate) use identifiers::SourceTableUse;
use identifiers::{ColumnOwners, ColumnRef, RelationName};
mod joins;
mod provider;
pub(crate) use provider::{
    EqualityCursor, EqualityOutcome, PreparedSource, RootResolver, RowCursor, RowProvider, RowYield,
};
mod recursive;
mod results;
mod scalar;
pub(crate) use scalar::text as scalar_text;
mod selection;
mod subquery;
mod syntax;
mod task_stack;
mod term_owner;
mod term_tree;
mod topk;
mod tree_debug;
mod tree_equality;
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
use lowering::term;
// Keep the large public Expr move inside a nonrecursive helper. The shared
// lowering driver stores only owned handles across query continuations.
#[inline(never)]
fn take_query_expression(
    expr: &mut Expr,
) -> (
    ast_owner::Owned<Query>,
    Option<(ast_owner::Owned<Expr>, bool)>,
    Option<bool>,
) {
    match std::mem::replace(expr, Expr::Value(Value::Null.with_empty_span())) {
        Expr::InSubquery {
            expr,
            subquery,
            negated,
        } => (
            ast_owner::Owned::new(subquery),
            Some((ast_owner::Owned::new(expr), negated)),
            None,
        ),
        Expr::Subquery(query) => (ast_owner::Owned::new(query), None, None),
        Expr::Exists { subquery, negated } => {
            (ast_owner::Owned::new(subquery), None, Some(negated))
        }
        _ => unreachable!("query_term only receives query expressions"),
    }
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
#[cfg(test)]
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
#[cfg(test)]
use binding::bind as bind_with_aggregates;

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
    evaluation::constant(t, row, columns, alias, params, &context, work)
}
#[cfg(test)]
use evaluation::eval as eval_scoped;

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
// Borrow the boxed SELECT without moving the public SetExpr enum's large
// unboxed Statement storage into the query continuation driver.
#[inline(never)]
fn select_body(body: &mut SetExpr) -> Result<&mut Select> {
    match body {
        SetExpr::Select(select) => Ok(select),
        _ => Err(unsupported()),
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
        Self::compile_with(sql, &syntax::QueryDialect::default())
    }
    pub(crate) fn compile_controlled(
        sql: &str,
        control: &lycheedev_core::control::OperationControl,
        budget: &crate::resource::Budget,
    ) -> Result<Self> {
        Self::compile_with(sql, &syntax::QueryDialect::controlled(control, budget))
    }
    fn compile_with(sql: &str, dialect: &syntax::QueryDialect) -> Result<Self> {
        let check = &|| dialect.check();
        let mut tokens = lexical::tokens_checked(sql, check)?;
        let (explain, analyze) = explain::prefix(&mut tokens);
        // Keep the public parser's compatibility ceiling. Production retrieval
        // and expressions use their shared heap driver, not native Pratt/query
        // recursion; original active-frame and logical-node limits stay 128.
        let mut parser = Parser::new(dialect)
            .with_recursion_limit(256)
            .with_tokens_with_locations(tokens.clone());
        check()?;
        #[cfg(test)]
        compiler_trace::stage("parse");
        let parsed = retrieval::parse(dialect, &mut parser).map(ast_owner::Owned::new);
        // A dialect hook may stop a speculative parse that sqlparser otherwise
        // rewinds. It must never become a syntax error or silently disappear.
        dialect.take_stop()?;
        let mut query = match parsed {
            Ok(query) => query,
            Err(error) => {
                let diagnostic = dialect.diagnostic.borrow_mut().take();
                // A failing parse_identifier may have consumed its bad token.
                // Only UNION tokens strictly before the typed error location
                // can supersede it; a bad identifier spelled UNION cannot.
                let consumed = diagnostic.as_ref().map_or_else(
                    || parser.get_current_index().saturating_add(1),
                    |diagnostic| {
                        tokens.partition_point(|token| {
                            (token.span.start.line, token.span.start.column)
                                < (diagnostic.location.line, diagnostic.location.column)
                        })
                    },
                );
                compound::validate_all_keyword(sql, &tokens, consumed)?;
                return Err(match error {
                    sqlparser::parser::ParserError::RecursionLimitExceeded => budget(),
                    error => match diagnostic {
                        Some(diagnostic) => {
                            diagnostic::at(sql, diagnostic.location, &diagnostic.detail)
                        }
                        None => problem("relational.invalid_syntax", &error.to_string()),
                    },
                });
            }
        };
        parser.consume_token(&sqlparser::tokenizer::Token::SemiColon);
        if parser.peek_token().token != sqlparser::tokenizer::Token::EOF {
            return Err(diagnostic::at(
                sql,
                parser.peek_token().span.start,
                "expected end of statement",
            ));
        }
        // Check UNION's retained token after normal left-to-right parsing, so
        // a later missing ALL cannot conceal an earlier malformed expression.
        compound::validate_all_keyword(sql, &tokens, usize::MAX)?;
        #[cfg(test)]
        compiler_trace::stage("admission");
        parser_admission::validate(&query, &tokens, check)?;
        #[cfg(test)]
        compiler_trace::stage("lowering");
        let mut program = Self::compile_query(&mut query, 0, check)?;
        #[cfg(test)]
        compiler_trace::stage("inspection");
        subquery::validate_nodes(&program, check)?;
        #[cfg(test)]
        compiler_trace::stage("complete");
        program.explain = explain;
        program.analyze = analyze;
        Ok(program)
    }
    fn compile_query(
        query: &mut Query,
        depth: usize,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<Self> {
        lowering::query(query, depth, check)
    }
    fn suffix(
        order_by: &mut Option<OrderBy>,
        limit_clause: &mut Option<LimitClause>,
        parameters: &mut BTreeSet<String>,
        depth: usize,
        check: &dyn Fn() -> Result<()>,
    ) -> Result<QuerySuffix> {
        check()?;
        let mut order = vec![];
        if let Some(order_by) = order_by {
            if order_by.interpolate.is_some() {
                return Err(unsupported());
            }
            let OrderByKind::Expressions(expressions) = &mut order_by.kind else {
                return Err(unsupported());
            };
            for e in expressions {
                check()?;
                if e.with_fill.is_some() {
                    return Err(unsupported());
                }
                let descending = matches!(e.options.sort, Some(OrderBySort::Desc));
                if matches!(e.options.sort, Some(OrderBySort::Using(_))) {
                    return Err(unsupported());
                }
                order.push((
                    term(&mut e.expr, parameters, depth, check)?,
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
            let count = |expr: &mut Expr, parameters: &mut BTreeSet<String>| {
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
                term(expr, parameters, depth, check)
            };
            limit = l.as_mut().map(|e| count(e, parameters)).transpose()?;
            offset = o
                .as_mut()
                .map(|e| count(&mut e.value, parameters))
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
        execution::execute(self, sources, scope, parameters, work)
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

#[cfg(test)]
mod compiler_control_tests;

#[cfg(test)]
mod retrieval_order_tests;

#[cfg(test)]
mod expression_parser_tests;

#[cfg(test)]
mod stack_safety_tests;
#[cfg(all(test, feature = "datafusion-qualification"))]
pub(crate) mod engine_compat;
