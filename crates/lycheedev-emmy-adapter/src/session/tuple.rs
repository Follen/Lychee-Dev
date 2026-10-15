//! The RHS dense-table projection that precedes ordinary logical operations.
//! Query order is member order; sorting/density validation happens afterwards.
use super::{BindingSite, ExpressionExecutionBackend, SelectedProjector};
use emmylua_code_analysis::{FileId, LuaMemberKey, LuaTupleStatus, LuaType, SemanticId};

#[derive(Debug)]
pub enum DenseTupleProjection<V> {
    Projected(V),
    Unchanged(V),
}

/// Required operations of the same owned query. In particular, a tuple field
/// must not use raw upstream inference merely because its table was NotManaged.
pub trait DenseTupleBackend: ExpressionExecutionBackend {
    /// Ordinary expression query with the owning backend's original cache and
    /// in-progress state, not a flow-at query or an uncached foreign evaluator.
    fn expression_value(
        &mut self,
        projector: &SelectedProjector<'_>,
        site: BindingSite,
    ) -> Result<Self::Value, Self::Error>;

    /// Preserve every slot's selected carrier; the fixed public tuple status is
    /// InferResolve. This is not Lua result-pack expansion or scalar adjustment.
    fn tuple_value(
        &mut self,
        slots: Vec<Self::Value>,
        status: LuaTupleStatus,
    ) -> Result<Self::Value, Self::Error>;
}

impl SelectedProjector<'_> {
    /// The exact table_literal_as_tuple ordering, using public facts. Failure to
    /// form a dense tuple returns the original carrier, not Unknown. Completed
    /// field queries remain in the sole ledger even if a later field rejects it.
    pub fn project_dense_table_tuple<B: DenseTupleBackend>(
        &self,
        file_id: FileId,
        original: B::Value,
        backend: &mut B,
    ) -> Result<DenseTupleProjection<B::Value>, B::Error> {
        backend.checkpoint()?;
        let LuaType::TableConst(table) = backend.value_type(&original) else {
            return Ok(DenseTupleProjection::Unchanged(original));
        };
        let owner = SemanticId::member(table.file_id, table.value);
        let model = self.analysis().semantic_model(file_id);
        let members = model.members_of_owner(&owner);
        backend.checkpoint()?;
        if members.is_empty() {
            return Ok(DenseTupleProjection::Unchanged(original));
        }
        let mut entries = Vec::with_capacity(members.len());
        for member_ref in members.iter() {
            backend.checkpoint()?;
            let Some(member) = model
                .file_facts_of(member_ref.file_id)
                .and_then(|facts| facts.member_by_id(&member_ref.id))
            else {
                return Ok(DenseTupleProjection::Unchanged(original));
            };
            let LuaMemberKey::Integer(key) = &member.key else {
                return Ok(DenseTupleProjection::Unchanged(original));
            };
            if *key <= 0 {
                return Ok(DenseTupleProjection::Unchanged(original));
            }
            let Some(syntax) = member.value_syntax else {
                return Ok(DenseTupleProjection::Unchanged(original));
            };
            // Preserve the pinned model.type_of_expr(value_syntax) behavior:
            // query in the CURRENT model's file, even for foreign TableConst
            // member facts. Changing to member_ref.file_id is a semantic fix,
            // not a transparent migration, and needs its own reviewed evidence.
            let value = backend.expression_value(self, BindingSite { file_id, syntax })?;
            backend.checkpoint()?;
            entries.push((*key, value));
        }
        backend.checkpoint()?;
        entries.sort_by_key(|(key, _)| *key);
        backend.checkpoint()?;
        for (index, (key, _)) in entries.iter().enumerate() {
            backend.checkpoint()?;
            if *key != index as i64 + 1 {
                return Ok(DenseTupleProjection::Unchanged(original));
            }
        }
        let mut slots = Vec::with_capacity(entries.len());
        for (_, value) in entries {
            backend.checkpoint()?;
            slots.push(value);
        }
        let value = backend.tuple_value(slots, LuaTupleStatus::InferResolve)?;
        backend.checkpoint()?;
        Ok(DenseTupleProjection::Projected(value))
    }
}
