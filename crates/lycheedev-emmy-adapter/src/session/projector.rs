//! Selected-decision dispatch and the three atomic value projections.
//!
//! This is one part of the owned semantic query implementation, not a replacement
//! for type_of_expr. Context-recursive projections remain mandatory backend
//! operations. In particular, they must not fall back to raw inference when the
//! selected value is unavailable, or discard object provenance/session tokens.
use super::{
    BindingCoverage, BindingDecision, BindingSite, BoundValue, DerivedNumberRule,
    EnvironmentCallable, EnvironmentSlot, EnvironmentTypeCatalog, FunctionRef, ObjectRef,
    OwnedBindingSession, SelectedValue,
};
use emmylua_code_analysis::{
    AsyncState, EmmyLuaAnalysis, LuaFunctionType, LuaType, LuaUnionType, SemanticId, VariadicType,
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
                let result = environment_function(callable, &mut || backend.checkpoint())?;
                Ok(result.map(|function| backend.plain(LuaType::DocFunction(Arc::new(function)))))
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
                let result = environment_slot(slot, &result.callable.types, 0, &mut || {
                    backend.checkpoint()
                })?;
                Ok(result.map(|ty| backend.plain(ty)))
            }
        }
    }
}

fn environment_slot<E>(
    slot: &EnvironmentSlot,
    types: &EnvironmentTypeCatalog,
    depth: usize,
    check: &mut impl FnMut() -> Result<(), E>,
) -> ProjectionResult<LuaType, E> {
    let ty = match environment_type(&slot.type_name, &slot.inner_type, types, depth, check)? {
        Ok(ty) => ty,
        Err(reason) => return Ok(Err(reason)),
    };
    Ok(Ok(if slot.nilable {
        LuaType::Union(Arc::new(LuaUnionType::from_vec(vec![ty, LuaType::Nil])))
    } else {
        ty
    }))
}

fn environment_type<E>(
    name: &str,
    inner: &str,
    types: &EnvironmentTypeCatalog,
    depth: usize,
    check: &mut impl FnMut() -> Result<(), E>,
) -> ProjectionResult<LuaType, E> {
    check()?;
    if depth >= 32 {
        return Ok(Err("environment_type_depth"));
    }
    if let Some(alias) = types.aliases.get(name) {
        return environment_type(alias, inner, types, depth + 1, check);
    }
    Ok(Ok(match name {
        "number" => LuaType::Number,
        "integer" => LuaType::Integer,
        "string" => LuaType::String,
        "bool" | "boolean" => LuaType::Boolean,
        "nil" => LuaType::Nil,
        "any" => LuaType::Any,
        "function" | "LuaFunction" => LuaType::Function,
        "table" if inner.is_empty() => LuaType::Table,
        _ => return Ok(Err("environment_type_unavailable")),
    }))
}

fn environment_function<E>(
    callable: &EnvironmentCallable,
    check: &mut impl FnMut() -> Result<(), E>,
) -> ProjectionResult<LuaFunctionType, E> {
    check()?;
    if callable.declaration.kind != "function"
        || callable.origin.identity.is_empty()
        || callable.origin.material_sha256.len() != 64
    {
        return Ok(Err("environment_callable_identity_invalid"));
    }
    if callable.declaration.owner.is_some() {
        return Ok(Err("environment_receiver_unavailable"));
    }
    if callable.declaration.parameters.len() > 32 || callable.declaration.returns.len() > 32 {
        return Ok(Err("environment_signature_budget"));
    }
    let mut parameters = Vec::with_capacity(callable.declaration.parameters.len());
    for (index, slot) in callable.declaration.parameters.iter().enumerate() {
        check()?;
        if slot.index != index + 1 {
            return Ok(Err("environment_parameter_index_invalid"));
        }
        let ty = match environment_slot(slot, &callable.types, 0, check)? {
            Ok(ty) => ty,
            Err(reason) => return Ok(Err(reason)),
        };
        parameters.push((slot.name.clone(), Some(ty)));
    }
    let mut returns = Vec::with_capacity(callable.declaration.returns.len());
    for (index, slot) in callable.declaration.returns.iter().enumerate() {
        check()?;
        if slot.index != index + 1 {
            return Ok(Err("environment_return_index_invalid"));
        }
        let ty = match environment_slot(slot, &callable.types, 0, check)? {
            Ok(ty) => ty,
            Err(reason) => return Ok(Err(reason)),
        };
        returns.push(ty);
    }
    let returns = if callable.declaration.raw.get("MayReturnNothing")
        == Some(&serde_json::Value::Bool(true))
    {
        LuaType::Variadic(Arc::new(VariadicType::Base(LuaType::Unknown)))
    } else {
        LuaType::Variadic(Arc::new(VariadicType::Multi(returns)))
    };
    check()?;
    Ok(Ok(LuaFunctionType::new(
        AsyncState::None,
        false,
        false,
        parameters,
        returns,
        None,
    )))
}
