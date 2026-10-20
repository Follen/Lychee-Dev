//! Thin numeric port for the DataFusion adapter. No parser, binding, scan,
//! joining or relational execution is duplicated here.
use super::{Scalar, arithmetic, compare, integer, numeric, rational};
use lycheedev_core::Result;
use num_rational::BigRational;
use num_traits::ToPrimitive;
use sqlparser::ast::BinaryOperator;
use std::cmp::Ordering;

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
    arithmetic(
        left,
        &match op {
            Binary::Add => BinaryOperator::Plus,
            Binary::Subtract => BinaryOperator::Minus,
            Binary::Multiply => BinaryOperator::Multiply,
            Binary::Divide => BinaryOperator::Divide,
            Binary::Modulo => BinaryOperator::Modulo,
            _ => unreachable!("arithmetic"),
        },
        right,
    )
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

pub(crate) fn type_error() -> lycheedev_core::ToolkitError {
    super::type_error()
}
pub(crate) fn text(value: &Scalar) -> Result<std::borrow::Cow<'_, str>> {
    super::scalar::text(value)
}
