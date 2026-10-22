//! Selected expression entry, observed calls and Lua result-pack adjustment.
//!
//! Only a genuine NotManaged result permits the enclosing owned semantic model
//! to continue inference. Unknown or denied evidence never takes that path.
use super::{
    BindingCoverage, BindingDecision, BindingSite, ObservedCallee, SelectedProjectionBackend,
    SelectedProjector, SelectedValue,
};
use emmylua_code_analysis::{LuaType, VariadicType};
use emmylua_parser::{
    LuaAssignStat, LuaAstNode, LuaCallArgList, LuaCallExpr, LuaExpr, LuaForRangeStat, LuaLocalStat,
    LuaReturnStat, LuaTableExpr, LuaTableField,
};
use std::sync::Arc;

/// Result-pack operations belong to the same owned carrier as recursive
/// projection. Implementations must preserve each slot's object provenance and
/// session token. A zero-slot pack becomes Nil only in a scalar position.
pub trait SelectedExpressionBackend: SelectedProjectionBackend {
    fn result_pack(&mut self, slots: Vec<Self::Value>) -> Self::Value;
    fn scalar_result(&mut self, value: Self::Value) -> Self::Value;

    /// A validated typed permit can authorize static signature inference even
    /// when there is no observed runtime return. Primitive projector backends
    /// have no call executor and therefore retain the unavailable result.
    fn infer_permitted_call(
        &mut self,
        _projector: &SelectedProjector<'_>,
        _site: BindingSite,
        _permit: &super::CallPermit,
    ) -> Result<Option<Self::Value>, Self::Error> {
        Ok(None)
    }

    /// The complete backend records quality after a validated permit's return
    /// projection attempt. A rejected call gate never enters this phase;
    /// argument-check availability is deliberately independent.
    fn finish_call_projection(
        &mut self,
        _site: BindingSite,
        _value: &Self::Value,
        _permit: &super::CallPermit,
    ) -> Result<(), Self::Error> {
        Ok(())
    }
}

