//! Authenticated static/effective sources for the production Arrow engine.
//! Generation reuse is an explicit representation choice, never a source or
//! Hotfix fallback. Each retained buffer keeps its original admission owner.
pub(crate) mod demand;
use super::{
    ImportLimits, ImportedBatchAllocation, ImportedSchema, Reuse, StaticIdentity, StaticSource,
    TableImporter, import,
};
use crate::{
    capture, db2, fault,
    local_query::{effective, provider::Physical},
    relational::Scalar,
    remote::provider::FileScope,
    resource::{Budget, Cost, OwnedRetainedReservation, RetainedReservation},
    table,
};
use arrow_array::{
    ArrayRef, RecordBatch,
    builder::{
        ArrayBuilder, FixedSizeListBuilder, Float32Builder, Int64Builder, StringBuilder,
        UInt64Builder,
    },
};
use arrow_buffer::{MemoryPool, MemoryReservation};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use lycheedev_core::{Invocation, Result, ToolkitError};
use serde_json::{Value, json};
use std::{fmt, sync::Arc};

// Batch sizing is a production policy, not an aggregate table limit. All
// retained batches compete in the original session Budget before allocation.
const BATCH_ROWS: usize = 1024;
const BATCH_VALUES: usize = 1 << 20;

pub(crate) struct Materialized {
    pub(crate) schema: SchemaRef,
    pub(crate) batches: Vec<RecordBatch>,
    pub(crate) provenance: Value,
    pub(crate) complete: bool,
    pub(crate) rows: usize,
    pub(crate) selective: bool,
    pub(crate) covered_id_ranges: Vec<(u32, u32)>,
    pub(crate) confirmed_missing_ids: Vec<u32>,
    _schema_owner: Arc<SchemaOwner>,
    _vector: OwnedRetainedReservation,
    _metadata: OwnedRetainedReservation,
}

struct SchemaOwner {
    schema: SchemaRef,
    _original: Arc<ImportedSchema>,
    _extra: Option<OwnedRetainedReservation>,
}
enum Admission {
    Imported(ImportedBatchAllocation),
    Generation(OwnedRetainedReservation),
    Effective(OwnedRetainedReservation),
}
/// There is deliberately no Array, Buffer or RecordBatch in this owner.
struct BufferOwner {
    _admission: Admission,
    _schema: Arc<SchemaOwner>,
    _hooks: OwnedRetainedReservation,
}
impl fmt::Debug for BufferOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OriginalSourceAdmission")
    }
}
#[derive(Debug)]
struct InputPool(Arc<BufferOwner>);
#[derive(Debug)]
struct InputLease {
    bytes: usize,
    _owner: Arc<BufferOwner>,
}
impl MemoryPool for InputPool {
    fn reserve(&self, bytes: usize) -> Box<dyn MemoryReservation> {
        Box::new(InputLease {
            bytes,
            _owner: self.0.clone(),
        })
    }
    fn available(&self) -> isize {
        0
    }
    fn used(&self) -> usize {
        0
    }
    fn capacity(&self) -> usize {
        0
    }
}
impl MemoryReservation for InputLease {
    fn size(&self) -> usize {
        self.bytes
    }
    fn resize(&mut self, bytes: usize) {
        self.bytes = bytes;
    }
}

fn keep(
    records: RecordBatch,
    admission: Admission,
    schema: &Arc<SchemaOwner>,
    budget: &Arc<Budget>,
) -> Result<RecordBatch> {
    // Known primitive/string and one-level fixed-list input representation:
    // four hooks per field conservatively cover values, offsets and validity.
    let bytes = add(1024, mul(add(mul(records.num_columns(), 4)?, 1)?, 256)?)?;
    let hooks = budget.reserve_retained(bytes)?.into_owned(budget)?;
    let owner = Arc::new(BufferOwner {
        _admission: admission,
        _schema: schema.clone(),
        _hooks: hooks,
    });
    records.claim(&InputPool(owner));
    Ok(records)
}

