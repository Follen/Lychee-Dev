//! Opt-in, total-bounded ephemeral TableProvider qualification. This exercises
//! the real authenticated DB2 -> admitted typed batch -> DataFusion seam; it is
//! not the disk generation reader or a production SQL/default-route switch.
//!
//! Source buffers keep their original admission through Arrow's safe shared
//! buffer lifetime hook. DataFusion operator/output allocations are a separate
//! qualification responsibility, not covered by attaching input reservations.
mod ownership;
mod runtime;
#[cfg(test)]
mod tests;

use super::{QueryOwner, external, unsupported};
use crate::{
    db2, fault,
    generation::{ImportLimits, ImportedSchema, StaticIdentity, TableImporter},
    remote::provider::{FileScope, OpenedScope},
    resource::{Budget, OwnedRetainedReservation},
    table,
};
use arrow_array::{RecordBatch, RecordBatchOptions};
use arrow_schema::SchemaRef;
use async_trait::async_trait;
use datafusion::{
    catalog::{Session, TableProvider},
    common::Result as DfResult,
    execution::TaskContext,
    logical_expr::{Expr, TableType},
    physical_expr::EquivalenceProperties,
    physical_plan::{
        DisplayAs, DisplayFormatType, ExecutionPlan, Partitioning, PlanProperties,
        RecordBatchStream, SendableRecordBatchStream,
        execution_plan::{Boundedness, EmissionType},
    },
};
use futures_util::{Stream, StreamExt};
use lycheedev_core::{Invocation, Result, ToolkitError};
use ownership::OwnedBatch;
use std::{
    fmt,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// An explicit effective request cannot silently receive static rows. The
/// existing effective/Hotfix engine remains authoritative until adapted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SourceMode {
    Static,
    Effective,
}

#[derive(Clone, Copy)]
pub(super) struct SnapshotLimits {
    pub rows: usize,
    pub batches: usize,
    /// Aggregate conservative Arrow/schema/owner admission, not final payload
    /// size or RSS. The original DB2 source is separately budgeted as before.
    pub arrow_bytes: usize,
    pub batch: ImportLimits,
}
impl Default for SnapshotLimits {
    fn default() -> Self {
        Self {
            rows: 65_536,
            batches: 128,
            arrow_bytes: 128 << 20,
            batch: ImportLimits {
                rows: 1024,
                value_bytes: 1 << 20,
                text_bytes: 1 << 20,
            },
        }
    }
}
impl SnapshotLimits {
    fn validate(self) -> Result<Self> {
        if self.rows == 0
            || self.rows > 1_000_000
            || self.batches == 0
            || self.batches > 4096
            || self.arrow_bytes == 0
            || self.arrow_bytes > 512 << 20
        {
            return Err(bound());
        }
        Ok(self)
    }
}
fn bound() -> ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Ephemeral DataFusion snapshot exceeds its admitted bound",
    )
}
fn add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right).ok_or_else(bound)
}
fn multiply(left: usize, right: usize) -> Result<usize> {
    left.checked_mul(right).ok_or_else(bound)
}
fn within(bytes: usize, maximum: usize) -> Result<usize> {
    if bytes > maximum {
        Err(bound())
    } else {
        Ok(bytes)
    }
}

