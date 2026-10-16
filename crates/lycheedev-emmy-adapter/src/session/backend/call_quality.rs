//! Result and return-check quality are separate from argument-check quality.
//!
//! The public `native_projected` / `native_checked` states describe completion
//! quality; `DiagnosticRun::execution` separately identifies actual owned checker
//! execution. In particular, an unknown
//! argument cannot erase a known fixed signature return, and a permit alone is
//! never a completed projection/check. All records use the one session ledger.
use super::{OwnedQuery, QueryResult};
use crate::session::{
    BindingDecision, BindingSite, CallPermit, OwnedValue, SelectedProjectionBackend,
    SelectedProjector, SelectedValue,
};
use emmylua_parser::{LuaAstNode, LuaCallExpr, LuaExpr};

impl<E> OwnedQuery<'_, '_, E> {
    pub(super) fn infer_authenticated_signature_call(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        permit: &CallPermit,
    ) -> QueryResult<Option<OwnedValue>, E> {
        self.checkpoint()?;
        // The projector has checked the AST callee and capability profile. A
        // missing observation does not authenticate a missing selected callee.
        let selected = self.session.context().resolve(permit.callee);
        self.checkpoint()?;
        let declared_signature = match selected {
            BindingDecision::Bound(bound) => match bound.value {
                SelectedValue::Function(function) => self
                    .analysis
                    .semantic_model(function.file_id)
                    .file_facts()
                    .and_then(|facts| facts.signature_by_closure(function.closure))
                    .and_then(|signature| signature.docs.as_ref())
                    .is_some_and(|docs| {
                        !docs.returns.is_empty() || !docs.return_overloads.is_empty()
                    }),
                SelectedValue::EnvironmentCallable(_) => true,
                _ => false,
            },
            _ => false,
        };
        self.checkpoint()?;
        // No runtime observation is not permission to recover an arbitrary
        // inferred body value. Only an actual declared return contract can
        // supply independent type knowledge; unannotated calls stay Unknown.
        if !declared_signature {
            return Ok(None);
        }
        let value = projector.evaluate_permitted_call(site, self)?;
        self.checkpoint()?;
        Ok(Some(value))
    }

    pub(super) fn record_call_result_quality(
        &mut self,
        site: BindingSite,
        value: &OwnedValue,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let decision = self.session.context().call(site);
        self.checkpoint()?;
        let (proof, reason) = match decision {
            BindingDecision::NotManaged => return Ok(()),
            BindingDecision::Bound(permit) => {
                let reason = self
                    .binding_type_has_unknown(value)?
                    .then(|| "native_call_result_type_unavailable".to_owned());
                (permit.proof, reason)
            }
            BindingDecision::KnownAbsent { proof } => {
                (proof, Some("known_absent_callee".to_owned()))
            }
            BindingDecision::Unavailable { proof, reason } => (proof, Some(reason)),
        };
        self.record_check(
            site.syntax,
            "call_result",
            if reason.is_none() {
                "native_projected"
            } else {
                "unavailable"
            },
            proof,
            reason,
        )
    }

    /// Called only after the return checker actually evaluated the two types.
    /// `compared` is false for its Unknown/Any/Never early exits; those must not
    /// acquire a successful receipt merely because a call permit exists.
    pub(super) fn finish_return_quality(
        &mut self,
        expression: &LuaExpr,
        source: &OwnedValue,
        target: &OwnedValue,
        compared: bool,
    ) -> QueryResult<(), E> {
        let LuaExpr::CallExpr(call) = expression else {
            return Ok(());
        };
        self.finish_call_return_quality(call, source, target, compared)
    }

    fn finish_call_return_quality(
        &mut self,
        call: &LuaCallExpr,
        source: &OwnedValue,
        target: &OwnedValue,
        compared: bool,
    ) -> QueryResult<(), E> {
        self.checkpoint()?;
        let site = self.site(call.get_syntax_id());
        let decision = self.session.context().call(site);
        self.checkpoint()?;
        let BindingDecision::Bound(permit) = decision else {
            // Preserve the original provider's denial and proof through the
            // same call gate; an unmanaged expression emits no selected claim.
            if !matches!(decision, BindingDecision::NotManaged) {
                self.binding_call_allowed(call, "return_check")?;
            }
            return Ok(());
        };
        if !self.binding_call_allowed(call, "return_check")? {
            return Ok(());
        }
        let ready = compared
            && !self.binding_type_has_unknown(source)?
            && !self.binding_type_has_unknown(target)?
            && self.structure_allowed(source)?
            && self.structure_allowed(target)?;
        self.record_check(
            site.syntax,
            "return_check",
            if ready {
                "native_checked"
            } else {
                "unavailable"
            },
            permit.proof,
            (!ready).then(|| "native_return_check_inputs_unavailable".to_owned()),
        )
    }
}