impl SelectedProjector<'_> {
    /// Complete selected overlay entry, not ordinary expression inference. The
    /// same session's RAII active guard protects cross-file recursive backends;
    /// interruption releases it without resetting the caller's deadline.
    pub fn project_selected<B: SelectedExpressionBackend>(
        &self,
        site: BindingSite,
        backend: &mut B,
    ) -> Result<Option<B::Value>, B::Error> {
        let Some(_guard) = self.session().enter(site, || backend.checkpoint())? else {
            self.query_record(
                site,
                "resolve",
                "unavailable",
                String::new(),
                Some("binding_projection_cycle".into()),
                backend,
            )?;
            return Ok(Some(backend.plain(LuaType::Unknown)));
        };
        backend.checkpoint()?;
        let model = self.analysis().semantic_model(site.file_id);
        let call = model
            .syntax_tree()
            .and_then(|tree| site.syntax.to_node_from_root(&tree.get_red_root()))
            .and_then(LuaCallExpr::cast);
        backend.checkpoint()?;
        // Keep the original call-gate observation through projection. The
        // validated Bound branch alone owns result completion; rereading the
        // provider later would add an observation without semantic work.
        let call_decision = if call.is_some() {
            backend.checkpoint()?;
            let decision = self.session().context().call(site);
            backend.checkpoint()?;
            Some(decision)
        } else {
            None
        };
        let value = self.selected_inner(site, call.as_ref(), call_decision.as_ref(), backend)?;
        Ok(match (value, call) {
            (Some(value), Some(call)) if !call_expands(&call) => {
                backend.checkpoint()?;
                Some(backend.scalar_result(value))
            }
            (value, _) => value,
        })
    }

    fn selected_inner<B: SelectedExpressionBackend>(
        &self,
        site: BindingSite,
        call: Option<&LuaCallExpr>,
        call_decision: Option<&BindingDecision<super::CallPermit>>,
        backend: &mut B,
    ) -> Result<Option<B::Value>, B::Error> {
        if let Some(call) = call {
            match call_decision
                .expect("call gate observed before projection")
                .clone()
            {
                BindingDecision::KnownAbsent { proof } => {
                    self.query_record(site, "call", "known_absent", proof, None, backend)?;
                    return Ok(Some(backend.plain(LuaType::Unknown)));
                }
                BindingDecision::Unavailable { proof, reason } => {
                    self.query_record(site, "call", "unavailable", proof, Some(reason), backend)?;
                    return Ok(Some(backend.plain(LuaType::Unknown)));
                }
                BindingDecision::Bound(permit) => {
                    if !call_permit_matches(site, call, &permit) {
                        self.query_record(
                            site,
                            "call",
                            "unavailable",
                            permit.proof,
                            Some("call_permit_callee_or_profile_mismatch".into()),
                            backend,
                        )?;
                        return Ok(Some(backend.plain(LuaType::Unknown)));
                    }
                    let value = self.permitted_call_inner(site, &permit, backend)?;
                    if let Some(value) = &value {
                        backend.finish_call_projection(site, value, &permit)?;
                    }
                    return Ok(value);
                }
                BindingDecision::NotManaged => {}
            }
        }
        backend.checkpoint()?;
        let decision = self.session().context().resolve(site);
        backend.checkpoint()?;
        self.project_decision(site, decision, "resolve", backend)
    }

    // Admission has already checked the actual callee and capability profile.
    // Every return below belongs to a real projection attempt, including an
    // unavailable observation. Gate denial must never synthesize its completion.
    fn permitted_call_inner<B: SelectedExpressionBackend>(
        &self,
        site: BindingSite,
        permit: &super::CallPermit,
        backend: &mut B,
    ) -> Result<Option<B::Value>, B::Error> {
        backend.checkpoint()?;
        let observation = self.session().context().call_observation(site);
        backend.checkpoint()?;
        match observation {
            BindingDecision::Bound(observation) => {
                let selected = self.session().context().resolve(permit.callee);
                backend.checkpoint()?;
                if !call_observation_matches(&selected, &observation) {
                    self.query_record(
                        site,
                        "call_return",
                        "unavailable",
                        observation.proof,
                        Some("call_observation_identity_or_budget".into()),
                        backend,
                    )?;
                    return Ok(Some(backend.plain(LuaType::Unknown)));
                }
                if !observation.open_tail
                    && matches!(&observation.callee, ObservedCallee::Environment(callable)
                    if callable.declaration.raw.get("MayReturnNothing") == Some(&serde_json::Value::Bool(true)))
                {
                    self.query_record(
                        site,
                        "call_return",
                        "unavailable",
                        observation.proof,
                        Some("environment_return_cardinality_unavailable".into()),
                        backend,
                    )?;
                    return Ok(Some(backend.plain(LuaType::Variadic(Arc::new(
                        VariadicType::Base(LuaType::Unknown),
                    )))));
                }
                if observation.open_tail {
                    self.query_record(
                        site,
                        "call_return",
                        "unavailable",
                        observation.proof.clone(),
                        Some("host_sequence_requires_cardinality_projection".into()),
                        backend,
                    )?;
                    // A real prefix takes precedence over a declaration. With
                    // no observed prefix only explicit signature type knowledge
                    // can be projected; it still proves no actual slot count.
                    if observation.returns.is_empty()
                        && let Some(value) = backend.infer_permitted_call(self, site, permit)?
                    {
                        return Ok(Some(value));
                    }
                }
                let mut slots = Vec::with_capacity(observation.returns.len());
                for decision in observation.returns {
                    backend.checkpoint()?;
                    let value = self.project_decision(site, decision, "call_return", backend)?;
                    slots.push(match value {
                        Some(value) => value,
                        None => {
                            self.query_record(
                                site,
                                "call_return",
                                "unavailable",
                                String::new(),
                                Some("observed_return_slot_not_managed".into()),
                                backend,
                            )?;
                            backend.plain(LuaType::Unknown)
                        }
                    });
                }
                if observation.open_tail {
                    slots.push(backend.plain(LuaType::Variadic(Arc::new(VariadicType::Base(
                        LuaType::Unknown,
                    )))));
                }
                self.query_record(site, "call", "bound", permit.proof.clone(), None, backend)?;
                return Ok(Some(backend.result_pack(slots)));
            }
            BindingDecision::Unavailable { proof, reason } => {
                let declared_return = matches!(
                    reason.as_str(),
                    "host_sequence_requires_cardinality_projection"
                        | "conditional_return_path_candidates_require_path_projection"
                        | "native_call_return_observation_missing"
                );
                self.query_record(
                    site,
                    "call_return",
                    "unavailable",
                    proof,
                    Some(reason),
                    backend,
                )?;
                // Actual return shape remains unavailable above. An explicit
                // declaration can still supply type knowledge, never an actual
                // body value or a finite observed return count.
                if declared_return
                    && let Some(value) = backend.infer_permitted_call(self, site, permit)?
                {
                    return Ok(Some(value));
                }
                return Ok(Some(backend.plain(LuaType::Unknown)));
            }
            BindingDecision::KnownAbsent { proof } => {
                self.query_record(
                    site,
                    "call_return",
                    "unavailable",
                    proof,
                    Some("call_return_observation_absent".into()),
                    backend,
                )?;
                return Ok(Some(backend.plain(LuaType::Unknown)));
            }
            BindingDecision::NotManaged => {
                let decision = self.session().context().resolve(site);
                backend.checkpoint()?;
                if let Some(value) =
                    self.project_decision(site, decision, "call_return", backend)?
                {
                    return Ok(Some(value));
                }
                // Absence of an observation is not a denial of the
                // authenticated native signature. Execute the same
                // owned VM and selected children, never a raw model
                // fallback or a synthetic result from the permit.
                if let Some(value) = backend.infer_permitted_call(self, site, permit)? {
                    return Ok(Some(value));
                }
                self.query_record(
                    site,
                    "call_return",
                    "unavailable",
                    permit.proof.clone(),
                    Some("call_return_observation_missing".into()),
                    backend,
                )?;
                return Ok(Some(backend.plain(LuaType::Unknown)));
            }
        }
    }

    fn query_record<B: SelectedProjectionBackend>(
        &self,
        site: BindingSite,
        operation: &'static str,
        state: &'static str,
        proof: String,
        reason: Option<String>,
        backend: &mut B,
    ) -> Result<(), B::Error> {
        self.session().record(
            BindingCoverage {
                site,
                operation,
                state,
                proof,
                reason,
                parameter: None,
                parameter_index: None,
                argument: None,
            },
            || backend.checkpoint(),
        )
    }
}

