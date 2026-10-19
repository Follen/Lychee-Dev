//! DataFusion expression-planner extension over the existing numeric primitives.
use super::{QueryOwner, external, unsupported, values};
use crate::relational::engine_compat::{self, Binary};
use arrow_schema::DataType;
use datafusion::{
    common::{DFSchema, Result, ScalarValue},
    logical_expr::{
        ColumnarValue, ExprSchemable, Volatility,
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
        if !kinds.iter().all(values::numeric) {
            return Ok(PlannerResult::Original(expression));
        }
        // The pinned native integer comparator widens signed/UInt64 pairs to
        // Decimal128(20, 0) without losing bits. Keep those BinaryExprs visible
        // to equijoin extraction instead of hiding valid hash keys in a UDF.
        if op.comparison() && kinds.iter().all(values::integer) {
            return Ok(PlannerResult::Original(expression));
        }
        let result_kind = if op.comparison() {
            DataType::Boolean
        } else if op != Binary::Divide && kinds.iter().all(values::integer) {
            DataType::Decimal128(20, 0)
        } else {
            DataType::Float64
        };
        let return_kind = result_kind.clone();
        let owner = self.owner.clone();
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
        Ok(PlannerResult::Planned(
            udf.call(vec![expression.left, expression.right]),
        ))
    }
}
