//! Actual direct-call successor for builtin-free zero-argument DocFunctions.
//! This is a semantic subset, not a fixture-name/empty-body fast path. Remaining
//! overload/generic/receiver/argument work stays on the same owned backend.
use super::{ExpressionValue, OwnedQuery, OwnedQueryCapability, QueryResult};
use crate::session::{OwnedValue, SelectedProjectionBackend};
use emmylua_code_analysis::{LuaType, VariadicType};
use emmylua_parser::LuaSyntaxId;

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn call_plain_signature(
        &mut self,
        callee: ExpressionValue<OwnedValue>,
        arguments: Vec<ExpressionValue<OwnedValue>>,
        colon_call: bool,
        generic_type_syntaxes: &[LuaSyntaxId],
    ) -> QueryResult<ExpressionValue<OwnedValue>, E> {
        self.checkpoint()?;
        // The fixed call VM takes this branch before builtin/signature lookup.
        if matches!(callee.value.structural_type(), LuaType::Any) {
            return Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Any)));
        }
        // An ownerless primitive has no callable candidate, builtin/member
        // lookup, alias or metatable to inspect. Keep Nil/Unknown as actual
        // non-callable inputs; this expression result does not claim a checker
        // ran or suppress the later call-non-callable diagnostic.
        if callee.owner.is_none()
            && callee.receiver.is_none()
            && callee.closure_syntax.is_none()
            && plain_atom(callee.value.structural_type())
        {
            return Ok(ExpressionValue::plain(OwnedValue::plain(LuaType::Unknown)));
        }
        let LuaType::DocFunction(function) = callee.value.structural_type() else {
            return self.continuation(OwnedQueryCapability::Call, None, None, None);
        };
        if !callee.value.is_context_free()
            || callee.owner.is_some()
            || callee.receiver.is_some()
            || callee.closure_syntax.is_some()
            || !arguments.is_empty()
            || colon_call
            || !generic_type_syntaxes.is_empty()
            || !function.get_params().is_empty()
            || !function.get_generic_params().is_empty()
            || function.is_colon_define()
            || function.is_variadic()
        {
            return self.continuation(OwnedQueryCapability::CallSignature, None, None, None);
        }
        // With no owner/params/arguments/receiver/generics, candidate selection
        // chooses this sole function with empty bindings and no type checks.
        // For atoms and packs of atoms the subsequent binding, substitution,
        // alias/conditional, self, sanitization and unpack stages are identities.
        // In particular Tuple status changes and selected object identity are
        // NOT silently passed through this restricted branch.
        let mut pending = vec![function.get_ret()];
        while let Some(value) = pending.pop() {
            self.checkpoint()?;
            match value {
                LuaType::Variadic(values) => match values.as_ref() {
                    VariadicType::Base(value) => pending.push(value),
                    VariadicType::Multi(values) => {
                        for value in values.iter().rev() {
                            self.checkpoint()?;
                            pending.push(value);
                        }
                    }
                },
                value if plain_atom(value) => {}
                _ => {
                    return self.continuation(
                        OwnedQueryCapability::CallReturnType,
                        None,
                        None,
                        None,
                    );
                }
            }
        }
        self.checkpoint()?;
        Ok(ExpressionValue::plain(OwnedValue::plain(
            function.get_ret().clone(),
        )))
    }
}

pub(super) fn plain_atom(value: &LuaType) -> bool {
    matches!(
        value,
        LuaType::Unknown
            | LuaType::Any
            | LuaType::Nil
            | LuaType::Table
            | LuaType::Userdata
            | LuaType::Function
            | LuaType::Thread
            | LuaType::Boolean
            | LuaType::String
            | LuaType::Integer
            | LuaType::Number
            | LuaType::Io
            | LuaType::Never
            | LuaType::BooleanConst(_)
            | LuaType::StringConst(_)
            | LuaType::IntegerConst(_)
            | LuaType::FloatConst(_)
            | LuaType::DocStringConst(_)
            | LuaType::DocIntegerConst(_)
            | LuaType::DocBooleanConst(_)
    )
}
