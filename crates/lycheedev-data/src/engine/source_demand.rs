//! Request-owned demand decoding and immutable Arrow snapshot binding.
use super::api::{ColumnInfo, DatasetInfo, SourceMode, TableRequest};
use crate::{
    fault,
    generation::engine_adapter::demand::{self, DemandSource, PreparedDemand},
    remote::provider::{FileScope, OpenedScope},
    resource::{Budget, OwnedRetainedReservation},
    table_source,
};
use arrow_schema::SchemaRef;
use crate::request::DataContext;
use lycheedev_core::{Result, control::OperationControl};
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(super) enum SourceUse {
    Whole,
    PhysicalIds(Vec<u32>),
    PublicIds(Vec<u32>),
    Scan {
        filter: Option<(String, u32)>,
        cap: usize,
    },
}
pub(super) struct PreparedSnapshot {
    table: TableRequest,
    pin: lycheedev_core::selection::PinnedSet,
    materialized: Arc<PreparedDemand>,
    promoted: Option<Arc<DemandSource>>,
    coverage: SourceUse,
    _allocation: OwnedRetainedReservation,
    _pin_allocation: OwnedRetainedReservation,
}
impl PreparedSnapshot {
    pub(super) fn table(&self) -> &TableRequest {
        &self.table
    }
    pub(super) fn pin(&self) -> &lycheedev_core::selection::PinnedSet {
        &self.pin
    }
    pub(super) fn schema(&self) -> SchemaRef {
        self.materialized.source.schema()
    }
    pub(super) fn provenance(&self) -> &serde_json::Value {
        &self.materialized.provenance
    }
    pub(super) fn complete(&self) -> bool {
        self.materialized.complete
    }
    pub(super) fn rows(&self) -> usize {
        self.materialized.rows
    }
    pub(super) fn physical_ids(&self) -> &[u32] {
        &self.materialized.physical_ids
    }
    pub(super) fn demand(&self, budget: Arc<Budget>) -> Result<Arc<DemandSource>> {
        if let Some(promoted) = &self.promoted {
            return promoted.rebind_arrow(budget)?.ok_or_else(limit);
        }
        if let Some(source) = self.materialized.source.rebind_arrow(budget.clone())? {
            return Ok(source);
        }
        if !self.materialized.source.uses_budget(&budget) {
            return Err(limit());
        }
        Ok(self.materialized.source.clone())
    }
    pub(super) fn public_equals_physical(&self) -> bool {
        self.materialized
            .source
            .columns()
            .iter()
            .any(|f| f.identity && f.name == "ID" && !f.array && !f.signed)
    }
    pub(super) fn reusable(&self) -> bool {
        self.promoted.is_some() || self.materialized.source.is_arrow()
    }
    pub(super) fn covers(&self, requested: &SourceUse) -> bool {
        if self.coverage == SourceUse::Whole {
            return true;
        }
        match (&self.coverage, requested) {
            (SourceUse::PhysicalIds(cached), SourceUse::PhysicalIds(ids)) => {
                ids.iter().all(|id| cached.binary_search(id).is_ok())
            }
            (SourceUse::PhysicalIds(cached), SourceUse::PublicIds(ids))
                if self.public_equals_physical() =>
            {
                ids.iter().all(|id| cached.binary_search(id).is_ok())
            }
            (left, right) => left == right,
        }
    }
    pub(super) fn promote(
        self: &Arc<Self>,
        batches: Vec<crate::generation::engine_adapter::demand::DemandBatch>,
        requested: &SourceUse,
        budget: &Arc<Budget>,
    ) -> Result<Option<Arc<Self>>> {
        let descriptor_bytes = crate::capture::json_size(requested, budget)?
            .checked_mul(8)
            .and_then(|n| n.checked_add(8192))
            .ok_or_else(limit)?;
        let size = batches.iter().try_fold(descriptor_bytes, |n, b| {
            n.checked_add(b.ids.len().checked_mul(8).ok_or_else(limit)?)
                .ok_or_else(limit)
        })?;
        let publication_peak = size
            .checked_mul(4)
            .and_then(|n| n.checked_add(batches.len().checked_mul(2048)?))
            .and_then(|n| n.checked_add(32 << 10))
            .ok_or_else(limit)?;
        if u64::try_from(publication_peak).map_err(|_| limit())?
            > budget.remaining_retained_bytes()? / 2
        {
            return Ok(None);
        }
        let selection_guard = budget.reserve_retained(size)?.into_owned(budget)?;
        let mut visited = Vec::new();
        for batch in &batches {
            for id in batch.ids.values() {
                budget.check()?;
                if visited.last().is_some_and(|last| last >= id) {
                    return Ok(None);
                }
                if self.physical_ids().binary_search(id).is_err() {
                    return Err(limit());
                }
                visited.push(*id);
            }
        }
        let coverage = match requested {
            SourceUse::Whole => {
                if visited != self.physical_ids() {
                    return Ok(None);
                }
                SourceUse::Whole
            }
            SourceUse::PublicIds(ids) if self.public_equals_physical() => {
                SourceUse::PhysicalIds(ids.clone())
            }
            SourceUse::PhysicalIds(ids) => SourceUse::PhysicalIds(ids.clone()),
            SourceUse::Scan { filter, cap } => SourceUse::Scan {
                filter: filter.clone(),
                cap: *cap,
            },
            _ => return Ok(None),
        };
        if let SourceUse::PhysicalIds(ids) = &coverage {
            let mut actual = visited.iter();
            for id in ids {
                if self.physical_ids().binary_search(id).is_ok() && actual.next() != Some(id) {
                    return Ok(None);
                }
            }
            if actual.next().is_some() {
                return Ok(None);
            }
        }
        let Some(promoted) = self
            .materialized
            .source
            .promote_visited(batches, &visited)?
        else {
            return Ok(None);
        };
        let bytes = crate::capture::json_size(&(&self.table, &self.pin), budget)?
            .checked_mul(8)
            .and_then(|n| n.checked_add(size))
            .ok_or_else(limit)?;
        let allocation = budget.reserve_retained(bytes)?.into_owned(budget)?;
        let pin_allocation = budget.reserve_retained(0)?.into_owned(budget)?;
        drop(selection_guard);
        Ok(Some(Arc::new(Self {
            table: self.table.clone(),
            pin: self.pin.clone(),
            materialized: self.materialized.clone(),
            promoted: Some(promoted),
            coverage,
            _allocation: allocation,
            _pin_allocation: pin_allocation,
        })))
    }
    pub(super) fn absent_is_unresolved(&self, id: u32, budget: &Arc<Budget>) -> Result<bool> {
        self.demand(budget.clone())?.absent_is_unresolved(id)
    }
}
fn check(invocation: &DataContext, budget: &Budget, control: &OperationControl) -> Result<()> {
    invocation.control.check()?;
    if !control.same_scope(&invocation.control) {
        return Err(fault(
            "records.file_query",
            4,
            "Source preparation control differs from the original DataContext",
        ));
    }
    control.check()?;
    budget.check()
}