/// Private fields prevent callers from substituting arbitrary schema or source
/// metadata. The prepared source and complete decoded rows are immutable. Only
/// prepare() exposes a snapshot, after original source verification and checked
/// reader/workspace close have succeeded. No late source I/O occurs in scan.
/// This deliberately bounded materialization is an interface oracle, not the
/// final streaming production architecture or a performance result.
pub(super) struct FixedSnapshot {
    source: db2::Prepared,
    source_sha256: String,
    complete: bool,
    schema: SchemaRef,
    batches: Vec<OwnedBatch>,
    _schema_owner: Arc<ImportedSchema>,
    rows: usize,
    budget: Arc<Budget>,
    _allocation: OwnedRetainedReservation,
}
impl fmt::Debug for FixedSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixedStaticSnapshot")
            .field("source_sha256", &self.source_sha256)
            .field("rows", &self.rows)
            .field("complete", &self.complete)
            .finish_non_exhaustive()
    }
}
impl FixedSnapshot {
    pub(super) fn prepare(
        invocation: &Invocation,
        name: &str,
        mode: SourceMode,
        limits: SnapshotLimits,
        budget: Arc<Budget>,
    ) -> Result<Arc<Self>> {
        Self::prepare_finishing(invocation, name, mode, limits, budget, |scope, result| {
            scope.finish(result)
        })
    }
    fn prepare_finishing(
        invocation: &Invocation,
        name: &str,
        mode: SourceMode,
        limits: SnapshotLimits,
        budget: Arc<Budget>,
        finish: impl for<'a> FnOnce(
            crate::remote::provider::Scope<'a>,
            Result<Arc<Self>>,
        ) -> Result<Arc<Self>>,
    ) -> Result<Arc<Self>> {
        budget.check()?;
        if mode != SourceMode::Static {
            return Err(fault(
                "relational.unsupported",
                3,
                "DataFusion qualification does not implement effective Hotfix overlays",
            ));
        }
        let limits = limits.validate()?;
        let (mut scope, pin) = OpenedScope::open(invocation, &budget)?.admit_pinned(fault(
            "records.data_pin_required",
            4,
            "A fixed DataFusion source requires an exact data pin",
        ))?;
        let result = (|| {
            let data = pin.data.as_ref().ok_or_else(|| {
                fault("records.data_pin_required", 4, "Missing admitted data pin")
            })?;
            let run = |reader: &mut dyn FileScope| {
                let prepared = db2::prepare_scoped(
                    invocation,
                    name,
                    &budget,
                    Some(&mut *reader),
                    Some(pin.clone()),
                )?;
                reader.check_consumer(invocation, &budget)?;
                let snapshot = Self::from_prepared(prepared, budget.clone(), limits)?;
                reader.verify_source()?;
                budget.check()?;
                Ok(snapshot)
            };
            if invocation.flag_bool("--cdn") {
                scope.with_session(data, |reader| run(reader))
            } else {
                scope.with_local_session(data, run)
            }
        })();
        finish(scope, result)
    }
    fn from_prepared(
        source: db2::Prepared,
        budget: Arc<Budget>,
        limits: SnapshotLimits,
    ) -> Result<Arc<Self>> {
        let identity = StaticIdentity::from_prepared(&source)?;
        let complete = identity.complete();
        let source_sha256 = identity.digest(&budget)?;
        let view = table::open_budgeted(
            &source.file.bytes,
            &source.definition,
            source.limits,
            &source.availability.skipped,
            &source.availability.ranges,
            budget.clone(),
        )?;
        let rows = view.logical_count();
        if rows > limits.rows {
            return Err(bound());
        }
        // Include the importer's schema allowance in the snapshot ceiling
        // before it allocates. Own vector/Arc overhead is admitted separately.
        let schema_bound = view
            .columns()
            .iter()
            .try_fold(512usize, |n, field| add(n, add(field.name.len(), 512)?))?;
        let own_bound = add(4096, multiply(limits.batches, 256)?)?;
        let mut retained = within(add(schema_bound, own_bound)?, limits.arrow_bytes)?;
        let allocation = budget.reserve_retained(own_bound)?.into_owned(&budget)?;
        let mut batches = Vec::with_capacity(limits.batches);
        let mut importer = TableImporter::new(&view, budget.clone(), limits.batch)?;
        let schema = importer.schema().clone();
        let schema_owner = importer.schema_owner();
        let owner_bytes = ownership::allocation_bound(schema.fields().len())?;
        let per_batch = add(importer.batch_reservation_bytes(), owner_bytes)?;
        let mut seen = 0usize;
        while seen < rows {
            budget.check()?;
            if batches.len() == limits.batches {
                return Err(bound());
            }
            // Reject aggregate capacity BEFORE the importer enters Arrow's
            // builders. A post-allocation memory-size check is not admission.
            retained = within(add(retained, per_batch)?, limits.arrow_bytes)?;
            let batch = importer
                .next_batch()?
                .ok_or_else(|| fault("records.generation_order", 4, "Typed source ended early"))?;
            if batch.ordinal() != batches.len() || batch.records().num_rows() == 0 {
                return Err(fault(
                    "records.generation_order",
                    4,
                    "Invalid typed batch order",
                ));
            }
            seen = add(seen, batch.records().num_rows())?;
            if seen > rows {
                return Err(fault(
                    "records.generation_order",
                    4,
                    "Typed source added rows",
                ));
            }
            batches.push(OwnedBatch::new(batch, &budget, owner_bytes)?);
        }
        if importer.next_batch()?.is_some() {
            return Err(fault(
                "records.generation_order",
                4,
                "Typed source changed size",
            ));
        }
        drop(importer);
        drop(view);
        budget.check()?;
        Ok(Arc::new(Self {
            source,
            source_sha256,
            complete,
            schema,
            batches,
            _schema_owner: schema_owner,
            rows,
            budget,
            _allocation: allocation,
        }))
    }
    /// Full authenticated fixed-source evidence, including product/build,
    /// locale, DBD identity, content versus partialContent and missing coverage.
    /// An empty query over this snapshot does not upgrade complete=false.
    pub(super) fn identity(&self) -> Result<StaticIdentity<'_>> {
        StaticIdentity::from_prepared(&self.source)
    }
    pub(super) fn complete(&self) -> bool {
        self.complete
    }
    pub(super) fn source_sha256(&self) -> &str {
        &self.source_sha256
    }
    /// Reserve the complete retained-output allowance before polling the first
    /// batch. Observe actual array capacities conservatively (aliases may count
    /// twice) and enforce rows/batches as well. Transient allocations inside DF
    /// operators remain separately limited by its pool/host execution boundary.
    pub(super) async fn collect_result(
        self: &Arc<Self>,
        mut stream: SendableRecordBatchStream,
        owner: Arc<QueryOwner>,
        limits: OutputLimits,
    ) -> DfResult<QualifiedResult> {
        if !Arc::ptr_eq(&self.budget, &owner.budget) {
            return Err(unsupported(
                "DataFusion result has a different budget owner",
            ));
        }
        if limits.rows == 0
            || limits.rows > 1_000_000
            || limits.batches == 0
            || limits.batches > 4096
            || limits.bytes == 0
            || limits.bytes > 512 << 20
        {
            return Err(external(bound()));
        }
        let allowance = add(
            limits.bytes,
            multiply(limits.batches, 256).map_err(external)?,
        )
        .map_err(external)?;
        let allocation = self
            .budget
            .reserve_retained(allowance)
            .map_err(external)?
            .into_owned(&self.budget)
            .map_err(external)?;
        let mut records = Vec::with_capacity(limits.batches);
        let mut rows = 0usize;
        let mut bytes = 0usize;
        while let Some(next) = stream.next().await {
            owner.check()?;
            let batch = next?;
            rows = within(add(rows, batch.num_rows()).map_err(external)?, limits.rows)
                .map_err(external)?;
            bytes = within(
                add(bytes, batch.get_array_memory_size()).map_err(external)?,
                limits.bytes,
            )
            .map_err(external)?;
            if records.len() == limits.batches {
                return Err(external(bound()));
            }
            records.push(batch);
        }
        owner.check()?;
        Ok(QualifiedResult {
            records,
            observed_bytes: bytes,
            _source: self.clone(),
            _owner: owner,
            _allocation: allocation,
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct OutputLimits {
    pub rows: usize,
    pub batches: usize,
    pub bytes: usize,
}

/// Bounded qualification output lifetime, including actual retained capacity.
/// Metadata and freshly computed output arrays must be consumed while this
/// owner remains alive. This is not an arbitrary-query allocation guarantee.
pub(super) struct QualifiedResult {
    records: Vec<RecordBatch>,
    observed_bytes: usize,
    _source: Arc<FixedSnapshot>,
    _owner: Arc<QueryOwner>,
    _allocation: OwnedRetainedReservation,
}
impl QualifiedResult {
    pub(super) fn records(&self) -> &[RecordBatch] {
        &self.records
    }
    pub(super) fn complete(&self) -> bool {
        self._source.complete
    }
    pub(super) fn observed_bytes(&self) -> usize {
        self.observed_bytes
    }
}

#[derive(Debug)]
pub(super) struct FixedTableProvider {
    snapshot: Arc<FixedSnapshot>,
    owner: Arc<QueryOwner>,
}
impl FixedTableProvider {
    pub(super) fn new(snapshot: Arc<FixedSnapshot>, owner: Arc<QueryOwner>) -> DfResult<Self> {
        owner.check()?;
        if !Arc::ptr_eq(&snapshot.budget, &owner.budget) {
            return Err(unsupported("DataFusion table has a different budget owner"));
        }
        Ok(Self { snapshot, owner })
    }
}
#[async_trait]
impl TableProvider for FixedTableProvider {
    fn schema(&self) -> SchemaRef {
        self.snapshot.schema.clone()
    }
    fn table_type(&self) -> TableType {
        TableType::Base
    }
    async fn scan(
        &self,
        state: &dyn Session,
        projection: Option<&Vec<usize>>,
        filters: &[Expr],
        limit: Option<usize>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        self.owner.check()?;
        // Default supports_filters_pushdown is Unsupported. Reject direct
        // misuse instead of silently dropping a predicate or misapplying limit.
        if !filters.is_empty() {
            return Err(unsupported(
                "Fixed snapshot has no pushed filter implementation",
            ));
        }
        let columns = projection.map_or(self.snapshot.schema.fields().len(), Vec::len);
        if columns > 65_536
            || projection
                .is_some_and(|p| p.iter().any(|i| *i >= self.snapshot.schema.fields().len()))
        {
            return Err(unsupported("Fixed snapshot projection is out of bounds"));
        }
        let batch_rows = state.config_options().execution.batch_size;
        if batch_rows == 0 {
            return Err(unsupported("DataFusion batch size must be nonzero"));
        }
        self.owner
            .retain(add(4096, multiply(columns, 1024).map_err(external)?).map_err(external)?)?;
        let projection = projection
            .cloned()
            .unwrap_or_else(|| (0..columns).collect());
        let schema = Arc::new(self.snapshot.schema.project(&projection)?);
        let properties = Arc::new(PlanProperties::new(
            EquivalenceProperties::new(schema),
            Partitioning::UnknownPartitioning(1),
            EmissionType::Incremental,
            Boundedness::Bounded,
        ));
        Ok(Arc::new(FixedScan {
            snapshot: self.snapshot.clone(),
            owner: self.owner.clone(),
            projection: Arc::new(projection),
            batch_rows,
            limit,
            properties,
        }))
    }
}

#[derive(Debug)]
struct FixedScan {
    snapshot: Arc<FixedSnapshot>,
    owner: Arc<QueryOwner>,
    projection: Arc<Vec<usize>>,
    batch_rows: usize,
    limit: Option<usize>,
    properties: Arc<PlanProperties>,
}
impl DisplayAs for FixedScan {
    fn fmt_as(&self, _: DisplayFormatType, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FixedStaticScan: source={}, complete={}",
            self.snapshot.source_sha256, self.snapshot.complete
        )
    }
}
impl ExecutionPlan for FixedScan {
    fn name(&self) -> &'static str {
        "FixedStaticScan"
    }
    fn properties(&self) -> &Arc<PlanProperties> {
        &self.properties
    }
    fn children(&self) -> Vec<&Arc<dyn ExecutionPlan>> {
        Vec::new()
    }
    fn with_new_children(
        self: Arc<Self>,
        children: Vec<Arc<dyn ExecutionPlan>>,
    ) -> DfResult<Arc<dyn ExecutionPlan>> {
        if !children.is_empty() {
            return Err(unsupported("Fixed scan cannot acquire child plans"));
        }
        Ok(self)
    }
    fn execute(
        &self,
        partition: usize,
        _: Arc<TaskContext>,
    ) -> DfResult<SendableRecordBatchStream> {
        self.owner.check()?;
        if partition != 0 {
            return Err(unsupported("Fixed snapshot has exactly one partition"));
        }
        self.owner.retain(1024)?;
        Ok(Box::pin(FixedStream {
            snapshot: self.snapshot.clone(),
            owner: self.owner.clone(),
            schema: self.schema(),
            projection: self.projection.clone(),
            batch_rows: self.batch_rows,
            remaining: self.limit.unwrap_or(usize::MAX),
            batch: 0,
            offset: 0,
            done: false,
        }))
    }
}
struct FixedStream {
    snapshot: Arc<FixedSnapshot>,
    owner: Arc<QueryOwner>,
    schema: SchemaRef,
    projection: Arc<Vec<usize>>,
    batch_rows: usize,
    remaining: usize,
    batch: usize,
    offset: usize,
    done: bool,
}
impl FixedStream {
    fn next_batch(&mut self) -> DfResult<Option<RecordBatch>> {
        self.owner.check()?;
        if self.remaining == 0 || self.batch == self.snapshot.batches.len() {
            return Ok(None);
        }
        let source = &self.snapshot.batches[self.batch].records;
        let rows = (source.num_rows() - self.offset)
            .min(self.remaining)
            .min(self.batch_rows);
        // Slicing only aliases existing buffers. Admit array/vector metadata
        // before making it and retain through the qualification result owner.
        self.owner.retain(
            add(
                512,
                multiply(self.projection.len(), 1024).map_err(external)?,
            )
            .map_err(external)?,
        )?;
        self.owner
            .work(rows.saturating_add(self.projection.len()))?;
        let arrays = self
            .projection
            .iter()
            .map(|index| source.column(*index).slice(self.offset, rows))
            .collect();
        let records = RecordBatch::try_new_with_options(
            self.schema.clone(),
            arrays,
            &RecordBatchOptions::new().with_row_count(Some(rows)),
        )?;
        self.offset += rows;
        self.remaining -= rows;
        if self.offset == source.num_rows() {
            self.batch += 1;
            self.offset = 0;
        }
        Ok(Some(records))
    }
}
impl Stream for FixedStream {
    type Item = DfResult<RecordBatch>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.done {
            return Poll::Ready(None);
        }
        match self.next_batch() {
            Ok(Some(records)) => Poll::Ready(Some(Ok(records))),
            Ok(None) => {
                self.done = true;
                Poll::Ready(None)
            }
            Err(error) => {
                self.done = true;
                Poll::Ready(Some(Err(error)))
            }
        }
    }
}
impl RecordBatchStream for FixedStream {
    fn schema(&self) -> SchemaRef {
        self.schema.clone()
    }
}
