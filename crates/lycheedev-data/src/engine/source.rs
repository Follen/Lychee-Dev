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
    pin: lycheedev_core::selection::PinnedSet,
    materialized: Materialized,
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
        |bytes, id| bytes.checked_add(id.len() + 128).ok_or_else(limit),
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
                budget.clone(),
            )?;
            reader.verify_source()?;
            check(invocation, &budget, &control)?;
            Ok(Arc::new(PreparedSnapshot {
                table: request.clone(),
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
