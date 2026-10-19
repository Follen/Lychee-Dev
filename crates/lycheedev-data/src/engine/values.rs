//! Exact Arrow scalar seam; integers never pass through a binary float.
use super::{external, unsupported};
use crate::relational::{Scalar, engine_compat};
use arrow_schema::DataType;
use datafusion::common::{Result, ScalarValue};

pub(super) fn numeric(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::Null
            | DataType::Int64
            | DataType::UInt64
            | DataType::Float32
            | DataType::Float64
            | DataType::Decimal128(20, 0)
    )
}
pub(super) fn integer(kind: &DataType) -> bool {
    matches!(
        kind,
        DataType::Null | DataType::Int64 | DataType::UInt64 | DataType::Decimal128(20, 0)
    )
}
pub(super) fn read(value: ScalarValue) -> Result<Scalar> {
    if value.is_null() {
        return Ok(Scalar::Null);
    }
    Ok(match value {
        ScalarValue::Int64(Some(value)) => Scalar::Int(value),
        ScalarValue::UInt64(Some(value)) => Scalar::UInt(value),
        ScalarValue::Decimal128(Some(value), 20, 0) => {
            engine_compat::checked_integer(value).map_err(external)?
        }
        ScalarValue::Float32(Some(value)) if value.is_finite() => Scalar::Float(f64::from(value)),
        ScalarValue::Float64(Some(value)) if value.is_finite() => Scalar::Float(value),
        ScalarValue::Boolean(Some(value)) => Scalar::Bool(value),
        ScalarValue::Utf8(Some(value)) => Scalar::String(value),
        _ => {
            return Err(unsupported(
                "Arrow scalar has no verified public numeric representation",
            ));
        }
    })
}
pub(super) fn write(value: Scalar, kind: &DataType) -> Result<ScalarValue> {
    Ok(match (value, kind) {
        (Scalar::Null, kind) => ScalarValue::try_new_null(kind)?,
        (Scalar::Int(value), DataType::Decimal128(20, 0)) => {
            ScalarValue::Decimal128(Some(i128::from(value)), 20, 0)
        }
        (Scalar::UInt(value), DataType::Decimal128(20, 0)) => {
            ScalarValue::Decimal128(Some(i128::from(value)), 20, 0)
        }
        (Scalar::Float(value), DataType::Float64) if value.is_finite() => {
            ScalarValue::Float64(Some(value))
        }
        (Scalar::Bool(value), DataType::Boolean) => ScalarValue::Boolean(Some(value)),
        _ => {
            return Err(unsupported(
                "Compatibility scalar differs from its planned exact Arrow type",
            ));
        }
    })
}
