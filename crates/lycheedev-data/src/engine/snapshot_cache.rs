//! Immutable, session-admitted Arrow shards. No decoder or execution control is
//! retained here. Every read is governed by the calling request's fresh control.
//! Integration contract: publish only after authenticated source close succeeds;
//! source_sha256 is the verified engine-adapter static-identity digest, not a
//! table-name hash. Keep authenticated provenance separately under its owner.
use super::{api::TableRequest, lifetime::RequestLifetime};
use crate::{fault, resource::{Budget, Cost, OwnedRetainedReservation}};
use arrow_array::{Array, RecordBatch, UInt32Array};
use arrow_buffer::{MemoryPool, MemoryReservation};
use arrow_schema::{DataType, SchemaRef};
use lycheedev_core::{Result, control::OperationControl, selection::PinnedSet};
use std::{collections::{BTreeMap, BTreeSet}, fmt, sync::Arc};

/// Borrowed binding avoids allocating a second identity on every lookup.
#[derive(Clone, Copy)]
pub(super) struct SnapshotBinding<'a> {
    pub table: &'a TableRequest,
    pub pin: &'a PinnedSet,
    pub source_sha256: &'a str,
}

/// One compact SOURCE batch, never a projection/filter/join query result.
/// Private physical IDs must be decoder-issued UInt32 IDs, not a public ID cast.
/// The original guard covers exactly these buffers and their schema; it must
/// not be a combined per-query output guard. Move it, do not fabricate a second
/// admission for the same body. Buffers MUST be freshly compacted and unclaimed:
/// Arrow claim REPLACES existing hooks. Never pass upstream arrays or slices.
/// The transferred reservation, not a preexisting buffer hook, owns these bodies.
pub(super) struct CacheBatch {
    records: RecordBatch,
    ids: UInt32Array,
    _owner: Arc<BufferOwner>,
}
impl CacheBatch {
    pub(super) fn new(
        records: RecordBatch,
        ids: UInt32Array,
        source_allocation: OwnedRetainedReservation,
        lifetime: Arc<RequestLifetime>,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        lifetime.budget.check()?;
        if records.num_rows() != ids.len() || ids.null_count() != 0 {
            return Err(shape());
        }
        let mut hooks = 1usize;
        for field in records.schema().fields() {
            hooks = add(hooks, buffer_hooks(field.data_type(), 0)?)?;
        }
        let allocation = lifetime.budget.reserve_retained(add(1024, mul(hooks, 256)?)?)?
            .into_owned(&lifetime.budget)?;
        let owner = Arc::new(BufferOwner {
            allocation: Some(source_allocation), hooks: Some(allocation), lifetime,
        });
        let pool = RetainBuffers(owner.clone());
        records.claim(&pool);
        ids.claim(&pool);
        control.check()?;
        Ok(Self { records, ids, _owner: owner })
    }
    pub(super) fn records(&self) -> &RecordBatch { &self.records }
    pub(super) fn physical_ids(&self) -> &UInt32Array { &self.ids }
}

/// The caller may assert absence only for IDs it actually resolved. `all_ids`
/// means the entire physical-ID domain was resolved, not merely that the source
/// bytes were complete. A stopped prefix MUST use false; unvisited rows remain
/// NotCovered. Ranges must be fully resolved, sorted and nonoverlapping.
#[derive(Default)]
pub(super) struct SnapshotCoverage<'a> {
    pub all_ids: bool,
    pub resolved_ranges: &'a [(u32, u32)],
    pub confirmed_absent: &'a [u32],
}

pub(super) enum CachedLookup<'a> {
    Present { batch: &'a RecordBatch, row: usize },
    KnownAbsent,
    NotCovered,
}

