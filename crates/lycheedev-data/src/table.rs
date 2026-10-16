// SPDX-License-Identifier: MIT
// WDC structural and storage contracts adapted from the frozen MIT Go/wowdata implementation.
//! Borrowed, bounded WDC metadata and row views. No table-wide row materialization.
use crate::{
    fault,
    resource::{Budget, Cost},
    schema::{Definition, Field},
};
use lycheedev_core::Result;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    ops::Range,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
fn invalid<T>() -> Result<T> {
    Err(fault(
        "table.invalid_format",
        4,
        "Invalid WDC structure, storage or row",
    ))
}
fn limit<T>() -> Result<T> {
    Err(fault(
        "table.limit_exceeded",
        3,
        "WDC resource budget exhausted",
    ))
}
fn unsupported<T>() -> Result<T> {
    Err(fault(
        "table.unsupported_format",
        3,
        "Unsupported WDC format or storage",
    ))
}
#[derive(Debug, Clone, Copy)]
pub struct TableLimits {
    pub file_bytes: usize,
    pub metadata_bytes: usize,
    pub rows: u32,
    pub columns: u32,
    pub partitions: u32,
    pub decode_work: u64,
}
impl Default for TableLimits {
    fn default() -> Self {
        Self {
            file_bytes: 256 << 20,
            metadata_bytes: 64 << 20,
            rows: 10_000_000,
            columns: 65536,
            partitions: 65536,
            decode_work: 100_000_000,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Storage {
    pub bit_offset: u16,
    pub bit_width: u16,
    pub extra_bytes: u32,
    pub codec: u32,
    pub parameters: [u32; 3],
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    pub width_delta: i16,
    pub byte_offset: u16,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Partition {
    pub key_id: u64,
    pub offset: u32,
    pub rows: u32,
    pub string_bytes: u32,
    pub identity_bytes: u32,
    pub relation_bytes: u32,
    pub copies: u32,
    pub sparse_offset: u32,
    pub sparse_count: u32,
    pub encrypted_ids: Vec<u32>,
    pub ids_complete: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub version: u8,
    pub schema_revision: u32,
    pub schema_build: String,
    pub rows: u32,
    pub columns: u32,
    pub stride: u32,
    pub table_hash: u32,
    pub layout_hash: u32,
    pub first_id: u32,
    pub last_id: u32,
    pub locale: u32,
    pub flags: u16,
    pub identity_column: u16,
    pub packed_offset: u32,
    pub lookup_columns: u32,
    pub fields: Vec<Storage>,
    pub placements: Vec<Placement>,
    pub partitions: Vec<Partition>,
    pub palette_offset: usize,
    pub palette_bytes: u32,
    pub common_offset: usize,
    pub common_bytes: u32,
    pub metadata_end: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Value {
    Null,
    Int(i64),
    UInt(u64),
    Float32(f32),
    Float(f64),
    String(String),
    Array(Vec<Value>),
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub rows: Vec<BTreeMap<String, Value>>,
    pub after_id: Option<u32>,
    pub truncated: bool,
    pub complete: bool,
    pub row_count: usize,
}
#[derive(Debug, Clone, Copy)]
struct Location {
    offset: usize,
    partition: usize,
    origin: u32,
    length: u32,
    relation: Option<u32>,
}
struct Program {
    storage: Storage,
    palette: Range<usize>,
    common: HashMap<u32, u32>,
}
/// One decoded row may stay live in a suspended query cursor without escaping
/// its allocation lease. Only Rust retained safety bytes are temporary; the
/// original table work and cumulative metadata charges are never refunded.
struct RowAllocation<'a> {
    budget: Option<&'a Budget>,
    scoped: bool,
    reservation: Option<crate::resource::RetainedReservation<'a>>,
}
impl<'a> RowAllocation<'a> {
    fn new(budget: Option<&'a Budget>, scoped: bool) -> Self {
        Self {
            budget,
            scoped,
            reservation: None,
        }
    }
    fn retain(&mut self, bytes: usize) -> Result<()> {
        let Some(budget) = self.budget else {
            return Ok(());
        };
        if self.scoped {
            if let Some(reservation) = &mut self.reservation {
                reservation.grow(bytes)?;
            } else {
                self.reservation = Some(budget.reserve_retained(bytes)?);
            }
            Ok(())
        } else {
            // The legacy owned-row/page API keeps its previous lifetime: its
            // caller can retain the returned map after the table is dropped.
            budget.charge(Cost::retained(bytes))
        }
    }
}
/// A decoded row and the exact refundable allocation that owns its contents.
/// Mapping moves values (including strings) without refunding or re-admitting
/// them. The value is dropped before its guard, including on consumer failure.
pub(crate) struct LeasedRow<'table, T> {
    value: T,
    allocation: RowAllocation<'table>,
}
impl<'table, T> LeasedRow<'table, T> {
    pub(crate) fn value(&self) -> &T {
        &self.value
    }
    pub(crate) fn try_map<U>(
        self,
        map: impl FnOnce(T) -> Result<U>,
    ) -> Result<LeasedRow<'table, U>> {
        let Self { value, allocation } = self;
        let value = map(value)?;
        Ok(LeasedRow { value, allocation })
    }
}
// An owned query body retains its exact allocation/verification owner. The
// index stores offsets, so no field borrows this enum or another sibling.
enum TableBytes<'a> {
    Borrowed(&'a [u8]),
    Owned(Box<dyn AsRef<[u8]> + Send + Sync>),
}
impl std::ops::Deref for TableBytes<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        match self {
            Self::Borrowed(raw) => raw,
            Self::Owned(raw) => raw.as_ref().as_ref(),
        }
    }
}
struct Source<'a> {
    raw: TableBytes<'a>,
    missing: Vec<Range<usize>>,
    budget: Option<Arc<Budget>>,
    metadata_bytes: AtomicU64,
    metadata_limit: usize,
}
impl<'a> Source<'a> {
    fn new(
        raw: &'a [u8],
        missing: &[Range<usize>],
        limits: TableLimits,
        budget: Option<Arc<Budget>>,
    ) -> Result<Self> {
        Self::with_bytes(TableBytes::Borrowed(raw), missing, limits, budget)
    }
    fn with_bytes(
        raw: TableBytes<'a>,
        missing: &[Range<usize>],
        limits: TableLimits,
        budget: Option<Arc<Budget>>,
    ) -> Result<Self> {
        let mut source = Self {
            raw,
            missing: vec![],
            budget,
            metadata_bytes: AtomicU64::new(0),
            metadata_limit: limits.metadata_bytes,
        };
        source.metadata(
            missing
                .len()
                .checked_mul(std::mem::size_of::<Range<usize>>())
                .ok_or_else(|| {
                    fault(
                        "table.limit_exceeded",
                        3,
                        "Missing range allocation overflow",
                    )
                })?,
        )?;
        source.missing = missing.to_vec();
        Ok(source)
    }
    fn metadata(&self, bytes: usize) -> Result<()> {
        self.metadata_split(bytes, bytes)
    }
    // Public Go accounting and conservative Rust allocation admission are
    // separate ledgers. Equal estimates on 64-bit targets are not measured RSS.
    fn metadata_split(&self, logical_bytes: usize, bytes: usize) -> Result<()> {
        let used = self.metadata_bytes.load(Ordering::Relaxed);
        if bytes as u64 > (self.metadata_limit as u64).saturating_sub(used) {
            return limit();
        }
        if let Some(budget) = &self.budget {
            budget.charge_split(Cost::metadata(logical_bytes), Cost::metadata(bytes))?;
        }
        self.metadata_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
        Ok(())
    }
    fn temporary(&self, bytes: usize) -> Result<()> {
        if let Some(budget) = &self.budget {
            budget.charge(Cost::retained(bytes))?;
        }
        Ok(())
    }
    fn read(&self, offset: usize, length: usize) -> Result<&[u8]> {
        if let Some(budget) = &self.budget {
            budget.check()?;
        }
        if offset > self.raw.len() || length > self.raw.len() - offset {
            return invalid();
        }
        let end = offset + length;
        if self
            .missing
            .iter()
            .any(|range| offset < range.end && end > range.start)
        {
            return Err(fault(
                "table.partition_unavailable",
                3,
                "Authenticated source bytes require a missing decryption key",
            ));
        }
        Ok(&self.raw[offset..end])
    }
    // Index construction runs before an owned body is promoted into this
    // source. Keeping its external borrow lets the index mutate disjoint owned
    // maps while reading auxiliary bytes, without copying or self references.
    fn index_read(&self, offset: usize, length: usize) -> Result<&'a [u8]> {
        self.read(offset, length)?;
        match &self.raw {
            TableBytes::Borrowed(raw) => {
                let raw = *raw;
                Ok(&raw[offset..offset + length])
            }
            TableBytes::Owned(_) => Err(fault(
                "records.internal",
                8,
                "WDC index cannot be rebuilt after ownership promotion",
            )),
        }
    }
}
fn u16at(raw: &[u8], offset: usize) -> Result<u16> {
    raw.get(offset..offset + 2)
        .map(|b| u16::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| fault("table.invalid_format", 4, "Truncated WDC word"))
}
fn u32at(raw: &[u8], offset: usize) -> Result<u32> {
    raw.get(offset..offset + 4)
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| fault("table.invalid_format", 4, "Truncated WDC word"))
}
fn u64at(raw: &[u8], offset: usize) -> Result<u64> {
    raw.get(offset..offset + 8)
        .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| fault("table.invalid_format", 4, "Truncated WDC word"))
}
struct Reader<'s, 'a> {
    source: &'s Source<'a>,
    offset: usize,
    budget: usize,
}
impl<'s, 'a> Reader<'s, 'a> {
    fn take(&mut self, n: usize) -> Result<&'s [u8]> {
        if n > self.budget.saturating_sub(self.offset) {
            return limit();
        }
        let data = self.source.read(self.offset, n)?;
        self.offset += n;
        Ok(data)
    }
    fn skip(&mut self, n: usize) -> Result<()> {
        self.take(n).map(|_| ())
    }
}
pub fn inspect(raw: &[u8], limits: TableLimits) -> Result<Layout> {
    inspect_source(&Source::new(raw, &[], limits, None)?, limits)
}
pub fn inspect_budgeted(raw: &[u8], limits: TableLimits, budget: Arc<Budget>) -> Result<Layout> {
    inspect_source(&Source::new(raw, &[], limits, Some(budget))?, limits)
}
fn inspect_source(source: &Source<'_>, limits: TableLimits) -> Result<Layout> {
    if limits.file_bytes == 0
        || limits.file_bytes > 1usize << 40
        || source.raw.len() > limits.file_bytes
        || limits.metadata_bytes == 0
        || limits.metadata_bytes > 64 << 20
        || limits.rows == 0
        || limits.columns == 0
        || limits.columns > 65536
        || limits.partitions == 0
        || limits.partitions > 65536
    {
        return limit();
    }
    let mut reader = Reader {
        source,
        offset: 0,
        budget: limits.metadata_bytes,
    };
    let version = match reader.take(4)? {
        b"WDC2" | b"1SLC" => 2,
        b"WDC3" => 3,
        b"WDC4" => 4,
        b"WDC5" => 5,
        _ => return unsupported(),
    };
    let (mut schema_revision, mut schema_build) = (0, String::new());
    if version == 5 {
        let data = reader.take(132)?;
        schema_revision = u32at(data, 0)?;
        let bytes = &data[4..];
        let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
        source.metadata(end)?;
        schema_build = String::from_utf8(bytes[..end].to_vec())
            .map_err(|_| fault("table.invalid_format", 4, "Invalid WDC schema-build text"))?;
    }
    let header = reader.take(68)?;
    let rows = u32at(header, 0)?;
    let columns = u32at(header, 4)?;
    let stride = u32at(header, 8)?;
    let first_id = u32at(header, 24)?;
    let last_id = u32at(header, 28)?;
    let flags = u16at(header, 36)?;
    let total_columns = u32at(header, 40)?;
    let storage_bytes = u32at(header, 52)?;
    let common_bytes = u32at(header, 56)?;
    let palette_bytes = u32at(header, 60)?;
    let partition_count = u32at(header, 64)?;
    if rows > limits.rows
        || total_columns > limits.columns
        || columns > limits.columns
        || partition_count > limits.partitions
    {
        return limit();
    }
    if rows > 0 && (partition_count == 0 || (stride == 0 && flags & 1 == 0) || last_id < first_id)
        || storage_bytes % 24 != 0
        || storage_bytes / 24 > total_columns
    {
        return invalid();
    }
    let estimated =
        partition_count as usize * 128 + total_columns as usize * 16 + storage_bytes as usize * 2;
    if estimated > limits.metadata_bytes {
        return limit();
    }
    source.metadata(
        partition_count as usize * std::mem::size_of::<Partition>()
            + total_columns as usize * std::mem::size_of::<Placement>()
            + storage_bytes as usize / 24 * std::mem::size_of::<Storage>(),
    )?;
    let mut partitions = Vec::with_capacity(partition_count as usize);
    let mut partition_rows = 0u64;
    for _ in 0..partition_count {
        let data = reader.take(if version == 2 { 36 } else { 40 })?;
        let identity_bytes = u32at(data, if version == 2 { 28 } else { 24 })?;
        if identity_bytes % 4 != 0 {
            return invalid();
        }
        let rows = u32at(data, 12)?;
        partition_rows += rows as u64;
        if partition_rows > limits.rows as u64 {
            return limit();
        }
        let (mut copies, mut sparse_offset, mut relation_bytes, mut sparse_count) = (
            u32at(data, 36.min(data.len() - 4))?,
            u32at(data, 20)?,
            u32at(data, 28)?,
            0,
        );
        if version == 2 {
            let bytes = u32at(data, 20)?;
            if bytes % 8 != 0 {
                return invalid();
            }
            copies = bytes / 8;
            sparse_offset = u32at(data, 24)?;
            relation_bytes = u32at(data, 32)?;
        } else {
            sparse_count = u32at(data, 32)?;
        }
        partitions.push(Partition {
            key_id: u64at(data, 0)?,
            offset: u32at(data, 8)?,
            rows,
            string_bytes: u32at(data, 16)?,
            identity_bytes,
            relation_bytes,
            copies,
            sparse_offset,
            sparse_count,
            encrypted_ids: vec![],
            ids_complete: false,
        });
    }
    if partition_rows != rows as u64 {
        return invalid();
    }
    let placements_data = reader.take(total_columns as usize * 4)?;
    let mut placements = Vec::with_capacity(total_columns as usize);
    for i in 0..total_columns as usize {
        placements.push(Placement {
            width_delta: u16at(placements_data, i * 4)? as i16,
            byte_offset: u16at(placements_data, i * 4 + 2)?,
        });
    }
    let mut fields = Vec::with_capacity(storage_bytes as usize / 24);
    let (mut palette_total, mut common_total) = (0u64, 0u64);
    for _ in 0..storage_bytes / 24 {
        let data = reader.take(24)?;
        let field = Storage {
            bit_offset: u16at(data, 0)?,
            bit_width: u16at(data, 2)?,
            extra_bytes: u32at(data, 4)?,
            codec: u32at(data, 8)?,
            parameters: [u32at(data, 12)?, u32at(data, 16)?, u32at(data, 20)?],
        };
        if field.codec > 5 {
            return unsupported();
        }
        if field.codec == 2 && !field.extra_bytes.is_multiple_of(8)
            || (field.codec == 3 || field.codec == 4) && !field.extra_bytes.is_multiple_of(4)
            || field.codec == 4
                && (field.parameters[2] == 0
                    || !(field.extra_bytes / 4).is_multiple_of(field.parameters[2]))
        {
            return invalid();
        }
        if field.codec == 2 {
            common_total += field.extra_bytes as u64;
        }
        if [3, 4].contains(&field.codec) {
            palette_total += field.extra_bytes as u64;
        }
        fields.push(field);
    }
    if palette_total != palette_bytes as u64 || common_total != common_bytes as u64 {
        return invalid();
    }
    let palette_offset = reader.offset;
    reader.skip(palette_bytes as usize)?;
    let common_offset = reader.offset;
    reader.skip(common_bytes as usize)?;
    if version >= 4 {
        let mut total = 0u64;
        for partition in &mut partitions {
            if partition.key_id == 0 {
                continue;
            }
            let count = u32at(reader.take(4)?, 0)?;
            total += count as u64;
            if total > limits.rows as u64 {
                return limit();
            }
            let data = reader.take(count as usize * 4)?;
            source.metadata(count as usize * 64)?;
            let mut seen = BTreeSet::new();
            for i in 0..count as usize {
                let id = u32at(data, i * 4)?;
                if !seen.insert(id) {
                    return invalid();
                }
                partition.encrypted_ids.push(id);
            }
            partition.ids_complete =
                count as u64 == partition.rows as u64 + partition.copies as u64;
        }
    }
    let metadata_end = reader.offset;
    let mut previous_end = metadata_end as u64;
    for p in &partitions {
        let start = p.offset as u64;
        if start < previous_end || start > source.raw.len() as u64 {
            return invalid();
        }
        let payload = if flags & 1 == 0 {
            p.rows as u64 * stride as u64 + p.string_bytes as u64
        } else {
            if p.sparse_offset < p.offset {
                return invalid();
            }
            p.sparse_offset as u64 - start
                + if version == 2 {
                    (last_id as u64 - first_id as u64 + 1) * 6
                } else {
                    0
                }
        };
        let payload = payload
            + p.identity_bytes as u64
            + p.relation_bytes as u64
            + p.copies as u64 * 8
            + if version >= 3 {
                p.sparse_count as u64 * 10
            } else {
                0
            };
        if payload > source.raw.len() as u64 - start {
            return invalid();
        }
        previous_end = start + payload;
    }
    Ok(Layout {
        version,
        schema_revision,
        schema_build,
        rows,
        columns,
        stride,
        table_hash: u32at(header, 16)?,
        layout_hash: u32at(header, 20)?,
        first_id,
        last_id,
        locale: u32at(header, 32)?,
        flags,
        identity_column: u16at(header, 38)?,
        packed_offset: u32at(header, 44)?,
        lookup_columns: u32at(header, 48)?,
        fields,
        placements,
        partitions,
        palette_offset,
        palette_bytes,
        common_offset,
        common_bytes,
        metadata_end,
    })
}
pub fn extract_bits(record: &[u8], offset: u64, width: u32) -> Result<u64> {
    if width > 64
        || offset > record.len() as u64 * 8
        || width as u64 > record.len() as u64 * 8 - offset
    {
        return invalid();
    }
    if offset.is_multiple_of(8) {
        let at = (offset / 8) as usize;
        match width {
            8 => return Ok(record[at] as u64),
            16 => return Ok(u16::from_le_bytes(record[at..at + 2].try_into().unwrap()) as u64),
            32 => return Ok(u32::from_le_bytes(record[at..at + 4].try_into().unwrap()) as u64),
            64 => return Ok(u64::from_le_bytes(record[at..at + 8].try_into().unwrap())),
            _ => {}
        }
    }
    let mut value = 0u64;
    let mut copied = 0u32;
    while copied < width {
        let position = offset + copied as u64;
        let shift = (position & 7) as u32;
        let count = (8 - shift).min(width - copied);
        let fragment = (record[position as usize / 8] >> shift) as u64 & ((1u64 << count) - 1);
        value |= fragment << copied;
        copied += count;
    }
    Ok(value)
}
pub struct Table<'a> {
    source: Source<'a>,
    layout: Layout,
    programs: Vec<Program>,
    // Logical IDs, including copies, share immutable physical locations.
    locations: HashMap<u32, u32>,
    physical: Vec<Location>,
    ids: Vec<u32>,
    definition: Definition,
    column_mapping: Vec<Option<usize>>,
    unavailable: BTreeSet<usize>,
    work: AtomicU64,
    limits: TableLimits,
}
pub fn open<'a>(raw: &'a [u8], definition: &Definition, limits: TableLimits) -> Result<Table<'a>> {
    open_available(raw, definition, limits, &BTreeSet::new(), &[])
}
pub fn open_available<'a>(
    raw: &'a [u8],
    definition: &Definition,
    limits: TableLimits,
    unavailable: &BTreeSet<usize>,
    missing_ranges: &[Range<usize>],
) -> Result<Table<'a>> {
    open_inner(raw, definition, limits, unavailable, missing_ranges, None)
}
pub fn open_budgeted<'a>(
    raw: &'a [u8],
    definition: &Definition,
    limits: TableLimits,
    unavailable: &BTreeSet<usize>,
    missing_ranges: &[Range<usize>],
    budget: Arc<Budget>,
) -> Result<Table<'a>> {
    open_inner(
        raw,
        definition,
        limits,
        unavailable,
        missing_ranges,
        Some(budget),
    )
}
fn open_inner<'a>(
    raw: &'a [u8],
    definition: &Definition,
    limits: TableLimits,
    unavailable: &BTreeSet<usize>,
    missing_ranges: &[Range<usize>],
    budget: Option<Arc<Budget>>,
) -> Result<Table<'a>> {
    if missing_ranges
        .iter()
        .any(|r| r.start >= r.end || r.end > raw.len())
    {
        return invalid();
    }
    let source = Source::new(raw, missing_ranges, limits, budget)?;
    open_source(source, definition, limits, unavailable)
}
/// Own a previously authenticated immutable body without copying it. Its
/// allocation/verification lease remains alive until the last Table owner drops.
pub(crate) fn open_owned_budgeted<B>(
    raw: B,
    definition: &Definition,
    limits: TableLimits,
    unavailable: &BTreeSet<usize>,
    missing_ranges: &[Range<usize>],
    budget: Arc<Budget>,
) -> Result<Table<'static>>
where
    B: AsRef<[u8]> + Send + Sync + 'static,
{
    // Build the index once while bytes are an external borrow, then move all
    // owned metadata out before moving its body. No field borrows another field.
    let Table {
        source,
        layout,
        programs,
        locations,
        physical,
        ids,
        definition,
        column_mapping,
        unavailable,
        work,
        limits,
    } = open_inner(
        raw.as_ref(),
        definition,
        limits,
        unavailable,
        missing_ranges,
        Some(budget),
    )?;
    let Source {
        raw: _,
        missing,
        budget,
        metadata_bytes,
        metadata_limit,
    } = source;
    if let Some(budget) = &budget {
        // The original body is already accounted. Only the wrapper is extra
        // Rust safety memory; Vec and CDN allocation owners move unchanged.
        budget
            .safety_only()
            .charge(Cost::retained(std::mem::size_of::<B>()))?;
    }
    Ok(Table {
        source: Source {
            raw: TableBytes::Owned(Box::new(raw)),
            missing,
            budget,
            metadata_bytes,
            metadata_limit,
        },
        layout,
        programs,
        locations,
        physical,
        ids,
        definition,
        column_mapping,
        unavailable,
        work,
        limits,
    })
}
fn open_source<'a>(
    source: Source<'a>,
    definition: &Definition,
    limits: TableLimits,
    unavailable: &BTreeSet<usize>,
) -> Result<Table<'a>> {
    let layout = inspect_source(&source, limits)?;
    if layout.stride > 1 << 20 {
        return limit();
    }
    for index in unavailable {
        if *index >= layout.partitions.len() || layout.partitions[*index].key_id == 0 {
            return invalid();
        }
    }
    let copies = layout
        .partitions
        .iter()
        .map(|p| p.copies as u64)
        .sum::<u64>();
    if layout.rows as u64 + copies > limits.rows as u64 {
        return limit();
    }
    let (index_logical, index_safety) = index_allocation(&layout, copies);
    let estimated = index_safety
        + layout.fields.len() as u64 * 96
        + definition.fields.len() as u64 * 192
        + layout.metadata_end as u64;
    if estimated > limits.metadata_bytes as u64 {
        return limit();
    }
    let field_strings = definition.fields.iter().try_fold(0usize, |used, field| {
        used.checked_add(
            field.name.len()
                + field.kind.len()
                + field.foreign_table.len()
                + field.foreign_column.len(),
        )
        .ok_or_else(|| {
            fault(
                "table.limit_exceeded",
                3,
                "Definition string allocation overflow",
            )
        })
    })?;
    let metadata = layout.fields.len() as u64 * std::mem::size_of::<Program>() as u64
        + (layout.common_bytes as u64 / 8) * 64
        + definition.fields.len() as u64
            * (std::mem::size_of::<Field>() + std::mem::size_of::<Option<usize>>() + 64) as u64
        + field_strings as u64
        + definition.sha256.len() as u64
        + definition.r#match.len() as u64
        + definition.build.len() as u64
        + definition.layout.len() as u64
        + unavailable.len() as u64 * 64;
    source.metadata_split(
        usize::try_from(metadata + index_logical)
            .map_err(|_| fault("table.limit_exceeded", 3, "Metadata allocation overflow"))?,
        usize::try_from(metadata + index_safety)
            .map_err(|_| fault("table.limit_exceeded", 3, "Metadata allocation overflow"))?,
    )?;
    let mut programs = Vec::with_capacity(layout.fields.len());
    let (mut common_offset, mut palette_offset) = (layout.common_offset, layout.palette_offset);
    for storage in &layout.fields {
        let mut program = Program {
            storage: storage.clone(),
            palette: 0..0,
            common: HashMap::new(),
        };
        match storage.codec {
            2 => {
                let data = source.read(common_offset, storage.extra_bytes as usize)?;
                common_offset += data.len();
                for i in (0..data.len()).step_by(8) {
                    if program
                        .common
                        .insert(u32at(data, i)?, u32at(data, i + 4)?)
                        .is_some()
                    {
                        return invalid();
                    }
                }
            }
            3 | 4 => {
                program.palette = palette_offset..palette_offset + storage.extra_bytes as usize;
                source.read(program.palette.start, program.palette.len())?;
                palette_offset = program.palette.end;
            }
            _ => {
                if storage.extra_bytes != 0 {
                    return invalid();
                }
            }
        }
        programs.push(program);
    }
    let capacity = usize::try_from(layout.rows as u64 + copies).map_err(|_| {
        fault(
            "table.limit_exceeded",
            3,
            "Record index allocation overflow",
        )
    })?;
    let physical_capacity = usize::try_from(layout.rows).map_err(|_| {
        fault(
            "table.limit_exceeded",
            3,
            "Physical index allocation overflow",
        )
    })?;
    let mut table = Table {
        source,
        layout,
        programs,
        locations: HashMap::with_capacity(capacity),
        physical: Vec::with_capacity(physical_capacity),
        ids: vec![],
        definition: definition.clone(),
        column_mapping: vec![],
        unavailable: unavailable.clone(),
        work: AtomicU64::new(0),
        limits,
    };
    table.bind()?;
    table.index()?;
    Ok(table)
}
// Go 3.2.3 charges ID/copy maps, physical locations, sorted/partition IDs and
// chain scratch up front. Rust's independent safety estimate covers compact
// HashMap/HashSet capacity rounding and aggregate growth peaks, Vec storage,
// and fixed small-map control allocations. Sparse offset/extent storage has
// its own allowance. None of these conservative estimates measure RSS.
fn index_allocation(layout: &Layout, copies: u64) -> (u64, u64) {
    let rows = u64::from(layout.rows);
    let sparse = if layout.flags & 1 != 0 { rows * 80 } else { 0 };
    let logical =
        (rows + copies) * 32 + rows * 40 + copies * 72 + u64::from(layout.stride) + sparse;
    let safety = (rows + copies) * 32
        + rows * (std::mem::size_of::<Location>() + 2 * std::mem::size_of::<u32>()) as u64
        + copies * (32 + 32 + 2 * std::mem::size_of::<u32>()) as u64
        + u64::from(layout.stride)
        + sparse
        + 256; // Small-map minimum buckets/control bytes, independent of row count.
    (logical, safety)
}
// The supported Rust toolchain doubles Vec capacity. For these non-ZST
// elements, 3x the whole remaining domain covers old and new buffers at once.
// Insert-only hash maps use 7/8 occupancy and power-of-two buckets: old plus
// new bucket counts are below 4x the entry domain; each bucket has one control
// byte. The caller separately covers fixed minimum allocations/control groups.
// This is safety accounting only, never part of the fixed Go logical charge.
fn compact_growth_peak<T>(
    capacity: usize,
    length: usize,
    remaining: usize,
    multiplier: usize,
    control_bytes: usize,
) -> Option<usize> {
    let domain = capacity.max(length.checked_add(remaining)?);
    domain
        .checked_mul(std::mem::size_of::<T>().checked_add(control_bytes)?)?
        .checked_mul(multiplier)
}
impl<'a> Table<'a> {
    /// Conservative bytes actually admitted for this parser, not a default reservation or RSS.
    pub fn admitted_metadata_bytes(&self) -> u64 {
        self.source.metadata_bytes.load(Ordering::Relaxed)
    }
    pub fn layout(&self) -> &Layout {
        &self.layout
    }
    pub fn columns(&self) -> &[Field] {
        &self.definition.fields
    }
    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.ids.iter().copied()
    }
    pub(crate) fn id_at(&self, index: usize) -> Option<u32> {
        self.ids.get(index).copied()
    }
    pub fn logical_count(&self) -> usize {
        self.ids.len()
    }
    pub(crate) fn is_complete(&self) -> bool {
        self.unavailable.is_empty()
    }
    pub fn coverage(&self) -> serde_json::Value {
        serde_json::json!({"complete":self.unavailable.is_empty(),"availableRows":self.ids.len(),"physicalRows":self.layout.rows,"unavailablePartitions":self.unavailable,"decodeWork":self.work.load(Ordering::Relaxed),"materialization":"one borrowed record at a time"})
    }
    fn charge(&self, work: u64) -> Result<()> {
        if let Some(budget) = &self.source.budget {
            budget.charge(Cost {
                decode_work: work,
                ..Default::default()
            })?;
        }
        self.work
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current
                    .checked_add(work)
                    .filter(|next| *next <= self.limits.decode_work)
            })
            .map(|_| ())
            .map_err(|_| {
                fault(
                    "table.limit_exceeded",
                    3,
                    "WDC decode work budget exhausted",
                )
            })
    }
    fn location(&self, id: u32) -> Result<&Location> {
        let index = self.locations.get(&id).ok_or_else(|| {
            fault(
                "table.record_not_found",
                3,
                "Requested WDC record is absent",
            )
        })?;
        // Only successful physical insertions and copies of their indices enter
        // locations; every index remains valid for the table's lifetime.
        Ok(&self.physical[*index as usize])
    }
    pub fn raw_record(&self, id: u32) -> Result<&[u8]> {
        let location = self.location(id)?;
        self.source.read(location.offset, location.length as usize)
    }
    pub fn raw_values(&self, id: u32, column: usize, elements: u32) -> Result<Vec<u64>> {
        if self.layout.flags & 1 != 0 {
            return unsupported();
        }
        if elements == 0 || elements > 65536 {
            return limit();
        }
        self.source
            .temporary(elements as usize * std::mem::size_of::<u64>())?;
        let location = *self.location(id)?;
        if self.layout.partitions[location.partition].identity_bytes == 0
            && column == self.layout.identity_column as usize
        {
            if elements != 1 {
                return invalid();
            }
            return Ok(vec![id as u64]);
        }
        self.values(
            self.source
                .read(location.offset, location.length as usize)?,
            location.origin,
            column,
            elements,
            None,
        )
    }
    fn bind(&mut self) -> Result<()> {
        if self.definition.r#match == "layout"
            && self.definition.layout.to_ascii_uppercase()
                != format!("{:08X}", self.layout.layout_hash)
        {
            return Err(fault(
                "schema.definition_missing",
                3,
                "DBD layout does not match this WDC",
            ));
        }
        let (mut inline, mut identities, mut cells) = (0usize, 0usize, 0u64);
        let mut names = BTreeSet::new();
        for field in &self.definition.fields {
            if !names.insert(&field.name)
                || field.elements == 0
                || field.elements > 65536
                || field.bits > 64
            {
                return invalid();
            }
            cells += field.elements as u64;
            if cells > 65536 {
                return limit();
            }
            let column = if field.inline {
                let index = inline;
                inline += 1;
                if index >= self.programs.len() {
                    return invalid();
                }
                let storage = &self.programs[index].storage;
                if self.layout.flags & 1 == 0 && storage.codec == 0 {
                    let width = if ["string", "locstring"].contains(&field.kind.as_str()) {
                        if !storage.bit_offset.is_multiple_of(8) {
                            return invalid();
                        }
                        32
                    } else {
                        field.bits as u32
                    };
                    if width * field.elements != storage.bit_width as u32 {
                        return invalid();
                    }
                }
                if storage.codec == 4 && storage.parameters[2] != field.elements {
                    return invalid();
                }
                Some(index)
            } else {
                None
            };
            if field.identity {
                identities += 1;
                if field.array {
                    return invalid();
                }
                if self.layout.flags & 1 == 0 {
                    for p in &self.layout.partitions {
                        if p.rows > 0 && field.inline != (p.identity_bytes == 0) {
                            return invalid();
                        }
                    }
                    if field.inline && column != Some(self.layout.identity_column as usize) {
                        return invalid();
                    }
                }
            }
            self.column_mapping.push(column);
        }
        if identities != 1 || inline != self.programs.len() {
            return invalid();
        }
        Ok(())
    }
    fn values(
        &self,
        record: &[u8],
        id: u32,
        column: usize,
        elements: u32,
        bit_override: Option<u16>,
    ) -> Result<Vec<u64>> {
        let program = self
            .programs
            .get(column)
            .ok_or_else(|| fault("table.invalid_format", 4, "Column is missing"))?;
        let storage = &program.storage;
        let offset = bit_override.unwrap_or(storage.bit_offset) as u64;
        if storage.codec == 0 {
            if !(storage.bit_width as u32).is_multiple_of(elements) {
                return invalid();
            }
            let width = storage.bit_width as u32 / elements;
            if width == 0 || width > 64 {
                return invalid();
            }
            return (0..elements)
                .map(|i| extract_bits(record, offset + i as u64 * width as u64, width))
                .collect();
        }
        if storage.codec != 4 && elements != 1 {
            return invalid();
        }
        if storage.codec == 2 {
            return Ok(vec![
                *program.common.get(&id).unwrap_or(&storage.parameters[0]) as u64,
            ]);
        }
        let width = storage.parameters[1];
        let value = extract_bits(record, offset, width)?;
        match storage.codec {
            1 => Ok(vec![value]),
            5 => Ok(vec![if width > 0 && width < 64 {
                ((value << (64 - width)) as i64 >> (64 - width)) as u64
            } else {
                value
            }]),
            3 => {
                if value >= program.palette.len() as u64 / 4 {
                    return invalid();
                }
                Ok(vec![u32at(
                    self.source
                        .read(program.palette.start, program.palette.len())?,
                    value as usize * 4,
                )? as u64])
            }
            4 => {
                if elements != storage.parameters[2]
                    || value >= program.palette.len() as u64 / 4 / elements as u64
                {
                    return invalid();
                }
                let data = self
                    .source
                    .read(program.palette.start, program.palette.len())?;
                (0..elements)
                    .map(|i| {
                        Ok(u32at(data, (value * elements as u64 + i as u64) as usize * 4)? as u64)
                    })
                    .collect()
            }
            _ => unsupported(),
        }
    }
    fn scalar(&self, record: &[u8], id: u32, column: usize) -> Result<u64> {
        let program = &self.programs[column];
        let storage = &program.storage;
        if storage.codec == 0 {
            if storage.bit_width == 0 || storage.bit_width > 64 {
                return invalid();
            }
            return extract_bits(record, storage.bit_offset as u64, storage.bit_width as u32);
        }
        if storage.codec == 2 {
            return Ok(*program.common.get(&id).unwrap_or(&storage.parameters[0]) as u64);
        }
        let width = storage.parameters[1];
        let value = extract_bits(record, storage.bit_offset as u64, width)?;
        match storage.codec {
            1 => Ok(value),
            5 => Ok(if width > 0 && width < 64 {
                ((value << (64 - width)) as i64 >> (64 - width)) as u64
            } else {
                value
            }),
            3 => {
                if value >= program.palette.len() as u64 / 4 {
                    return invalid();
                }
                Ok(u32at(
                    self.source
                        .read(program.palette.start, program.palette.len())?,
                    value as usize * 4,
                )? as u64)
            }
            _ => invalid(),
        }
    }
    fn check_insert(&self, id: u32) -> Result<()> {
        if self.locations.contains_key(&id) {
            return invalid();
        }
        if self.locations.len() >= self.limits.rows as usize {
            return limit();
        }
        Ok(())
    }
    fn insert(&mut self, id: u32, index: u32) -> Result<()> {
        self.check_insert(id)?;
        self.locations.insert(id, index);
        Ok(())
    }
    fn insert_physical(&mut self, id: u32, location: Location) -> Result<u32> {
        let index = u32::try_from(self.physical.len())
            .map_err(|_| fault("table.limit_exceeded", 3, "Physical record index overflow"))?;
        self.insert(id, index)?;
        self.physical.push(location);
        Ok(index)
    }
    fn index(&mut self) -> Result<()> {
        let copies_count = self
            .layout
            .partitions
            .iter()
            .map(|p| u64::from(p.copies))
            .sum::<u64>();
        let copies_capacity = usize::try_from(copies_count)
            .map_err(|_| fault("table.limit_exceeded", 3, "Copy index allocation overflow"))?;
        let mut copies = HashMap::with_capacity(copies_capacity);
        let mut auxiliary = self.layout.metadata_end as u64;
        for pi in 0..self.layout.partitions.len() {
            if self.unavailable.contains(&pi) {
                continue;
            }
            let p = self.layout.partitions[pi].clone();
            let sparse = self.layout.flags & 1 != 0;
            if !sparse && p.sparse_count != 0 {
                return unsupported();
            }
            if !sparse && p.identity_bytes != 0 && p.identity_bytes as u64 != p.rows as u64 * 4 {
                return invalid();
            }
            let aux_bytes = p.identity_bytes as u64
                + p.copies as u64 * 8
                + p.relation_bytes as u64
                + if sparse {
                    if self.layout.version == 2 {
                        (self.layout.last_id as u64 - self.layout.first_id as u64 + 1) * 6
                    } else {
                        p.sparse_count as u64 * 10
                    }
                } else {
                    0
                };
            auxiliary += aux_bytes;
            if auxiliary > self.limits.metadata_bytes as u64 {
                return limit();
            }
            self.charge(p.rows as u64 + aux_bytes / 4)?;
            let position = if sparse {
                p.sparse_offset as usize
            } else {
                p.offset as usize
                    + p.rows as usize * self.layout.stride as usize
                    + p.string_bytes as usize
            };
            let aux = self.source.index_read(position, aux_bytes as usize)?;
            let (mut ids, copy_start, relation_start) = if sparse {
                self.index_sparse(pi, &p, aux)?
            } else {
                let mut ids = Vec::with_capacity(p.rows as usize);
                for ordinal in 0..p.rows {
                    let offset = p.offset as usize + ordinal as usize * self.layout.stride as usize;
                    let id = if p.identity_bytes != 0 {
                        u32at(aux, ordinal as usize * 4)?
                    } else {
                        let column = self.layout.identity_column as usize;
                        if self
                            .programs
                            .get(column)
                            .is_none_or(|p| p.storage.codec == 2)
                        {
                            return invalid();
                        }
                        let value = self.values(
                            self.source.read(offset, self.layout.stride as usize)?,
                            0,
                            column,
                            1,
                            None,
                        )?[0];
                        if value > u32::MAX as u64 {
                            return invalid();
                        }
                        value as u32
                    };
                    self.insert_physical(
                        id,
                        Location {
                            origin: id,
                            partition: pi,
                            offset,
                            length: self.layout.stride,
                            relation: None,
                        },
                    )?;
                    ids.push(id);
                }
                (
                    ids,
                    p.identity_bytes as usize,
                    p.identity_bytes as usize + p.copies as usize * 8,
                )
            };
            for i in 0..p.copies as usize {
                let at = copy_start + i * 8;
                let destination = u32at(aux, at)?;
                let origin = u32at(aux, at + 4)?;
                if copies.insert(destination, origin).is_some() {
                    return invalid();
                }
            }
            if p.relation_bytes != 0 {
                if p.relation_bytes < 12 {
                    return invalid();
                }
                let count = u32at(aux, relation_start)?;
                if count as u64 * 8 + 12 != p.relation_bytes as u64 {
                    return invalid();
                }
                for i in 0..count as usize {
                    let at = relation_start + 12 + i * 8;
                    let foreign = u32at(aux, at)?;
                    let ordinal = u32at(aux, at + 4)?;
                    let id = if self.layout.version >= 4 && self.layout.flags & 2 != 0 {
                        ordinal
                    } else {
                        *ids.get(ordinal as usize).ok_or_else(|| {
                            fault(
                                "table.invalid_format",
                                4,
                                "Relationship ordinal exceeds partition",
                            )
                        })?
                    };
                    let index = self.locations.get(&id).ok_or_else(|| {
                        fault("table.invalid_format", 4, "Relationship row is absent")
                    })?;
                    let location = &mut self.physical[*index as usize];
                    if location.partition != pi || location.relation.is_some() {
                        return invalid();
                    }
                    location.relation = Some(foreign);
                }
            }
            ids.clear();
        }
        if copies.keys().any(|id| self.locations.contains_key(id)) {
            return invalid();
        }
        if self.locations.len() + copies.len() > self.limits.rows as usize {
            return limit();
        }
        for destination in copies.keys().copied() {
            let mut path = vec![];
            let mut seen = HashSet::new();
            let mut id = destination;
            loop {
                if let Some(location) = self.locations.get(&id).copied() {
                    for alias in path {
                        self.locations.insert(alias, location);
                    }
                    break;
                }
                if !seen.insert(id) {
                    return invalid();
                }
                path.push(id);
                id = *copies
                    .get(&id)
                    .ok_or_else(|| fault("table.invalid_format", 4, "Copy origin is absent"))?;
            }
        }
        self.ids = self.locations.keys().copied().collect();
        self.ids.sort_unstable();
        Ok(())
    }
    fn index_sparse(
        &mut self,
        partition: usize,
        p: &Partition,
        aux: &[u8],
    ) -> Result<(Vec<u32>, usize, usize)> {
        if p.string_bytes != 0 {
            return invalid();
        }
        let count = if self.layout.version == 2 {
            self.layout.last_id as usize - self.layout.first_id as usize + 1
        } else {
            p.sparse_count as usize
        };
        if self.layout.version >= 3 && count != p.rows as usize || p.rows as usize > count {
            return invalid();
        }
        let (map_start, id_start, relation_start, copy_start) = if self.layout.version == 2 {
            let copy = count * 6 + p.identity_bytes as usize;
            (0, 0, copy + p.copies as usize * 8, copy)
        } else {
            let copy = p.identity_bytes as usize;
            let map = copy + p.copies as usize * 8;
            let relation = map + count * 6;
            let (id, relation) = if self.layout.version >= 4 && self.layout.flags & 2 != 0 {
                (relation, relation + count * 4)
            } else {
                (relation + p.relation_bytes as usize, relation)
            };
            (map, id, relation, copy)
        };
        if map_start + count * 6 > aux.len()
            || relation_start + p.relation_bytes as usize > aux.len()
        {
            return invalid();
        }
        let mut ids = Vec::with_capacity(p.rows as usize);
        let mut intervals = Vec::with_capacity(p.rows as usize);
        let mut by_offset = HashMap::<usize, u32>::with_capacity(p.rows as usize);
        let mut malformed_growth_admitted = false;
        for i in 0..count {
            let offset = u32at(aux, map_start + i * 6)? as usize;
            let length = u16at(aux, map_start + i * 6 + 4)? as usize;
            if self.layout.version == 2 && offset == 0 && length == 0 {
                continue;
            }
            if length == 0
                || offset < p.offset as usize
                || offset > p.sparse_offset as usize
                || length > p.sparse_offset as usize - offset
            {
                return invalid();
            }
            let id = if self.layout.version >= 3 {
                u32at(aux, id_start + i * 4)?
            } else {
                self.layout.first_id + i as u32
            };
            // Preserve Go's duplicate/row-limit errors before either alias
            // validation or its additional query-budget admission.
            self.check_insert(id)?;
            let alias = by_offset.get(&offset).copied();
            if let Some(previous) = alias {
                if self.physical[previous as usize].length as usize != length {
                    return invalid();
                }
                if self.layout.version == 2 {
                    // WDC2 Rows counts unique physical records, not implicit
                    // aliases. Admit each extra logical ID before insertion.
                    self.source.metadata(32)?;
                    self.insert(id, previous)?;
                    continue;
                }
            }
            if ids.len() >= p.rows as usize && !malformed_growth_admitted {
                // A malformed WDC2 map can underdeclare physical Rows. Keep
                // Go's late count-mismatch/preceding-error order, but admit the
                // entire remaining map domain before any unreserved growth.
                // Include this entry; later aliases also occupy logical IDs.
                let remaining = count.checked_sub(i).ok_or_else(|| {
                    fault("table.limit_exceeded", 3, "Sparse map domain overflow")
                })?;
                let allocation = (|| {
                    let mut bytes = 256usize;
                    for peak in [
                        compact_growth_peak::<Location>(
                            self.physical.capacity(),
                            self.physical.len(),
                            remaining,
                            3,
                            0,
                        ),
                        compact_growth_peak::<u32>(ids.capacity(), ids.len(), remaining, 3, 0),
                        compact_growth_peak::<Range<usize>>(
                            intervals.capacity(),
                            intervals.len(),
                            remaining,
                            3,
                            0,
                        ),
                        compact_growth_peak::<(u32, u32)>(
                            self.locations.capacity(),
                            self.locations.len(),
                            remaining,
                            4,
                            1,
                        ),
                        compact_growth_peak::<(usize, u32)>(
                            by_offset.capacity(),
                            by_offset.len(),
                            remaining,
                            4,
                            1,
                        ),
                    ] {
                        bytes = bytes.checked_add(peak?)?;
                    }
                    // Conservative full-length stable-sort scratch, even
                    // though the malformed count normally fails before sort.
                    bytes.checked_add(
                        intervals
                            .len()
                            .checked_add(remaining)?
                            .checked_mul(std::mem::size_of::<Range<usize>>())?,
                    )
                })()
                .ok_or_else(|| {
                    fault(
                        "table.limit_exceeded",
                        3,
                        "Sparse index allocation overflow",
                    )
                })?;
                // Cumulative admission is not refunded: it remains held for
                // this Budget's lifetime, beyond every covered allocation.
                self.source.metadata_split(0, allocation)?;
                malformed_growth_admitted = true;
            }
            let index = self.insert_physical(
                id,
                Location {
                    origin: id,
                    partition,
                    offset,
                    length: u32::try_from(length).map_err(|_| {
                        fault("table.invalid_format", 4, "Sparse record length overflow")
                    })?,
                    relation: None,
                },
            )?;
            if alias.is_none() {
                by_offset.insert(offset, index);
                intervals.push(offset..offset + length);
            }
            // WDC3+ same-offset IDs remain distinct physical rows: their
            // origins and relationship ordinals must not be merged.
            ids.push(id);
        }
        if ids.len() != p.rows as usize {
            return invalid();
        }
        intervals.sort_by_key(|r| r.start);
        if intervals.windows(2).any(|w| w[1].start < w[0].end) {
            return invalid();
        }
        if p.identity_bytes != 0 {
            if p.identity_bytes as usize != ids.len() * 4 {
                return invalid();
            }
            let start = if self.layout.version == 2 {
                count * 6
            } else {
                0
            };
            for (i, id) in ids.iter().enumerate() {
                if u32at(aux, start + i * 4)? != *id {
                    return invalid();
                }
            }
        }
        Ok((ids, copy_start, relation_start))
    }
    fn terminated(
        &self,
        start: usize,
        end: usize,
        budget: usize,
        allocation: &mut RowAllocation<'_>,
    ) -> Result<String> {
        if start >= end {
            return invalid();
        }
        let length = (end - start).min(budget.saturating_add(1));
        let data = self.source.read(start, length)?;
        let Some(zero) = data.iter().position(|b| *b == 0) else {
            return if length < end - start {
                limit()
            } else {
                invalid()
            };
        };
        if zero > budget {
            return limit();
        }
        allocation.retain(zero)?;
        String::from_utf8(data[..zero].to_vec())
            .map_err(|_| fault("table.invalid_format", 4, "WDC string is invalid UTF-8"))
    }
    fn strings(
        &self,
        location: Location,
        record: &[u8],
        storage: &Storage,
        elements: u32,
        budget: &mut usize,
        allocation: &mut RowAllocation<'_>,
    ) -> Result<Vec<String>> {
        if storage.codec != 0 {
            return unsupported();
        }
        let field = storage.bit_offset as usize / 8;
        if !storage.bit_offset.is_multiple_of(8) || field + elements as usize * 4 > record.len() {
            return invalid();
        }
        let prior_rows = self.layout.partitions[..location.partition]
            .iter()
            .map(|p| p.rows as i64)
            .sum::<i64>();
        let mut strings = Vec::with_capacity(elements as usize);
        for i in 0..elements as usize {
            let position = field + i * 4;
            let pointer = u32at(record, position)?;
            if pointer == 0 {
                strings.push(String::new());
                continue;
            }
            let target = if self.layout.version >= 3 {
                (prior_rows - self.layout.rows as i64) * self.layout.stride as i64
                    + location.offset as i64
                    - self.layout.partitions[location.partition].offset as i64
                    + position as i64
                    + pointer as i32 as i64
            } else {
                location.offset as i64 + position as i64 + pointer as i32 as i64
            };
            let mut base = 0i64;
            let mut found = None;
            for p in &self.layout.partitions {
                let start = p.offset as usize + p.rows as usize * self.layout.stride as usize;
                let local = if self.layout.version >= 3 {
                    target - base
                } else {
                    target - start as i64
                };
                base += p.string_bytes as i64;
                if local >= 0 && local < p.string_bytes as i64 {
                    let value = self.terminated(
                        start + local as usize,
                        start + p.string_bytes as usize,
                        *budget,
                        allocation,
                    )?;
                    *budget = budget.checked_sub(value.len()).ok_or_else(|| {
                        fault("table.limit_exceeded", 3, "String output budget exhausted")
                    })?;
                    found = Some(value);
                    break;
                }
            }
            strings.push(found.ok_or_else(|| {
                fault(
                    "table.invalid_format",
                    4,
                    "Relative string target is outside all string pools",
                )
            })?);
        }
        Ok(strings)
    }
    pub fn row(&self, id: u32, text_bytes: usize) -> Result<BTreeMap<String, Value>> {
        self.row_projected(id, text_bytes, None)
    }
    pub fn row_projected(
        &self,
        id: u32,
        text_bytes: usize,
        selected: Option<&BTreeSet<String>>,
    ) -> Result<BTreeMap<String, Value>> {
        let mut allocation = RowAllocation::new(self.source.budget.as_deref(), false);
        self.row_projected_inner(id, text_bytes, selected, &mut allocation)
    }
    /// Decode once and keep the temporary row lease live until the cursor has
    /// finished its consumer. Retained copies need their own admission.
    pub(crate) fn leased_row_projected(
        &self,
        id: u32,
        text_bytes: usize,
        selected: Option<&BTreeSet<String>>,
    ) -> Result<LeasedRow<'_, BTreeMap<String, Value>>> {
        let mut allocation = RowAllocation::new(self.source.budget.as_deref(), true);
        let value = self.row_projected_inner(id, text_bytes, selected, &mut allocation)?;
        Ok(LeasedRow { value, allocation })
    }
    fn row_projected_inner(
        &self,
        id: u32,
        text_bytes: usize,
        selected: Option<&BTreeSet<String>>,
        allocation: &mut RowAllocation<'_>,
    ) -> Result<BTreeMap<String, Value>> {
        if text_bytes == 0 || text_bytes > 1 << 20 {
            return limit();
        }
        if selected.is_some_and(|names| {
            names
                .iter()
                .any(|name| !self.definition.fields.iter().any(|f| &f.name == name))
        }) {
            return invalid();
        }
        self.charge(
            1 + self
                .definition
                .fields
                .iter()
                .map(|f| f.elements as u64)
                .sum::<u64>(),
        )?;
        let location = *self.location(id)?;
        let record = self
            .source
            .read(location.offset, location.length as usize)?;
        let sparse = self.layout.flags & 1 != 0;
        let mut cursor = 0u64;
        let mut remaining = text_bytes;
        let mut result = BTreeMap::new();
        let temporary = self
            .definition
            .fields
            .iter()
            .try_fold(0usize, |used, field| {
                used.checked_add(field.name.len() + std::mem::size_of::<Value>() + 96)
                    .and_then(|size| {
                        size.checked_add(if field.array {
                            field.elements as usize
                                * (std::mem::size_of::<Value>() + std::mem::size_of::<String>() + 8)
                        } else {
                            0
                        })
                    })
                    .ok_or_else(|| fault("table.limit_exceeded", 3, "Row allocation overflow"))
            })?;
        allocation.retain(temporary)?;
        for (index, field) in self.definition.fields.iter().enumerate() {
            let wanted = selected.is_none_or(|s| s.contains(&field.name));
            let Some(column) = self.column_mapping[index] else {
                if wanted {
                    result.insert(
                        field.name.clone(),
                        if field.identity {
                            Value::UInt(id as u64)
                        } else {
                            location
                                .relation
                                .map(|v| Value::UInt(v as u64))
                                .unwrap_or(Value::Null)
                        },
                    );
                }
                continue;
            };
            let storage = &self.programs[column].storage;
            if !sparse && !field.array && !["string", "locstring"].contains(&field.kind.as_str()) {
                let value = self.scalar(record, location.origin, column)?;
                if field.identity {
                    if value != location.origin as u64 {
                        return invalid();
                    }
                    if wanted {
                        result.insert(field.name.clone(), Value::UInt(id as u64));
                    }
                } else {
                    let value = typed(value, field)?;
                    if wanted {
                        result.insert(field.name.clone(), value);
                    }
                }
                continue;
            }
            if ["string", "locstring"].contains(&field.kind.as_str()) {
                let strings = if sparse {
                    if !cursor.is_multiple_of(8) {
                        return invalid();
                    }
                    let mut values = vec![];
                    for _ in 0..field.elements {
                        let start = (cursor / 8) as usize;
                        let length = record.len().checked_sub(start).ok_or_else(|| {
                            fault("table.invalid_format", 4, "Sparse string cursor overflow")
                        })?;
                        let data = &record[start..];
                        let zero = data[..length.min(remaining.saturating_add(1))]
                            .iter()
                            .position(|b| *b == 0)
                            .ok_or_else(|| {
                                fault(
                                    if length > remaining {
                                        "table.limit_exceeded"
                                    } else {
                                        "table.invalid_format"
                                    },
                                    if length > remaining { 3 } else { 4 },
                                    "Sparse string is unterminated",
                                )
                            })?;
                        allocation.retain(zero)?;
                        let value = std::str::from_utf8(&data[..zero])
                            .map_err(|_| {
                                fault("table.invalid_format", 4, "Sparse string is invalid UTF-8")
                            })?
                            .to_owned();
                        remaining -= zero;
                        cursor += (zero as u64 + 1) * 8;
                        values.push(value);
                    }
                    values
                } else {
                    self.strings(
                        location,
                        record,
                        storage,
                        field.elements,
                        &mut remaining,
                        allocation,
                    )?
                };
                if wanted {
                    result.insert(
                        field.name.clone(),
                        if field.array {
                            Value::Array(strings.into_iter().map(Value::String).collect())
                        } else {
                            Value::String(strings.into_iter().next().ok_or_else(|| {
                                fault("table.invalid_format", 4, "Empty scalar string")
                            })?)
                        },
                    );
                }
                continue;
            }
            let values = if sparse && storage.codec == 0 {
                let mut values = Vec::with_capacity(field.elements as usize);
                for _ in 0..field.elements {
                    values.push(extract_bits(record, cursor, field.bits as u32)?);
                    cursor += field.bits as u64;
                }
                values
            } else if sparse {
                if cursor / 8 > record.len() as u64 {
                    return invalid();
                }
                let values = self.values(
                    &record[cursor as usize / 8..],
                    location.origin,
                    column,
                    field.elements,
                    Some((cursor % 8) as u16),
                )?;
                if storage.codec != 2 {
                    cursor += storage.parameters[1] as u64;
                }
                values
            } else {
                self.values(record, location.origin, column, field.elements, None)?
            };
            if field.identity {
                if values.len() != 1 || values[0] != location.origin as u64 {
                    return invalid();
                }
                if wanted {
                    result.insert(field.name.clone(), Value::UInt(id as u64));
                }
                continue;
            }
            let mut converted = Vec::with_capacity(if wanted { values.len() } else { 0 });
            for value in values {
                let value = typed(value, field)?;
                if wanted {
                    converted.push(value);
                }
            }
            if wanted {
                result.insert(
                    field.name.clone(),
                    if field.array {
                        Value::Array(converted)
                    } else {
                        converted
                            .into_iter()
                            .next()
                            .ok_or_else(|| fault("table.invalid_format", 4, "Empty scalar value"))?
                    },
                );
            }
        }
        if sparse {
            let total = record.len() as u64 * 8;
            let padding_limit = if total.is_multiple_of(32) { 31 } else { 7 };
            if cursor > total
                || total - cursor > padding_limit
                || extract_bits(record, cursor, (total - cursor) as u32)? != 0
            {
                return invalid();
            }
        }
        Ok(result)
    }
    pub fn page(
        &self,
        after_id: Option<u32>,
        limit_rows: usize,
        text_bytes: usize,
    ) -> Result<Page> {
        if limit_rows == 0 || limit_rows > 5000 {
            return limit();
        }
        let start = after_id
            .map(|id| self.ids.partition_point(|v| *v <= id))
            .unwrap_or(0);
        let end = (start + limit_rows).min(self.ids.len());
        self.source
            .temporary((end - start) * std::mem::size_of::<BTreeMap<String, Value>>())?;
        let mut rows = Vec::with_capacity(end - start);
        for id in &self.ids[start..end] {
            rows.push(self.row(*id, text_bytes)?);
        }
        let truncated = end < self.ids.len();
        Ok(Page {
            rows,
            after_id: if truncated {
                self.ids.get(end - 1).copied()
            } else {
                None
            },
            truncated,
            complete: self.unavailable.is_empty() && !truncated,
            row_count: self.ids.len(),
        })
    }
}
fn typed(value: u64, field: &Field) -> Result<Value> {
    if field.kind == "float" {
        let number = f32::from_bits(value as u32);
        if !number.is_finite() {
            return invalid();
        }
        return Ok(Value::Float32(number));
    }
    if field.kind != "int" || field.bits == 0 || field.bits > 64 {
        return invalid();
    }
    if field.signed {
        let shift = 64 - field.bits;
        Ok(Value::Int((value << shift) as i64 >> shift))
    } else {
        Ok(Value::UInt(if field.bits < 64 {
            value & ((1u64 << field.bits) - 1)
        } else {
            value
        }))
    }
}

#[cfg(test)]
mod compact_tests;
