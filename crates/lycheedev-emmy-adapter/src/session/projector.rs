//! Selected-decision dispatch and the three atomic value projections.
//!
//! This is one part of the owned semantic query implementation, not a replacement
//! for type_of_expr. Context-recursive projections remain mandatory backend
//! operations. In particular, they must not fall back to raw inference when the
//! selected value is unavailable, or discard object provenance/session tokens.
use super::{
    BindingCoverage, BindingDecision, BindingSite, BoundValue, DerivedNumberRule,
    EnvironmentRecord, EnvironmentSlot, EnvironmentTypeCatalog, FunctionRef, ObjectRef,
    OwnedBindingSession, OwnedValue, SelectedValue,
};
use emmylua_code_analysis::{
    AsyncState, EmmyLuaAnalysis, LuaFunctionType, LuaType, SemanticId, VariadicType,
};
use emmylua_parser::{LuaAstNode, LuaForStat};
use std::sync::Arc;

/// The outer error is the caller's original cancellation/deadline failure.
/// The inner failure is missing semantic evidence and becomes authoritative
/// Unknown, with its original reason recorded at the producer's execution point.
pub type ProjectionResult<T, E> = Result<Result<T, &'static str>, E>;

/// Required context-recursive operations, supplied by the same owned query
/// implementation. There are no default raw-inference or success implementations.
///
/// `Value` intentionally belongs to the query implementation: a selected object
/// can retain its unforgeable session token, epoch and nested provenance instead
/// of being erased into an ordinary upstream LuaType by this dispatcher.
pub trait SelectedProjectionBackend {
    type Value;
    type Error;

    fn checkpoint(&mut self) -> Result<(), Self::Error>;
    fn plain(&mut self, ty: LuaType) -> Self::Value;

    /// Context-free test backends may inspect ordinary signatures. An owned
    /// backend must explicitly retain nominal children instead of flattening them.
    fn environment_value(
        &mut self,
        value: OwnedValue,
    ) -> ProjectionResult<Self::Value, Self::Error> {
        Ok(match value.into_context_free_type() {
            Ok(ty) => Ok(self.plain(ty)),
            Err(_) => Err("environment_carrier_backend_unavailable"),
        })
    }

    fn expression(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        origin: BindingSite,
    ) -> ProjectionResult<Self::Value, Self::Error>;
    fn parameter(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        parameter: &SemanticId,
    ) -> ProjectionResult<Self::Value, Self::Error>;
    fn function(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        function: &FunctionRef,
    ) -> ProjectionResult<Self::Value, Self::Error>;
    fn object(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
        object: &ObjectRef,
    ) -> ProjectionResult<Self::Value, Self::Error>;
}

/// Borrows the existing analysis and the sole owned evidence/ledger session.
/// No database, provider, source graph, source text or completed ledger is copied.
pub struct SelectedProjector<'a> {
    analysis: &'a EmmyLuaAnalysis,
    session: &'a OwnedBindingSession,
}

impl<'a> SelectedProjector<'a> {
    pub fn new(analysis: &'a EmmyLuaAnalysis, session: &'a OwnedBindingSession) -> Self {
        Self { analysis, session }
    }

