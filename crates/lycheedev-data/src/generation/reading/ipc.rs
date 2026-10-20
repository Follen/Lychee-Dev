//! Bounded decoder for this producer's exact uncompressed IPC representation.
//! Native Arrow validates arrays; untrusted lengths are checked before allocation.
use super::{admit_bulk_work, invalid, plus, times};
use crate::{
    generation::application::reserve_schema_encoding,
    resource::{Budget, Cost, OwnedRetainedReservation, RetainedReservation},
    table::Table,
};
use arrow_array::{Array, FixedSizeListArray, Float32Array, RecordBatch, UInt32Array};
use arrow_buffer::{Buffer, MutableBuffer};
use arrow_ipc::{
    Block, FooterBuilder, MetadataVersion,
    convert::IpcSchemaEncoder,
    reader::{FileDecoder, read_footer_length},
    writer::{DictionaryTracker, IpcDataGenerator, IpcWriteOptions, write_message},
};
use arrow_schema::{DataType, Field, SchemaRef};
use flatbuffers::{FlatBufferBuilder, VerifierOptions};
use lycheedev_core::{Result, workspace::data_disk::{ObjectReader, ReaderCheck}};
use std::{io::SeekFrom, sync::Arc};

// This owner is private: no cloned array escapes its allocation guard.
struct LeasedBatch<'a> {
    records: RecordBatch,
    _encoded: Buffer,
    _allocation: RetainedReservation<'a>,
}
/// This private owned value is lent to one cursor; bare arrays never escape it.
pub(crate) struct OwnedBatch {
    pub(crate) records: RecordBatch,
    _encoded: Buffer,
    _allocation: OwnedRetainedReservation,
}
impl OwnedBatch {
    /// Transfer the original admission into an array-free lifetime owner.
    /// Decoded arrays already hold shared references to their encoded buffers.
    /// Keeping `_encoded` in that owner would create a buffer/owner cycle after
    /// Arrow's claim hook is installed, so its redundant reference is dropped.
    pub(crate) fn into_parts(self) -> (RecordBatch, OwnedRetainedReservation) {
        (self.records, self._allocation)
    }
}
#[derive(Clone, Copy)]
pub(in crate::generation) struct BatchBlock {
    pub(in crate::generation) block: Block,
    pub(in crate::generation) first: u32,
    pub(in crate::generation) last: u32,
    pub(in crate::generation) rows: usize,
}
pub(in crate::generation) struct Directory {
    pub(in crate::generation) blocks: Vec<BatchBlock>,
    _allocation: OwnedRetainedReservation,
}
pub(in crate::generation) fn directory<C: ReaderCheck>(
    reader: &mut ObjectReader<C>,
    schema: &SchemaRef,
    table: &Table<'_>,
    batch_count: usize,
    budget: &Arc<Budget>,
) -> Result<Directory> {
    if batch_count > table.logical_count() {
        return Err(invalid());
    }
    let allocation = budget.reserve_retained(plus(
        times(batch_count, std::mem::size_of::<BatchBlock>())?,
        256,
    )?)?;
    let mut blocks = Vec::new();
    blocks
        .try_reserve_exact(batch_count)
        .map_err(|_| super::limit())?;
    inspect(reader, schema, table, batch_count, budget, &mut |entry| {
        blocks.push(entry);
        Ok(())
    })?;
    Ok(Directory {
        blocks,
        _allocation: allocation.into_owned(budget)?,
    })
}
pub(in crate::generation) fn read_owned_batch<C: ReaderCheck>(
    reader: &mut ObjectReader<C>,
    schema: &SchemaRef,
    block: &Block,
    budget: &Arc<Budget>,
) -> Result<OwnedBatch> {
    let batch = decode_batch(reader, schema, block, budget)?;
    Ok(OwnedBatch {
        records: batch.records,
        _encoded: batch._encoded,
        _allocation: batch._allocation.into_owned(budget)?,
    })
}
fn read_at<C: ReaderCheck>(reader: &mut ObjectReader<C>, offset: u64, bytes: &mut [u8]) -> Result<()> {
    reader.seek_checked(SeekFrom::Start(offset))?;
    reader.read_exact_checked(bytes)
}
fn nonnegative(n: i64) -> Result<usize> {
    usize::try_from(n).map_err(|_| invalid())
}
fn verify_options(bytes: usize) -> VerifierOptions {
    VerifierOptions {
        max_depth: 16,
        max_tables: bytes / 4 + 1,
        max_apparent_size: bytes.saturating_mul(8),
        ..Default::default()
    }
}

