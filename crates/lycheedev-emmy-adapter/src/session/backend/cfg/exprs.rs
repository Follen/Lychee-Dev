// Adapted from EmmyLua 08b0873962e83bb8307e9a81052e3f41578fbc78 (MIT).
// Owned per-query CFG; public AST/facts only, no upstream private types.
use emmylua_parser::LuaExpr;

use super::FlowId;
use super::binder::FlowBinder;
use super::engine::run_bind_expr;

pub use super::bind_binary_expr::is_binary_logical;

/// Bind an expression (explicit task stack engine; the result always equals the input current)
pub fn bind_expr(binder: &mut FlowBinder, expr: LuaExpr, current: FlowId) -> FlowId {
    run_bind_expr(binder, expr, current)
}
