//! Immutable native Arrow representation of an authenticated generation.
//!
//! Construction consumes every IPC batch, but neither closes nor retains the
//! reader. The caller MUST checked-close Generation before publishing this
//! snapshot, including on construction failure. Reuse executes exclusively
//! against the new caller's budget; original budgets survive only as guards.
use super::*;
use crate::generation::Generation;
use arrow_array::{Array, FixedSizeListArray, Float32Array};

struct IndexedBatch {
    batch: DemandBatch,
    // Actual physical IDs, not the SQL-visible column named ID. The vector's
    // ordinal is its native Arrow row offset; no scalar conversion is involved.
    index: Vec<u32>,
    _metadata: OwnedRetainedReservation,
}

pub(in crate::generation::engine_adapter) struct ArrowSnapshot {
    batches: Vec<IndexedBatch>,
    schema: Arc<SchemaOwner>,
    rows: usize,
    _metadata: OwnedRetainedReservation,
}

impl ArrowSnapshot {
    pub(in crate::generation::engine_adapter) fn from_generation(
        generation: &Arc<Generation>,
        schema: &Arc<SchemaOwner>,
        budget: &Arc<Budget>,
    ) -> Result<Self> {
        budget.check()?;
        let count = generation.batch_count();
        let metadata = budget.reserve_retained(add(
            1024,
            mul(count, std::mem::size_of::<IndexedBatch>())?,
        )?)?.into_owned(budget)?;
        let mut batches = Vec::new();
        batches.try_reserve_exact(count).map_err(|_| limit())?;
        let mut rows = 0usize;
        let mut previous = None;
        for ordinal in 0..count {
            budget.check()?;
            // The authenticated decoder checks exact IPC field/node shape and
            // requires native alignment. It preserves UInt64, signed bits,
            // Float32, UTF-8, fixed lists and nulls without a JSON/Scalar path.
            let (physical, allocation) = generation.read_batch(ordinal)?.into_parts();
            let fields = schema.schema.fields().len();
            if physical.num_columns() != add(fields, 1)? || physical.num_rows() == 0 {
                return Err(shape());
            }
            let physical_schema = physical.schema();
            if physical_schema.fields()[..fields] != schema.schema.fields()[..]
                || physical_schema.metadata() != schema.schema.metadata()
            {
                return Err(shape());
            }
            let locator = physical_schema.field(fields);
            if locator.data_type() != &DataType::UInt32
                || locator.is_nullable()
                || !locator.name().is_empty()
                || locator.metadata().len() != 1
                || locator.metadata().get("lycheedev.role").map(String::as_str)
                    != Some("logical-record-id")
            {
                return Err(shape());
            }
            let ids = physical.column(fields).as_any()
                .downcast_ref::<UInt32Array>().ok_or_else(shape)?;
            if ids.null_count() != 0 || ids.len() != physical.num_rows() {
                return Err(shape());
            }
            // Admit both the retained index and the projection's array handles
            // before either Vec is allocated. Buffer bytes keep the ORIGINAL
            // read_owned_batch admission, never a substitute reservation.
            let batch_metadata = budget.reserve_retained(add(
                add(1024, mul(fields, 512)?)?,
                mul(ids.len(), std::mem::size_of::<u32>())?,
            )?)?.into_owned(budget)?;
            let mut index = Vec::new();
            index.try_reserve_exact(ids.len()).map_err(|_| limit())?;
            for id in ids.values() {
                budget.charge(Cost::work(1))?;
                if previous.is_some_and(|last| last >= *id) {
                    return Err(shape());
                }
                previous = Some(*id);
                index.push(*id);
            }
            for array in physical.columns() {
                validate_values(array.as_ref(), budget)?;
            }
            let mut arrays = Vec::new();
            arrays.try_reserve_exact(fields).map_err(|_| limit())?;
            arrays.extend(physical.columns()[..fields].iter().cloned());
            let records = RecordBatch::try_new(schema.schema.clone(), arrays)
                .map_err(|_| shape())?;
            let batch = keep_demand(
                records, ids.clone(), Admission::Generation(allocation), schema, budget,
            )?;
            rows = add(rows, batch.records.num_rows())?;
            batches.push(IndexedBatch { batch, index, _metadata: batch_metadata });
        }
        budget.check()?;
        Ok(Self { batches, schema: schema.clone(), rows, _metadata: metadata })
    }

