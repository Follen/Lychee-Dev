//! Compact a bounded window before collecting the next lazy source window.
//!
//! The caller MUST retain its full output reservation while using this collector
//! and transfer the returned batches only into that reservation's ResultOwner.
//! `maximum_bytes` excludes the caller's separately counted result metadata.
//! This is not a second reservation or a complete native-allocator ledger:
//! incoming buffers keep their upstream owners, DataFusion keeps its pool, and
//! the externally supervised worker still supplies the process hard cap.
use crate::{
    fault,
    resource::{Budget, Cost},
};
use arrow_array::{ArrayRef, RecordBatch, RecordBatchOptions, make_array};
use arrow_buffer::{BooleanBuffer, Buffer, NullBuffer};
use arrow_schema::{DataType, SchemaRef};
use datafusion::arrow::{array::ArrayData, compute::concat_batches};
use lycheedev_core::Result;
use std::{mem::size_of, sync::Arc};

const WINDOW_ROWS: usize = 1024;
const WINDOW_INPUT_BYTES: usize = 8 << 20;
// Preadmitted conservative Arrow array/builder/extension metadata, per schema
// node and input. This is intentionally not described as measured RSS.
const NODE_SCRATCH_BYTES: usize = 1024;

pub(super) struct OutputBatches {
    schema: SchemaRef,
    maximum_bytes: usize,
    batches: Vec<RecordBatch>,
    pending: Vec<RecordBatch>,
    pending_bytes: usize,
    pending_rows: usize,
    retained_bytes: usize,
    rows: usize,
    failed: bool,
}

impl OutputBatches {
    pub(super) fn new(schema: SchemaRef, maximum_bytes: usize) -> Self {
        Self {
            schema,
            maximum_bytes,
            batches: Vec::new(),
            pending: Vec::new(),
            pending_bytes: 0,
            pending_rows: 0,
            retained_bytes: 0,
            rows: 0,
            failed: false,
        }
    }

    pub(super) fn push(&mut self, batch: RecordBatch, budget: &Arc<Budget>) -> Result<()> {
        if self.failed {
            return Err(bound());
        }
        // A failed push must never be mistaken for a complete partial result.
        self.failed = true;
        let result = self.push_inner(batch, budget);
        if result.is_ok() {
            self.failed = false;
        }
        result
    }

    fn push_inner(&mut self, batch: RecordBatch, budget: &Arc<Budget>) -> Result<()> {
        budget.check()?;
        if batch.schema().as_ref() != self.schema.as_ref() {
            return Err(fault(
                "records.result_schema",
                3,
                "Output batch differs from its admitted schema",
            ));
        }
        let total_rows = add(self.rows, batch.num_rows())?;
        let input_bytes = batch_bytes(&batch)?;
        let nodes = schema_nodes(&self.schema)?;
        let mut offset = 0;
        while offset < batch.num_rows() {
            budget.check()?;
            let take = (batch.num_rows() - offset).min(WINDOW_ROWS);
            if !self.pending.is_empty()
                && (add(self.pending_rows, take)? > WINDOW_ROWS
                    || add(self.pending_bytes, input_bytes)? > WINDOW_INPUT_BYTES)
            {
                self.flush(budget)?;
            }
            let count = add(self.pending.len(), 1)?;
            let bytes = add(self.pending_bytes, input_bytes)?;
            let capacity = next_capacity(self.pending.len(), self.pending.capacity(), WINDOW_ROWS)?;
            // Count the old vector as well while reallocating. The original
            // incoming batch remains alive until all its slices have copied;
            // its source reservation is never refunded here.
            let old_capacity = if capacity != self.pending.capacity() {
                self.pending.capacity()
            } else {
                0
            };
            self.check_peak(
                bytes,
                count,
                add(capacity, old_capacity)?,
                self.batches.capacity(),
                nodes,
            )?;
            reserve_to(&mut self.pending, capacity)?;
            // slice() owns fresh array/column wrappers but shares source buffers.
            // Both the wrappers and copy overlap were admitted above.
            self.pending.push(batch.slice(offset, take));
            self.pending_bytes = bytes;
            self.pending_rows = add(self.pending_rows, take)?;
            offset = add(offset, take)?;
            if self.pending_rows == WINDOW_ROWS || self.pending_bytes >= WINDOW_INPUT_BYTES {
                self.flush(budget)?;
            }
        }
        self.rows = total_rows;
        budget.check()
    }

