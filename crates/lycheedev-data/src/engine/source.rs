//! Invocation-bound, lazy-per-table source preparation for the unified engine.
//! No qualification-only row/batch ceiling and no static/effective fallback.
use super::api::{ColumnInfo, DatasetInfo, SourceMode, TableRequest};
use crate::{
    db2, fault,
    generation::engine_adapter::{self, Materialized},
    remote::provider::{FileScope, OpenedScope},
    resource::{Budget, OwnedRetainedReservation},
};
use arrow_array::RecordBatch;
use arrow_schema::SchemaRef;
use lycheedev_core::{Invocation, Result, control::OperationControl};
use std::sync::Arc;

/// Published only after checked IPC/source/workspace close. All source arrays
/// retain their original allocation admissions through Arrow's buffer hooks.
/// The session owns this snapshot while its schema/provenance are in use.
pub(super) struct PreparedSnapshot {
    table: TableRequest,
    selected_ids: Option<Vec<u32>>,
    pin: lycheedev_core::selection::PinnedSet,
    materialized: Materialized,
    _allocation: OwnedRetainedReservation,
    _pin_allocation: OwnedRetainedReservation,
}
impl PreparedSnapshot {
    pub(super) fn table(&self) -> &TableRequest {
        &self.table
    }
    pub(super) fn covers(&self, ids: Option<&[u32]>, budget: &Budget) -> Result<bool> {
        match (&self.selected_ids, ids) {
            (None, _) => Ok(true),
            (Some(_), None) => Ok(false),
            (Some(have), Some(want)) => {
                budget.charge(crate::resource::Cost::work(
                    want.len().checked_mul(64).ok_or_else(limit)?,
                ))?;
                for (slot, id) in want.iter().enumerate() {
                    if slot % 1024 == 0 {
                        budget.check()?;
                    }
                    if have.binary_search(id).is_ok() {
                        continue;
                    }
                    let ranges = &self.materialized.covered_id_ranges;
                    let end = ranges.partition_point(|(first, _)| first <= id);
                    if !end
                        .checked_sub(1)
                        .and_then(|index| ranges.get(index))
                        .is_some_and(|(_, last)| id <= last)
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
    pub(super) fn pin(&self) -> &lycheedev_core::selection::PinnedSet {
        &self.pin
    }
    pub(super) fn schema(&self) -> SchemaRef {
        self.materialized.schema.clone()
    }
    pub(super) fn batches(&self) -> &[RecordBatch] {
        &self.materialized.batches
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
}
impl std::fmt::Debug for PreparedSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedSnapshot")
            .field("table", &self.table)
            .field("rows", &self.rows())
            .field("complete", &self.complete())
            .finish_non_exhaustive()
    }
}

fn check(invocation: &Invocation, budget: &Budget, control: &OperationControl) -> Result<()> {
    invocation.control.check()?;
    if !control.same_scope(&invocation.control) {
        return Err(fault(
            "records.file_query",
            4,
            "Source preparation control differs from the original invocation",
        ));
    }
    control.check()?;
    budget.check()
}

/// Preparation is demand-driven by the session's first reference to a table.
/// The underlying authenticated DB2/Hotfix APIs are synchronous; deliberately
/// do not detach them into a blocking task whose work can outlive cancellation.
/// Subsequent DataFusion execution sees only immutable, fully owned batches.
pub(super) async fn prepare(
    invocation: &Invocation,
    request: &TableRequest,
    budget: Arc<Budget>,
    control: OperationControl,
) -> Result<Arc<PreparedSnapshot>> {
    prepare_selected(invocation, request, None, budget, control).await
}
pub(super) async fn prepare_selected(
    invocation: &Invocation,
    request: &TableRequest,
    selected_ids: Option<&[u32]>,
    budget: Arc<Budget>,
    control: OperationControl,
) -> Result<Arc<PreparedSnapshot>> {
    check(invocation, &budget, &control)?;
    if selected_ids.is_some_and(|ids| ids.windows(2).any(|pair| pair[0] >= pair[1])) {
        return Err(fault(
            "records.file_query",
            4,
            "Selected source IDs must be sorted and unique",
        ));
    }
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
        |bytes, id| bytes.checked_add(id.len() + 128).ok_or_else(limit),
    )?;
    let own_bytes = own_bytes
        .checked_add(
            selected_ids
                .map_or(0, |ids| ids.len())
                .checked_mul(4)
                .ok_or_else(limit)?,
        )
        .ok_or_else(limit)?;
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
            let source = db2::prepare_scoped(
                invocation,
                &request.name,
                &budget,
                Some(&mut *reader),
                Some(pin.clone()),
            )?;
            reader.check_consumer(invocation, &budget)?;
            let materialized = engine_adapter::prepare(
                invocation,
                source,
                reader,
                request.mode == SourceMode::Effective,
                &request.hotfix_captures,
                selected_ids,
                budget.clone(),
            )?;
            reader.verify_source()?;
            check(invocation, &budget, &control)?;
            Ok(Arc::new(PreparedSnapshot {
                table: request.clone(),
                selected_ids: if materialized.selective {
                    selected_ids.map(<[u32]>::to_vec)
                } else {
                    None
                },
                pin: pin.clone(),
                materialized,
                _allocation: allocation,
                _pin_allocation: pin_allocation,
            }))
        };
        if invocation.flag_bool("--cdn") {
            scope.with_session(data, |reader| run(reader))
        } else {
            scope.with_local_session(data, run)
        }
    })();
    scope.finish(result)
}

