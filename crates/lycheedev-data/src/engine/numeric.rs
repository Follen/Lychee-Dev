//! Exact numeric expressions for the typed DataFusion plan frontend.
use super::{QueryOwner, external, unsupported, values};
use crate::query_value::{self as scalar, Binary};
use arrow_schema::DataType;
use datafusion::{
    common::{DFSchema, Result, ScalarValue},
    logical_expr::{
        ColumnarValue, Expr, ExprSchemable, Operator, Volatility, expr::BinaryExpr,
        expr_fn::create_udf,
    },
};
use std::sync::Arc;

fn operation(name: &str) -> Option<Binary> {
    Some(match name {
        "+" => Binary::Add,
        "-" => Binary::Subtract,
        "*" => Binary::Multiply,
        "/" => Binary::Divide,
        "%" => Binary::Modulo,
        "=" => Binary::Equal,
        "<>" | "!=" => Binary::NotEqual,
        "<" => Binary::Less,
        "<=" => Binary::LessEqual,
        ">" => Binary::Greater,
        ">=" => Binary::GreaterEqual,
        _ => return None,
    })
}
fn scalar_at(value: &ColumnarValue, row: usize) -> Result<ScalarValue> {
    match value {
        ColumnarValue::Scalar(value) => Ok(value.clone()),
        ColumnarValue::Array(array) => ScalarValue::try_from_array(array, row),
    }
}
fn needs_exact(op: Binary, kinds: &[DataType]) -> bool {
    // The pinned native comparator widens signed/UInt64 pairs to Decimal128
    // without losing bits. Keep integer equality visible to hash join planning.
    if op.comparison() && kinds.iter().all(values::integer) {
        return false;
    }
    if op.comparison()
        && kinds.len() == 2
        && kinds[0] == kinds[1]
        && matches!(kinds[0], DataType::Utf8 | DataType::Boolean)
    {
        return false;
    }
    true
}

/// Typed plans use the exact numeric seam before constructing native expressions.
pub(super) fn binary(
    left: Expr,
    operator: Operator,
    right: Expr,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    owner.check()?;
    if matches!(operator, Operator::And | Operator::Or) {
        return boolean_binary(left, operator, right, schema, owner);
    }
    if let Some(op) = operation(&operator.to_string()) {
        let kinds = vec![left.get_type(schema)?, right.get_type(schema)?];
        if needs_exact(op, &kinds) {
            return exact_binary(left, op, right, kinds, owner);
        }
    }
    Ok(Expr::BinaryExpr(BinaryExpr::new(
        Box::new(left),
        operator,
        Box::new(right),
    )))
}

fn exact_binary(
    left: Expr,
    op: Binary,
    right: Expr,
    kinds: Vec<DataType>,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    let result_kind = if op.comparison() {
        DataType::Boolean
    } else if op != Binary::Divide && kinds.iter().all(values::integer) {
        DataType::Decimal128(20, 0)
    } else {
        DataType::Float64
    };
    let return_kind = result_kind.clone();
    let numeric_inputs = kinds.iter().all(values::numeric);
    let owner = owner.clone();
    let udf = create_udf(
        &format!("__lychee_numeric_{op:?}"),
        kinds,
        result_kind,
        Volatility::Volatile,
        Arc::new(move |arguments| {
            owner.check()?;
            let [left, right] = arguments else {
                return Err(unsupported("Numeric function arity changed"));
            };
            let rows = arguments
                .iter()
                .filter_map(|arg| match arg {
                    ColumnarValue::Array(a) => Some(a.len()),
                    _ => None,
                })
                .next();
            owner.work(rows.unwrap_or(1))?;
            let _scratch = owner.budget.reserve_retained(8192).map_err(external)?;
            let calculate = |row| {
                if row % 64 == 0 {
                    owner.check()?;
                }
                let is_null = |value: &ColumnarValue| match value {
                    ColumnarValue::Scalar(value) => value.is_null(),
                    ColumnarValue::Array(array) => array.is_null(row),
                };
                if is_null(left) || is_null(right) {
                    return ScalarValue::try_new_null(&return_kind);
                }
                if !numeric_inputs {
                    return Err(external(scalar::type_error()));
                }
                let left = values::read(scalar_at(left, row)?)?;
                let right = values::read(scalar_at(right, row)?)?;
                values::write(
                    scalar::evaluate(&left, op, &right).map_err(external)?,
                    &return_kind,
                )
            };
            match rows {
                None => Ok(ColumnarValue::Scalar(calculate(0)?)),
                Some(rows) => {
                    if arguments
                        .iter()
                        .any(|arg| matches!(arg,ColumnarValue::Array(a) if a.len()!=rows))
                    {
                        return Err(unsupported("Numeric batch lengths differ"));
                    }
                    if rows == 0 {
                        return Ok(ColumnarValue::Array(arrow_array::new_empty_array(
                            &return_kind,
                        )));
                    }
                    owner.retain(
                        rows.checked_mul(2 * std::mem::size_of::<ScalarValue>() + 32)
                            .ok_or_else(|| external(super::limit()))?,
                    )?;
                    let results = (0..rows).map(calculate).collect::<Result<Vec<_>>>()?;
                    Ok(ColumnarValue::Array(ScalarValue::iter_to_array(results)?))
                }
            }
        }),
    );
    Ok(udf.call(vec![left, right]))
}