    pub(super) fn finish(
        mut self,
        budget: &Arc<Budget>,
    ) -> Result<(Vec<RecordBatch>, usize, usize)> {
        budget.check()?;
        if self.failed {
            return Err(bound());
        }
        self.flush(budget)?;
        budget.check()?;
        let bytes = add(
            self.retained_bytes,
            multiply(self.batches.capacity(), size_of::<RecordBatch>())?,
        )?;
        if bytes > self.maximum_bytes {
            return Err(bound());
        }
        // Pending capacity is not included in the returned retained observation:
        // it dies here, before the caller can shrink its output reservation.
        drop(self.pending);
        Ok((self.batches, bytes, self.rows))
    }

    fn check_peak(
        &self,
        input_bytes: usize,
        count: usize,
        pending_capacity: usize,
        completed_capacity: usize,
        nodes: usize,
    ) -> Result<()> {
        // Source buffers remain covered by their own upstream reservations.
        // Their observed size bounds our new concat copy; source bytes are not
        // refunded, silently transferred, or charged as a second ownership.
        let copied = add(input_bytes, multiply(nodes, NODE_SCRATCH_BYTES)?)?;
        let copies = if self.schema.fields().iter().any(|f| may_share(f.data_type())) {
            2
        } else {
            1
        };
        let scratch = multiply(
            multiply(nodes, add(count, 2)?)?,
            NODE_SCRATCH_BYTES,
        )?;
        let wrappers = multiply(count, batch_wrapper_bytes(self.schema.fields().len())?)?;
        let vectors = multiply(
            add(pending_capacity, completed_capacity)?,
            size_of::<RecordBatch>(),
        )?;
        let peak = add(
            add(self.retained_bytes, vectors)?,
            add(add(multiply(copied, copies)?, scratch)?, wrappers)?,
        )?;
        if peak > self.maximum_bytes {
            return Err(bound());
        }
        Ok(())
    }

    fn flush(&mut self, budget: &Arc<Budget>) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        budget.check()?;
        let nodes = schema_nodes(&self.schema)?;
        let capacity = next_capacity(self.batches.len(), self.batches.capacity(), usize::MAX)?;
        let old_capacity = if capacity != self.batches.capacity() {
            self.batches.capacity()
        } else {
            0
        };
        self.check_peak(
            self.pending_bytes,
            self.pending.len(),
            self.pending.capacity(),
            add(capacity, old_capacity)?,
            nodes,
        )?;
        // One bounded bulk charge, not a budget lock for every cell.
        let work = add(
            multiply(self.pending_rows, nodes.max(1))?,
            add(self.pending_bytes / 64, 1)?,
        )?;
        budget.charge(Cost::work(work))?;
        reserve_to(&mut self.batches, capacity)?;

        // Arrow 58.3 concat([one_array]) returns a slice. An empty sentinel
        // forces its copying path even for the final one-row window.
        let empty = RecordBatch::new_empty(self.schema.clone());
        let joined = concat_batches(
            &self.schema,
            std::iter::once(&empty).chain(self.pending.iter()),
        )
        .map_err(|error| super::error(error.into()))?;
        budget.check()?;
        let compact = detach_shared(joined)?;
        budget.check()?;
        if compact.num_rows() != self.pending_rows {
            return Err(bound());
        }
        let retained = add(self.retained_bytes, batch_bytes(&compact)?)?;
        if add(
            retained,
            multiply(self.batches.capacity(), size_of::<RecordBatch>())?,
        )? > self.maximum_bytes
        {
            return Err(bound());
        }
        // Keep every source guard through the entire copy and validation.
        // The only escaping arrays are compact output under caller ownership.
        self.pending.clear();
        self.pending_bytes = 0;
        self.pending_rows = 0;
        self.retained_bytes = retained;
        self.batches.push(compact);
        Ok(())
    }
}