pub(super) fn validate<C: ReaderCheck>(
    reader: &mut ObjectReader<C>,
    schema: &SchemaRef,
    table: &Table<'_>,
    batch_count: usize,
    budget: &Budget,
) -> Result<()> {
    inspect(reader, schema, table, batch_count, budget, &mut |_| Ok(()))
}
fn inspect<C: ReaderCheck>(
    reader: &mut ObjectReader<C>,
    schema: &SchemaRef,
    table: &Table<'_>,
    batch_count: usize,
    budget: &Budget,
    record: &mut dyn FnMut(BatchBlock) -> Result<()>,
) -> Result<()> {
    budget.check()?;
    let length = usize::try_from(reader.len()).map_err(|_| invalid())?;
    if length < 82 || batch_count > table.logical_count() {
        return Err(invalid());
    }
    let _schema_scratch = reserve_schema_encoding(schema, budget)?;
    let options = IpcWriteOptions::default();
    let encoded = IpcDataGenerator::default().schema_to_bytes_with_dictionary_tracker(
        schema,
        &mut DictionaryTracker::new(true),
        &options,
    );
    // This version uses the native writer's 64-byte header alignment.
    let mut header = vec![0u8; 64];
    header[..6].copy_from_slice(b"ARROW1");
    write_message(&mut header, encoded, &options).map_err(|_| invalid())?;
    let header_end = header.len();
    let mut actual_header = vec![0; header_end];
    admit_bulk_work(budget, plus(header_end, 18)?)?;
    read_at(reader, 0, &mut actual_header)?;
    if actual_header != header {
        return Err(invalid());
    }
    drop(actual_header);
    drop(header);

    let mut tail = [0u8; 10];
    read_at(reader, (length - 10) as u64, &mut tail)?;
    let footer_len = read_footer_length(tail).map_err(|_| invalid())?;
    let minimum_footer_start = plus(header_end, 8)?;
    let footer_start = length
        .checked_sub(plus(footer_len, 10)?)
        .filter(|start| *start >= minimum_footer_start)
        .ok_or_else(invalid)?;
    // Footer bytes, copied Block list, builder growth/old-new overlap and the
    // known schema encoder's scratch are reserved before entering Arrow.
    let _footer_allocation = budget.reserve_retained(plus(times(footer_len, 8)?, 65536)?)?;
    admit_bulk_work(budget, footer_len)?;
    let mut footer_bytes = vec![0; footer_len];
    read_at(reader, footer_start as u64, &mut footer_bytes)?;
    let footer = arrow_ipc::root_as_footer_with_opts(&verify_options(footer_len), &footer_bytes)
        .map_err(|_| invalid())?;
    let blocks = footer.recordBatches().ok_or_else(invalid)?;
    if blocks.len() != batch_count || footer.version() != MetadataVersion::V5 {
        return Err(invalid());
    }
    // Validate all fixed-width Block entries before collecting or constructing
    // the expected footer. The existing producer writes contiguous blocks.
    let mut end = header_end;
    for block in blocks.iter() {
        budget.check()?;
        let offset = nonnegative(block.offset())?;
        let meta = usize::try_from(block.metaDataLength()).map_err(|_| invalid())?;
        let body = nonnegative(block.bodyLength())?;
        if offset != end || meta < 8 || meta % 64 != 0 || body % 64 != 0 {
            return Err(invalid());
        }
        end = plus(offset, plus(meta, body)?)?;
        if end > footer_start {
            return Err(invalid());
        }
    }
    if plus(end, 8)? != footer_start {
        return Err(invalid());
    }
    let mut eos = [0u8; 8];
    read_at(reader, end as u64, &mut eos)?;
    if eos != [255, 255, 255, 255, 0, 0, 0, 0] {
        return Err(invalid());
    }
    // Encode trusted schema with mature APIs, rather than calling the native
    // fb_to_schema converter on untrusted data (which contains panic branches).
    // Exact canonical bytes intentionally support only our versioned producer.
    let copied_blocks: Vec<Block> = blocks.iter().copied().collect();
    let mut fbb = FlatBufferBuilder::new();
    let dictionaries = fbb.create_vector::<Block>(&[]);
    let batches = fbb.create_vector(&copied_blocks);
    let known_schema = IpcSchemaEncoder::new()
        .with_dictionary_tracker(&mut DictionaryTracker::new(true))
        .schema_to_fb_offset(&mut fbb, schema);
    let mut builder = FooterBuilder::new(&mut fbb);
    builder.add_version(MetadataVersion::V5);
    builder.add_schema(known_schema);
    builder.add_dictionaries(dictionaries);
    builder.add_recordBatches(batches);
    let root = builder.finish();
    fbb.finish(root, None);
    if fbb.finished_data() != footer_bytes {
        return Err(invalid());
    }
    drop(fbb);

    let mut ordinal = 0usize;
    for block in &copied_blocks {
        let batch = decode_batch(reader, schema, block, budget)?;
        for column in batch.records.columns() {
            validate_values(column.as_ref(), budget)?;
        }
        let ids = batch
            .records
            .column(schema.fields().len() - 1)
            .as_any()
            .downcast_ref::<UInt32Array>()
            .ok_or_else(invalid)?;
        if ids.null_count() != 0 {
            return Err(invalid());
        }
        for id in ids.values() {
            budget.charge(Cost::work(1))?;
            if table.id_at(ordinal) != Some(*id) {
                return Err(invalid());
            }
            ordinal = plus(ordinal, 1)?;
        }
        record(BatchBlock {
            block: *block,
            first: *ids.values().first().ok_or_else(invalid)?,
            last: *ids.values().last().ok_or_else(invalid)?,
            rows: ids.len(),
        })?;
        // The Arrow buffer aliases retained by records leave before its lease.
        drop(batch);
    }
    if ordinal != table.logical_count() {
        return Err(invalid());
    }
    budget.check()
}