/// Discovery uses the same fixed-source checks as execution. A session should
/// call `describe` on its cached snapshot instead of preparing a second copy.
pub(super) async fn discover(
    invocation: &Invocation,
    request: &TableRequest,
    budget: Arc<Budget>,
    control: OperationControl,
) -> Result<DatasetInfo> {
    let snapshot = prepare(invocation, request, budget.clone(), control).await?;
    describe(&snapshot, &budget)
}

pub(super) fn describe(snapshot: &PreparedSnapshot, budget: &Arc<Budget>) -> Result<DatasetInfo> {
    budget.check()?;
    let size = crate::capture::json_size(snapshot.provenance(), budget)?;
    let column_bytes = snapshot
        .schema()
        .fields()
        .iter()
        .try_fold(4096usize, |n, f| {
            n.checked_add(f.name().len() + 512).ok_or_else(limit)
        })?;
    let allocation = budget
        .reserve_retained(
            size.checked_mul(8)
                .and_then(|n| n.checked_add(column_bytes))
                .ok_or_else(limit)?,
        )?
        .into_owned(budget)?;
    Ok(DatasetInfo {
        _allocation: Some(allocation),
        _lifetime: None,
        table: snapshot.table.clone(),
        columns: snapshot
            .schema()
            .fields()
            .iter()
            .map(|field| ColumnInfo {
                name: field.name().clone(),
                data_type: field.data_type().to_string(),
                nullable: field.is_nullable(),
            })
            .collect(),
        row_count: snapshot.rows(),
        provenance: snapshot.provenance().clone(),
    })
}

fn limit() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Source metadata exceeds its admitted bound",
    )
}

#[cfg(test)]
#[path = "source_tests.rs"]
mod tests;

impl PreparedSnapshot {
    /// Current request explanation, independent of the original preparation's
    /// ID set. The caller pre-admits the JSON and temporary marker vector.
    pub(super) fn lookup_report(&self, ids: &[u32], budget: &Budget) -> Result<serde_json::Value> {
        use arrow_array::Array;
        budget.check()?;
        let schema = self.schema();
        let unavailable = |reason: &str| {
            serde_json::json!({"scope":"current_query_source_requirement",
            "table":self.table,"requestedIds":ids,"classification":reason,"missingIds":null,
            "unresolvedIds":null,"coverageComplete":self.complete()})
        };
        let column = match schema.index_of("ID") {
            Ok(index) => index,
            Err(_) => return Ok(unavailable("unavailable_missing_id_column")),
        };
        if !matches!(
            schema.field(column).data_type(),
            arrow_schema::DataType::UInt64 | arrow_schema::DataType::Int64
        ) {
            return Ok(unavailable("unavailable_non_integer_id"));
        }
        let mut found = vec![false; ids.len()];
        for batch in self.batches() {
            budget.charge(crate::resource::Cost::work(
                batch.num_rows().checked_mul(16).ok_or_else(limit)?,
            ))?;
            let values = batch.column(column);
            let unsigned = values.as_any().downcast_ref::<arrow_array::UInt64Array>();
            let signed = values.as_any().downcast_ref::<arrow_array::Int64Array>();
            if unsigned.is_none() && signed.is_none() {
                return Err(fault(
                    "relational.unsupported",
                    3,
                    "Lookup requires an exact integer ID column",
                ));
            }
            for row in 0..batch.num_rows() {
                if row % 1024 == 0 {
                    budget.check()?;
                }
                if values.is_null(row) {
                    continue;
                }
                let id = if let Some(values) = unsigned {
                    u32::try_from(values.value(row)).ok()
                } else {
                    u32::try_from(signed.ok_or_else(limit)?.value(row)).ok()
                };
                if let Some(id) = id
                    && let Ok(slot) = ids.binary_search(&id)
                {
                    found[slot] = true;
                }
            }
        }
        let (mut missing, mut unresolved) = (Vec::new(), Vec::new());
        for (id, found) in ids.iter().zip(found) {
            if !found {
                if self.complete()
                    || self
                        .materialized
                        .confirmed_missing_ids
                        .binary_search(id)
                        .is_ok()
                {
                    missing.push(*id);
                } else {
                    unresolved.push(*id);
                }
            }
        }
        Ok(
            serde_json::json!({"scope":"current_query_source_requirement", "table":self.table,
            "requestedIds":ids,"classification":"exact_visible_integer_id", "missingIds":missing,"unresolvedIds":unresolved,
            "coverageComplete":self.complete()}),
        )
    }
}