/// One immutable shard. Session registry bounds/replacement are the caller's
/// responsibility; all retained metadata and index entries use the SAME request
/// admission as the source buffers, with no cache-only allowance.
pub(super) struct SnapshotCache {
    table: Option<TableRequest>,
    pin: Option<PinnedSet>,
    source_sha256: String,
    schema: Option<SchemaRef>,
    batches: Vec<CacheBatch>,
    index: BTreeMap<u32, (usize, usize)>,
    absent: BTreeSet<u32>,
    ranges: Vec<(u32, u32)>,
    all_ids: bool,
    metadata: Option<OwnedRetainedReservation>,
    lifetime: Arc<RequestLifetime>,
}
impl SnapshotCache {
    /// Inputs are moved compact source bodies. Their native arrays are never
    /// converted to JSON or reparsed. Metadata/index admission happens before
    /// clones, BTree nodes or coverage vectors are allocated. Failed publication
    /// drops every moved body and its original reservation.
    pub(super) fn new(
        binding: SnapshotBinding<'_>,
        schema: SchemaRef,
        batches: Vec<CacheBatch>,
        coverage: SnapshotCoverage<'_>,
        lifetime: Arc<RequestLifetime>,
        control: &OperationControl,
    ) -> Result<Self> {
        control.check()?;
        let budget = &lifetime.budget;
        budget.check()?;
        if !lycheedev_core::selection::valid_digest(binding.source_sha256, 64)
            || binding.pin.data.is_none() {
            return Err(identity());
        }
        // Even an empty source needs one zero-row batch to retain its original
        // schema/source admission; an empty vector would lose that ownership.
        if batches.is_empty() { return Err(shape()); }
        let mut rows = 0;
        for batch in &batches {
            control.check()?;
            if batch.records.schema().as_ref() != schema.as_ref() {
                return Err(shape());
            }
            rows = add(rows, batch.records.num_rows())?;
        }
        let mut previous_end = None;
        for &(first, last) in coverage.resolved_ranges {
            control.check()?;
            budget.charge(Cost::work(1))?;
            if first > last || previous_end.is_some_and(|end| end >= first) {
                return Err(shape());
            }
            previous_end = Some(last);
        }
        let mut previous_id = None;
        for &id in coverage.confirmed_absent {
            control.check()?;
            budget.charge(Cost::work(1))?;
            if previous_id.is_some_and(|previous| previous >= id) { return Err(shape()); }
            previous_id = Some(id);
        }
        // Keep the actual admitted batch schema, not an unrelated equal schema
        // supplied by a caller merely to validate the shape.
        let schema = batches[0].records.schema();
        let pin_bytes = crate::capture::json_size(binding.pin, budget)?;
        let table_bytes = crate::capture::json_size(binding.table, budget)?;
        // 256 per tree entry conservatively includes node slots, pointers and
        // allocator overhead, including small underfilled tree nodes.
        let mut bytes = add(4096, mul(add(pin_bytes, table_bytes)?, 8)?)?;
        bytes = add(bytes, mul(add(rows, coverage.confirmed_absent.len())?, 256)?)?;
        bytes = add(bytes, mul(batches.capacity(), std::mem::size_of::<CacheBatch>())?)?;
        bytes = add(bytes, mul(coverage.resolved_ranges.len(), 16)?)?;
        let metadata = budget.reserve_retained(bytes)?.into_owned(budget)?;
        let mut index = BTreeMap::new();
        for (batch_index, batch) in batches.iter().enumerate() {
            for row in 0..batch.ids.len() {
                control.check()?;
                budget.charge(Cost::work(64))?;
                if index.insert(batch.ids.value(row), (batch_index, row)).is_some() {
                    return Err(shape());
                }
            }
        }
        let mut absent = BTreeSet::new();
        for id in coverage.confirmed_absent {
            control.check()?;
            budget.charge(Cost::work(64))?;
            if index.contains_key(id) { return Err(shape()); }
            absent.insert(*id);
        }
        control.check()?;
        Ok(Self {
            table: Some(binding.table.clone()), pin: Some(binding.pin.clone()),
            source_sha256: binding.source_sha256.to_owned(), schema: Some(schema),
            batches, index, absent, ranges: coverage.resolved_ranges.to_vec(),
            all_ids: coverage.all_ids, metadata: Some(metadata), lifetime,
        })
    }
    pub(super) fn matches(&self, binding: SnapshotBinding<'_>) -> bool {
        let Some(pin) = &self.pin else { return false; };
        self.table.as_ref() == Some(binding.table)
            && self.source_sha256 == binding.source_sha256
            && pin.schema == binding.pin.schema && pin.id == binding.pin.id
            && pin.parent == binding.pin.parent && pin.source == binding.pin.source
            && pin.data == binding.pin.data && pin.changes == binding.pin.changes
    }
    /// Borrowing native bodies is for providers that apply fresh controls and
    /// fresh work accounting while iterating. Callers must validate binding.
    pub(super) fn schema(&self) -> &SchemaRef {
        self.schema.as_ref().expect("live snapshot has schema")
    }
    pub(super) fn batches(&self) -> &[CacheBatch] { &self.batches }
    pub(super) fn covers_all_ids(&self) -> bool { self.all_ids }
    pub(super) fn row_count(&self) -> usize { self.index.len() }
    pub(super) fn lookup(
        &self, binding: SnapshotBinding<'_>, id: u32,
        budget: &Budget, control: &OperationControl,
    ) -> Result<CachedLookup<'_>> {
        control.check()?;
        budget.charge(Cost::work(128))?;
        if !self.matches(binding) { return Err(identity()); }
        // Real physical lookup: no Arrow batch scan or public-ID conversion.
        let result = if let Some(&(batch, row)) = self.index.get(&id) {
            CachedLookup::Present { batch: &self.batches[batch].records, row }
        } else if self.all_ids || self.absent.contains(&id) || self.in_resolved_range(id) {
            CachedLookup::KnownAbsent
        } else {
            CachedLookup::NotCovered
        };
        control.check()?;
        Ok(result)
    }
    fn in_resolved_range(&self, id: u32) -> bool {
        let end = self.ranges.partition_point(|(first, _)| *first <= id);
        end.checked_sub(1).and_then(|i| self.ranges.get(i))
            .is_some_and(|(_, last)| id <= *last)
    }
}
impl Drop for SnapshotCache {
    fn drop(&mut self) {
        // Free bodies/metadata BEFORE reconciling. settle only observes the
        // ownership ledger and never executes the old request's control.
        self.batches.clear();
        self.index.clear();
        self.absent.clear();
        drop(std::mem::take(&mut self.ranges));
        self.table.take(); self.pin.take(); self.schema.take();
        drop(std::mem::take(&mut self.source_sha256));
        drop(std::mem::take(&mut self.batches));
        self.metadata.take();
        let _ = self.lifetime.settle();
    }
}