// concat copies ordinary primitive/text/list buffers. View/dictionary and some
// less common Arrow types can retain source buffers; detach those column trees
// without changing their types or converting values through JSON/f64.
fn may_share(kind: &DataType) -> bool {
    match kind {
        DataType::Null
        | DataType::Boolean
        | DataType::Int8
        | DataType::Int16
        | DataType::Int32
        | DataType::Int64
        | DataType::UInt8
        | DataType::UInt16
        | DataType::UInt32
        | DataType::UInt64
        | DataType::Float16
        | DataType::Float32
        | DataType::Float64
        | DataType::Timestamp(_, _)
        | DataType::Date32
        | DataType::Date64
        | DataType::Time32(_)
        | DataType::Time64(_)
        | DataType::Duration(_)
        | DataType::Interval(_)
        | DataType::Decimal32(_, _)
        | DataType::Decimal64(_, _)
        | DataType::Decimal128(_, _)
        | DataType::Decimal256(_, _)
        | DataType::Binary
        | DataType::LargeBinary
        | DataType::FixedSizeBinary(_)
        | DataType::Utf8
        | DataType::LargeUtf8 => false,
        DataType::List(field) | DataType::LargeList(field) | DataType::FixedSizeList(field, _) => {
            may_share(field.data_type())
        }
        DataType::Struct(fields) => fields.iter().any(|field| may_share(field.data_type())),
        _ => true,
    }
}

fn detach_shared(batch: RecordBatch) -> Result<RecordBatch> {
    if !batch.columns().iter().any(|array| may_share(array.data_type())) {
        return Ok(batch);
    }
    let mut columns = Vec::with_capacity(batch.num_columns());
    for array in batch.columns() {
        columns.push(if may_share(array.data_type()) {
            make_array(copy_data(array.to_data(), 0)?)
        } else {
            array.clone()
        });
    }
    RecordBatch::try_new_with_options(
        batch.schema(),
        columns,
        &RecordBatchOptions::new().with_row_count(Some(batch.num_rows())),
    )
    .map_err(|error| super::error(error.into()))
}

fn copy_data(data: ArrayData, depth: usize) -> Result<ArrayData> {
    if depth > 64 {
        return Err(bound());
    }
    let buffers = data
        .buffers()
        .iter()
        .map(|buffer| Buffer::from_slice_ref(buffer.as_slice()))
        .collect();
    let children = data
        .child_data()
        .iter()
        .map(|child| copy_data(child.clone(), depth + 1))
        .collect::<Result<Vec<_>>>()?;
    let nulls = data.nulls().map(|nulls| {
        NullBuffer::new(BooleanBuffer::new(
            Buffer::from_slice_ref(nulls.inner().values()),
            nulls.offset(),
            nulls.len(),
        ))
    });
    data.into_builder()
        .buffers(buffers)
        .child_data(children)
        .nulls(nulls)
        .build()
        .map_err(|error| super::error(error.into()))
}

fn schema_nodes(schema: &SchemaRef) -> Result<usize> {
    schema.fields().iter().try_fold(1, |count, field| {
        add(count, type_nodes(field.data_type(), 0)?)
    })
}

fn type_nodes(kind: &DataType, depth: usize) -> Result<usize> {
    if depth > 64 {
        return Err(bound());
    }
    match kind {
        DataType::List(field)
        | DataType::LargeList(field)
        | DataType::FixedSizeList(field, _)
        | DataType::ListView(field)
        | DataType::LargeListView(field)
        | DataType::Map(field, _) => add(1, type_nodes(field.data_type(), depth + 1)?),
        DataType::Struct(fields) => fields.iter().try_fold(1, |count, field| {
            add(count, type_nodes(field.data_type(), depth + 1)?)
        }),
        DataType::Union(fields, _) => fields.iter().try_fold(1, |count, (_, field)| {
            add(count, type_nodes(field.data_type(), depth + 1)?)
        }),
        DataType::Dictionary(key, value) => add(
            1,
            add(type_nodes(key, depth + 1)?, type_nodes(value, depth + 1)?)?,
        ),
        DataType::RunEndEncoded(ends, values) => add(
            1,
            add(
                type_nodes(ends.data_type(), depth + 1)?,
                type_nodes(values.data_type(), depth + 1)?,
            )?,
        ),
        _ => Ok(1),
    }
}

fn batch_wrapper_bytes(columns: usize) -> Result<usize> {
    Ok(multiply(columns, size_of::<ArrayRef>())?.max(256))
}

fn batch_bytes(batch: &RecordBatch) -> Result<usize> {
    // Use a checked column sum rather than RecordBatch's unchecked usize sum.
    batch.columns().iter().try_fold(
        batch_wrapper_bytes(batch.num_columns())?,
        |bytes, array| add(bytes, array.get_array_memory_size()),
    )
}

fn next_capacity(length: usize, capacity: usize, maximum: usize) -> Result<usize> {
    if length < capacity {
        return Ok(capacity);
    }
    let target = if capacity == 0 {
        4.min(maximum)
    } else {
        multiply(capacity, 2)?.min(maximum)
    };
    if target <= length {
        return Err(bound());
    }
    Ok(target)
}