/// Preserve the fixed selected overlay's Lua list expansion rules. In
/// particular, parentheses do not expand, and equal-count assignments scalarize.
fn call_expands(call: &LuaCallExpr) -> bool {
    let Some(parent) = call.syntax().parent() else {
        return false;
    };
    let is_call = |expr: &LuaExpr| expr.get_syntax_id() == call.get_syntax_id();
    if let Some(args) = LuaCallArgList::cast(parent.clone()) {
        return args.get_args().last().as_ref().is_some_and(is_call);
    }
    if let Some(stat) = LuaReturnStat::cast(parent.clone()) {
        return stat.get_expr_list().last().as_ref().is_some_and(is_call);
    }
    if let Some(stat) = LuaLocalStat::cast(parent.clone()) {
        let values = stat.get_value_exprs().collect::<Vec<_>>();
        return stat.get_local_name_list().count() > values.len()
            && values.last().is_some_and(is_call);
    }
    if let Some(stat) = LuaAssignStat::cast(parent.clone()) {
        let (vars, values) = stat.get_var_and_expr_list();
        return vars.len() > values.len() && values.last().is_some_and(is_call);
    }
    if let Some(stat) = LuaForRangeStat::cast(parent.clone()) {
        return stat.get_expr_list().last().as_ref().is_some_and(is_call);
    }
    if let Some(field) = LuaTableField::cast(parent) {
        return field.is_value_field()
            && field
                .get_parent::<LuaTableExpr>()
                .and_then(|table| table.get_fields().last())
                .is_some_and(|last| last.get_syntax_id() == field.get_syntax_id());
    }
    false
}

// The same selected evidence boundary is used by value projection and count
// consumers. A rejected observation must never become an exact finite count.
pub(super) fn call_permit_matches(
    site: BindingSite,
    call: &LuaCallExpr,
    permit: &super::CallPermit,
) -> bool {
    let actual = call.get_prefix_expr().map(|expr| BindingSite {
        file_id: site.file_id,
        syntax: expr.get_syntax_id(),
    });
    actual == Some(permit.callee)
        && matches!(
            permit.capability_profile.as_str(),
            "typed-call-v1" | "authenticated_environment_signature_v1" | "fixture_intrinsic_value"
        )
}

pub(super) fn call_observation_matches(
    selected: &BindingDecision<super::BoundValue>,
    observation: &super::CallObservation,
) -> bool {
    let valid = match selected {
        BindingDecision::Bound(value) if value.identity == observation.selected_callee_identity => {
            match (&value.value, &observation.callee) {
                (SelectedValue::Function(a), ObservedCallee::Source(b)) => a == b,
                (SelectedValue::EnvironmentCallable(a), ObservedCallee::Environment(b)) => {
                    a.origin.identity == b.origin.identity
                        && a.origin.material_sha256 == b.origin.material_sha256
                        && a.origin.pointer == b.origin.pointer
                }
                _ => false,
            }
        }
        _ => false,
    };
    valid
        && observation.returns.len() <= 32
        && !observation.invocation_identity.is_empty()
        && (!matches!(&observation.callee, ObservedCallee::Source(_))
            || !observation.closure_identity.is_empty())
}
