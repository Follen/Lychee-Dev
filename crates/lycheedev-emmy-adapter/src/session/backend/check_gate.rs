//! Selected call gates and completion at the original producer positions.
//! Adapted from fixed Emmy binding.rs; Copyright (c) CppCXY, MIT.
use super::*;
use crate::session::{BindingCoverage, BindingDecision};
use emmylua_code_analysis::LuaTypeNode;
use emmylua_parser::LuaCallExpr;

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

    pub(super) fn record_check(
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
                if !crate::session::query::call_permit_matches(self.site(syntax), call, &permit)
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
        // A known function shape is callable even when its parameter or return
        // value types are unavailable. Those belong to their own checkers.
        let ready = matches!(
            value.structural_type(),
            LuaType::DocFunction(_) | LuaType::Function | LuaType::Signature(_)
        ) || !self.binding_type_has_unknown(&value)?;
        self.record_check(
            syntax,
            operation,
            if ready {
                "native_checked"
            } else {
                "unavailable"
            },
            permit.proof,
            (!ready).then(|| "native_check_inputs_or_signature_unavailable".into()),
        )
    }

    /// Actual cardinality is independent of declared type knowledge. None
    /// retains the contract of providers that do not manage call observations.
    pub(super) fn binding_call_return_arity(
        &mut self,
        call: &LuaCallExpr,
    ) -> QueryResult<Option<super::return_pack::ReturnArity>, E> {
        use super::return_pack::ReturnArity;
        self.checkpoint()?;
        let observation = self
            .session
            .context()
            .call_observation(self.site(call.get_syntax_id()));
        self.checkpoint()?;
        let open = ReturnArity { min: 0, max: None };
        let permit = self.session.context().call(self.site(call.get_syntax_id()));
        self.checkpoint()?;
        let permit = match permit {
            BindingDecision::Bound(permit)
                if crate::session::query::call_permit_matches(
                    self.site(call.get_syntax_id()),
                    call,
                    &permit,
                ) =>
            {
                permit
            }
            BindingDecision::NotManaged if matches!(observation, BindingDecision::NotManaged) => {
                return Ok(None);
            }
            _ => return Ok(Some(open)),
        };
        Ok(match observation {
            BindingDecision::Bound(observation) => {
                let selected = self.session.context().resolve(permit.callee);
                self.checkpoint()?;
                if !crate::session::query::call_observation_matches(&selected, &observation) {
                    return Ok(Some(open));
                }
                if !observation.open_tail
                    && matches!(&observation.callee,
                    crate::session::ObservedCallee::Environment(callable)
                    if callable.declaration.raw.get("MayReturnNothing") == Some(&serde_json::Value::Bool(true)))
                {
                    Some(open)
                } else if observation.open_tail {
                    Some(ReturnArity {
                        min: observation.returns.len(),
                        max: None,
                    })
                } else {
                    Some(ReturnArity::fixed(observation.returns.len()))
                }
            }
            BindingDecision::Unavailable { .. } | BindingDecision::KnownAbsent { .. } => Some(open),
            BindingDecision::NotManaged => None,
        })
    }

    pub(super) fn binding_type_has_unknown(&mut self, value: &OwnedValue) -> QueryResult<bool, E> {
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