fn decode_batch<'a, C: ReaderCheck>(
    reader: &mut ObjectReader<C>,
    schema: &SchemaRef,
    block: &Block,
    budget: &'a Budget,
) -> Result<LeasedBatch<'a>> {
    budget.check()?;
    let meta_len = usize::try_from(block.metaDataLength()).map_err(|_| invalid())?;
    let body_len = nonnegative(block.bodyLength())?;
    let total = plus(meta_len, body_len)?;
    let nodes = schema.fields().iter().try_fold(0usize, |n, f| {
        plus(
            n,
            if matches!(f.data_type(), DataType::FixedSizeList(_, _)) {
                2
            } else {
                1
            },
        )
    })?;
    let allocation =
        budget.reserve_retained(plus(plus(total, 64)?, plus(times(nodes, 4096)?, 4096)?)?)?;
    std::alloc::Layout::from_size_align(total, 64).map_err(|_| invalid())?;
    admit_bulk_work(budget, total)?;
    let mut bytes = MutableBuffer::from_len_zeroed(total);
    read_at(reader, block.offset() as u64, bytes.as_slice_mut())?;
    validate_message(bytes.as_slice(), meta_len, body_len, schema)?;
    let bytes: Buffer = bytes.into();
    let records = FileDecoder::new(schema.clone(), MetadataVersion::V5)
        .with_require_alignment(true)
        .read_record_batch(block, &bytes)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    Ok(LeasedBatch {
        records,
        _encoded: bytes,
        _allocation: allocation,
    })
}