pub(super) async fn prepare_selected(
    invocation: &DataContext,
    request: &TableRequest,
    whole_source_consumed: bool,
    budget: Arc<Budget>,
    control: OperationControl,
) -> Result<Arc<PreparedSnapshot>> {
    super::source_io::run(|| {
        prepare_selected_sync(invocation, request, whole_source_consumed, budget, control)
    })
}

fn prepare_selected_sync(
    invocation: &DataContext,
    request: &TableRequest,
    whole_source_consumed: bool,
    budget: Arc<Budget>,
    control: OperationControl,
) -> Result<Arc<PreparedSnapshot>> {
    check(invocation, &budget, &control)?;
    if !crate::definitions::valid_name(&request.name) {
        return Err(fault("schema.invalid_definition", 4, "Invalid table name"));
    }
    match request.mode {
        SourceMode::Static if !request.hotfix_captures.is_empty() => {
            return Err(fault(
                "records.hotfix_identity",
                4,
                "A static table cannot select Hotfix captures",
            ));
        }
        SourceMode::Effective
            if request.hotfix_captures.is_empty() || request.hotfix_captures.len() > 16 =>
        {
            return Err(fault(
                "records.hotfix_identity",
                4,
                "An effective table requires 1 to 16 exact Hotfix captures",
            ));
        }
        _ => {}
    }
    let own_bytes = request.hotfix_captures.iter().try_fold(
        request.name.len().checked_add(4096).ok_or_else(limit)?,
        |n, id| n.checked_add(id.len() + 128).ok_or_else(limit),
    )?;
    let allocation = budget.reserve_retained(own_bytes)?.into_owned(&budget)?;
    let (mut scope, pin) = OpenedScope::open(invocation, &budget)?.admit_pinned(fault(
        "records.data_pin_required",
        4,
        "A unified query source requires an exact data pin",
    ))?;
    let result = (|| {
        let pin_bytes = crate::capture::json_size(&pin, &budget)?;
        let pin_allocation = budget
            .reserve_retained(
                pin_bytes
                    .checked_mul(8)
                    .and_then(|n| n.checked_add(4096))
                    .ok_or_else(limit)?,
            )?
            .into_owned(&budget)?;
        let data = pin
            .data
            .as_ref()
            .ok_or_else(|| fault("records.data_pin_required", 4, "Missing admitted data pin"))?;
        let run = |reader: &mut dyn FileScope| {
            check(invocation, &budget, &control)?;
            let source = table_source::prepare_scoped(
                invocation,
                &request.name,
                &budget,
                Some(&mut *reader),
                Some(pin.clone()),
            )?;
            let materialized = demand::prepare_demand(
                invocation,
                source,
                reader,
                request.mode == SourceMode::Effective,
                &request.hotfix_captures,
                whole_source_consumed,
                budget.clone(),
            )?;
            reader.verify_source()?;
            check(invocation, &budget, &control)?;
            Ok(Arc::new(PreparedSnapshot {
                table: request.clone(),
                pin: pin.clone(),
                materialized: Arc::new(materialized),
                promoted: None,
                coverage: SourceUse::Whole,
                _allocation: allocation,
                _pin_allocation: pin_allocation,
            }))
        };
        if invocation.source.cdn {
            scope.with_session(data, |reader| run(reader))
        } else {
            scope.with_local_session(data, run)
        }
    })();
    scope.finish(result)
}