fn reserve_to<T>(values: &mut Vec<T>, capacity: usize) -> Result<()> {
    if capacity > values.capacity() {
        values
            .try_reserve_exact(capacity - values.len())
            .map_err(|_| bound())?;
        // No collection growth is silently accepted beyond its preflight.
        if values.capacity() > capacity {
            return Err(bound());
        }
    }
    Ok(())
}

fn add(left: usize, right: usize) -> Result<usize> {
    left.checked_add(right).ok_or_else(bound)
}

fn multiply(left: usize, right: usize) -> Result<usize> {
    left.checked_mul(right).ok_or_else(bound)
}

fn bound() -> lycheedev_core::ToolkitError {
    fault(
        "records.query_resource_budget",
        3,
        "Unified query output exceeds its admitted bound",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::OwnedRetainedReservation;
    use arrow_array::{
        Array, FixedSizeListArray, Float32Array, Int64Array, StringArray, StringViewArray, UInt32Array,
        UInt64Array,
    };
    use arrow_buffer::{MemoryPool, MemoryReservation};
    use arrow_schema::{Field, Schema};
    use lycheedev_core::control::TrackedCancellation;
    use std::{
        fmt,
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, Instant},
    };

    fn budget(cancel: Option<TrackedCancellation>) -> Arc<Budget> {
        Arc::new(Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(60),
            cancel,
        ))
    }

    struct InputOwner {
        live: Arc<AtomicUsize>,
        _allocation: OwnedRetainedReservation,
    }
    impl Drop for InputOwner {
        fn drop(&mut self) {
            self.live.fetch_sub(1, Ordering::SeqCst);
        }
    }
    impl fmt::Debug for InputOwner {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("PreadmittedTestSource")
        }
    }
    #[derive(Debug)]
    struct Pool(Arc<InputOwner>);
    impl MemoryPool for Pool {
        fn reserve(&self, size: usize) -> Box<dyn MemoryReservation> {
            Box::new(Lease {
                bytes: size,
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
    #[derive(Debug)]
    struct Lease {
        bytes: usize,
        _owner: Arc<InputOwner>,
    }
    impl MemoryReservation for Lease {
        fn size(&self) -> usize {
            self.bytes
        }
        fn resize(&mut self, size: usize) {
            self.bytes = size;
        }
    }
    fn retain_input(batch: &RecordBatch, budget: &Arc<Budget>, live: &Arc<AtomicUsize>) {
        let allocation = budget
            .reserve_retained((40 << 10) + batch.get_array_memory_size())
            .unwrap()
            .into_owned(budget)
            .unwrap();
        live.fetch_add(1, Ordering::SeqCst);
        let pool = Pool(Arc::new(InputOwner {
            live: live.clone(),
            _allocation: allocation,
        }));
        batch.claim(&pool);
    }

    fn exact_schema() -> SchemaRef {
        Arc::new(Schema::new(vec![
            Field::new("signed", DataType::Int64, false),
            Field::new("unsigned", DataType::UInt64, false),
            Field::new("float32", DataType::Float32, false),
            Field::new("text", DataType::Utf8, true),
            Field::new(
                "array",
                DataType::FixedSizeList(Arc::new(Field::new("item", DataType::UInt32, true)), 3),
                false,
            ),
        ]))
    }

    fn exact_row(schema: SchemaRef, row: usize) -> RecordBatch {
        let list = FixedSizeListArray::try_new(
            Arc::new(Field::new("item", DataType::UInt32, true)),
            3,
            Arc::new(UInt32Array::from(vec![Some(row as u32), None, Some(u32::MAX)])),
            None,
        )
        .unwrap();
        RecordBatch::try_new(
            schema,
            vec![
                Arc::new(Int64Array::from(vec![9_007_199_254_740_993 + row as i64])),
                Arc::new(UInt64Array::from(vec![u64::MAX - row as u64])),
                Arc::new(Float32Array::from(vec![if row % 2 == 0 { -0.0 } else { 0.1 }])),
                Arc::new(StringArray::from(vec![if row % 2 == 0 {
                    Some("中文🙂")
                } else {
                    None
                }])),
                Arc::new(list),
            ],
        )
        .unwrap()
    }

    #[test]
    fn sixteen_thousand_one_row_owners_release_at_window_boundaries_and_preserve_values() {
        let budget = budget(None);
        let maximum = 128 << 20;
        let mut output = budget
            .reserve_retained(maximum)
            .unwrap()
            .into_owned(&budget)
            .unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        let schema = exact_schema();
        let mut collector = OutputBatches::new(schema.clone(), maximum);
        for row in 0..16_384 {
            let batch = exact_row(schema.clone(), row);
            retain_input(&batch, &budget, &live);
            collector.push(batch, &budget).unwrap();
            assert!(live.load(Ordering::SeqCst) < WINDOW_ROWS);
        }
        let (batches, bytes, rows) = collector.finish(&budget).unwrap();
        assert_eq!(rows, 16_384);
        assert_eq!(batches.len(), 16);
        assert_eq!(live.load(Ordering::SeqCst), 0);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, maximum as u64);
        let mut index = 0usize;
        for batch in &batches {
            assert_eq!(batch.num_rows(), WINDOW_ROWS);
            let signed = batch.column(0).as_any().downcast_ref::<Int64Array>().unwrap();
            let unsigned = batch.column(1).as_any().downcast_ref::<UInt64Array>().unwrap();
            let float = batch.column(2).as_any().downcast_ref::<Float32Array>().unwrap();
            let text = batch.column(3).as_any().downcast_ref::<StringArray>().unwrap();
            let lists = batch.column(4).as_any().downcast_ref::<FixedSizeListArray>().unwrap();
            for row in 0..batch.num_rows() {
                assert_eq!(signed.value(row), 9_007_199_254_740_993 + index as i64);
                assert_eq!(unsigned.value(row), u64::MAX - index as u64);
                let expected: f32 = if index % 2 == 0 { -0.0 } else { 0.1 };
                assert_eq!(float.value(row).to_bits(), expected.to_bits());
                assert_eq!(text.is_null(row), index % 2 != 0);
                if index % 2 == 0 {
                    assert_eq!(text.value(row), "中文🙂");
                }
                let values = lists.value(row);
                let values = values.as_any().downcast_ref::<UInt32Array>().unwrap();
                assert_eq!(values.value(0), index as u32);
                assert!(values.is_null(1));
                assert_eq!(values.value(2), u32::MAX);
                index += 1;
            }
        }
        output.shrink_to(bytes).unwrap();
        assert_eq!(budget.snapshot().unwrap().retained_bytes, bytes as u64);
        drop(batches);
        drop(output);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }

    #[test]
    fn final_single_batch_copies_instead_of_retaining_arrow_slice_owner() {
        let budget = budget(None);
        let output = budget.reserve_retained(1 << 20).unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        let schema = exact_schema();
        let batch = exact_row(schema.clone(), 0);
        retain_input(&batch, &budget, &live);
        let mut collector = OutputBatches::new(schema, 1 << 20);
        collector.push(batch, &budget).unwrap();
        assert_eq!(live.load(Ordering::SeqCst), 1);
        let (batches, _, rows) = collector.finish(&budget).unwrap();
        assert_eq!(rows, 1);
        assert_eq!(live.load(Ordering::SeqCst), 0);
        drop(batches);
        drop(output);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }

    #[test]
    fn string_view_copy_drops_source_data_and_preserves_nulls() {
        let budget = budget(None);
        let output = budget.reserve_retained(1 << 20).unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        let schema = Arc::new(Schema::new(vec![Field::new("text", DataType::Utf8View, true)]));
        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![Arc::new(StringViewArray::from(vec![
                Some("a long non-inline string carrying a source owner"),
                None,
                Some("short"),
            ]))],
        )
        .unwrap();
        retain_input(&batch, &budget, &live);
        let mut collector = OutputBatches::new(schema, 1 << 20);
        collector.push(batch, &budget).unwrap();
        let (batches, _, rows) = collector.finish(&budget).unwrap();
        assert_eq!(rows, 3);
        assert_eq!(live.load(Ordering::SeqCst), 0);
        let values = batches[0].column(0).as_any().downcast_ref::<StringViewArray>().unwrap();
        assert_eq!(values.value(0), "a long non-inline string carrying a source owner");
        assert!(values.is_null(1));
        assert_eq!(values.value(2), "short");
        drop(batches);
        drop(output);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }

    #[test]
    fn zero_column_batches_keep_row_counts_and_split_large_inputs() {
        let budget = budget(None);
        let output = budget.reserve_retained(1 << 20).unwrap();
        let schema = Arc::new(Schema::empty());
        let mut collector = OutputBatches::new(schema.clone(), 1 << 20);
        for rows in [0, 1, 2049] {
            collector
                .push(
                    RecordBatch::try_new_with_options(
                        schema.clone(),
                        vec![],
                        &RecordBatchOptions::new().with_row_count(Some(rows)),
                    )
                    .unwrap(),
                    &budget,
                )
                .unwrap();
        }
        let (batches, _, rows) = collector.finish(&budget).unwrap();
        assert_eq!(rows, 2050);
        assert_eq!(batches.iter().map(RecordBatch::num_rows).sum::<usize>(), rows);
        assert!(batches.iter().all(|batch| batch.num_rows() <= WINDOW_ROWS));
        assert!(batches.iter().all(|batch| batch.num_columns() == 0));
        drop(batches);
        drop(output);
    }

    #[test]
    fn observed_input_threshold_flushes_before_accumulating_another_source_window() {
        let budget = budget(None);
        let output = budget.reserve_retained(64 << 20).unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        let schema = Arc::new(Schema::new(vec![Field::new("text", DataType::Utf8, false)]));
        let text = "x".repeat(5 << 20);
        let mut collector = OutputBatches::new(schema.clone(), 64 << 20);
        for _ in 0..2 {
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![Arc::new(StringArray::from(vec![text.as_str()]))],
            )
            .unwrap();
            retain_input(&batch, &budget, &live);
            collector.push(batch, &budget).unwrap();
            assert_eq!(live.load(Ordering::SeqCst), 1);
        }
        let (batches, _, rows) = collector.finish(&budget).unwrap();
        assert_eq!(rows, 2);
        assert_eq!(batches.len(), 2);
        assert_eq!(live.load(Ordering::SeqCst), 0);
        for batch in &batches {
            let values = batch.column(0).as_any().downcast_ref::<StringArray>().unwrap();
            assert_eq!(values.value(0), text);
        }
        drop(batches);
        drop(output);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }

    #[test]
    fn rejected_output_drops_input_and_cannot_finish_partial_result() {
        let budget = budget(None);
        let live = Arc::new(AtomicUsize::new(0));
        let schema = exact_schema();
        let batch = exact_row(schema.clone(), 0);
        retain_input(&batch, &budget, &live);
        let mut collector = OutputBatches::new(schema, 0);
        assert!(collector.push(batch, &budget).is_err());
        assert_eq!(live.load(Ordering::SeqCst), 0);
        assert!(collector.finish(&budget).is_err());
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }

    #[test]
    fn cancellation_and_schema_errors_release_pending_guards_on_drop() {
        let cancelled = TrackedCancellation::new();
        let cancelled_budget = budget(Some(cancelled.clone()));
        let output = cancelled_budget.reserve_retained(1 << 20).unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        let schema = exact_schema();
        let batch = exact_row(schema.clone(), 0);
        retain_input(&batch, &cancelled_budget, &live);
        let mut collector = OutputBatches::new(schema.clone(), 1 << 20);
        collector.push(batch, &cancelled_budget).unwrap();
        cancelled.cancel();
        assert!(collector.finish(&cancelled_budget).is_err());
        assert_eq!(live.load(Ordering::SeqCst), 0);
        let fresh = budget(None);
        let mut collector = OutputBatches::new(schema, 1 << 20);
        assert!(collector
            .push(RecordBatch::new_empty(Arc::new(Schema::empty())), &fresh)
            .is_err());
        assert!(collector.finish(&fresh).is_err());
        drop(output);
        assert_eq!(
            cancelled_budget.snapshot().unwrap().retained_bytes,
            0
        );
    }

    #[test]
    fn work_budget_is_checked_before_copy_and_pending_owners_drop_on_error() {
        let mut limits = Cost::defaults();
        limits.decode_work = 0;
        let budget = Arc::new(Budget::new(
            limits,
            Instant::now() + Duration::from_secs(60),
            None,
        ));
        let output = budget.reserve_retained(1 << 20).unwrap();
        let live = Arc::new(AtomicUsize::new(0));
        let schema = exact_schema();
        let batch = exact_row(schema.clone(), 0);
        retain_input(&batch, &budget, &live);
        let mut collector = OutputBatches::new(schema, 1 << 20);
        collector.push(batch, &budget).unwrap();
        assert!(collector.finish(&budget).is_err());
        assert_eq!(live.load(Ordering::SeqCst), 0);
        drop(output);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }
}
