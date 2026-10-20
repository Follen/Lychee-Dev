//! Narrow business scalar semantics inside mature DataFusion execution.
//! Admission is per batch; no legacy row interpreter or per-cell async task.
use super::{
    QueryOwner,
    api::{CastKind, ScalarFunction},
    external, unsupported, values,
};
use crate::relational::{Scalar, engine_compat};
use arrow_array::Array;
use arrow_schema::DataType;
use datafusion::{
    common::{DFSchema, Result, ScalarValue},
    logical_expr::{ColumnarValue, Expr, ExprSchemable, Volatility, create_udf},
};
use num_traits::{ToPrimitive, Zero};
use std::sync::Arc;

#[derive(Clone, Copy)]
enum Function {
    Cast(CastKind),
    Call(ScalarFunction),
    Like(bool),
    Between(bool),
    ValidateBits,
}
fn type_error() -> datafusion::common::DataFusionError {
    external(engine_compat::type_error())
}
fn numeric_error() -> datafusion::common::DataFusionError {
    external(engine_compat::numeric_error())
}
fn at(value: &ColumnarValue, row: usize) -> Result<ScalarValue> {
    match value {
        ColumnarValue::Scalar(v) => Ok(v.clone()),
        ColumnarValue::Array(a) => ScalarValue::try_from_array(a, row),
    }
}
fn output(value: Scalar) -> Result<ScalarValue> {
    Ok(match value {
        Scalar::Null => ScalarValue::Null,
        Scalar::Bool(v) => ScalarValue::Boolean(Some(v)),
        Scalar::String(v) => ScalarValue::Utf8(Some(v)),
        Scalar::Int(v) => ScalarValue::Int64(Some(v)),
        Scalar::UInt(v) => ScalarValue::UInt64(Some(v)),
        Scalar::Float(v) if v.is_finite() => ScalarValue::Float64(Some(v)),
        _ => return Err(numeric_error()),
    })
}
pub(super) fn cast(
    value: Expr,
    target: CastKind,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    build(Function::Cast(target), vec![value], schema, owner)
}
pub(super) fn call(
    function: ScalarFunction,
    args: Vec<Expr>,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    if matches!(function, ScalarFunction::HasFlag | ScalarFunction::BitAnd) {
        let [left, right]: [Expr; 2] = args.try_into().map_err(|_| type_error())?;
        let left = build(Function::ValidateBits, vec![left], schema, owner)?;
        let result = build(
            Function::Call(function),
            vec![left.clone(), right],
            schema,
            owner,
        )?;
        let kind = if matches!(function, ScalarFunction::HasFlag) {
            DataType::Boolean
        } else {
            DataType::UInt64
        };
        return Ok(Expr::Case(datafusion::logical_expr::expr::Case::new(
            None,
            vec![(
                Box::new(left.is_null()),
                Box::new(Expr::Literal(ScalarValue::try_new_null(&kind)?, None)),
            )],
            Some(Box::new(result)),
        )));
    }
    build(Function::Call(function), args, schema, owner)
}
pub(super) fn like(
    value: Expr,
    pattern: Expr,
    negated: bool,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    build(Function::Like(negated), vec![value, pattern], schema, owner)
}
pub(super) fn between(
    value: Expr,
    low: Expr,
    high: Expr,
    negated: bool,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    build(
        Function::Between(negated),
        vec![value, low, high],
        schema,
        owner,
    )
}
fn build(
    function: Function,
    args: Vec<Expr>,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    let arity = match function {
        Function::Between(_) => 3,
        Function::Like(_)
        | Function::Call(
            ScalarFunction::Concat | ScalarFunction::HasFlag | ScalarFunction::BitAnd,
        ) => 2,
        _ => 1,
    };
    if args.len() != arity {
        return Err(type_error());
    }
    let kinds = args
        .iter()
        .map(|e| e.get_type(schema))
        .collect::<Result<Vec<_>>>()?;
    for kind in &kinds {
        // Bind scalar shape, not eager value-domain errors. NULL and lazy CASE
        // must not evaluate an unreachable invalid string/numeric conversion.
        if !values::numeric(kind) && !matches!(kind, DataType::Utf8 | DataType::Boolean) {
            return Err(type_error());
        }
    }
    let result = match function {
        Function::Cast(CastKind::Integer) | Function::Call(ScalarFunction::Length) => {
            DataType::Int64
        }
        Function::Cast(CastKind::Float) => DataType::Float64,
        Function::ValidateBits | Function::Call(ScalarFunction::BitAnd) => DataType::UInt64,
        Function::Cast(CastKind::Boolean)
        | Function::Like(_)
        | Function::Between(_)
        | Function::Call(ScalarFunction::HasFlag) => DataType::Boolean,
        _ => DataType::Utf8,
    };
    let return_type = result.clone();
    let owner = owner.clone();
    let name = match function {
        Function::Cast(CastKind::String) => "cast_string",
        Function::Cast(CastKind::Integer) => "cast_integer",
        Function::Cast(CastKind::Float) => "cast_float",
        Function::Cast(CastKind::Boolean) => "cast_boolean",
        Function::Call(ScalarFunction::Lower) => "lower",
        Function::Call(ScalarFunction::Upper) => "upper",
        Function::Call(ScalarFunction::Length) => "length",
        Function::Call(ScalarFunction::Concat) => "concat",
        Function::Call(ScalarFunction::HasFlag) => "has_flag",
        Function::Call(ScalarFunction::BitAnd) => "bit_and",
        Function::ValidateBits => "validate_bits",
        Function::Between(false) => "between",
        Function::Between(true) => "not_between",
        Function::Like(false) => "like",
        Function::Like(true) => "not_like",
    };
    let udf = create_udf(
        &format!("__lychee_{name}"),
        kinds,
        result,
        Volatility::Volatile,
        Arc::new(move |args| {
            owner.check()?;
            let rows = args.iter().find_map(|v| match v {
                ColumnarValue::Array(a) => Some(a.len()),
                _ => None,
            });
            let count = rows.unwrap_or(1);
            if args
                .iter()
                .any(|v| matches!(v,ColumnarValue::Array(a) if a.len()!=count))
            {
                return Err(unsupported("Scalar batch lengths differ"));
            }
            let mut bytes = count
                .checked_mul(128)
                .ok_or_else(|| external(super::limit()))?;
            for arg in args {
                bytes = bytes
                    .checked_add(match arg {
                        ColumnarValue::Array(a) => a.get_array_memory_size(),
                        ColumnarValue::Scalar(ScalarValue::Utf8(Some(s))) => s
                            .len()
                            .checked_mul(count)
                            .ok_or_else(|| external(super::limit()))?,
                        _ => 64,
                    })
                    .ok_or_else(|| external(super::limit()))?;
            }
            let allowance = bytes
                .checked_mul(8)
                .and_then(|n| n.checked_add(8192))
                .ok_or_else(|| external(super::limit()))?;
            owner.retain(allowance)?;
            owner.work(count)?;
            let calculate = |row| -> Result<ScalarValue> {
                if row % 64 == 0 {
                    owner.check()?;
                }
                let mut input = Vec::with_capacity(args.len());
                for arg in args {
                    input.push(values::read(at(arg, row)?)?);
                }
                let value = match function {
                    Function::Cast(target) => cast_value(input.remove(0), target)?,
                    Function::ValidateBits => bits_value(input.remove(0))?,
                    Function::Call(kind) => call_value(kind, &input, &owner)?,
                    Function::Like(negated) => like_value(&input[0], &input[1], negated, &owner)?,
                    Function::Between(negated) => {
                        // All three argument expressions have completed before comparisons.
                        let low = engine_compat::evaluate(
                            &input[0],
                            engine_compat::Binary::GreaterEqual,
                            &input[1],
                        )
                        .map_err(external)?;
                        let high = engine_compat::evaluate(
                            &input[0],
                            engine_compat::Binary::LessEqual,
                            &input[2],
                        )
                        .map_err(external)?;
                        if low == Scalar::Bool(false) || high == Scalar::Bool(false) {
                            Scalar::Bool(negated)
                        } else if low == Scalar::Bool(true) && high == Scalar::Bool(true) {
                            Scalar::Bool(!negated)
                        } else {
                            Scalar::Null
                        }
                    }
                };
                if value == Scalar::Null {
                    ScalarValue::try_new_null(&return_type)
                } else {
                    output(value)
                }
            };
            if rows.is_none() {
                return Ok(ColumnarValue::Scalar(calculate(0)?));
            }
            if count == 0 {
                return Ok(ColumnarValue::Array(arrow_array::new_empty_array(
                    &return_type,
                )));
            }
            let output = (0..count).map(calculate).collect::<Result<Vec<_>>>()?;
            Ok(ColumnarValue::Array(ScalarValue::iter_to_array(output)?))
        }),
    );
    Ok(udf.call(args))
}
fn cast_value(value: Scalar, target: CastKind) -> Result<Scalar> {
    if value == Scalar::Null {
        return Ok(value);
    }
    if let Scalar::String(s) = &value {
        if s.len() > 1 << 20 {
            return Err(external(super::limit()));
        }
    }
    if matches!(target, CastKind::String) {
        return match value {
            Scalar::String(_) => Ok(value),
            _ => Ok(Scalar::String(
                engine_compat::text(&value).map_err(external)?.into_owned(),
            )),
        };
    }
    if matches!(target, CastKind::Boolean) {
        return match value {
            Scalar::Bool(_) => Ok(value),
            Scalar::String(s) => match s.as_str() {
                "1" | "t" | "T" | "TRUE" | "true" | "True" => Ok(Scalar::Bool(true)),
                "0" | "f" | "F" | "FALSE" | "false" | "False" => Ok(Scalar::Bool(false)),
                _ => Err(type_error()),
            },
            _ => {
                let number = engine_compat::exact(&value).map_err(external)?;
                if number.is_zero() {
                    Ok(Scalar::Bool(false))
                } else if number == num_rational::BigRational::from_integer(1.into()) {
                    Ok(Scalar::Bool(true))
                } else {
                    Err(type_error())
                }
            }
        };
    }
    let value = match value {
        Scalar::String(s) => crate::relational::literal(&s).map_err(external)?,
        v => v,
    };
    let exact = engine_compat::exact(&value).map_err(external)?;
    if matches!(target, CastKind::Integer) {
        return exact
            .to_integer()
            .to_i64()
            .map(Scalar::Int)
            .ok_or_else(numeric_error);
    }
    exact
        .to_f64()
        .filter(|v| v.is_finite())
        .map(Scalar::Float)
        .ok_or_else(numeric_error)
}
fn call_value(kind: ScalarFunction, input: &[Scalar], owner: &Arc<QueryOwner>) -> Result<Scalar> {
    if input.iter().any(|v| *v == Scalar::Null) {
        return Ok(Scalar::Null);
    }
    if matches!(kind, ScalarFunction::HasFlag | ScalarFunction::BitAnd) {
        let Scalar::UInt(left) = bits_value(input[0].clone())? else {
            return Err(type_error());
        };
        let Scalar::UInt(right) = bits_value(input[1].clone())? else {
            return Err(type_error());
        };
        return Ok(if matches!(kind, ScalarFunction::HasFlag) {
            Scalar::Bool(left & right == right)
        } else {
            Scalar::UInt(left & right)
        });
    }
    let Scalar::String(text) = &input[0] else {
        return Err(type_error());
    };
    if text.len() > 1 << 20 {
        return Err(external(super::limit()));
    }
    owner.work(text.len())?;
    Ok(match kind {
        ScalarFunction::Length => Scalar::Int(text.chars().count() as i64),
        ScalarFunction::Lower | ScalarFunction::Upper => {
            let upper = matches!(kind, ScalarFunction::Upper);
            let mut bytes = 0usize;
            for (index, c) in text.chars().enumerate() {
                if index % 1024 == 0 {
                    owner.check()?;
                }
                bytes = bytes
                    .checked_add(crate::unicode_simple::map(c, upper).len_utf8())
                    .ok_or_else(|| external(super::limit()))?;
                if bytes > 1 << 20 {
                    return Err(external(super::limit()));
                }
            }
            let mut result = String::with_capacity(bytes);
            for (index, c) in text.chars().enumerate() {
                if index % 1024 == 0 {
                    owner.check()?;
                }
                result.push(crate::unicode_simple::map(c, upper));
            }
            Scalar::String(result)
        }
        ScalarFunction::HasFlag | ScalarFunction::BitAnd => {
            unreachable!("handled before text functions")
        }
        ScalarFunction::Concat => {
            let Scalar::String(right) = &input[1] else {
                return Err(type_error());
            };
            let length = text
                .len()
                .checked_add(right.len())
                .ok_or_else(|| external(super::limit()))?;
            if length > 1 << 20 {
                return Err(external(super::limit()));
            }
            owner.work(right.len())?;
            let mut result = String::with_capacity(length);
            result.push_str(text);
            result.push_str(right);
            Scalar::String(result)
        }
    })
}
fn like_value(
    value: &Scalar,
    pattern: &Scalar,
    negated: bool,
    owner: &Arc<QueryOwner>,
) -> Result<Scalar> {
    if value == &Scalar::Null || pattern == &Scalar::Null {
        return Ok(Scalar::Null);
    }
    let (Scalar::String(text), Scalar::String(mask)) = (value, pattern) else {
        return Err(type_error());
    };
    if text.len() > 1 << 20 || mask.len() > 1 << 20 {
        return Err(external(super::limit()));
    }
    let count = mask.chars().count();
    let work = text
        .chars()
        .count()
        .checked_mul(count + 1)
        .and_then(|n| n.checked_add(count))
        .ok_or_else(|| external(super::limit()))?;
    owner.work(work)?;
    let letters: Vec<char> = mask.chars().collect();
    let mut previous = vec![false; count + 1];
    let mut next = vec![false; count + 1];
    previous[0] = true;
    for (i, c) in letters.iter().enumerate() {
        if i % 1024 == 0 {
            owner.check()?;
        }
        previous[i + 1] = previous[i] && *c == '%';
    }
    for (row, c) in text.chars().enumerate() {
        if row % 64 == 0 {
            owner.check()?;
        }
        next[0] = false;
        for (i, token) in letters.iter().enumerate() {
            if i % 1024 == 0 {
                owner.check()?;
            }
            next[i + 1] = match token {
                '%' => next[i] || previous[i + 1],
                '_' => previous[i],
                token => previous[i] && *token == c,
            };
        }
        std::mem::swap(&mut previous, &mut next);
    }
    Ok(Scalar::Bool(previous[count] != negated))
}