// No back-edge to Arrow arrays: clones/slices retain only the allocation
// provenance, hook reservation and admission lifetime, not whole result output.
struct BufferOwner {
    allocation: Option<OwnedRetainedReservation>,
    hooks: Option<OwnedRetainedReservation>,
    lifetime: Arc<RequestLifetime>,
}
impl Drop for BufferOwner {
    fn drop(&mut self) {
        self.allocation.take(); self.hooks.take();
        let _ = self.lifetime.settle();
    }
}
impl fmt::Debug for BufferOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("ImmutableSourceAdmission") }
}
#[derive(Debug)]
struct RetainBuffers(Arc<BufferOwner>);
#[derive(Debug)]
struct BufferLease { bytes: usize, _owner: Arc<BufferOwner> }
impl MemoryPool for RetainBuffers {
    fn reserve(&self, bytes: usize) -> Box<dyn MemoryReservation> {
        Box::new(BufferLease { bytes, _owner: self.0.clone() })
    }
    fn available(&self) -> isize { 0 }
    fn used(&self) -> usize { 0 }
    fn capacity(&self) -> usize { 0 }
}
impl MemoryReservation for BufferLease {
    fn size(&self) -> usize { self.bytes }
    fn resize(&mut self, bytes: usize) { self.bytes = bytes; }
}
fn buffer_hooks(data_type: &DataType, depth: usize) -> Result<usize> {
    if depth > 64 { return Err(shape()); }
    match data_type {
        DataType::Null | DataType::Boolean | DataType::Int8 | DataType::Int16
        | DataType::Int32 | DataType::Int64 | DataType::UInt8 | DataType::UInt16
        | DataType::UInt32 | DataType::UInt64 | DataType::Float32 | DataType::Float64
        | DataType::Decimal128(_, _) | DataType::Utf8 | DataType::LargeUtf8 => Ok(4),
        DataType::FixedSizeList(field, _) | DataType::List(field) | DataType::LargeList(field) =>
            add(4, buffer_hooks(field.data_type(), depth + 1)?),
        _ => Err(shape()),
    }
}
fn add(a: usize, b: usize) -> Result<usize> { a.checked_add(b).ok_or_else(bound) }
fn mul(a: usize, b: usize) -> Result<usize> { a.checked_mul(b).ok_or_else(bound) }
fn bound() -> lycheedev_core::ToolkitError {
    fault("records.query_resource_budget", 3, "Immutable snapshot admission exceeded")
}
fn shape() -> lycheedev_core::ToolkitError {
    fault("records.source_identity", 3, "Immutable source batch or physical coverage is inconsistent")
}
fn identity() -> lycheedev_core::ToolkitError {
    fault("records.source_identity", 3, "Immutable snapshot binding differs")
}

