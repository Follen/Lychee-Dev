//! Owned session identity and producer-ordered coverage.
//!
//! This foundation is not wired to production yet. Every selected producer must
//! append here at its actual execution point before the engine ledger can be
//! removed; merging separate completed ledgers cannot preserve its contract.
mod backend;
pub use backend::{
    OwnedMemberInfo, OwnedQuery, OwnedQueryCapability, OwnedQueryContinuation, OwnedQueryFailure,
};
mod binary;
pub use binary::PrimitiveBinaryProjection;
mod evidence;
mod execution;
pub use execution::{ExpressionExecutionBackend, ExpressionValue};
mod expression;
pub use expression::{ExpressionInstruction, ExpressionPlan};
mod object;
mod projector;
mod query;
mod tuple;
mod value;
pub use evidence::*;
pub use projector::{ProjectionResult, SelectedProjectionBackend, SelectedProjector};
pub use query::SelectedExpressionBackend;
use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};
pub use tuple::{DenseTupleBackend, DenseTupleProjection};
pub use value::{OwnedObjectInfo, OwnedValue};

const MAX_COVERAGE_ROWS: usize = 16_384;
const MAX_ACTIVE: usize = 64;
const MAX_SHAPES: usize = 1_024;

#[derive(Clone, Debug)]
struct SessionToken(Arc<()>);
impl PartialEq for SessionToken {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for SessionToken {}

#[derive(Default)]
struct CoverageLedger {
    rows: Vec<BindingCoverage>,
    seen: HashSet<BindingCoverage>,
}
impl CoverageLedger {
    /// Exact old append semantics, including duplicate-at-limit and the dropped
    /// producer's site on the sentinel. No sorting, replay or bulk merge.
    fn append(&mut self, row: BindingCoverage) {
        if self.rows.len() >= MAX_COVERAGE_ROWS {
            return;
        }
        if self.seen.contains(&row) {
            return;
        }
        if self.rows.len() == MAX_COVERAGE_ROWS - 1 {
            self.rows.push(BindingCoverage {
                site: row.site,
                operation: "coverage",
                state: "unavailable",
                proof: String::new(),
                reason: Some("binding_coverage_budget".into()),
                parameter: None,
                parameter_index: None,
                argument: None,
            });
            return;
        }
        self.seen.insert(row.clone());
        self.rows.push(row);
    }
}

/// This token stays outside upstream LuaObjectType. The selected-type adapter
/// must carry it through every owned substitution/member/return projection.
/// It is not a completed type-check or proof of source-origin validation.
#[derive(Clone, Debug)]
pub(crate) struct FrozenShape {
    token: SessionToken,
    pub(crate) object: ObjectRef,
    pub(crate) shape: Arc<ObjectShape>,
}

/// Exactly one frozen provider, identity token, active guards and ordered ledger.
/// It neither owns an Emmy database nor copies a Source graph or source text.
pub struct OwnedBindingSession {
    token: SessionToken,
    context: Arc<dyn BindingContext>,
    identity: String,
    coverage: Mutex<CoverageLedger>,
    active: Mutex<HashSet<BindingSite>>,
    active_objects: Mutex<HashSet<ObjectViewKey>>,
    shapes: Mutex<HashMap<ObjectViewKey, FrozenShape>>,
}
impl OwnedBindingSession {
    pub fn new_checked<E>(
        context: Arc<dyn BindingContext>,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Arc<Self>, E> {
        check()?;
        let value = Arc::new(Self {
            token: SessionToken(Arc::new(())),
            identity: context.identity().into(),
            context,
            coverage: Mutex::new(CoverageLedger::default()),
            active: Mutex::new(HashSet::new()),
            active_objects: Mutex::new(HashSet::new()),
            shapes: Mutex::new(HashMap::new()),
        });
        check()?;
        Ok(value)
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn same_session(&self, other: &Self) -> bool {
        self.token == other.token
    }
    pub(crate) fn context(&self) -> &dyn BindingContext {
        self.context.as_ref()
    }

    /// Internal producer operation. Callers only record work that really ran;
    /// this function cannot turn a gate or a missing checker into completion.
    pub(crate) fn record<E>(
        &self,
        row: BindingCoverage,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<(), E> {
        check()?;
        self.coverage
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .append(row);
        Ok(())
    }
    /// A single bounded report snapshot, never called per producer to reconstruct
    /// ordering. Appended rows are immutable: the initial length defines an exact
    /// prefix snapshot even if another reader runs. Checks run outside the lock;
    /// interruption returns the original E and drops the partial output Vec.
    pub fn coverage_checked<E>(
        &self,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Vec<BindingCoverage>, E> {
        check()?;
        let len = self
            .coverage
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .rows
            .len();
        let mut rows = Vec::with_capacity(len);
        for index in 0..len {
            check()?;
            let row = self.coverage.lock().unwrap_or_else(|p| p.into_inner()).rows[index].clone();
            rows.push(row);
        }
        check()?;
        Ok(rows)
    }

    pub(crate) fn enter<E>(
        &self,
        site: BindingSite,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Option<ProjectionGuard<'_>>, E> {
        check()?;
        let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
        if active.len() >= MAX_ACTIVE || !active.insert(site) {
            return Ok(None);
        }
        Ok(Some(ProjectionGuard {
            session: self,
            key: ActiveKey::Site(site),
        }))
    }
    pub(crate) fn enter_object<E>(
        &self,
        key: ObjectViewKey,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Option<ProjectionGuard<'_>>, E> {
        check()?;
        let mut active = self
            .active_objects
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        if active.len() >= MAX_ACTIVE || !active.insert(key.clone()) {
            return Ok(None);
        }
        Ok(Some(ProjectionGuard {
            session: self,
            key: ActiveKey::Object(key),
        }))
    }

    /// Preserve the selected object identity/epoch and shape budget. The owned
    /// semantic projection must separately validate the Lua/XML origin and each
    /// field before it emits object_shape or structure_check coverage.
    pub(crate) fn freeze_shape<E>(
        &self,
        object: &ObjectRef,
        mut check: impl FnMut() -> Result<(), E>,
    ) -> Result<Result<FrozenShape, &'static str>, E> {
        check()?;
        if object.view.context != self.identity()
            || object.identity.is_empty()
            || object.epoch.is_empty()
        {
            return Ok(Err("object_context_or_epoch_mismatch"));
        }
        let cached = self
            .shapes
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&object.view)
            .cloned();
        if let Some(cached) = cached {
            check()?;
            return Ok(if &cached.object == object {
                Ok(cached)
            } else {
                Err("object_frozen_identity_mismatch")
            });
        }
        let decision = self.context.object_shape(&object.view);
        check()?;
        let BindingDecision::Bound(shape) = decision else {
            return Ok(Err("object_shape_unavailable"));
        };
        if shape.key != object.view || shape.proof.is_empty() || shape.fields.len() > 128 {
            return Ok(Err("object_shape_identity_or_budget"));
        }
        let frozen = FrozenShape {
            token: self.token.clone(),
            object: object.clone(),
            shape: Arc::new(shape),
        };
        check()?;
        let mut shapes = self.shapes.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(cached) = shapes.get(&object.view) {
            return Ok(if &cached.object == object {
                Ok(cached.clone())
            } else {
                Err("object_frozen_identity_mismatch")
            });
        }
        if shapes.len() >= MAX_SHAPES {
            return Ok(Err("object_shape_budget"));
        }
        shapes.insert(object.view.clone(), frozen.clone());
        Ok(Ok(frozen))
    }
    pub(crate) fn owns_shape(&self, shape: &FrozenShape) -> bool {
        self.token == shape.token && shape.object.view.context == self.identity()
    }
}

enum ActiveKey {
    Site(BindingSite),
    Object(ObjectViewKey),
}
/// Cancellation/unwind cannot strand an active key and poison later queries.
/// The guard owns no database or copied evidence and never extends a deadline.
pub(crate) struct ProjectionGuard<'a> {
    session: &'a OwnedBindingSession,
    key: ActiveKey,
}
impl Drop for ProjectionGuard<'_> {
    fn drop(&mut self) {
        match &self.key {
            ActiveKey::Site(site) => {
                self.session
                    .active
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(site);
            }
            ActiveKey::Object(key) => {
                self.session
                    .active_objects
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .remove(key);
            }
        }
    }
}

#[cfg(test)]
mod tests;