/// Zero-based, checked access to the actual nested Arrow element type.
pub(super) fn index(
    value: Expr,
    index: usize,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    let kind = value.get_type(schema)?;
    let output = match &kind {
        DataType::FixedSizeList(field, _) => field.data_type().clone(),
        DataType::List(field) | DataType::LargeList(field) => field.data_type().clone(),
        _ => return Err(type_error()),
    };
    let result = output.clone();
    let owner = owner.clone();
    let udf = create_udf(
        "__lychee_array_index",
        vec![kind],
        output,
        Volatility::Volatile,
        Arc::new(move |args| {
            let [input] = args else {
                return Err(type_error());
            };
            owner.check()?;
            let rows = match input {
                ColumnarValue::Scalar(_) => None,
                ColumnarValue::Array(a) => Some(a.len()),
            };
            let count = rows.unwrap_or(1);
            owner.work(count)?;
            let input_bytes = match input {
                ColumnarValue::Array(a) => a.get_array_memory_size(),
                ColumnarValue::Scalar(ScalarValue::FixedSizeList(a)) => a.get_array_memory_size(),
                ColumnarValue::Scalar(ScalarValue::List(a)) => a.get_array_memory_size(),
                ColumnarValue::Scalar(ScalarValue::LargeList(a)) => a.get_array_memory_size(),
                _ => 0,
            };
            owner.retain(
                count
                    .checked_mul(128)
                    .and_then(|n| {
                        input_bytes
                            .checked_mul(2)
                            .and_then(|bytes| n.checked_add(bytes))
                    })
                    .and_then(|n| n.checked_add(1024))
                    .ok_or_else(|| external(super::limit()))?,
            )?;
            let get = |row| -> Result<ScalarValue> {
                if row % 64 == 0 {
                    owner.check()?;
                }
                let item = at(input, row)?;
                if item.is_null() {
                    return ScalarValue::try_new_null(&result);
                }
                let array = match item {
                    ScalarValue::FixedSizeList(a) => a.value(0),
                    ScalarValue::List(a) => a.value(0),
                    ScalarValue::LargeList(a) => a.value(0),
                    _ => return Err(type_error()),
                };
                if index >= array.len() {
                    return Err(unsupported("Array index is out of bounds"));
                }
                ScalarValue::try_from_array(array.as_ref(), index)
            };
            if rows.is_none() {
                return Ok(ColumnarValue::Scalar(get(0)?));
            }
            if count == 0 {
                return Ok(ColumnarValue::Array(arrow_array::new_empty_array(&result)));
            }
            Ok(ColumnarValue::Array(ScalarValue::iter_to_array(
                (0..count).map(get).collect::<Result<Vec<_>>>()?,
            )?))
        }),
    );
    Ok(udf.call(vec![value]))
}

fn bits_value(value: Scalar) -> Result<Scalar> {
    match value {
        Scalar::Null => Ok(Scalar::Null),
        Scalar::Int(value) => Ok(Scalar::UInt(value as u64)),
        Scalar::UInt(value) => Ok(Scalar::UInt(value)),
        _ => Err(type_error()),
    }
}