struct Batches<'a> {
    records: Vec<RecordBatch>,
    allocation: RetainedReservation<'a>,
    admitted: usize,
    rows: usize,
}
impl<'a> Batches<'a> {
    fn new(budget: &'a Budget) -> Result<Self> {
        Ok(Self {
            records: Vec::new(),
            allocation: budget.reserve_retained(0)?,
            admitted: 0,
            rows: 0,
        })
    }
    fn push(&mut self, records: RecordBatch) -> Result<()> {
        if records.num_rows() == 0 {
            return Err(shape());
        }
        if self.records.len() == self.records.capacity() {
            let next = self
                .records
                .capacity()
                .max(4)
                .checked_mul(2)
                .ok_or_else(limit)?;
            // Both old and new backing vectors can coexist during relocation.
            let bound = mul(next, mul(std::mem::size_of::<RecordBatch>(), 2)?)?;
            self.allocation
                .grow(bound.checked_sub(self.admitted).ok_or_else(limit)?)?;
            self.admitted = bound;
            self.records
                .try_reserve_exact(next - self.records.len())
                .map_err(|_| limit())?;
        }
        self.rows = add(self.rows, records.num_rows())?;
        self.records.push(records);
        Ok(())
    }
}

pub(crate) fn prepare(
    invocation: &Invocation,
    source: db2::Prepared,
    scope: &mut dyn FileScope,
    is_effective: bool,
    capture_ids: &[String],
    selected_ids: Option<&[u32]>,
    budget: Arc<Budget>,
) -> Result<Materialized> {
    invocation.control.check()?;
    budget.check()?;
    scope.check_consumer(invocation, &budget)?;
    let identity = StaticIdentity::from_prepared(&source)?;
    let complete = identity.complete();
    let source_sha256 = identity.digest(&budget)?;
    let data = source.pin.data.as_ref().ok_or_else(shape)?;
    let view = table::open_budgeted(
        &source.file.bytes,
        &source.definition,
        source.limits,
        &source.availability.skipped,
        &source.availability.ranges,
        budget.clone(),
    )?;
    // Lookup currently names the public ID column. A locator is safe only
    // when this column is the authenticated physical record identity.
    let locator_safe = view
        .columns()
        .iter()
        .any(|field| field.identity && !field.array && field.name == "ID");
    let visible = import::schema_for_table(&view, &budget)?;
    let original_schema = visible.schema().clone();
    let (schema, extra) = if is_effective {
        let bytes = add(4096, mul(original_schema.fields().len(), 256)?)?;
        let allocation = budget.reserve_retained(bytes)?.into_owned(&budget)?;
        let mut fields = original_schema.fields().iter().cloned().collect::<Vec<_>>();
        fields.extend([
            Arc::new(Field::new("__hotfix_source", DataType::Utf8, true)),
            Arc::new(Field::new("__hotfix_push", DataType::Int64, true)),
            Arc::new(Field::new("__hotfix_index", DataType::UInt64, true)),
        ]);
        (Arc::new(Schema::new(fields)), Some(allocation))
    } else {
        (original_schema, None)
    };
    let schema_owner = Arc::new(SchemaOwner {
        schema: schema.clone(),
        _original: visible,
        _extra: extra,
    });
    // The same authenticated manifest/object path used by ordinary static SQL.
    // Missing-body/unsupported representation are explicit cache misses only;
    // corruption, denial, cancellation and close failures never take this path.
    let _cache_metadata = budget.reserve_retained(8192)?;
    let reuse = super::open_existing(
        scope,
        StaticSource {
            data,
            bundle: &source.bundle,
            definition: &source.definition,
            layout: &source.layout,
            reading: &source.file.reading,
            missing: source.file.bytes.missing(),
            unavailable_partitions: &source.availability.skipped,
            bytes: source.file.bytes.len(),
        },
        &view,
        source.limits.file_bytes,
        &budget,
    )?;
    let (generation, cache) = match reuse {
        Reuse::Hit(generation) => {
            let cache = json!({"status":"hit","generation":generation.key,
                "object":generation.object,"reused":true,"publicationThisCommand":false});
            (Some(generation), cache)
        }
        Reuse::Absent => (
            None,
            json!({"status":"miss","reason":"not_prepared",
            "reused":false,"source":"authenticated-wdc","publicationThisCommand":false}),
        ),
        Reuse::UnsupportedSchema => (
            None,
            json!({"status":"unavailable",
            "reason":"schema_representation_limit","reused":false,
            "source":"authenticated-wdc","publicationThisCommand":false}),
        ),
        Reuse::MissingBody { generation, object } => (
            None,
            json!({"status":"unavailable",
            "reason":"materialized_body_missing","generation":generation,"object":object,
            "reused":false,"source":"authenticated-wdc","publicationThisCommand":false}),
        ),
    };
    let result = (|| {
        let mut batches = Batches::new(&budget)?;
        let selective = selected_ids.filter(|_| locator_safe);
        let _selection =
            budget.reserve_retained(add(256, mul(selective.map_or(0, |ids| ids.len()), 256)?)?)?;
        let mut found = selective.map(|ids| vec![false; ids.len()]);
        let mut confirmed_missing_ids = Vec::new();
        let mut deletion_admission = budget.reserve_retained(0)?;
        let mut covered_id_ranges = Vec::with_capacity(selective.map_or(0, |ids| ids.len()));
        let mut decoded_batches = 0usize;
        let overlay_report = if is_effective {
            let mut base = Physical::new(view, budget.clone())?;
            base.generation = generation.clone();
            let mut snapshots = effective::Snapshots::new(invocation, data, &budget);
            let (plan, report) = snapshots.prepare(
                &base.schema().columns,
                base.schema().complete,
                &source.definition,
                &source.bundle,
                capture_ids,
            )?;
            // Tombstones name physical record identity, not an arbitrary
            // visible field called ID. Never transfer proofs across domains.
            if locator_safe {
                deletion_admission.grow(mul(plan.patch_count(), std::mem::size_of::<u32>())?)?;
                confirmed_missing_ids = Vec::with_capacity(plan.patch_count());
                for id in plan.deleted_ids() {
                    budget.charge(Cost::work(1))?;
                    confirmed_missing_ids.push(id);
                }
            }
            let mut cursor: Box<dyn crate::relational::RowCursor + '_> =
                if let Some(ids) = selective {
                    Box::new(SelectedEffective {
                        plan: &plan,
                        base: &base,
                        budget: &budget,
                        ids,
                        found: found.as_mut().ok_or_else(shape)?,
                        next: 0,
                        current_slot: 0,
                        current: None,
                        done: false,
                    })
                } else {
                    plan.cursor(&base, &budget)?
                };
            import_effective(cursor.as_mut(), &schema_owner, &budget, &mut batches)?;
            // The cursor is gone before IPC close and before the source closes.
            drop(cursor);
            Some(report)
        } else {
            if let Some(generation) = &generation {
                let _indices = budget.reserve_retained(mul(
                    selective.map_or(generation.batch_count(), |ids| ids.len()),
                    std::mem::size_of::<usize>(),
                )?)?;
                let indices = if let Some(ids) = selective {
                    let mut indices = Vec::with_capacity(ids.len());
                    for id in ids {
                        budget.charge(Cost::work(1))?;
                        if let Some(index) = generation.locate_batch(*id) {
                            if indices.last() != Some(&index) {
                                indices.push(index);
                            }
                        }
                    }
                    indices
                } else {
                    (0..generation.batch_count()).collect()
                };
                for index in indices {
                    invocation.control.check()?;
                    budget.check()?;
                    let batch = generation.read_batch(index)?;
                    let (physical, allocation) = batch.into_parts();
                    decoded_batches += 1;
                    if physical.num_columns() != schema.fields().len() + 1 {
                        return Err(shape());
                    }
                    if let (Some(ids), Some(found)) = (selective, found.as_mut()) {
                        let record_ids = physical
                            .column(schema.fields().len())
                            .as_any()
                            .downcast_ref::<arrow_array::UInt32Array>()
                            .ok_or_else(shape)?;
                        let first = *record_ids.values().first().ok_or_else(shape)?;
                        let last = *record_ids.values().last().ok_or_else(shape)?;
                        covered_id_ranges.push((first, last));
                        budget.charge(Cost::work(ids.len()))?;
                        for (slot, id) in ids.iter().enumerate() {
                            if slot % 1024 == 0 {
                                budget.check()?;
                            }
                            found[slot] |= record_ids.values().binary_search(id).is_ok();
                        }
                    }
                    // Exclude the authenticated private record-ID locator.
                    let _projection =
                        budget.reserve_retained(add(256, mul(schema.fields().len(), 64)?)?)?;
                    let arrays = physical.columns()[..schema.fields().len()].to_vec();
                    let records =
                        RecordBatch::try_new(schema.clone(), arrays).map_err(|_| shape())?;
                    batches.push(keep(
                        records,
                        Admission::Generation(allocation),
                        &schema_owner,
                        &budget,
                    )?)?;
                }
            } else {
                let mut importer = TableImporter::new(
                    &view,
                    budget.clone(),
                    ImportLimits {
                        rows: BATCH_ROWS,
                        value_bytes: BATCH_VALUES,
                        text_bytes: 1 << 20,
                    },
                )?;
                if let Some(ids) = selective {
                    importer = importer.select_ids(ids)?;
                }
                while let Some(batch) = importer.next_batch()? {
                    invocation.control.check()?;
                    if batch.ordinal() != batches.records.len() {
                        return Err(shape());
                    }
                    let (records, ids, _, allocation) = batch.into_parts();
                    decoded_batches += 1;
                    if let (Some(requested), Some(found)) = (selective, found.as_mut()) {
                        budget.charge(Cost::work(requested.len()))?;
                        for (slot, id) in requested.iter().enumerate() {
                            if slot % 1024 == 0 {
                                budget.check()?;
                            }
                            found[slot] |= ids.values().binary_search(id).is_ok();
                        }
                    }
                    drop(ids);
                    batches.push(keep(
                        records,
                        Admission::Imported(allocation),
                        &schema_owner,
                        &budget,
                    )?)?;
                }
            }
            if selective.is_none() && batches.rows != view.logical_count() {
                return Err(shape());
            }
            None
        };
        let selection_report = if let Some(ids) = selective {
            let found = found.as_ref().ok_or_else(shape)?;
            let absent = ids
                .iter()
                .zip(found)
                .filter_map(|(id, found)| (!found).then_some(*id))
                .collect::<Vec<_>>();
            json!({"kind":if is_effective { "authenticated_effective_id_lookup" } else if generation.is_some() { "authenticated_id_locator_batches" } else { "authenticated_wdc_id_lookup" }, "requestedIds":ids,
                "materializationDecodedBatches":if is_effective { None } else { Some(decoded_batches) }, "candidateRows":batches.rows,
                "scope":"source_preparation_not_current_task", "directoryAuthentication":"full_existing_validation_unchanged",
                "missingIds":absent.iter().filter(|id| complete || confirmed_missing_ids.binary_search(id).is_ok()).copied().collect::<Vec<_>>(),
                "unresolvedIds":absent.iter().filter(|id| !complete && confirmed_missing_ids.binary_search(id).is_err()).copied().collect::<Vec<_>>(),
                "coverageComplete":complete})
        } else {
            json!({"kind":"full_source", "selectionAppliedByPlan": selected_ids.is_some()})
        };
        let metadata_size = capture::json_size(
            &(&identity, &cache, &overlay_report, &selection_report),
            &budget,
        )?;
        let metadata = budget
            .reserve_retained(add(
                add(mul(metadata_size, 8)?, 8192)?,
                add(
                    mul(
                        covered_id_ranges.capacity(),
                        std::mem::size_of::<(u32, u32)>(),
                    )?,
                    mul(confirmed_missing_ids.capacity(), std::mem::size_of::<u32>())?,
                )?,
            )?)?
            .into_owned(&budget)?;
        let provenance = json!({
            "sourceMode": if is_effective { "effective" } else { "static" },
            "sourceIdentity": identity,
            "sourceSha256": source_sha256,
            "typedGenerationCache": cache,
            "effective": overlay_report,
            "sourceSelection": selection_report,
            "complete": complete,
            "truncated": false,
            "availableRows": batches.rows,
            "resourceAccounting": {
                "wholeQueryAccountingComplete": false,
                "sourceBuffers": "original-admission-retained-by-arrow-buffer",
                "preparation": "lazy-per-table-budgeted-materialization",
                "remainingGaps": ["uninterruptible single codec call", "DataFusion native operator internals"]
            }
        });
        invocation.control.check()?;
        budget.check()?;
        Ok(Materialized {
            schema,
            batches: batches.records,
            rows: batches.rows,
            selective: selective.is_some(),
            covered_id_ranges,
            confirmed_missing_ids,
            provenance,
            complete,
            _schema_owner: schema_owner,
            _vector: batches.allocation.into_owned(&budget)?,
            _metadata: metadata,
        })
    })();
    // A primary failure and a close failure are both preserved. No early `?`
    // after acquiring a generation reader bypasses its checked close.
    crate::remote::admission::finish(
        result,
        generation
            .as_ref()
            .and_then(|generation| generation.close().err()),
        Ok(()),
        Ok(()),
    )
}

