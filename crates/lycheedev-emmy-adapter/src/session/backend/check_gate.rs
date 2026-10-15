//! Selected call gates and completion at the original producer positions.
//! Adapted from fixed Emmy binding.rs; Copyright (c) CppCXY, MIT.
use super::*;
use crate::session::{BindingCoverage, BindingDecision};
use emmylua_code_analysis::LuaTypeNode;
use emmylua_parser::{LuaCallExpr, LuaExpr};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn binding_manages(&mut self, syntax: LuaSyntaxId) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let result = !matches!(
            self.session.context().resolve(self.site(syntax)),
            BindingDecision::NotManaged
        );
        self.checkpoint()?;
        Ok(result)
    }

    pub(super) fn binding_manages_call(&mut self, syntax: LuaSyntaxId) -> QueryResult<bool, E> {
        self.checkpoint()?;
        if !matches!(
            self.session.context().call(self.site(syntax)),
            BindingDecision::NotManaged
        ) {
            return Ok(true);
        }
        self.binding_manages(syntax)
    }

    pub(super) fn binding_type_of_expr(
        &mut self,
        syntax: LuaSyntaxId,
    ) -> QueryResult<Option<OwnedValue>, E> {
        SelectedProjector::new(self.analysis, self.session)
            .project_selected(self.site(syntax), self)
    }

    fn record_check(
        &mut self,
        syntax: LuaSyntaxId,
        operation: &'static str,
        state: &'static str,
        proof: String,
        reason: Option<String>,
    ) -> QueryResult<(), E> {
        self.session.record(
            BindingCoverage {
                site: self.site(syntax),
                operation,
                state,
                proof,
                reason,
                parameter: None,
                parameter_index: None,
                argument: None,
            },
            || self.checkpoint(),
        )
    }

    pub(super) fn binding_call_allowed(
        &mut self,
        call: &LuaCallExpr,
        operation: &'static str,
    ) -> QueryResult<bool, E> {
        self.checkpoint()?;
        let syntax = call.get_syntax_id();
        match self.session.context().call(self.site(syntax)) {
            BindingDecision::NotManaged => Ok(true),
            BindingDecision::Bound(permit) => {
                let actual = call
                    .get_prefix_expr()
                    .map(|expr| self.site(expr.get_syntax_id()));
                if actual != Some(permit.callee)
                    || !matches!(
                        permit.capability_profile.as_str(),
                        "typed-call-v1"
                            | "authenticated_environment_signature_v1"
                            | "fixture_intrinsic_value"
                    )
                    || !matches!(
                        self.session.context().resolve(permit.callee),
                        BindingDecision::Bound(_)
                    )
                {
                    self.record_check(
                        syntax,
                        operation,
                        "unavailable",
                        permit.proof,
                        Some("call_permit_or_selected_callee_unavailable".into()),
                    )?;
                    return Ok(false);
                }
                if operation == "argument_check"
                    && let Some(last) = call.get_args_list().and_then(|args| args.get_args().last())
                    && self
                        .type_of_expr(last.get_syntax_id())?
                        .structural_type()
                        .contain_multi_return()
                {
                    self.record_check(
                        syntax,
                        operation,
                        "unavailable",
                        permit.proof,
                        Some("native_multi_argument_projection_unavailable".into()),
                    )?;
                    return Ok(false);
                }
                self.record_check(syntax, "call_permit", "bound", permit.proof, None)?;
                Ok(true)
            }
            BindingDecision::KnownAbsent { proof } => {
                self.record_check(
                    syntax,
                    operation,
                    "unavailable",
                    proof,
                    Some("known_absent_callee".into()),
                )?;
                Ok(false)
            }
            BindingDecision::Unavailable { proof, reason } => {
                self.record_check(syntax, operation, "unavailable", proof, Some(reason))?;
                Ok(false)
            }
        }
    }

    pub(super) fn binding_finish_call_check(
        &mut self,
        call: &LuaCallExpr,
        operation: &'static str,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let syntax = call.get_syntax_id();
        let BindingDecision::Bound(permit) = self.session.context().call(self.site(syntax)) else {
            return Ok(());
        };
        if !self.binding_call_allowed(call, operation)? {
            return Ok(());
        }
        let Some(callee) = call.get_prefix_expr() else {
            return Ok(());
        };
        let value = self.type_of_expr(callee.get_syntax_id())?;
        let mut ready = !self.binding_type_has_unknown(&value)?;
        if operation != "callable_check" {
            // &= is intentional: callable collection still executes when the
            // initial unknown scan was false, preserving coverage producer order.
            ready &= !self.callable_functions_cached(&value)?.is_empty();
            let arguments: Vec<_> = call
                .get_args_list()
                .map(|args| args.get_args().collect())
                .unwrap_or_default();
            if operation == "argument_check" {
                let mut all_known = true;
                for argument in &arguments {
                    let value = self.type_of_expr(argument.get_syntax_id())?;
                    if self.binding_type_has_unknown(&value)? {
                        all_known = false;
                        break;
                    }
                }
                ready &= all_known;
            } else if let Some(last) = arguments.last() {
                let value = self.type_of_expr(last.get_syntax_id())?;
                ready &= match value.structural_type() {
                    LuaType::Variadic(values) => values.get_max_len().is_some(),
                    LuaType::Unknown | LuaType::Any if matches!(last, LuaExpr::CallExpr(_)) => {
                        false
                    }
                    _ => true,
                };
            }
        }
        self.record_check(
            syntax,
            operation,
            if ready {
                "owned_checked"
            } else {
                "unavailable"
            },
            permit.proof,
            (!ready).then(|| "native_check_inputs_or_signature_unavailable".into()),
        )
    }

    fn binding_type_has_unknown(&mut self, value: &OwnedValue) -> QueryResult<bool, E> {
        let mut stack = vec![value.structural_type()];
        let mut count = 0;
        while let Some(value) = stack.pop() {
            self.checkpoint()?;
            count += 1;
            if count > 4096 || matches!(value, LuaType::Unknown | LuaType::Any) {
                return Ok(true);
            }
            value.push_direct_children(&mut stack);
        }
        Ok(false)
    }
}
