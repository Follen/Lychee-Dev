//! Exact typed scalar operations and authenticated row cursor contracts.
use crate::fault;
use lycheedev_core::Result;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use serde::Serialize;
use std::{borrow::Cow, cmp::Ordering};
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
    fault("relational.unsupported", 3, "Unknown scalar operation")
}
fn numeric() -> lycheedev_core::ToolkitError {
    problem(
        "relational.numeric_range",
        "Query number exceeds supported finite 64-bit range",
    )
}
pub(crate) fn type_error() -> lycheedev_core::ToolkitError {
    problem(
        "relational.type_mismatch",
        "Query scalar types are incompatible",
    )
}
fn budget() -> lycheedev_core::ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "Query scalar work/memory/deadline budget exceeded",
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
    if !decimal_literal(text) {
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
fn arithmetic(a: &Scalar, op: &Binary, b: &Scalar) -> Result<Scalar> {
    if matches!(a, Scalar::Null) || matches!(b, Scalar::Null) {
        return Ok(Scalar::Null);
    }
    if let (Some(a), Some(b)) = (int(a), int(b)) {
        let n = match op {
            Binary::Add => a.checked_add(b),
            Binary::Subtract => a.checked_sub(b),
            Binary::Multiply => a.checked_mul(b),
            Binary::Modulo if b == 0 => return Ok(Scalar::Null),
            Binary::Modulo => a.checked_rem(b),
            Binary::Divide => return rational_arithmetic(&integer(a)?, op, &integer(b)?),
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
fn rational_arithmetic(a: &Scalar, op: &Binary, b: &Scalar) -> Result<Scalar> {
    let (a, b) = (rational(a)?, rational(b)?);
    // Every operand is first canonicalized to i64/u64/finite f64. A binary
    // f64 needs at most 1075 denominator bits; arithmetic never retains a
    // rational between expression nodes. Guard both inputs and output explicitly.
    let bounded = |n: &BigRational| n.numer().bits() <= 4096 && n.denom().bits() <= 4096;
    if !bounded(&a) || !bounded(&b) {
        return Err(budget());
    }
    if matches!(op, Binary::Divide | Binary::Modulo) && b.is_zero() {
        return Ok(Scalar::Null);
    }
    let result = match op {
        Binary::Add => a + b,
        Binary::Subtract => a - b,
        Binary::Multiply => a * b,
        Binary::Divide => a / b,
        Binary::Modulo => {
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
pub(crate) fn decimal_literal(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut at = usize::from(bytes.first() == Some(&b'-'));
    let before = at;
    while bytes.get(at).is_some_and(u8::is_ascii_digit) {
        at += 1;
    }
    let mut digits = at - before;
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        let before = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        digits += at - before;
    }
    if digits == 0 {
        return false;
    }
    if bytes.get(at).is_some_and(|b| matches!(b, b'e' | b'E')) {
        at += 1;
        if bytes.get(at).is_some_and(|b| matches!(b, b'+' | b'-')) {
            at += 1;
        }
        let before = at;
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == before {
            return false;
        }
    }
    at == bytes.len()
}

/// Exact integer spelling and shortest finite binary64 text for query casts.
pub(crate) fn text(value: &Scalar) -> Result<Cow<'_, str>> {
    Ok(match value {
        Scalar::Null => Cow::Borrowed(""),
        Scalar::String(value) => Cow::Borrowed(value),
        Scalar::Bool(true) => Cow::Borrowed("true"),
        Scalar::Bool(false) => Cow::Borrowed("false"),
        Scalar::Int(value) => Cow::Owned(value.to_string()),
        Scalar::UInt(value) => Cow::Owned(value.to_string()),
        Scalar::Float(value) => {
            if !value.is_finite() {
                return Err(numeric());
            }
            let absolute = value.abs();
            Cow::Owned(if absolute != 0.0 && !(1e-4..1e6).contains(&absolute) {
                let scientific = format!("{value:e}");
                let (mantissa, exponent) = scientific.split_once('e').ok_or_else(numeric)?;
                let exponent: i32 = exponent.parse().map_err(|_| numeric())?;
                format!("{mantissa}e{exponent:+03}")
            } else {
                value.to_string()
            })
        }
    })
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Binary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
impl Binary {
    pub(crate) fn comparison(self) -> bool {
        matches!(
            self,
            Self::Equal
                | Self::NotEqual
                | Self::Less
                | Self::LessEqual
                | Self::Greater
                | Self::GreaterEqual
        )
    }
}
pub(crate) fn evaluate(left: &Scalar, op: Binary, right: &Scalar) -> Result<Scalar> {
    if matches!(left, Scalar::Null) || matches!(right, Scalar::Null) {
        return Ok(Scalar::Null);
    }
    if op.comparison() {
        let order = compare(left, right)?;
        return Ok(Scalar::Bool(match op {
            Binary::Equal => order == Ordering::Equal,
            Binary::NotEqual => order != Ordering::Equal,
            Binary::Less => order == Ordering::Less,
            Binary::LessEqual => order != Ordering::Greater,
            Binary::Greater => order == Ordering::Greater,
            Binary::GreaterEqual => order != Ordering::Less,
            _ => unreachable!("comparison"),
        }));
    }
    arithmetic(left, &op, right)
}
pub(crate) fn exact(value: &Scalar) -> Result<BigRational> {
    rational(value)
}
pub(crate) fn finish_sum(value: &BigRational, all_integer: bool) -> Result<Scalar> {
    if all_integer {
        integer(value.to_integer().to_i128().ok_or_else(numeric)?)
    } else {
        Ok(Scalar::Float(
            value
                .to_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(numeric)?,
        ))
    }
}
pub(crate) fn finish_average(value: &BigRational, count: i64) -> Result<Scalar> {
    if count <= 0 {
        return Err(numeric());
    }
    let ratio = value / BigRational::from_integer(count.into());
    Ok(Scalar::Float(
        ratio
            .to_f64()
            .filter(|v| v.is_finite())
            .ok_or_else(numeric)?,
    ))
}
pub(crate) fn checked_integer(value: i128) -> Result<Scalar> {
    integer(value)
}
pub(crate) fn numeric_error() -> lycheedev_core::ToolkitError {
    numeric()
}

pub(crate) struct Source {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Scalar>>,
    pub complete: bool,
}
/// A single opened producer. The current row and all decoding leases remain
/// owned here until advance or drop. Never recreate a scan to resume it.
pub(crate) trait RowCursor {
    fn advance(&mut self) -> Result<bool>;
    fn row(&self) -> &[Scalar];
}
/// Convert an already parsed, exact JSON scalar at the Hotfix overlay boundary.
pub(crate) fn from_json(value: serde_json::Value) -> Result<Scalar> {
    use serde_json::Value;
    Ok(match value {
        Value::Null => Scalar::Null,
        Value::Bool(value) => Scalar::Bool(value),
        Value::String(value) => Scalar::String(value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Scalar::Int(value)
            } else if let Some(value) = value.as_u64() {
                Scalar::UInt(value)
            } else if let Some(value) = value.as_f64().filter(|value| value.is_finite()) {
                Scalar::Float(value)
            } else {
                return Err(numeric());
            }
        }
        _ => return Err(type_error()),
    })
}
