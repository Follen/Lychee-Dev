//! One authenticated Table decoder feeds bounded Arrow batches. No JSON
//! roundtrip, numeric coercion or second WDC implementation is involved.
use crate::{
    fault,
    resource::{Budget, Cost, OwnedRetainedReservation},
    schema,
    table::{self, OwnedLeasedRow, Table},
};
use arrow_array::{
    Array, ArrayRef, RecordBatch, UInt32Array,
    builder::{
        ArrayBuilder, FixedSizeListBuilder, Float32Builder, Int64Builder, StringBuilder,
        UInt32Builder, UInt64Builder,
    },
};
use arrow_schema::{DataType, Field, Schema, SchemaRef};
use lycheedev_core::{Result, ToolkitError};
use std::sync::Arc;

fn limit() -> ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Typed table batch exceeds its admitted bound",
    )
}
fn shape() -> ToolkitError {
    fault(
        "records.generation_type",
        4,
        "Decoded table value differs from its exact typed schema",
    )
}
fn plus(a: usize, b: usize) -> Result<usize> {
    a.checked_add(b).ok_or_else(limit)
}
fn times(a: usize, b: usize) -> Result<usize> {
    a.checked_mul(b).ok_or_else(limit)
}

#[derive(Clone, Copy)]
pub(crate) struct ImportLimits {
    pub rows: usize,
    /// Values in one batch, including all array elements and UTF-8 bytes.
    pub value_bytes: usize,
    /// The existing decoder's whole-row text bound, unchanged by Arrow.
    pub text_bytes: usize,
}
impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            rows: 1024,
            value_bytes: 16 << 20,
            text_bytes: 1 << 20,
        }
    }
}
impl ImportLimits {
    fn validate(self) -> Result<Self> {
        if self.rows == 0
            || self.rows > 65536
            || self.value_bytes == 0
            || self.value_bytes > 512 << 20
            || self.text_bytes == 0
            || self.text_bytes > 1 << 20
        {
            return Err(limit());
        }
        Ok(self)
    }
}