    pub fn analysis(&self) -> &'a EmmyLuaAnalysis {
        self.analysis
    }

    pub fn session(&self) -> &'a OwnedBindingSession {
        self.session
    }

    /// This deliberately does not implement expression recursion, call permits,
    /// result-pack adjustment or the expression cycle guard. The enclosing owned
    /// semantic query must perform those operations in their original order.
    pub fn project_decision<B: SelectedProjectionBackend>(
        &self,
        site: BindingSite,
        decision: BindingDecision<BoundValue>,
        operation: &'static str,
        backend: &mut B,
    ) -> Result<Option<B::Value>, B::Error> {
        backend.checkpoint()?;
        let (value, state, proof, reason) = match decision {
            BindingDecision::NotManaged => return Ok(None),
            BindingDecision::KnownAbsent { proof } => {
                (backend.plain(LuaType::Nil), "known_absent", proof, None)
            }
            BindingDecision::Unavailable { proof, reason } => (
                backend.plain(LuaType::Unknown),
                "unavailable",
                proof,
                Some(reason),
            ),
            BindingDecision::Bound(bound) => {
                match self.project_value(site, &bound.value, backend)? {
                    Ok(value) => (value, "bound", bound.proof, None),
                    Err(reason) => (
                        backend.plain(LuaType::Unknown),
                        "unavailable",
                        bound.proof,
                        Some(reason.into()),
                    ),
                }
            }
        };
        // Child projections append to this same ledger before the enclosing
        // result is recorded, preserving producer order and the 16,384th slot.
        self.session.record(
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
        )?;
        Ok(Some(value))
    }

    fn project_value<B: SelectedProjectionBackend>(
        &self,
        site: BindingSite,
        value: &SelectedValue,
        backend: &mut B,
    ) -> ProjectionResult<B::Value, B::Error> {
        backend.checkpoint()?;
        match value {
            SelectedValue::Expression(origin) => backend.expression(self, site, *origin),
            SelectedValue::Parameter(parameter) => backend.parameter(self, site, parameter),
            SelectedValue::Function(function) => backend.function(self, site, function),
            SelectedValue::Object(object) => backend.object(self, site, object),
            SelectedValue::DerivedNumber(number) => {
                if !number.value.is_finite()
                    || number.occurrence_identity.is_empty()
                    || number.proof.is_empty()
                {
                    return Ok(Err("derived_number_evidence_invalid"));
                }
                match &number.rule {
                    DerivedNumberRule::NumericForIteration {
                        statement,
                        header,
                        iteration,
                        predecessor,
                    } => {
                        backend.checkpoint()?;
                        let model = self.analysis.semantic_model(statement.file_id);
                        let statement_valid = model
                            .syntax_tree()
                            .and_then(|tree| {
                                statement.syntax.to_node_from_root(&tree.get_red_root())
                            })
                            .and_then(LuaForStat::cast)
                            .is_some();
                        backend.checkpoint()?;
                        if !statement_valid
                            || !(2..=3).contains(&header.len())
                            || (*iteration == 0) != predecessor.is_none()
                        {
                            return Ok(Err("derived_number_rule_invalid"));
                        }
                    }
                }
                Ok(Ok(backend.plain(LuaType::FloatConst(number.value))))
            }
            SelectedValue::EnvironmentCallable(callable) => {
                if callable.declaration.kind != "function"
                    || callable.origin.identity.is_empty()
                    || callable.origin.material_sha256.len() != 64
                {
                    return Ok(Err("environment_callable_identity_invalid"));
                }
                let result = EnvironmentProjection::new(&callable.types, self.session, &mut || {
                    backend.checkpoint()
                })
                .function(
                    &callable.declaration,
                    callable.receiver_type_name(),
                    0,
                )?;
                match result {
                    Ok(value) => backend.environment_value(value),
                    Err(reason) => Ok(Err(reason)),
                }
            }
            SelectedValue::EnvironmentReturn(result) => {
                if result.invocation_identity.is_empty() || result.slot == 0 {
                    return Ok(Err("environment_return_identity_invalid"));
                }
                let Some(slot) = result.callable.declaration.returns.get(result.slot - 1) else {
                    return Ok(Err("environment_return_slot_unavailable"));
                };
                if slot.index != result.slot
                    || result.callable.origin.identity.is_empty()
                    || result.callable.origin.material_sha256.len() != 64
                {
                    return Ok(Err("environment_return_slot_or_origin_invalid"));
                }
                let result =
                    EnvironmentProjection::new(&result.callable.types, self.session, &mut || {
                        backend.checkpoint()
                    })
                    .slot(slot, 0, false)?;
                match result {
                    Ok(value) => backend.environment_value(value),
                    Err(reason) => Ok(Err(reason)),
                }
            }
        }
    }
}

