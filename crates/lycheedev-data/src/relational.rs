//! Read-only typed SQL evaluation. Integer lexemes/providers retain all 64 bits;
//! parsing does not authorize writes, implicit casts or external I/O.
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
mod joins;
mod recursive;
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
    Column(String),
    Parameter(String),
    Unary(UnaryOperator, Box<Term>),
    Binary(Box<Term>, BinaryOperator, Box<Term>),
    NullTest(Box<Term>, bool),
    Between(Box<Term>, Box<Term>, Box<Term>, bool),
    In(Box<Term>, Vec<Term>, bool),
    Call(String, Vec<Term>),
    Aggregate(String, Option<Box<Term>>, bool),
    Case(Option<Box<Term>>, Vec<(Term, Term)>, Option<Box<Term>>),
}
fn term(expr: &Expr, parameters: &mut BTreeSet<String>, depth: usize) -> Result<Term> {
    if depth > 128 {
        return Err(budget());
    }
    let mut nested = |e: &Expr| term(e, parameters, depth + 1);
    Ok(match expr {
        Expr::Identifier(name) => Term::Column(name.value.clone()),
        Expr::CompoundIdentifier(names) if names.len() == 2 => Term::Column(
            names
                .iter()
                .map(|n| n.value.clone())
                .collect::<Vec<_>>()
                .join("."),
        ),
        Expr::Value(v) => match &v.value {
            Value::Null => Term::Value(Scalar::Null),
            Value::Boolean(v) => Term::Value(Scalar::Bool(*v)),
            Value::Number(n, _) => Term::Value(literal(n)?),
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
            let name = function.name.to_string().to_ascii_uppercase();
            if ["COUNT", "MIN", "MAX", "SUM", "AVG"].contains(&name.as_str()) {
                if args.args.len() != 1 {
                    return Err(type_error());
                }
                let distinct =
                    matches!(args.duplicate_treatment, Some(DuplicateTreatment::Distinct));
                let input = match &args.args[0] {
                    FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) => {
                        Some(Box::new(nested(expr)?))
                    }
                    FunctionArg::Unnamed(FunctionArgExpr::Wildcard)
                        if name == "COUNT" && !distinct =>
                    {
                        None
                    }
                    _ => return Err(type_error()),
                };
                return Ok(Term::Aggregate(name, input, distinct));
            }
            if args.duplicate_treatment.is_some() {
                return Err(unsupported());
            }
            if ![
                "LOWER", "UPPER", "LENGTH", "COALESCE", "NULLIF", "HAS_FLAG", "BIT_AND",
            ]
            .contains(&name.as_str())
            {
                return Err(unsupported());
            }
            let mut terms = vec![];
            for arg in &args.args {
                let FunctionArg::Unnamed(FunctionArgExpr::Expr(expr)) = arg else {
                    return Err(unsupported());
                };
                terms.push(nested(expr)?);
            }
            Term::Call(name, terms)
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
            else_result
                .as_ref()
                .map(|e| nested(e).map(Box::new))
                .transpose()?,
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
    alias: &str,
    params: &BTreeMap<String, Scalar>,
) -> Result<()> {
    match term {
        Term::Column(name) => {
            column(columns, alias, name)?;
        }
        Term::Parameter(name) if !params.contains_key(name) => return Err(binding()),
        Term::Unary(_, t) | Term::NullTest(t, _) => bind(t, columns, alias, params)?,
        Term::Binary(a, _, b) => {
            bind(a, columns, alias, params)?;
            bind(b, columns, alias, params)?;
        }
        Term::Between(a, b, c, _) => {
            for t in [a, b, c] {
                bind(t, columns, alias, params)?;
            }
        }
        Term::In(t, list, _) => {
            bind(t, columns, alias, params)?;
            for t in list {
                bind(t, columns, alias, params)?;
            }
        }
        Term::Aggregate(_, Some(input), _) => {
            if aggregate::contains(input) {
                return Err(type_error());
            }
            bind(input, columns, alias, params)?;
        }
        Term::Call(name, list) => {
            check_arity(name, list.len())?;
            for t in list {
                bind(t, columns, alias, params)?;
            }
        }
        Term::Case(operand, cases, fallback) => {
            if let Some(t) = operand {
                bind(t, columns, alias, params)?;
            }
            for (a, b) in cases {
                bind(a, columns, alias, params)?;
                bind(b, columns, alias, params)?;
            }
            if let Some(t) = fallback {
                bind(t, columns, alias, params)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn column(columns: &[String], alias: &str, name: &str) -> Result<usize> {
    if alias == joins::QUALIFIED {
        let mut found = columns.iter().enumerate().filter(|(_, c)| {
            if name.contains('.') {
                c.eq_ignore_ascii_case(name)
            } else {
                c.rsplit('.').next().unwrap().eq_ignore_ascii_case(name)
            }
        });
        let index = found
            .next()
            .map(|(i, _)| i)
            .ok_or_else(|| missing_column(columns, alias, name))?;
        if found.next().is_some() {
            return Err(binding());
        }
        return Ok(index);
    }
    let original = name;
    let name = if let Some((qualifier, name)) = name.split_once('.') {
        if !qualifier.eq_ignore_ascii_case(alias) {
            return Err(missing_column(columns, alias, original));
        }
        name
    } else {
        name
    };
    let mut indices = columns
        .iter()
        .enumerate()
        .filter(|(_, c)| c.eq_ignore_ascii_case(name));
    let Some((index, _)) = indices.next() else {
        return Err(missing_column(columns, alias, original));
    };
    if indices.next().is_some() {
        return Err(binding());
    }
    Ok(index)
}
fn missing_column(columns: &[String], alias: &str, name: &str) -> lycheedev_core::ToolkitError {
    let (qualifier, field) = name.split_once('.').unwrap_or(("", name));
    let mut candidates = Vec::<(usize, String)>::new();
    for column in columns {
        let (owner, candidate) = if alias == joins::QUALIFIED {
            column.rsplit_once('.').unwrap_or(("", column))
        } else {
            (alias, column.as_str())
        };
        if !qualifier.is_empty() && !qualifier.eq_ignore_ascii_case(owner) {
            continue;
        }
        let distance = column_distance(field, candidate);
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
    let mut error = binding();
    error.message = format!(
        "relational.unresolved_binding: column {name}; candidates: {}",
        candidates.join(", ")
    );
    error.details = serde_json::json!({"column":name,"candidates":candidates});
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
}
impl Work<'_> {
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
        if let Scalar::String(text) = value {
            if text.len() > 1 << 20 {
                return Err(budget());
            }
            self.retain(text.len())?;
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
            work.spend()?;
        }
        bytes = bytes
            .checked_add(crate::unicode_simple::map(c, upper).len_utf8())
            .ok_or_else(budget)?;
    }
    if bytes > 1 << 20 {
        return Err(budget());
    }
    work.retain(bytes)?;
    let mut output = String::with_capacity(bytes);
    for (i, c) in text.chars().enumerate() {
        if i % 4096 == 0 {
            work.spend()?;
        }
        output.push(crate::unicode_simple::map(c, upper));
    }
    Ok(output)
}
fn eval(
    t: &Term,
    row: &[Scalar],
    columns: &[String],
    alias: &str,
    params: &BTreeMap<String, Scalar>,
    work: &mut Work<'_>,
) -> Result<Scalar> {
    work.spend()?;
    let mut value = |t: &Term| eval(t, row, columns, alias, params, work);
    match t {
        Term::Aggregate(_, _, _) => Err(binding()),
        Term::Value(v) => work.clone_scalar(v),
        Term::Column(name) => {
            work.clone_scalar(row.get(column(columns, alias, name)?).ok_or_else(binding)?)
        }
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
                let a = truth(&a)?;
                let b = truth(&value(b)?)?;
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
                    work.retain(bytes)?;
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
                        work.retain(8 << 10)?;
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
        Term::Call(name, args) => {
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
            work.retain(
                args.len()
                    .checked_mul(std::mem::size_of::<Scalar>())
                    .ok_or_else(budget)?,
            )?;
            let values = args
                .iter()
                .map(|arg| eval(arg, row, columns, alias, params, work))
                .collect::<Result<Vec<_>>>()?;
            if name == "NULLIF" && values.len() == 2 {
                return Ok(
                    if values[0] != Scalar::Null
                        && values[1] != Scalar::Null
                        && compare(&values[0], &values[1])? == Ordering::Equal
                    {
                        Scalar::Null
                    } else {
                        work.clone_scalar(&values[0])?
                    },
                );
            }
            if values.contains(&Scalar::Null) {
                return Ok(Scalar::Null);
            }
            match (name.as_str(), values.as_slice()) {
                ("LOWER", [Scalar::String(v)]) => Ok(Scalar::String(case_text(v, false, work)?)),
                ("UPPER", [Scalar::String(v)]) => Ok(Scalar::String(case_text(v, true, work)?)),
                ("LENGTH", [Scalar::String(v)]) => {
                    work.consume(v.len())?;
                    Ok(Scalar::Int(v.chars().count() as i64))
                }
                ("HAS_FLAG" | "BIT_AND", [a, b]) if int(a).is_some() && int(b).is_some() => {
                    let (a, b) = (int(a).unwrap() as u64, int(b).unwrap() as u64);
                    Ok(if name == "HAS_FLAG" {
                        Scalar::Bool(a & b == b)
                    } else {
                        Scalar::UInt(a & b)
                    })
                }
                _ => Err(type_error()),
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
        };
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
                if s.len() > 1 << 20 {
                    return Err(budget());
                }
                cost = cost.checked_add(s.len()).ok_or_else(budget)?;
            }
            _ => {}
        }
    }
    Ok(cost)
}
struct Projection {
    expr: Option<Term>,
    name: String,
}
type QuerySuffix = (Vec<(Term, bool, Option<bool>)>, Option<Term>, Option<Term>);
pub struct Program {
    recursive: bool,
    bindings: Vec<(String, Program)>,
    branches: Vec<Program>,
    derived: Option<Box<Program>>,
    table: String,
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
#[derive(Debug, Serialize)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Scalar>>,
    pub complete: bool,
}
impl Program {
    pub fn compile(sql: &str) -> Result<Self> {
        if sql.is_empty() || sql.len() > 1 << 20 {
            return Err(budget());
        }
        compound::validate_all_keyword(sql)?;
        let mut parser = Parser::new(&GenericDialect {})
            .with_recursion_limit(128)
            .try_with_sql(sql)
            .map_err(|e| problem("relational.invalid_syntax", &e.to_string()))?;
        let mut statements = parser
            .parse_statements()
            .map_err(|e| problem("relational.invalid_syntax", &e.to_string()))?;
        if statements.len() != 1 {
            return Err(problem(
                "relational.invalid_syntax",
                "Exactly one read-only retrieval required",
            ));
        }
        let Statement::Query(query) = statements.remove(0) else {
            return Err(problem(
                "relational.invalid_syntax",
                "Only SELECT retrieval is permitted",
            ));
        };
        Self::compile_query(*query, 0)
    }
    fn compile_query(mut query: Query, depth: usize) -> Result<Self> {
        if depth > 64 {
            return Err(budget());
        }
        let mut bindings = vec![];
        let mut recursive = false;
        if let Some(with) = query.with.take() {
            recursive = with.recursive;
            if with.cte_tables.len() > 128 {
                return Err(budget());
            }
            let mut names = BTreeSet::new();
            for cte in with.cte_tables {
                if cte.from.is_some() || cte.materialized.is_some() || !cte.alias.columns.is_empty()
                {
                    return Err(unsupported());
                }
                let name = cte.alias.name.value;
                if name.is_empty() || name.len() > 512 {
                    return Err(binding());
                }
                if !names.insert(name.to_ascii_lowercase()) {
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
                branches.push(Self::compile_query(branch, depth + 1)?);
            }
            let mut parameters = BTreeSet::new();
            for body in branches.iter().chain(bindings.iter().map(|(_, b)| b)) {
                parameters.extend(body.parameters.iter().cloned());
            }
            let (order, limit, offset) =
                Self::suffix(query.order_by, query.limit_clause, &mut parameters)?;
            return Ok(Self {
                recursive,
                bindings,
                branches,
                derived: None,
                table: "\0union:output".into(),
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
        if !modifiers.is_empty() || group_expressions.len() > 1024 {
            return Err(unsupported());
        }
        let (table, alias, derived) =
            joins::relation(&select.from[0].relation, "\0derived:base", depth)?;
        let mut parameters = BTreeSet::new();
        if select.from[0].joins.len() > 31 {
            return Err(budget());
        }
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
                    expr: Some(term(e, &mut parameters, 0)?),
                    name: String::new(),
                },
                SelectItem::ExprWithAlias { expr, alias } => Projection {
                    expr: Some(term(expr, &mut parameters, 0)?),
                    name: alias.value.clone(),
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
                    name: name.to_string(),
                },
                _ => return Err(unsupported()),
            });
        }
        if projection.is_empty() || projection.len() > 1024 {
            return Err(budget());
        }
        let groups = group_expressions
            .iter()
            .map(|e| term(e, &mut parameters, 0))
            .collect::<Result<Vec<_>>>()?;
        let having = select
            .having
            .as_ref()
            .map(|e| term(e, &mut parameters, 0))
            .transpose()?;
        let filter = select
            .selection
            .as_ref()
            .map(|e| term(e, &mut parameters, 0))
            .transpose()?;
        let (order, limit, offset) =
            Self::suffix(query.order_by, query.limit_clause, &mut parameters)?;
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
                    term(&e.expr, parameters, 0)?,
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
                return Err(unsupported());
            };
            if !limit_by.is_empty() {
                return Err(unsupported());
            }
            limit = l.as_ref().map(|e| term(e, parameters, 0)).transpose()?;
            offset = o
                .as_ref()
                .map(|e| term(&e.value, parameters, 0))
                .transpose()?;
        }
        Ok((order, limit, offset))
    }
    pub fn source_tables(&self) -> Vec<&str> {
        let mut tables = vec![];
        compound::dependencies(self, &BTreeSet::new(), &mut tables);
        tables
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
        if limits.work == 0 || limits.memory_bytes == 0 || limits.memory_bytes > 1 << 30 {
            return Err(budget());
        }
        let mut work = Work {
            left: limits.work,
            bytes: limits.memory_bytes,
            deadline: limits.deadline,
            cancel,
            shared,
        };
        work.spend()?;
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
        let mut inputs = BTreeMap::new();
        work.retain(sources.keys().map(|s| s.len() + 96).sum())?;
        for (name, source) in sources {
            inputs.insert(name.clone(), source);
        }
        self.execute_scoped(&inputs, parameters, &mut work)
    }
    fn execute_scoped(
        &self,
        sources: &BTreeMap<String, &Source>,
        parameters: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
    ) -> Result<QueryResult> {
        work.spend()?;
        compound::execute(self, sources, parameters, work)
    }
    fn execute_select(
        &self,
        sources: &BTreeMap<String, &Source>,
        parameters: &BTreeMap<String, Scalar>,
        work: &mut Work<'_>,
    ) -> Result<QueryResult> {
        let joined = if self.joins.is_empty() {
            None
        } else {
            Some(joins::prepare(
                &self.table,
                &self.alias,
                &self.joins,
                sources,
                parameters,
                work,
                self,
            )?)
        };
        let alias = if joined.is_some() {
            joins::QUALIFIED
        } else {
            self.alias.as_str()
        };
        let mut inputs = sources
            .iter()
            .filter(|(name, _)| name.eq_ignore_ascii_case(&self.table));
        let Some((_, &base_source)) = inputs.next() else {
            return Err(binding());
        };
        let source = joined.as_ref().map(|p| &p.source).unwrap_or(base_source);
        if inputs.next().is_some() || source.columns.is_empty() || source.columns.len() > 1024 {
            return Err(binding());
        }
        for column in &source.columns {
            if column.is_empty() || column.len() > 4096 {
                return Err(binding());
            }
        }
        let mut ordering_terms = self.order.clone();
        let mut columns = vec![];
        let mut outputs = vec![];
        for p in &self.projection {
            if let Some(t) = &p.expr {
                bind(t, &source.columns, alias, parameters)?;
                outputs.push(t.clone());
                columns.push(if p.name.is_empty() {
                    if let Term::Column(name) = t {
                        name.rsplit('.').next().unwrap().into()
                    } else {
                        format!("column_{}", columns.len() + 1)
                    }
                } else {
                    p.name.clone()
                });
            } else {
                let mut selected = 0usize;
                for column in &source.columns {
                    if !p.name.is_empty() {
                        let qualifier = if joined.is_some() {
                            column.rsplit_once('.').map(|(q, _)| q).unwrap_or("")
                        } else {
                            alias
                        };
                        if !qualifier.eq_ignore_ascii_case(&p.name) {
                            continue;
                        }
                    }
                    selected += 1;
                    outputs.push(Term::Column(column.clone()));
                    columns.push(if joined.is_some() {
                        column.rsplit('.').next().unwrap().into()
                    } else {
                        column.clone()
                    });
                }
                if selected == 0 {
                    return Err(binding());
                }
            }
        }
        if columns.len() > 1024 {
            return Err(budget());
        }
        for key in &self.groups {
            if aggregate::contains(key) {
                return Err(type_error());
            }
            bind(key, &source.columns, alias, parameters)?;
        }
        if let Some(t) = &self.having {
            bind(t, &source.columns, alias, parameters)?;
        }
        if let Some(t) = &self.filter {
            if aggregate::contains(t) {
                return Err(type_error());
            }
            bind(t, &source.columns, alias, parameters)?;
        }
        for (t, _, _) in &ordering_terms {
            if bind(t, &source.columns, alias, parameters).is_err() {
                bind(t, &columns, alias, parameters)?;
            }
        }
        for t in self.limit.iter().chain(self.offset.iter()) {
            bind(t, &[], "", parameters)?;
        }
        let count = |term: &Option<Term>, default: usize, work: &mut Work<'_>| -> Result<usize> {
            let Some(t) = term else {
                return Ok(default);
            };
            let v = eval(t, &[], &[], "", parameters, work)?;
            let n = int(&v).filter(|n| *n >= 0).ok_or_else(type_error)?;
            usize::try_from(n).map_err(|_| budget())
        };
        let limit = count(&self.limit, usize::MAX, work)?;
        let offset = count(&self.offset, 0, work)?;
        let grouped = if !self.groups.is_empty()
            || self.having.is_some()
            || outputs.iter().any(aggregate::contains)
            || ordering_terms
                .iter()
                .any(|(t, _, _)| aggregate::contains(t))
        {
            let mut grouping = aggregate::Grouping::new(
                &self.groups,
                &mut outputs,
                &mut ordering_terms,
                self.having.as_ref(),
                &columns,
                work,
            )?;
            let mut add = |row: &[Scalar], work: &mut Work<'_>| -> Result<()> {
                work.spend()?;
                if row.len() != source.columns.len() {
                    return Err(binding());
                }
                row_cost(row)?;
                if let Some(t) = &self.filter
                    && truth(&eval(t, row, &source.columns, alias, parameters, work)?)?
                        != Some(true)
                {
                    return Ok(());
                }
                grouping.add(row, &source.columns, alias, parameters, work)
            };
            if let Some(plan) = &joined {
                plan.visit(parameters, work, &mut add)?;
            } else {
                for row in &source.rows {
                    add(row, work)?;
                }
            }
            Some(grouping.finish(source.complete, parameters, work)?)
        } else {
            None
        };
        let source = grouped.as_ref().unwrap_or(source);
        let alias = if grouped.is_some() { "" } else { alias };
        let mut result: Vec<(Vec<Scalar>, Vec<Scalar>)> = vec![];
        let mut seen = BTreeSet::new();
        let mut project_row = |row: &[Scalar], work: &mut Work<'_>| -> Result<()> {
            work.spend()?;
            if row.len() != source.columns.len() {
                return Err(binding());
            }
            row_cost(row)?;
            if grouped.is_none()
                && let Some(t) = &self.filter
                && truth(&eval(t, row, &source.columns, alias, parameters, work)?)? != Some(true)
            {
                return Ok(());
            }
            let projected = outputs
                .iter()
                .map(|t| eval(t, row, &source.columns, alias, parameters, work))
                .collect::<Result<Vec<_>>>()?;
            if self.distinct {
                let key = projected
                    .iter()
                    .map(|v| match v {
                        v if int(v).is_some() => format!("integer:{}", int(v).unwrap()),
                        Scalar::Float(v)
                            if v.is_finite()
                                && v.fract() == 0.
                                && *v >= i64::MIN as f64
                                && *v <= u64::MAX as f64 =>
                        {
                            format!("integer:{}", *v as i128)
                        }
                        _ => serde_json::to_string(v).unwrap(),
                    })
                    .map(|part| format!("{}:{}", part.len(), part))
                    .collect::<String>();
                work.retain(key.len() + 32)?;
                if !seen.insert(key) {
                    return Ok(());
                }
            }
            let mut ordering = vec![];
            for (t, _, _) in &ordering_terms {
                let v = if bind(t, &source.columns, alias, parameters).is_ok() {
                    eval(t, row, &source.columns, alias, parameters, work)?
                } else {
                    eval(t, &projected, &columns, alias, parameters, work)?
                };
                ordering.push(v);
            }
            work.retain(row_cost(&projected)? + row_cost(&ordering)? + 48)?;
            result.push((projected, ordering));
            Ok(())
        };
        if grouped.is_none()
            && let Some(plan) = &joined
        {
            plan.visit(parameters, work, &mut project_row)?;
        } else {
            for row in &source.rows {
                project_row(row, work)?;
            }
        }
        // Fallible stable merge sort: comparison errors never become a fake
        // ordering relation and no partial result is returned on failure.
        let mut width = 1;
        work.retain(
            result
                .len()
                .checked_mul(std::mem::size_of::<(Vec<Scalar>, Vec<Scalar>)>())
                .ok_or_else(budget)?,
        )?;
        while !ordering_terms.is_empty() && width < result.len() {
            let mut start = 0;
            while start < result.len() {
                let middle = (start + width).min(result.len());
                let end = (middle + width).min(result.len());
                let mut left = start;
                let mut right = middle;
                let mut merged = Vec::with_capacity(end - start);
                while left < middle || right < end {
                    work.spend()?;
                    let take_left = if right == end {
                        true
                    } else if left == middle {
                        false
                    } else {
                        let mut order = Ordering::Equal;
                        for ((_, desc, nulls), (a, b)) in ordering_terms
                            .iter()
                            .zip(result[left].1.iter().zip(&result[right].1))
                        {
                            order = if a == &Scalar::Null && b == &Scalar::Null {
                                Ordering::Equal
                            } else if a == &Scalar::Null || b == &Scalar::Null {
                                let first = nulls.unwrap_or(!desc);
                                if (a == &Scalar::Null) == first {
                                    Ordering::Less
                                } else {
                                    Ordering::Greater
                                }
                            } else {
                                let mut c = compare(a, b)?;
                                if *desc {
                                    c = c.reverse();
                                }
                                c
                            };
                            if order != Ordering::Equal {
                                break;
                            }
                        }
                        order != Ordering::Greater
                    };
                    let index = if take_left {
                        let n = left;
                        left += 1;
                        n
                    } else {
                        let n = right;
                        right += 1;
                        n
                    };
                    merged.push(std::mem::take(&mut result[index]));
                }
                for (slot, row) in result[start..end].iter_mut().zip(merged) {
                    *slot = row;
                }
                start = end;
            }
            width = width.saturating_mul(2);
        }
        Ok(QueryResult {
            columns,
            rows: result
                .into_iter()
                .skip(offset)
                .take(limit)
                .map(|r| r.0)
                .collect(),
            complete: source.complete,
        })
    }
}