impl CacheBatch {
    /// Transfer the ONE cache-specific compaction admission into an array-free
    /// owner shared by all batches. The final column must be the authenticated
    /// private physical UInt32 locator. It is removed from the visible schema.
    /// Buffers MUST be freshly compacted/unclaimed, never source slices or
    /// guarded upstream arrays: Arrow claim replaces an existing reservation.
    /// This factory never accepts an entire query ResultOwner/output admission.
    pub(super) fn from_compact_batches(
        records: Vec<RecordBatch>,
        source_allocation: OwnedRetainedReservation,
        lifetime: Arc<RequestLifetime>,
        control: &OperationControl,
    ) -> Result<Vec<Self>> {
        control.check()?;
        lifetime.budget.check()?;
        if records.is_empty() { return Err(shape()); }
        let mut bytes = 4096usize;
        for batch in &records {
            control.check()?;
            let last = batch.num_columns().checked_sub(1).ok_or_else(shape)?;
            let ids = batch.column(last).as_any().downcast_ref::<UInt32Array>().ok_or_else(shape)?;
            if ids.null_count() != 0 { return Err(shape()); }
            let mut hooks = 0;
            for field in batch.schema().fields() {
                hooks = add(hooks, buffer_hooks(field.data_type(), 0)?)?;
            }
            bytes = add(bytes, add(1024, mul(hooks, 256)?)?)?;
            // Projection's field/column vectors and its temporary indices.
            bytes = add(bytes, mul(batch.num_columns(), 128)?)?;
            // Schema::project clones schema-level metadata, unlike Arc fields.
            for (key, value) in batch.schema().metadata() {
                bytes = add(bytes, add(128, add(key.len(), value.len())?)?)?;
            }
        }
        bytes = add(bytes, mul(records.len(), std::mem::size_of::<Self>())?)?;
        let allocation = lifetime.budget.reserve_retained(bytes)?.into_owned(&lifetime.budget)?;
        let owner = Arc::new(BufferOwner {
            allocation: Some(source_allocation), hooks: Some(allocation), lifetime,
        });
        let pool = RetainBuffers(owner.clone());
        let mut batches = Vec::new();
        batches.try_reserve_exact(records.len()).map_err(|_| bound())?;
        for batch in records {
            control.check()?;
            let last = batch.num_columns().checked_sub(1).ok_or_else(shape)?;
            // Fresh unclaimed bodies only. Claim before cloning so every
            // escaped Arrow body keeps this exact transferred admission.
            batch.claim(&pool);
            let ids = batch.column(last).as_any().downcast_ref::<UInt32Array>()
                .ok_or_else(shape)?.clone();
            let mut projection = Vec::new();
            projection.try_reserve_exact(last).map_err(|_| bound())?;
            projection.extend(0..last);
            let visible = batch.project(&projection).map_err(|_| shape())?;
            batches.push(Self { records: visible, ids, _owner: owner.clone() });
        }
        control.check()?;
        Ok(batches)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{api::SourceMode, lifetime::SessionMemory};
    use arrow_array::{ArrayRef, UInt64Array};
    use arrow_schema::{Field, Schema};
    use lycheedev_core::selection::{DataPin, SelectionSpec};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    fn table() -> TableRequest {
        TableRequest { name: "CacheFixture".into(), mode: SourceMode::Static, hotfix_captures: vec![] }
    }
    fn pin() -> PinnedSet {
        PinnedSet { schema: "fixture".into(), id: "pin".into(), spec: SelectionSpec {
            data: Some(DataPin::default()), ..Default::default()
        } }
    }
    fn native(ids: Vec<u32>, values: Vec<u64>) -> RecordBatch {
        let schema = Arc::new(Schema::new(vec![
            Field::new("value", DataType::UInt64, false),
            Field::new("private_locator", DataType::UInt32, false),
        ]));
        RecordBatch::try_new(schema, vec![
            Arc::new(UInt64Array::from(values)) as ArrayRef,
            Arc::new(UInt32Array::from(ids)) as ArrayRef,
        ]).unwrap()
    }
    fn compact(records: Vec<RecordBatch>, lifetime: &Arc<RequestLifetime>, control: &OperationControl) -> Vec<CacheBatch> {
        let allocation = lifetime.budget.reserve_retained(4096).unwrap()
            .into_owned(&lifetime.budget).unwrap();
        CacheBatch::from_compact_batches(records, allocation, lifetime.clone(), control).unwrap()
    }
    fn build(records: Vec<RecordBatch>, coverage: SnapshotCoverage<'_>, lifetime: &Arc<RequestLifetime>, control: &OperationControl) -> Result<SnapshotCache> {
        let table = table(); let pin = pin(); let digest = "a".repeat(64);
        let batches = compact(records, lifetime, control);
        let schema = batches[0].records().schema();
        SnapshotCache::new(SnapshotBinding { table: &table, pin: &pin, source_sha256: &digest },
            schema, batches, coverage, lifetime.clone(), control)
    }
    #[test]
    fn indexed_native_lookup_preserves_u64_and_partial_absence() {
        let session = SessionMemory::new(1 << 20).unwrap();
        let control = OperationControl::default();
        let lifetime = session.begin(10000, control.clone()).unwrap();
        let snapshot = build(vec![native(vec![8, 2], vec![u64::MAX, 9007199254740993])],
            SnapshotCoverage { all_ids: false, resolved_ranges: &[(20, 30)], confirmed_absent: &[3] },
            &lifetime, &control).unwrap();
        let table = table(); let pin = pin(); let digest = "a".repeat(64);
        let binding = SnapshotBinding { table: &table, pin: &pin, source_sha256: &digest };
        match snapshot.lookup(binding, 2, &lifetime.budget, &control).unwrap() {
            CachedLookup::Present { batch, row } => {
                assert_eq!(batch.num_columns(), 1);
                assert_eq!(batch.column(0).as_any().downcast_ref::<UInt64Array>().unwrap().value(row), 9007199254740993);
            }
            _ => panic!("indexed row missing"),
        }
        assert!(matches!(snapshot.lookup(binding, 3, &lifetime.budget, &control).unwrap(), CachedLookup::KnownAbsent));
        assert!(matches!(snapshot.lookup(binding, 25, &lifetime.budget, &control).unwrap(), CachedLookup::KnownAbsent));
        assert!(matches!(snapshot.lookup(binding, 4, &lifetime.budget, &control).unwrap(), CachedLookup::NotCovered));
        assert!(!snapshot.covers_all_ids());
        assert_eq!(snapshot.row_count(), 2);
    }
    #[test]
    fn cancelled_original_request_does_not_control_reuse() {
        let session = SessionMemory::new(1 << 20).unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        let old = OperationControl::new(Instant::now() + Duration::from_secs(60), cancelled.clone());
        let lifetime = session.begin(10000, old.clone()).unwrap();
        let snapshot = build(vec![native(vec![7], vec![u64::MAX])], SnapshotCoverage::default(), &lifetime, &old).unwrap();
        lifetime.settle().unwrap();
        cancelled.store(true, Ordering::Release);
        assert!(lifetime.budget.check().is_err());
        let fresh = OperationControl::default();
        let current = session.begin(10000, fresh.clone()).unwrap();
        let table = table(); let pin = pin(); let digest = "a".repeat(64);
        let binding = SnapshotBinding { table: &table, pin: &pin, source_sha256: &digest };
        assert!(matches!(snapshot.lookup(binding, 7, &current.budget, &fresh).unwrap(), CachedLookup::Present { .. }));
        assert!(snapshot.lookup(binding, 7, &current.budget, &old).is_err());
    }
    #[test]
    fn duplicate_physical_ids_and_absent_present_conflicts_are_rejected() {
        for (ids, absent) in [(vec![1, 1], vec![]), (vec![1, 2], vec![1])] {
            let session = SessionMemory::new(1 << 20).unwrap();
            let control = OperationControl::default();
            let lifetime = session.begin(10000, control.clone()).unwrap();
            assert!(build(vec![native(ids, vec![10, 20])], SnapshotCoverage {
                all_ids: false, resolved_ranges: &[], confirmed_absent: &absent,
            }, &lifetime, &control).is_err());
        }
    }
    #[test]
    fn foreign_source_pin_and_table_are_not_cache_hits() {
        let session = SessionMemory::new(1 << 20).unwrap();
        let control = OperationControl::default();
        let lifetime = session.begin(10000, control.clone()).unwrap();
        let snapshot = build(vec![native(vec![1], vec![2])], SnapshotCoverage::default(), &lifetime, &control).unwrap();
        let mut table = table(); let mut pin = pin(); let digest = "a".repeat(64);
        table.hotfix_captures.push("capture".into());
        assert!(!snapshot.matches(SnapshotBinding { table: &table, pin: &pin, source_sha256: &digest }));
        table.hotfix_captures.clear(); pin.spec.data.as_mut().unwrap().language = "different".into();
        assert!(!snapshot.matches(SnapshotBinding { table: &table, pin: &pin, source_sha256: &digest }));
        pin.spec.data.as_mut().unwrap().language.clear();
        assert!(!snapshot.matches(SnapshotBinding { table: &table, pin: &pin, source_sha256: &"b".repeat(64) }));
    }
    #[test]
    fn escaped_native_clone_retains_only_compact_source_admission() {
        let session = SessionMemory::new(1 << 20).unwrap();
        let control = OperationControl::default();
        let lifetime = session.begin(10000, control.clone()).unwrap();
        let snapshot = build(vec![native(vec![1], vec![2]), native(vec![3], vec![4])],
            SnapshotCoverage { all_ids: true, ..Default::default() }, &lifetime, &control).unwrap();
        let escaped = snapshot.batches()[0].records().clone();
        let unrelated_output = lifetime.budget.reserve_retained(8000).unwrap().into_owned(&lifetime.budget).unwrap();
        lifetime.settle().unwrap();
        let before = session.budget.snapshot().unwrap().retained_bytes;
        drop(unrelated_output); lifetime.settle().unwrap();
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, before - 8000);
        drop(snapshot); drop(lifetime);
        assert!(session.budget.snapshot().unwrap().retained_bytes > 0);
        drop(escaped);
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 0);
    }
    #[test]
    fn empty_complete_source_keeps_schema_and_confirms_absence() {
        let session = SessionMemory::new(1 << 20).unwrap();
        let control = OperationControl::default();
        let lifetime = session.begin(10000, control.clone()).unwrap();
        let snapshot = build(vec![native(vec![], vec![])],
            SnapshotCoverage { all_ids: true, ..Default::default() }, &lifetime, &control).unwrap();
        let table = table(); let pin = pin(); let digest = "a".repeat(64);
        assert_eq!(snapshot.schema().fields().len(), 1);
        assert!(matches!(snapshot.lookup(SnapshotBinding { table: &table, pin: &pin, source_sha256: &digest },
            u32::MAX, &lifetime.budget, &control).unwrap(), CachedLookup::KnownAbsent));
        lifetime.settle().unwrap(); drop(snapshot); drop(lifetime);
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 0);
    }
    #[test]
    fn index_metadata_cannot_allocate_outside_session_admission() {
        let session = SessionMemory::new(1 << 20).unwrap();
        let control = OperationControl::default();
        let lifetime = session.begin(10000, control.clone()).unwrap();
        let batches = compact(vec![native(vec![1], vec![2])], &lifetime, &control);
        let schema = batches[0].records().schema();
        let used = lifetime.budget.snapshot().unwrap().retained_bytes as usize;
        let remaining = lifetime.budget.reserve_retained((1 << 20) - used).unwrap()
            .into_owned(&lifetime.budget).unwrap();
        let table = table(); let pin = pin(); let digest = "a".repeat(64);
        assert!(SnapshotCache::new(SnapshotBinding { table: &table, pin: &pin, source_sha256: &digest },
            schema, batches, SnapshotCoverage::default(), lifetime.clone(), &control).is_err());
        drop(remaining); drop(lifetime);
        assert_eq!(session.budget.snapshot().unwrap().retained_bytes, 0);
    }
}