    /// The base must already carry a fully authenticated generation. The caller
    /// closes that generation before publishing, on success and on failure.
    pub(in crate::generation::engine_adapter) fn from_effective(
        base: &Arc<Physical<'static>>,
        plan: &Arc<effective::OverlayPlan>,
        schema: &Arc<SchemaOwner>,
        budget: &Arc<Budget>,
    ) -> Result<Self> {
        budget.check()?;
        if base.generation.is_none() { return Err(shape()); }
        let column_guard = budget.reserve_retained(add(
            1024, mul(schema.schema.fields().len(), 128)?,
        )?)?;
        let columns = schema.schema.fields().iter()
            .map(|field| Column::from_type(field.data_type()))
            .collect::<Result<Vec<_>>>()?;
        let cells = columns.iter().try_fold(0usize, |n, column| {
            add(n, column.width.unwrap_or(1))
        })?;
        let row_bytes = columns.iter().try_fold(0usize, |n, column| {
            add(n, mul(column.width.unwrap_or(1), column.kind.width())?)
        })?;
        if cells == 0 || row_bytes > BATCH_VALUES { return Err(limit()); }
        let per_batch = BATCH_ROWS.min((BATCH_VALUES / row_bytes.max(1)).max(1));
        let fixed = mul(per_batch, add(row_bytes, cells)?)?;
        let peak = add(
            add(mul(fixed, 2)?, mul(BATCH_VALUES, 3)?)?,
            add(mul(columns.len(), 2048)?, add(4096, mul(per_batch, 8)?)?)?,
        )?;
        let mut metadata = budget.reserve_retained(1024)?;
        let mut batches: Vec<IndexedBatch> = Vec::new();
        let mut cursor = plan.owned_cursor(base.clone(), budget.clone())?;
        let mut pending = cursor.advance()?;
        let mut previous = None;
        let mut rows = 0usize;
        while pending {
            budget.check()?;
            if batches.len() == batches.capacity() {
                let capacity = mul(batches.capacity().max(4), 2)?;
                // Admit full replacement storage while the old vector exists.
                metadata.grow(mul(capacity, std::mem::size_of::<IndexedBatch>())?)?;
                batches.try_reserve_exact(capacity - batches.len()).map_err(|_| limit())?;
            }
            let mut allocation = budget.reserve_retained(peak)?.into_owned(budget)?;
            let index_metadata = budget.reserve_retained(add(
                1024, mul(per_batch, std::mem::size_of::<u32>())?,
            )?)?.into_owned(budget)?;
            let mut index = Vec::new();
            index.try_reserve_exact(per_batch).map_err(|_| limit())?;
            let mut builders = columns.iter().map(|column| column.builder(per_batch))
                .collect::<Result<Vec<_>>>()?;
            let mut bytes = 0usize;
            while pending && index.len() < per_batch {
                budget.check()?;
                let values = cursor.row();
                if values.len() != cells { return Err(shape()); }
                let mut position = 0usize;
                let mut size = 0usize;
                for column in &columns {
                    let end = add(position, column.width.unwrap_or(1))?;
                    for value in &values[position..end] {
                        size = add(size, column.kind.inspect(value)?)?;
                    }
                    position = end;
                }
                if size > BATCH_VALUES { return Err(limit()); }
                if add(bytes, size)? > BATCH_VALUES { break; }
                let id = cursor.current_id().ok_or_else(shape)?;
                if previous.is_some_and(|last| last >= id) { return Err(shape()); }
                previous = Some(id);
                budget.charge(Cost::work(add(size, 1)?))?;
                position = 0;
                for (column, builder) in columns.iter().zip(&mut builders) {
                    match builder {
                        Builder::Scalar(builder) => column.kind.append(builder.as_mut(), &values[position]),
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
                index.push(id);
                bytes += size;
                pending = cursor.advance()?;
            }
            if index.is_empty() { return Err(shape()); }
            let records = RecordBatch::try_new(
                schema.schema.clone(), builders.into_iter().map(Builder::finish).collect(),
            ).map_err(|_| shape())?;
            let ids = UInt32Array::from(index.clone());
            allocation.shrink_to(add(
                add(records.get_array_memory_size(), ids.get_array_memory_size())?,
                add(4096, mul(columns.len(), 2048)?)?,
            )?)?;
            let batch = keep_demand(records, ids, Admission::Effective(allocation), schema, budget)?;
            rows = add(rows, batch.records.num_rows())?;
            batches.push(IndexedBatch { batch, index, _metadata: index_metadata });
        }
        drop(cursor);
        drop(column_guard);
        budget.check()?;
        Ok(Self {
            batches, schema: schema.clone(), rows,
            _metadata: metadata.into_owned(budget)?,
        })
    }

    fn locate(&self, id: u32) -> Option<(usize, usize)> {
        let batch = self.batches.partition_point(|entry| {
            entry.index.last().is_some_and(|last| *last < id)
        });
        let row = self.batches.get(batch)?.index.binary_search(&id).ok()?;
        Some((batch, row))
    }

    pub(in crate::generation::engine_adapter) fn new_cursor(
        self: &Arc<Self>,
        ids: Option<&[u32]>,
        budget: Arc<Budget>,
    ) -> Result<ArrowCursor> {
        budget.check()?;
        let metadata = budget.reserve_retained(add(
            1024, mul(ids.map_or(0, <[u32]>::len), std::mem::size_of::<u32>())?,
        )?)?.into_owned(&budget)?;
        let ids = match ids {
            None => None,
            Some(ids) => {
                let mut selected = Vec::new();
                selected.try_reserve_exact(ids.len()).map_err(|_| limit())?;
                let mut previous = None;
                for id in ids {
                    budget.charge(Cost::work(1))?;
                    if previous.is_some_and(|last| last >= *id) { return Err(shape()); }
                    previous = Some(*id);
                    selected.push(*id);
                }
                Some(selected)
            }
        };
        Ok(ArrowCursor {
            snapshot: self.clone(), budget, ids, next: 0, batch: 0, row: 0,
            done: false, _metadata: metadata,
        })
    }
}

pub(in crate::generation::engine_adapter) struct ArrowCursor {
    snapshot: Arc<ArrowSnapshot>,
    budget: Arc<Budget>,
    ids: Option<Vec<u32>>,
    next: usize,
    batch: usize,
    row: usize,
    done: bool,
    _metadata: OwnedRetainedReservation,
}

impl ArrowCursor {
    pub(in crate::generation::engine_adapter) fn next_batch(
        &mut self, max_rows: usize,
    ) -> Result<Option<DemandBatch>> {
        if self.done { return Ok(None); }
        let result = self.next_inner(max_rows);
        if result.is_err() || matches!(&result, Ok(None)) { self.done = true; }
        result
    }

    fn next_inner(&mut self, max_rows: usize) -> Result<Option<DemandBatch>> {
        self.budget.check()?;
        let location = if let Some(ids) = &self.ids {
            let mut found = None;
            while let Some(id) = ids.get(self.next) {
                self.next += 1;
                // Account logarithmic index probes even for absent IDs.
                let work = add(
                    self.snapshot.batches.len().ilog2_or_zero(),
                    self.snapshot.rows.ilog2_or_zero(),
                )?;
                self.budget.charge(Cost::work(add(work, 2)?))?;
                if let Some(location) = self.snapshot.locate(*id) {
                    found = Some(location);
                    break;
                }
            }
            found
        } else {
            self.snapshot.batches.get(self.batch).map(|_| (self.batch, self.row))
        };
        let Some((batch, row)) = location else { return Ok(None); };
        let source = &self.snapshot.batches[batch].batch;
        if row >= source.records.num_rows() { return Err(shape()); }
        let maximum = max_rows.min(BATCH_ROWS);
        let (records, ids, view_allocation, retained_bytes) = if let Some(selected) = &self.ids {
            let _indices = self.budget.reserve_retained(add(1024, mul(maximum, 8)?)?)?;
            let mut positions = Vec::with_capacity(maximum);
            positions.push(u32::try_from(row).map_err(|_| limit())?);
            while positions.len() < maximum {
                let Some(id) = selected.get(self.next) else { break; };
                self.budget.charge(Cost::work(add(
                    add(self.snapshot.batches.len().ilog2_or_zero(), self.snapshot.rows.ilog2_or_zero())?, 2,
                )?))?;
                let location = self.snapshot.locate(*id);
                if location.is_some_and(|(ordinal, _)| ordinal != batch) { break; }
                self.next += 1;
                if let Some((_, position)) = location {
                    positions.push(u32::try_from(position).map_err(|_| limit())?);
                }
            }
            let contiguous = positions.iter().enumerate()
                .all(|(offset, position)| *position as usize == row + offset);
            if contiguous {
                let allocation = self.view_allocation(source.records.num_columns())?;
                let retained = add(source.retained_bytes, allocation.bytes())?;
                (source.records.slice(row, positions.len()), source.ids.slice(row, positions.len()), Some(allocation), retained)
            } else {
                // A sparse selection must not expose one whole-batch-backed
                // slice per ID to DF Sort. Take copies native values only; its
                // unique sorted indices cannot copy more values than the input.
                // Admit output, list-child indices and builder peak first.
                let input_bytes = add(source.records.get_array_memory_size(), source.ids.get_array_memory_size())?;
                let peak = add(mul(input_bytes, 2)?, add(16384, mul(source.records.num_columns(), 4096)?)?)?;
                let mut allocation = self.budget.reserve_retained(peak)?.into_owned(&self.budget)?;
                self.budget.charge(Cost::work(input_bytes))?;
                let indices = UInt32Array::from(positions);
                let arrays = source.records.columns().iter().map(|array| {
                    datafusion::arrow::compute::take(array.as_ref(), &indices, None).map_err(|_| shape())
                }).collect::<Result<Vec<_>>>()?;
                let records = RecordBatch::try_new(self.snapshot.schema.schema.clone(), arrays).map_err(|_| shape())?;
                let ids = datafusion::arrow::compute::take(&source.ids, &indices, None).map_err(|_| shape())?;
                let ids = ids.as_any().downcast_ref::<UInt32Array>().ok_or_else(shape)?.clone();
                allocation.shrink_to(add(add(records.get_array_memory_size(), ids.get_array_memory_size())?, 4096)?)?;
                let compact = keep_demand(records, ids, Admission::Effective(allocation), &self.snapshot.schema, &self.budget)?;
                (compact.records, compact.ids, compact._view_allocation, compact.retained_bytes)
            }
        } else {
            // IPC batch boundaries are already bounded by the producer. Do not
            // split each batch into rows sharing its entire buffer capacity:
            // native DF operators account those capacities per incoming batch.
            let count = maximum.min(source.records.num_rows() - row);
            let allocation = self.view_allocation(source.records.num_columns())?;
            let records = source.records.slice(row, count);
            let ids = source.ids.slice(row, count);
            self.row += count;
            if self.row == source.records.num_rows() { self.batch += 1; self.row = 0; }
            let retained = add(source.retained_bytes, allocation.bytes())?;
            (records, ids, Some(allocation), retained)
        };
        self.budget.charge(Cost::work(mul(records.num_rows(), add(records.num_columns(), 1)?)?))?;
        // Shared views inherit the original immutable buffer hooks. Only the
        // sparse Take result above owns a new, separately admitted allocation.
        Ok(Some(DemandBatch { records, ids, retained_bytes, _view_allocation: view_allocation }))
    }
    fn view_allocation(&self, columns: usize) -> Result<OwnedRetainedReservation> {
        self.budget.reserve_retained(add(1024, mul(columns, 1024)?)?)?.into_owned(&self.budget)
    }

}

// Native decoding already validates layout/alignment; this additionally checks
// the versioned producer's finite-float invariant without converting any values.
fn validate_values(array: &dyn Array, budget: &Budget) -> Result<()> {
    budget.check()?;
    match array.data_type() {
        DataType::Float32 => {
            let values = array.as_any().downcast_ref::<Float32Array>().ok_or_else(shape)?;
            for value in values.iter() {
                budget.charge(Cost::work(1))?;
                if value.is_some_and(|value| !value.is_finite()) { return Err(shape()); }
            }
        }
        DataType::FixedSizeList(_, _) => {
            let list = array.as_any().downcast_ref::<FixedSizeListArray>().ok_or_else(shape)?;
            validate_values(list.values().as_ref(), budget)?;
        }
        _ => {}
    }
    Ok(())
}

trait IndexDepth {
    fn ilog2_or_zero(self) -> usize;
}
impl IndexDepth for usize {
    fn ilog2_or_zero(self) -> usize {
        if self == 0 { 0 } else { self.ilog2() as usize }
    }
}

impl ArrowSnapshot {
    /// Publish only fully visited, already validated source rows. The original
    /// buffers retain their original hooks; no replacement claim is performed.
    pub(in crate::generation::engine_adapter) fn from_visited(
        records: Vec<DemandBatch>, expected_ids: &[u32], schema:&Arc<SchemaOwner>, budget:&Arc<Budget>,
    )->Result<Option<Self>> {
        budget.check()?;
        let mut count=0usize;
        for batch in &records {
            if batch.records.schema()!=schema.schema || batch.records.num_rows()!=batch.ids.len() || batch.ids.null_count()!=0 {return Err(shape());}
            for id in batch.ids.values() {
                budget.charge(Cost::work(1))?;
                if expected_ids.get(count)!=Some(id){return Ok(None);}
                count=add(count,1)?;
            }
        }
        if count!=expected_ids.len(){return Ok(None);}
        let metadata=budget.reserve_retained(add(4096,mul(records.len(),std::mem::size_of::<IndexedBatch>())?)?)?.into_owned(budget)?;
        let mut batches=Vec::with_capacity(records.len());
        for batch in records {
            let owner=budget.reserve_retained(add(1024,mul(batch.ids.len(),8)?)?)?.into_owned(budget)?;
            let index=batch.ids.values().to_vec();
            batches.push(IndexedBatch{batch,index,_metadata:owner});
        }
        Ok(Some(Self{batches,schema:schema.clone(),rows:count,_metadata:metadata}))
    }
}