struct EnvironmentProjection<'a, E> {
    types: &'a EnvironmentTypeCatalog,
    session: &'a OwnedBindingSession,
    check: &'a mut dyn FnMut() -> Result<(), E>,
    work: usize,
}
impl<'a, E> EnvironmentProjection<'a, E> {
    fn new(
        types: &'a EnvironmentTypeCatalog,
        session: &'a OwnedBindingSession,
        check: &'a mut dyn FnMut() -> Result<(), E>,
    ) -> Self {
        Self {
            types,
            session,
            check,
            work: 0,
        }
    }
    fn slot(
        &mut self,
        slot: &EnvironmentSlot,
        depth: usize,
        partial: bool,
    ) -> ProjectionResult<OwnedValue, E> {
        let value = match self.named(&slot.type_name, &slot.inner_type, depth)? {
            Ok(value) => value,
            // A missing nominal declaration is not permission to guess its
            // structure. Keep this slot unknown without losing known siblings.
            Err("environment_type_unavailable") if partial => OwnedValue::plain(LuaType::Unknown),
            Err(reason) => return Ok(Err(reason)),
        };
        Ok(Ok(if slot.nilable {
            OwnedValue::raw_union(vec![value, OwnedValue::plain(LuaType::Nil)])
        } else {
            value
        }))
    }
    fn named(&mut self, name: &str, inner: &str, depth: usize) -> ProjectionResult<OwnedValue, E> {
        (self.check)()?;
        self.work += 1;
        if self.work > 4096 {
            return Ok(Err("environment_projection_budget"));
        }
        if depth >= 32 {
            return Ok(Err("environment_type_depth"));
        }
        if let Some(alias) = self.types.aliases.get(name) {
            return self.named(alias, inner, depth + 1);
        }
        let primitive = match name {
            "number" | "float" | "double" => Some(LuaType::Number),
            "integer" | "int" => Some(LuaType::Integer),
            "string" | "cstring" | "localizedstring" | "cstringOptional" => Some(LuaType::String),
            "bool" | "boolean" => Some(LuaType::Boolean),
            "nil" => Some(LuaType::Nil),
            "any" => Some(LuaType::Any),
            "unknown" => Some(LuaType::Unknown),
            "thread" => Some(LuaType::Thread),
            "userdata" => Some(LuaType::Userdata),
            "function" | "LuaFunction" => Some(LuaType::Function),
            "table" if inner.is_empty() => Some(LuaType::Table),
            _ => None,
        };
        if let Some(ty) = primitive {
            return Ok(Ok(OwnedValue::plain(ty)));
        }
        if !inner.is_empty() {
            return Ok(Err("environment_type_unavailable"));
        }
        if self.types.nominal_objects.contains_key(name) {
            if self.types.identity.is_empty() {
                return Ok(Err("environment_declaration_identity_invalid"));
            }
            return Ok(Ok(OwnedValue::nominal(
                self.session,
                &self.types.identity,
                name,
            )));
        }
        let types = self.types;
        let Some(records) = types.declarations.get(name) else {
            return Ok(Err("environment_type_unavailable"));
        };
        let [record] = records.as_slice() else {
            return Ok(Err("environment_declaration_ambiguous"));
        };
        if record.name != name || types.identity.is_empty() {
            return Ok(Err("environment_declaration_identity_invalid"));
        }
        if !matches!(record.kind.as_str(), "callback" | "callbacktype") {
            return Ok(Err("environment_type_unavailable"));
        }
        self.function(record, None, depth + 1)
    }
    fn function(
        &mut self,
        record: &EnvironmentRecord,
        receiver: Option<&str>,
        depth: usize,
    ) -> ProjectionResult<OwnedValue, E> {
        (self.check)()?;
        if depth >= 32 {
            return Ok(Err("environment_type_depth"));
        }
        if record.owner.is_some() && receiver.is_none() {
            return Ok(Err("environment_receiver_unavailable"));
        }
        if record.parameters.len() > 32 || record.returns.len() > 32 {
            return Ok(Err("environment_signature_budget"));
        }
        let mut names = Vec::new();
        let mut params = Vec::new();
        if let Some(receiver) = receiver {
            if self.types.identity.is_empty() {
                return Ok(Err("environment_declaration_identity_invalid"));
            }
            names.push(("self".into(), None));
            params.push(Some(OwnedValue::nominal(
                self.session,
                &self.types.identity,
                receiver,
            )));
        }
        for (index, slot) in record.parameters.iter().enumerate() {
            (self.check)()?;
            if slot.index != index + 1 {
                return Ok(Err("environment_parameter_index_invalid"));
            }
            let value = match self.slot(slot, depth, true)? {
                Ok(value) => value,
                Err(reason) => return Ok(Err(reason)),
            };
            names.push((slot.name.clone(), None));
            params.push(Some(value));
        }
        let mut returns = Vec::new();
        for (index, slot) in record.returns.iter().enumerate() {
            (self.check)()?;
            if slot.index != index + 1 {
                return Ok(Err("environment_return_index_invalid"));
            }
            let value = match self.slot(slot, depth, true)? {
                Ok(value) => value,
                Err(reason) => return Ok(Err(reason)),
            };
            returns.push(value);
        }
        let ret = if record.raw.get("MayReturnNothing") == Some(&serde_json::Value::Bool(true)) {
            OwnedValue::plain(LuaType::Variadic(Arc::new(VariadicType::Base(
                LuaType::Unknown,
            ))))
        } else {
            OwnedValue::pack(returns)
        };
        let metadata = LuaFunctionType::new(
            AsyncState::None,
            false,
            false,
            names,
            ret.structural_type().clone(),
            None,
        );
        (self.check)()?;
        Ok(Ok(OwnedValue::function(&metadata, params, ret)))
    }
}