/// The Arrow allocations and the ID locator share one refund owner. Consumers
/// retaining cloned Arrow arrays must retain this entire owner as well.
/// Fields drop before `_allocation`; releasing a cursor cannot refund a batch
/// that its consumer still owns.
pub(crate) struct ImportedBatch {
    records: RecordBatch,
    record_ids: UInt32Array,
    ordinal: usize,
    _schema_owner: Arc<ImportedSchema>,
    _allocation: OwnedRetainedReservation,
}
/// Array-free reservation owner for consumers that attach its lifetime to
/// Arrow's shared buffers. It must never own a RecordBatch or an ArrayRef.
pub(crate) struct ImportedBatchAllocation {
    _schema_owner: Arc<ImportedSchema>,
    _allocation: OwnedRetainedReservation,
}
impl ImportedBatchAllocation {
    pub(crate) fn retained_bytes(&self) -> usize {
        self._allocation.bytes()
    }
}
pub(crate) struct ImportedSchema {
    schema: SchemaRef,
    _allocation: OwnedRetainedReservation,
}
impl ImportedSchema {
    pub(super) fn schema(&self) -> &SchemaRef {
        &self.schema
    }
}
impl ImportedBatch {
    pub(crate) fn into_parts(self) -> (RecordBatch, UInt32Array, usize, ImportedBatchAllocation) {
        (
            self.records,
            self.record_ids,
            self.ordinal,
            ImportedBatchAllocation {
                _schema_owner: self._schema_owner,
                _allocation: self._allocation,
            },
        )
    }
    pub(crate) fn records(&self) -> &RecordBatch {
        &self.records
    }
    pub(crate) fn record_ids(&self) -> &UInt32Array {
        &self.record_ids
    }
    pub(crate) fn ordinal(&self) -> usize {
        self.ordinal
    }
    pub(crate) fn locate(&self, record_id: u32) -> Option<usize> {
        self.record_ids
            .values()
            .as_ref()
            .binary_search(&record_id)
            .ok()
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Signed,
    Unsigned,
    Float32,
    Text,
}
impl Kind {
    fn data_type(self) -> DataType {
        match self {
            Self::Signed => DataType::Int64,
            Self::Unsigned => DataType::UInt64,
            Self::Float32 => DataType::Float32,
            Self::Text => DataType::Utf8,
        }
    }
    fn width(self) -> usize {
        match self {
            Self::Signed | Self::Unsigned => 8,
            Self::Float32 => 4,
            Self::Text => 4,
        }
    }
    fn builder(self, capacity: usize) -> Box<dyn ArrayBuilder> {
        match self {
            Self::Signed => Box::new(Int64Builder::with_capacity(capacity)),
            Self::Unsigned => Box::new(UInt64Builder::with_capacity(capacity)),
            Self::Float32 => Box::new(Float32Builder::with_capacity(capacity)),
            // String data grows only after the whole row has passed the shared
            // value-byte bound. Its geometric growth and realloc overlap are
            // reserved before any builder is constructed.
            Self::Text => Box::new(StringBuilder::with_capacity(capacity, 0)),
        }
    }
    fn inspect(self, value: &table::Value) -> Result<usize> {
        match (self, value) {
            (_, table::Value::Null) => Ok(self.width()),
            (Self::Signed, table::Value::Int(_)) | (Self::Unsigned, table::Value::UInt(_)) => Ok(8),
            (Self::Float32, table::Value::Float32(v)) if v.is_finite() => Ok(4),
            (Self::Text, table::Value::String(value)) => plus(4, value.len()),
            _ => Err(shape()),
        }
    }
    fn append(self, builder: &mut dyn ArrayBuilder, value: &table::Value) {
        // inspect() already checked this exact row before any append.
        match self {
            Self::Signed => {
                let builder = builder
                    .as_any_mut()
                    .downcast_mut::<Int64Builder>()
                    .expect("owned signed builder");
                match value {
                    table::Value::Int(value) => builder.append_value(*value),
                    _ => builder.append_null(),
                }
            }
            Self::Unsigned => {
                let builder = builder
                    .as_any_mut()
                    .downcast_mut::<UInt64Builder>()
                    .expect("owned unsigned builder");
                match value {
                    table::Value::UInt(value) => builder.append_value(*value),
                    _ => builder.append_null(),
                }
            }
            Self::Float32 => {
                let builder = builder
                    .as_any_mut()
                    .downcast_mut::<Float32Builder>()
                    .expect("owned float builder");
                match value {
                    table::Value::Float32(value) => builder.append_value(*value),
                    _ => builder.append_null(),
                }
            }
            Self::Text => {
                let builder = builder
                    .as_any_mut()
                    .downcast_mut::<StringBuilder>()
                    .expect("owned string builder");
                match value {
                    table::Value::String(value) => builder.append_value(value),
                    _ => builder.append_null(),
                }
            }
        }
    }
}
#[derive(Clone, Copy)]
struct Column {
    kind: Kind,
    elements: Option<usize>,
}
impl Column {
    fn from_field(field: &schema::Field) -> Result<Self> {
        // Table emits logical record/copy IDs and noninline relation IDs as
        // UInt regardless of the DBD signedness. Preserve that actual contract.
        let kind = if field.identity || !field.inline && field.relation {
            Kind::Unsigned
        } else {
            match field.kind.as_str() {
                "int" if (1..=64).contains(&field.bits) => {
                    if field.signed {
                        Kind::Signed
                    } else {
                        Kind::Unsigned
                    }
                }
                "float" => Kind::Float32,
                "string" | "locstring" => Kind::Text,
                _ => return Err(shape()),
            }
        };
        let elements = if field.array {
            if field.elements == 0 || field.elements > 65536 {
                return Err(shape());
            }
            Some(field.elements as usize)
        } else {
            None
        };
        Ok(Self { kind, elements })
    }
    fn data_type(self) -> DataType {
        match self.elements {
            Some(width) => DataType::FixedSizeList(
                Arc::new(Field::new("item", self.kind.data_type(), true)),
                width as i32,
            ),
            None => self.kind.data_type(),
        }
    }
    fn inspect(self, value: &table::Value) -> Result<usize> {
        match self.elements {
            None => self.kind.inspect(value),
            Some(width) => match value {
                table::Value::Null => times(width, self.kind.width()),
                table::Value::Array(values) if values.len() == width => values
                    .iter()
                    .try_fold(0, |bytes, value| plus(bytes, self.kind.inspect(value)?)),
                _ => Err(shape()),
            },
        }
    }
    fn builder(self, rows: usize) -> Result<Builder> {
        let child = self.kind.builder(times(rows, self.elements.unwrap_or(1))?);
        Ok(match self.elements {
            Some(width) => Builder::List(FixedSizeListBuilder::with_capacity(
                child,
                width as i32,
                rows,
            )),
            None => Builder::Scalar(child),
        })
    }
    fn append(self, builder: &mut Builder, value: &table::Value) {
        match builder {
            Builder::Scalar(builder) => self.kind.append(builder.as_mut(), value),
            Builder::List(builder) => match value {
                table::Value::Array(values) => {
                    for value in values {
                        self.kind.append(builder.values().as_mut(), value);
                    }
                    builder.append(true);
                }
                _ => {
                    for _ in 0..self.elements.expect("list width") {
                        self.kind
                            .append(builder.values().as_mut(), &table::Value::Null);
                    }
                    builder.append(false);
                }
            },
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

enum ImportTable<'table, 'body> {
    Borrowed(&'table Table<'body>),
    Owned(Arc<Table<'body>>),
}
impl<'body> std::ops::Deref for ImportTable<'_, 'body> {
    type Target = Table<'body>;
    fn deref(&self) -> &Self::Target {
        match self { Self::Borrowed(table) => table, Self::Owned(table) => table }
    }
}
pub(crate) struct TableImporter<'table, 'body> {
    table: ImportTable<'table, 'body>,
    selected_ids: Option<std::borrow::Cow<'table, [u32]>>,
    schema: Arc<ImportedSchema>,
    columns: Vec<Column>,
    budget: Arc<Budget>,
    limits: ImportLimits,
    batch_reservation: usize,
    next: usize,
    batch_ordinal: usize,
    pending: Option<(u32, OwnedLeasedRow<Vec<table::Value>>, usize)>,
    stopped: bool,
}
// One canonical schema builder serves importer and existing-generation lookup.
// Lookup must not inherit batch-size/import limits before it knows a cache hit.
fn prepare_schema(
    fields: &[schema::Field],
    budget: &Arc<Budget>,
) -> Result<(Arc<ImportedSchema>, Vec<Column>)> {
    budget.check()?;
    let schema_bytes = fields.iter().try_fold(512usize, |bytes, field| {
        plus(bytes, plus(field.name.len(), 512)?)
    })?;
    let schema_allocation = budget.reserve_retained(schema_bytes)?;
    let mut columns = Vec::with_capacity(fields.len());
    let mut arrow_fields = Vec::with_capacity(fields.len());
    for field in fields {
        budget.check()?;
        let column = Column::from_field(field)?;
        arrow_fields.push(Field::new(&field.name, column.data_type(), true));
        columns.push(column);
    }
    Ok((
        Arc::new(ImportedSchema {
            schema: Arc::new(Schema::new(arrow_fields)),
            _allocation: schema_allocation.into_owned(budget)?,
        }),
        columns,
    ))
}
pub(super) fn schema_for_table(
    table: &Table<'_>,
    budget: &Arc<Budget>,
) -> Result<Arc<ImportedSchema>> {
    Ok(prepare_schema(table.columns(), budget)?.0)
}
impl<'table, 'body> TableImporter<'table, 'body> {
    /// `budget` must be the same admitted query owner used to open `table`.
    pub(crate) fn new(
        table: &'table Table<'body>,
        budget: Arc<Budget>,
        limits: ImportLimits,
    ) -> Result<Self> {
        Self::new_inner(ImportTable::Borrowed(table), budget, limits)
    }
    fn new_inner(table: ImportTable<'table, 'body>, budget: Arc<Budget>, limits: ImportLimits) -> Result<Self> {
        budget.check()?;
        let mut limits = limits.validate()?;
        let fields = table.columns();
        let (schema, columns) = prepare_schema(fields, &budget)?;
        let mut row_bytes = 4usize;
        let mut cells = 1usize;
        for column in &columns {
            budget.check()?;
            let width = column.elements.unwrap_or(1);
            row_bytes = plus(row_bytes, times(width, column.kind.width())?)?;
            cells = plus(cells, width)?;
        }
        if row_bytes > limits.value_bytes {
            return Err(limit());
        }
        limits.rows = limits.rows.min((limits.value_bytes / row_bytes).max(1));
        // Fixed value buffers, offsets/validity, object/vector overhead and
        // worst-case simultaneous old/new geometric UTF-8 buffers. This is a
        // conservative admission bound, not an RSS measurement.
        let fixed = times(limits.rows, plus(row_bytes, cells)?)?;
        let overhead = plus(times(fields.len(), 2048)?, 4096)?;
        let batch_reservation = plus(
            plus(times(fixed, 2)?, times(limits.value_bytes, 3)?)?,
            overhead,
        )?;
        Ok(Self {
            table,
            selected_ids: None,
            schema,
            columns,
            budget,
            limits,
            batch_reservation,
            next: 0,
            batch_ordinal: 0,
            pending: None,
            stopped: false,
        })
    }
    pub(crate) fn schema(&self) -> &SchemaRef {
        &self.schema.schema
    }
    pub(crate) fn next_batch(&mut self) -> Result<Option<ImportedBatch>> {
        if self.stopped {
            return Err(fault(
                "records.generation_stopped",
                4,
                "Typed table importer cannot resume after an error",
            ));
        }
        let result = self.next_batch_inner();
        if result.is_err() {
            self.stopped = true;
            self.pending = None;
        }
        result
    }
    fn next_batch_inner(&mut self) -> Result<Option<ImportedBatch>> {
        self.budget.check()?;
        if self.pending.is_none()
            && self.next
                == self
                    .selected_ids.as_deref()
                    .map_or(self.table.logical_count(), <[u32]>::len)
        {
            return Ok(None);
        }
        let allocation = self.budget.reserve_retained(self.batch_reservation)?;
        let mut builders = self
            .columns
            .iter()
            .map(|column| column.builder(self.limits.rows))
            .collect::<Result<Vec<_>>>()?;
        let mut ids = UInt32Builder::with_capacity(self.limits.rows);
        let mut rows = 0usize;
        let mut bytes = 0usize;
        while rows < self.limits.rows {
            self.budget.check()?;
            let (id, row, row_bytes) = match self.pending.take() {
                Some(row) => row,
                None => {
                    let id = if let Some(ids) = self.selected_ids.as_deref() {
                        let mut found = None;
                        while let Some(id) = ids.get(self.next) {
                            self.budget.charge(Cost::work(32))?;
                            self.next += 1;
                            if self.table.contains_id(*id) {
                                found = Some(*id);
                                break;
                            }
                        }
                        let Some(id) = found else {
                            break;
                        };
                        id
                    } else {
                        let Some(id) = self.table.id_at(self.next) else {
                            break;
                        };
                        self.next += 1;
                        id
                    };
                    let row = self.table.leased_row_values(id, self.limits.text_bytes)?.into_owned(&self.budget)?;
                    if row.value().len() != self.columns.len() {
                        return Err(shape());
                    }
                    let row_bytes = self
                        .columns
                        .iter()
                        .zip(row.value())
                        .try_fold(4usize, |bytes, (column, value)| {
                            plus(bytes, column.inspect(value)?)
                        })?;
                    if row_bytes > self.limits.value_bytes {
                        return Err(limit());
                    }
                    (id, row, row_bytes)
                }
            };
            if plus(bytes, row_bytes)? > self.limits.value_bytes {
                self.pending = Some((id, row, row_bytes));
                break;
            }
            self.budget.charge(Cost::work(row_bytes))?;
            for ((column, builder), value) in
                self.columns.iter().zip(&mut builders).zip(row.value())
            {
                column.append(builder, value);
            }
            ids.append_value(id);
            bytes += row_bytes;
            rows += 1;
        }
        if rows == 0 {
            return Ok(None);
        }
        let arrays = builders.into_iter().map(Builder::finish).collect();
        let records =
            RecordBatch::try_new(self.schema.schema.clone(), arrays).map_err(|_| shape())?;
        let record_ids = ids.finish();
        let allocation = allocation.into_owned(&self.budget)?;
        let ordinal = self.batch_ordinal;
        self.batch_ordinal = plus(self.batch_ordinal, 1)?;
        Ok(Some(ImportedBatch {
            records,
            record_ids,
            ordinal,
            _schema_owner: self.schema.clone(),
            _allocation: allocation,
        }))
    }
}

impl TableImporter<'static, 'static> {
    pub(crate) fn new_owned(table: Arc<Table<'static>>, budget: Arc<Budget>, limits: ImportLimits) -> Result<Self> {
        Self::new_inner(ImportTable::Owned(table), budget, limits)
    }
    pub(crate) fn select_ids_owned(mut self, ids: Vec<u32>) -> Result<Self> {
        if self.next != 0 || self.batch_ordinal != 0 || ids.windows(2).any(|pair| pair[0] >= pair[1]) { return Err(shape()); }
        self.selected_ids = Some(std::borrow::Cow::Owned(ids));
        Ok(self)
    }
}
impl ImportedBatch {
    pub(crate) fn settle_retained(&mut self) -> Result<()> {
        let bytes = plus(plus(self.records.get_array_memory_size(), self.record_ids.get_array_memory_size())?,
            plus(4096, times(self.records.num_columns(), 2048)?)?)?;
        self._allocation.shrink_to(bytes)
    }
}
