//! DataFusion expression-planner extension over the existing numeric primitives.
use super::{QueryOwner, external, unsupported, values};
use crate::relational::engine_compat::{self, Binary};
use arrow_schema::DataType;
use datafusion::{
    common::{DFSchema, Result, ScalarValue},
    logical_expr::{
        ColumnarValue, Expr, ExprSchemable, Operator, Volatility,
        expr::BinaryExpr,
        expr_fn::create_udf,
        planner::{ExprPlanner, PlannerResult, RawBinaryExpr},
    },
};
use std::sync::Arc;

#[derive(Debug)]
pub(super) struct NumericPlanner {
    pub owner: Arc<QueryOwner>,
}
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
impl ExprPlanner for NumericPlanner {
    fn plan_binary_op(
        &self,
        expression: RawBinaryExpr,
        schema: &DFSchema,
    ) -> Result<PlannerResult<RawBinaryExpr>> {
        self.owner.check()?;
        let Some(op) = operation(&expression.op.to_string()) else {
            return Ok(PlannerResult::Original(expression));
        };
        let kinds = vec![
            expression.left.get_type(schema)?,
            expression.right.get_type(schema)?,
        ];
        if !needs_exact(op, &kinds) {
            return Ok(PlannerResult::Original(expression));
        }
        Ok(PlannerResult::Planned(exact_binary(
            expression.left,
            op,
            expression.right,
            kinds,
            &self.owner,
        )?))
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
    // Native mixed-domain coercion can turn a required type error into a
    // successful comparison/arithmetic result. Keep that decision in the
    // exact scalar seam, while preserving row-level NULL short-circuiting.
    true
}

/// Both SQL and the typed frontend use this seam. DataFrame expressions do not
/// pass through SQL ExprPlanner extensions, so raw arithmetic is not equivalent.
pub(super) fn binary(
    left: Expr,
    operator: Operator,
    right: Expr,
    schema: &DFSchema,
    owner: &Arc<QueryOwner>,
) -> Result<Expr> {
    owner.check()?;
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
        Volatility::Immutable,
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
            let calculate = |row| {
                owner.work(1)?;
                let _scratch = owner.budget.reserve_retained(8192).map_err(external)?;
                let is_null = |value: &ColumnarValue| match value {
                    ColumnarValue::Scalar(value) => value.is_null(),
                    ColumnarValue::Array(array) => array.is_null(row),
                };
                if is_null(left) || is_null(right) {
                    return ScalarValue::try_new_null(&return_kind);
                }
                if !numeric_inputs {
                    return Err(external(crate::fault(
                        "relational.type_mismatch",
                        4,
                        "SQL scalar types are incompatible",
                    )));
                }
                let left = values::read(scalar_at(left, row)?)?;
                let right = values::read(scalar_at(right, row)?)?;
                values::write(
                    engine_compat::evaluate(&left, op, &right).map_err(external)?,
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