fn validate_message(
    bytes: &[u8],
    meta_len: usize,
    body_len: usize,
    schema: &SchemaRef,
) -> Result<()> {
    if bytes.len() < 8 || bytes[..4] != [255, 255, 255, 255] {
        return Err(invalid());
    }
    let encoded = usize::try_from(i32::from_le_bytes(
        bytes[4..8].try_into().map_err(|_| invalid())?,
    ))
    .map_err(|_| invalid())?;
    if plus(encoded, 8)? != meta_len {
        return Err(invalid());
    }
    let message =
        arrow_ipc::root_as_message_with_opts(&verify_options(meta_len), &bytes[8..meta_len])
            .map_err(|_| invalid())?;
    if message.version() != MetadataVersion::V5
        || nonnegative(message.bodyLength())? != body_len
        || message.custom_metadata().is_some()
    {
        return Err(invalid());
    }
    let batch = message.header_as_record_batch().ok_or_else(invalid)?;
    let rows = nonnegative(batch.length())?;
    if rows == 0
        || rows > super::super::ImportLimits::default().rows
        || batch.compression().is_some()
        || batch.variadicBufferCounts().is_some_and(|v| !v.is_empty())
    {
        return Err(invalid());
    }
    let nodes = batch.nodes().ok_or_else(invalid)?;
    let buffers = batch.buffers().ok_or_else(invalid)?;
    let mut node_at = 0usize;
    let mut buffer_at = 0usize;
    for field in schema.fields() {
        validate_field(
            field,
            rows,
            &nodes,
            &buffers,
            &mut node_at,
            &mut buffer_at,
            body_len,
        )?;
    }
    if node_at != nodes.len() || buffer_at != buffers.len() {
        return Err(invalid());
    }
    Ok(())
}
fn validate_field(
    field: &Field,
    rows: usize,
    nodes: &flatbuffers::Vector<'_, arrow_ipc::FieldNode>,
    buffers: &flatbuffers::Vector<'_, arrow_ipc::Buffer>,
    node_at: &mut usize,
    buffer_at: &mut usize,
    body_len: usize,
) -> Result<()> {
    if *node_at >= nodes.len() {
        return Err(invalid());
    }
    let node = nodes.get(*node_at);
    *node_at += 1;
    let nulls = nonnegative(node.null_count())?;
    if nonnegative(node.length())? != rows || nulls > rows || (!field.is_nullable() && nulls != 0) {
        return Err(invalid());
    }
    let mut take = || -> Result<usize> {
        if *buffer_at >= buffers.len() {
            return Err(invalid());
        }
        let buffer = buffers.get(*buffer_at);
        *buffer_at += 1;
        let offset = nonnegative(buffer.offset())?;
        let len = nonnegative(buffer.length())?;
        if plus(offset, len)? > body_len || offset % 64 != 0 {
            return Err(invalid());
        }
        Ok(len)
    };
    let validity = take()?;
    let bitmap = plus(rows, 7)? / 8;
    if (validity != 0 && validity != bitmap) || (nulls != 0 && validity != bitmap) {
        return Err(invalid());
    }
    match field.data_type() {
        DataType::Int64 | DataType::UInt64 => {
            if take()? != times(rows, 8)? {
                return Err(invalid());
            }
        }
        DataType::Float32 | DataType::UInt32 => {
            if take()? != times(rows, 4)? {
                return Err(invalid());
            }
        }
        DataType::Utf8 => {
            if take()? != times(plus(rows, 1)?, 4)? {
                return Err(invalid());
            }
            take()?;
        }
        DataType::FixedSizeList(child, size) => {
            let width = usize::try_from(*size).map_err(|_| invalid())?;
            if width == 0 || matches!(child.data_type(), DataType::FixedSizeList(_, _)) {
                return Err(invalid());
            }
            validate_field(
                child,
                times(rows, width)?,
                nodes,
                buffers,
                node_at,
                buffer_at,
                body_len,
            )?;
        }
        _ => return Err(invalid()),
    }
    Ok(())
}