pub(super) fn describe(snapshot: &PreparedSnapshot, budget: &Arc<Budget>) -> Result<DatasetInfo> {
    budget.check()?;
    let bytes = crate::capture::json_size(snapshot.provenance(), budget)?
        .checked_mul(8)
        .and_then(|n| n.checked_add(4096))
        .ok_or_else(limit)?;
    let bytes = snapshot.schema().fields().iter().try_fold(bytes, |n, f| {
        n.checked_add(f.name().len() + 512).ok_or_else(limit)
    })?;
    let allocation = budget.reserve_retained(bytes)?.into_owned(budget)?;
    Ok(DatasetInfo {
        table: snapshot.table.clone(),
        columns: snapshot
            .schema()
            .fields()
            .iter()
            .map(|f| ColumnInfo {
                name: f.name().clone(),
                data_type: f.data_type().to_string(),
                nullable: f.is_nullable(),
            })
            .collect(),
        row_count: snapshot.rows(),
        provenance: snapshot.provenance().clone(),
        _allocation: Some(allocation),
        _lifetime: None,
    })
}
fn limit() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Demand source admission or ownership differs",
    )
}
impl PreparedSnapshot {
    pub(super) fn current_provenance(
        &self,
        reused: bool,
        budget: &Arc<Budget>,
    ) -> Result<serde_json::Value> {
        budget.check()?;
        let mut value = self.provenance().clone();
        value["sessionSnapshot"] = serde_json::json!({"reused":reused,"sourceReadingScope":if reused{"authenticated_at_snapshot_construction"}else{"current_request"},
            "coverage":match &self.coverage {SourceUse::Whole=>"whole_physical_source",SourceUse::PhysicalIds(_)|SourceUse::PublicIds(_)=>"resolved_physical_id_set",SourceUse::Scan{..}=>"exact_finite_scan_descriptor"},
            "requestControl":"fresh_request_not_cached_deadline","representation":if self.reusable(){"native_arrow_with_physical_id_index"}else{"demand_decoder"}});
        if reused && self.reusable() {
            value["typedGenerationCache"]["representationSelection"] =
                serde_json::json!("authenticated_arrow_session_snapshot");
            value["typedGenerationCache"]["representationReason"] =
                serde_json::json!("session_native_arrow_reuse");
        }
        Ok(value)
    }
}