/// Only total column equalities may use native conjunction. Keeping those
/// predicates visible lets DF pull correlated equality keys over aggregates;
/// neither side can raise an evaluation error that short-circuiting would hide.
fn native_equality_conjunction(value: &Expr, schema: &DFSchema) -> Result<bool> {
    let Expr::BinaryExpr(expression) = value else {
        return Ok(false);
    };
    match expression.op {
        Operator::And => Ok(native_equality_conjunction(&expression.left, schema)?
            && native_equality_conjunction(&expression.right, schema)?),
        Operator::Eq
            if matches!(
                expression.left.as_ref(),
                Expr::Column(_) | Expr::OuterReferenceColumn(_, _)
            ) && matches!(
                expression.right.as_ref(),
                Expr::Column(_) | Expr::OuterReferenceColumn(_, _)
            ) =>
        {
            let kinds = [
                expression.left.get_type(schema)?,
                expression.right.get_type(schema)?,
            ];
            Ok(!needs_exact(Binary::Equal, &kinds))
        }
        _ => Ok(false),
    }
}

/// Eager argument evaluation preserves the established scalar error/NULL
/// contract. Native DF AND/OR may skip the right side on a false/true batch.
fn boolean_binary(
    left: Expr,
    operator: Operator,
    right: Expr,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    if operator == Operator::And
        && native_equality_conjunction(&left, schema)?
        && native_equality_conjunction(&right, schema)?
    {
        return Ok(Expr::BinaryExpr(BinaryExpr::new(
            Box::new(left),
            operator,
            Box::new(right),
        )));
    }
    let kinds = vec![left.get_type(schema)?, right.get_type(schema)?];
    let owner = owner.clone();
    let and = operator == Operator::And;
    let udf = create_udf(
        if and {
            "__lychee_bool_and"
        } else {
            "__lychee_bool_or"
        },
        kinds,
        DataType::Boolean,
        Volatility::Volatile,
        Arc::new(move |args| {
            owner.check()?;
            let [left, right] = args else {
                return Err(unsupported("Boolean function arity changed"));
            };
            let rows = args.iter().find_map(|v| match v {
                ColumnarValue::Array(a) => Some(a.len()),
                _ => None,
            });
            let count = rows.unwrap_or(1);
            if args
                .iter()
                .any(|v| matches!(v,ColumnarValue::Array(a) if a.len()!=count))
            {
                return Err(unsupported("Boolean batch lengths differ"));
            }
            owner.work(count)?;
            owner.retain(
                count
                    .checked_mul(4)
                    .and_then(|n| n.checked_add(256))
                    .ok_or_else(|| external(super::limit()))?,
            )?;
            let truth = |value: &ColumnarValue, row: usize| -> Result<Option<bool>> {
                match value {
                    ColumnarValue::Scalar(ScalarValue::Boolean(value)) => Ok(*value),
                    ColumnarValue::Scalar(value) if value.is_null() => Ok(None),
                    ColumnarValue::Array(array) if array.is_null(row) => Ok(None),
                    ColumnarValue::Array(array) => array
                        .as_any()
                        .downcast_ref::<arrow_array::BooleanArray>()
                        .map(|a| Some(a.value(row)))
                        .ok_or_else(|| {
                            external(crate::fault(
                                "relational.type_mismatch",
                                4,
                                "SQL scalar types are incompatible",
                            ))
                        }),
                    _ => Err(external(crate::fault(
                        "relational.type_mismatch",
                        4,
                        "SQL scalar types are incompatible",
                    ))),
                }
            };
            let evaluate = |row| -> Result<Option<bool>> {
                if row % 64 == 0 {
                    owner.check()?;
                }
                let left = truth(left, row)?;
                let right = truth(right, row)?;
                Ok(match (and, left, right) {
                    (true, Some(false), _) | (true, _, Some(false)) => Some(false),
                    (false, Some(true), _) | (false, _, Some(true)) => Some(true),
                    (_, Some(left), Some(right)) => {
                        Some(if and { left && right } else { left || right })
                    }
                    _ => None,
                })
            };
            if rows.is_none() {
                return Ok(ColumnarValue::Scalar(ScalarValue::Boolean(evaluate(0)?)));
            }
            let values = (0..count).map(evaluate).collect::<Result<Vec<_>>>()?;
            Ok(ColumnarValue::Array(Arc::new(
                arrow_array::BooleanArray::from(values),
            )))
        }),
    );
    Ok(udf.call(vec![left, right]))
}