fn validate_values(array: &dyn Array, budget: &Budget) -> Result<()> {
    match array.data_type() {
        DataType::Float32 => {
            let floats = array
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(invalid)?;
            for value in floats.iter() {
                budget.charge(Cost::work(1))?;
                if value.is_some_and(|value| !value.is_finite()) {
                    return Err(invalid());
                }
            }
        }
        DataType::FixedSizeList(_, _) => {
            let list = array
                .as_any()
                .downcast_ref::<FixedSizeListArray>()
                .ok_or_else(invalid)?;
            validate_values(list.values().as_ref(), budget)?;
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };

    fn message(case: usize) -> (Vec<u8>, usize) {
        let mut fbb = FlatBufferBuilder::new();
        let nodes =
            fbb.create_vector(&[arrow_ipc::FieldNode::new(if case == 2 { -1 } else { 1 }, 0)]);
        let buffers = fbb.create_vector(&[
            arrow_ipc::Buffer::new(0, 0),
            arrow_ipc::Buffer::new(if case == 1 { 128 } else { 0 }, 4),
        ]);
        let compression = if case == 3 {
            Some(arrow_ipc::BodyCompression::create(
                &mut fbb,
                &Default::default(),
            ))
        } else {
            None
        };
        let variadic = if case == 4 {
            Some(fbb.create_vector(&[1i64]))
        } else {
            None
        };
        let batch = arrow_ipc::RecordBatch::create(
            &mut fbb,
            &arrow_ipc::RecordBatchArgs {
                length: 1,
                nodes: Some(nodes),
                buffers: if case == 5 { None } else { Some(buffers) },
                compression,
                variadicBufferCounts: variadic,
            },
        );
        let message = arrow_ipc::Message::create(
            &mut fbb,
            &arrow_ipc::MessageArgs {
                version: MetadataVersion::V5,
                header_type: arrow_ipc::MessageHeader::RecordBatch,
                header: Some(batch.as_union_value()),
                bodyLength: 64,
                custom_metadata: None,
            },
        );
        fbb.finish(message, None);
        let mut bytes = Vec::new();
        let (meta, body) = write_message(
            &mut bytes,
            arrow_ipc::writer::EncodedData {
                ipc_message: fbb.finished_data().to_vec(),
                arrow_data: vec![0; 64],
            },
            &IpcWriteOptions::default(),
        )
        .unwrap();
        assert_eq!(body, 64);
        (bytes, meta)
    }
    #[test]
    fn metadata_preflight_accepts_native_plain_batch_and_rejects_unsafe_shapes() {
        let schema = Arc::new(arrow_schema::Schema::new(vec![Field::new(
            "id",
            DataType::UInt32,
            false,
        )]));
        let (good, meta) = message(0);
        validate_message(&good, meta, 64, &schema).unwrap();
        for case in 1..=5 {
            let (bad, meta) = message(case);
            assert_eq!(
                validate_message(&bad, meta, 64, &schema).unwrap_err().code,
                "records.generation_invalid",
                "case {case}"
            );
        }
    }
    #[test]
    fn private_decoded_batch_keeps_allocation_until_arrow_arrays_are_dropped() {
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(5),
            None,
        );
        let baseline = budget.snapshot().unwrap().retained_bytes;
        let allocation = budget.reserve_retained(8192).unwrap();
        let schema = Arc::new(arrow_schema::Schema::new(vec![Field::new(
            "id",
            DataType::UInt32,
            false,
        )]));
        let records =
            RecordBatch::try_new(schema, vec![Arc::new(UInt32Array::from(vec![7]))]).unwrap();
        let batch = LeasedBatch {
            records,
            _encoded: Buffer::from(&[] as &[u8]),
            _allocation: allocation,
        };
        assert_eq!(budget.snapshot().unwrap().retained_bytes, baseline + 8192);
        assert_eq!(batch.records.num_rows(), 1);
        drop(batch);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, baseline);
    }
}
