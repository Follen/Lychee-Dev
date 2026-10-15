//! The fixed contextual binary core. Logical table/union/coverage work is an
//! explicit continuation on the same backend, never an Unknown fallback.
use super::{ExpressionExecutionBackend, SelectedProjector};
use emmylua_code_analysis::{FileId, LuaType};
use emmylua_parser::BinaryOperator;

#[derive(Debug)]
pub enum PrimitiveBinaryProjection<V> {
    Complete(V),
    /// The original inputs are moved, not reconstructed from public LuaTypes.
    /// The owned continuation must do RHS table-literal tuple projection before
    /// logical truthy/falsy union or type-coverage decisions, in that order.
    DeferredLogical {
        op: BinaryOperator,
        left: V,
        right: V,
    },
}

impl SelectedProjector<'_> {
    /// Called by the same owned execution backend after S5's nullable extras.
    /// This preserves the binding-session branch of the pinned engine; there
    /// is no mode that silently switches to its more permissive raw semantics.
    pub fn project_primitive_binary<B: ExpressionExecutionBackend>(
        &self,
        file_id: FileId,
        op: BinaryOperator,
        left: B::Value,
        right: B::Value,
        backend: &mut B,
    ) -> Result<PrimitiveBinaryProjection<B::Value>, B::Error> {
        use BinaryOperator::*;
        backend.checkpoint()?;
        // These two contextual fast paths precede Unknown/Any propagation and
        // preserve the exact carrier. Both operands already compiled/executed.
        if op == OpAnd && backend.value_type(&left).is_always_falsy() {
            backend.checkpoint()?;
            return Ok(PrimitiveBinaryProjection::Complete(left));
        }
        if op == OpOr && backend.value_type(&left).is_always_truthy() {
            backend.checkpoint()?;
            return Ok(PrimitiveBinaryProjection::Complete(left));
        }
        if matches!(backend.value_type(&left), LuaType::Unknown | LuaType::Any)
            || matches!(backend.value_type(&right), LuaType::Unknown | LuaType::Any)
        {
            backend.checkpoint()?;
            return Ok(PrimitiveBinaryProjection::Complete(
                backend.plain(LuaType::Unknown),
            ));
        }
        let model = self.analysis().semantic_model(file_id);
        if let Some(name) = operator_name(op) {
            backend.checkpoint()?;
            let overload = model.operator_type(name, backend.value_type(&left));
            backend.checkpoint()?;
            if let Some(ty) = overload {
                return Ok(PrimitiveBinaryProjection::Complete(backend.plain(ty)));
            }
            let overload = model.operator_type(name, backend.value_type(&right));
            backend.checkpoint()?;
            if let Some(ty) = overload {
                return Ok(PrimitiveBinaryProjection::Complete(backend.plain(ty)));
            }
        }
        if matches!(op, OpAnd | OpOr) {
            // Even a truthy `left and right` cannot simply return RHS yet:
            // the fixed path first evaluates dense table fields into a tuple.
            backend.checkpoint()?;
            return Ok(PrimitiveBinaryProjection::DeferredLogical { op, left, right });
        }
        let left_type = backend.value_type(&left);
        let right_type = backend.value_type(&right);
        let ty = match op {
            OpConcat => LuaType::String,
            OpLt | OpLe | OpGt | OpGe | OpEq | OpNe => {
                match (integer_constant(left_type), integer_constant(right_type)) {
                    (Some(left), Some(right)) => LuaType::BooleanConst(match op {
                        OpLt => left < right,
                        OpLe => left <= right,
                        OpGt => left > right,
                        OpGe => left >= right,
                        OpEq => left == right,
                        OpNe => left != right,
                        _ => unreachable!("comparison branch"),
                    }),
                    _ => LuaType::Boolean,
                }
            }
            OpAdd | OpSub | OpMul => {
                match (integer_constant(left_type), integer_constant(right_type)) {
                    (Some(left), Some(right)) => {
                        let value = match op {
                            OpAdd => left.checked_add(right),
                            OpSub => left.checked_sub(right),
                            OpMul => left.checked_mul(right),
                            _ => unreachable!("arithmetic branch"),
                        };
                        value.map(LuaType::IntegerConst).unwrap_or(LuaType::Number)
                    }
                    _ if is_integer(left_type) && is_integer(right_type) => LuaType::Integer,
                    _ => LuaType::Number,
                }
            }
            OpDiv | OpIDiv | OpMod | OpPow => LuaType::Number,
            // The fixed implementation does not fold bitwise/shift operators.
            _ => LuaType::Unknown,
        };
        backend.checkpoint()?;
        Ok(PrimitiveBinaryProjection::Complete(backend.plain(ty)))
    }
}

fn integer_constant(ty: &LuaType) -> Option<i64> {
    match ty {
        LuaType::IntegerConst(value) | LuaType::DocIntegerConst(value) => Some(*value),
        _ => None,
    }
}

fn is_integer(ty: &LuaType) -> bool {
    matches!(
        ty,
        LuaType::Integer | LuaType::IntegerConst(_) | LuaType::DocIntegerConst(_)
    )
}

fn operator_name(op: BinaryOperator) -> Option<&'static str> {
    use BinaryOperator::*;
    Some(match op {
        OpAdd => "add",
        OpSub => "sub",
        OpMul => "mul",
        OpDiv => "div",
        OpMod => "mod",
        OpPow => "pow",
        OpConcat => "concat",
        OpEq => "eq",
        OpLt => "lt",
        OpLe => "le",
        _ => return None,
    })
}