fn limit() -> ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Source Arrow allocation exceeds its admitted bound",
    )
}
fn shape() -> ToolkitError {
    fault(
        "records.generation_type",
        4,
        "Source value differs from its exact typed schema",
    )
}
fn add(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(limit)
}
fn mul(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(limit)
}

#[derive(Clone, Copy)]
enum Kind {
    Signed,
    Unsigned,
    Float,
    Text,
}
impl Kind {
    fn from_type(kind: &DataType) -> Result<Self> {
        match kind {
            DataType::Int64 => Ok(Self::Signed),
            DataType::UInt64 => Ok(Self::Unsigned),
            DataType::Float32 => Ok(Self::Float),
            DataType::Utf8 => Ok(Self::Text),
            _ => Err(shape()),
        }
    }
    fn width(self) -> usize {
        match self {
            Self::Signed | Self::Unsigned => 8,
            Self::Float | Self::Text => 4,
        }
    }
    fn builder(self, rows: usize) -> Box<dyn ArrayBuilder> {
        match self {
            Self::Signed => Box::new(Int64Builder::with_capacity(rows)),
            Self::Unsigned => Box::new(UInt64Builder::with_capacity(rows)),
            Self::Float => Box::new(Float32Builder::with_capacity(rows)),
            Self::Text => Box::new(StringBuilder::with_capacity(rows, 0)),
        }
    }
    fn inspect(self, value: &Scalar) -> Result<usize> {
        match (self, value) {
            (_, Scalar::Null) => Ok(self.width()),
            (Self::Signed, Scalar::Int(_)) | (Self::Unsigned, Scalar::UInt(_)) => Ok(8),
            // JSON-backed Hotfix fields may use the other integer variant, but
            // conversion must be exact and checked, never via binary float.
            (Self::Signed, Scalar::UInt(value)) if i64::try_from(*value).is_ok() => Ok(8),
            (Self::Unsigned, Scalar::Int(value)) if *value >= 0 => Ok(8),
            (Self::Float, Scalar::Float(value))
                if value.is_finite()
                    && (*value as f32).is_finite()
                    && f64::from(*value as f32) == *value =>
            {
                Ok(4)
            }
            (Self::Float, Scalar::Int(value)) if (*value as f32) as i128 == i128::from(*value) => {
                Ok(4)
            }
            (Self::Float, Scalar::UInt(value)) if (*value as f32) as u128 == u128::from(*value) => {
                Ok(4)
            }
            (Self::Text, Scalar::String(value)) => add(4, value.len()),
            _ => Err(shape()),
        }
    }
    fn append(self, builder: &mut dyn ArrayBuilder, value: &Scalar) {
        // `inspect` validates the complete row before any builder mutation.
        match self {
            Self::Signed => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<Int64Builder>()
                    .expect("signed builder");
                match value {
                    Scalar::Int(v) => out.append_value(*v),
                    Scalar::UInt(v) => out.append_value(*v as i64),
                    _ => out.append_null(),
                }
            }
            Self::Unsigned => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<UInt64Builder>()
                    .expect("unsigned builder");
                match value {
                    Scalar::UInt(v) => out.append_value(*v),
                    Scalar::Int(v) => out.append_value(*v as u64),
                    _ => out.append_null(),
                }
            }
            Self::Float => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<Float32Builder>()
                    .expect("float builder");
                match value {
                    Scalar::Float(v) => out.append_value(*v as f32),
                    Scalar::Int(v) => out.append_value(*v as f32),
                    Scalar::UInt(v) => out.append_value(*v as f32),
                    _ => out.append_null(),
                }
            }
            Self::Text => {
                let out = builder
                    .as_any_mut()
                    .downcast_mut::<StringBuilder>()
                    .expect("text builder");
                match value {
                    Scalar::String(v) => out.append_value(v),
                    _ => out.append_null(),
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
struct Column {
    kind: Kind,
    width: Option<usize>,
}
impl Column {
    fn from_type(kind: &DataType) -> Result<Self> {
        let (kind, width) = match kind {
            DataType::FixedSizeList(field, width) if *width > 0 => {
                (field.data_type(), Some(*width as usize))
            }
            _ => (kind, None),
        };
        Ok(Self {
            kind: Kind::from_type(kind)?,
            width,
        })
    }
    fn builder(self, rows: usize) -> Result<Builder> {
        match self.width {
            None => Ok(Builder::Scalar(self.kind.builder(rows))),
            Some(width) => Ok(Builder::List(FixedSizeListBuilder::with_capacity(
                self.kind.builder(mul(rows, width)?),
                width as i32,
                rows,
            ))),
        }
    }
}
enum Builder {
    Scalar(Box<dyn ArrayBuilder>),
    List(FixedSizeListBuilder<Box<dyn ArrayBuilder>>),
}
impl Builder {
    fn finish(self) -> ArrayRef {
        match self {
            Self::Scalar(mut builder) => builder.finish(),
            Self::List(mut builder) => Arc::new(builder.finish()),
        }
    }
}

fn import_effective(
    cursor: &mut dyn crate::relational::RowCursor,
    schema: &Arc<SchemaOwner>,
    budget: &Arc<Budget>,
    batches: &mut Batches<'_>,
) -> Result<()> {
    let _columns = budget.reserve_retained(add(1024, mul(schema.schema.fields().len(), 128)?)?)?;
    let columns = schema
        .schema
        .fields()
        .iter()
        .map(|field| Column::from_type(field.data_type()))
        .collect::<Result<Vec<_>>>()?;
    let cells = columns
        .iter()
        .try_fold(0usize, |n, column| add(n, column.width.unwrap_or(1)))?;
    let row_bytes = columns.iter().try_fold(0usize, |n, column| {
        add(n, mul(column.width.unwrap_or(1), column.kind.width())?)
    })?;
    if row_bytes > BATCH_VALUES || cells == 0 {
        return Err(limit());
    }
    let rows_per_batch = BATCH_ROWS.min((BATCH_VALUES / row_bytes.max(1)).max(1));
    let fixed = mul(rows_per_batch, add(row_bytes, cells)?)?;
    let reservation = add(
        add(mul(fixed, 2)?, mul(BATCH_VALUES, 3)?)?,
        add(mul(columns.len(), 2048)?, 4096)?,
    )?;
    let mut pending = cursor.advance()?;
    while pending {
        budget.check()?;
        let allocation = budget.reserve_retained(reservation)?;
        let mut builders = columns
            .iter()
            .map(|column| column.builder(rows_per_batch))
            .collect::<Result<Vec<_>>>()?;
        let mut rows = 0usize;
        let mut bytes = 0usize;
        while pending && rows < rows_per_batch {
            budget.check()?;
            let values = cursor.row();
            if values.len() != cells {
                return Err(shape());
            }
            let mut position = 0usize;
            let mut row_size = 0usize;
            for column in &columns {
                for value in &values[position..position + column.width.unwrap_or(1)] {
                    row_size = add(row_size, column.kind.inspect(value)?)?;
                }
                position += column.width.unwrap_or(1);
            }
            if row_size > BATCH_VALUES {
                return Err(limit());
            }
            if add(bytes, row_size)? > BATCH_VALUES {
                break;
            }
            budget.charge(Cost::work(row_size))?;
            let mut position = 0usize;
            for (column, builder) in columns.iter().zip(&mut builders) {
                match builder {
                    Builder::Scalar(builder) => {
                        column.kind.append(builder.as_mut(), &values[position])
                    }
                    Builder::List(builder) => {
                        let width = column.width.ok_or_else(shape)?;
                        for value in &values[position..position + width] {
                            column.kind.append(builder.values().as_mut(), value);
                        }
                        builder.append(true);
                    }
                }
                position += column.width.unwrap_or(1);
            }
            rows += 1;
            bytes += row_size;
            pending = cursor.advance()?;
        }
        let arrays = builders.into_iter().map(Builder::finish).collect();
        let records = RecordBatch::try_new(schema.schema.clone(), arrays).map_err(|_| shape())?;
        batches.push(keep(
            records,
            Admission::Effective(allocation.into_owned(budget)?),
            schema,
            budget,
        )?)?;
    }
    budget.check()
}

/// Overlay equality resolves replacement/deletion/addition before touching the
/// base. Each producer and its row leases remain owned until advance/drop.
struct SelectedEffective<'a> {
    plan: &'a effective::OverlayPlan,
    base: &'a dyn crate::relational::RowProvider,
    budget: &'a Budget,
    ids: &'a [u32],
    found: &'a mut [bool],
    next: usize,
    current_slot: usize,
    current: Option<Box<dyn crate::relational::RowCursor + 'a>>,
    done: bool,
}
impl SelectedEffective<'_> {
    fn advance_inner(&mut self) -> Result<bool> {
        loop {
            self.budget.check()?;
            if let Some(cursor) = &mut self.current {
                if cursor.advance()? {
                    self.found[self.current_slot] = true;
                    return Ok(true);
                }
            }
            self.current = None;
            let Some(id) = self.ids.get(self.next) else {
                self.done = true;
                return Ok(false);
            };
            self.current_slot = self.next;
            self.next += 1;
            self.budget.charge(Cost::work(1))?;
            match self.plan.equal_cursor(
                self.base,
                self.budget,
                "ID",
                &Scalar::UInt(u64::from(*id)),
            )? {
                crate::relational::EqualityCursor::Empty => {}
                crate::relational::EqualityCursor::Rows(cursor) => self.current = Some(cursor),
                crate::relational::EqualityCursor::Declined => return Err(shape()),
            }
        }
    }
}
impl crate::relational::RowCursor for SelectedEffective<'_> {
    fn advance(&mut self) -> Result<bool> {
        if self.done {
            return Ok(false);
        }
        let result = self.advance_inner();
        if result.is_err() {
            self.current = None;
            self.done = true;
        }
        result
    }
    fn row(&self) -> &[Scalar] {
        self.current.as_ref().map_or(&[], |cursor| cursor.row())
    }
}
