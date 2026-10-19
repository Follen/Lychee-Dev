//! Retain already-admitted imported buffers across raw Arrow clones/slices.
//! Arrow's pool hook is infallible and is NOT an allocation admission API.
use super::{add, multiply};
use crate::{
    generation::{ImportedBatch, ImportedBatchAllocation},
    resource::{Budget, OwnedRetainedReservation},
};
use arrow_array::{Array, RecordBatch, UInt32Array};
use arrow_buffer::{MemoryPool, MemoryReservation};
use lycheedev_core::Result;
use std::{fmt, sync::Arc};

pub(super) fn allocation_bound(columns: usize) -> Result<usize> {
    // The importer emits only primitive/string and one-level fixed-size-list
    // arrays: at most four buffer hooks per column plus the UInt32 locator.
    // Include Box reservations, Arc owner and wrapper metadata before claim().
    add(1024, multiply(add(multiply(columns, 4)?, 1)?, 256)?)
}

pub(super) struct OwnedBatch {
    pub records: RecordBatch,
    pub ids: UInt32Array,
    // Explicitly retained even for an empty/no-buffer batch. Schema and object
    // metadata are not themselves Arrow buffers.
    _owner: Arc<BufferOwner>,
}
impl OwnedBatch {
    pub(super) fn new(batch: ImportedBatch, budget: &Arc<Budget>, bytes: usize) -> Result<Self> {
        let allocation = budget.reserve_retained(bytes)?.into_owned(budget)?;
        let (records, ids, _, imported) = batch.into_parts();
        let owner = Arc::new(BufferOwner {
            _imported: imported,
            _allocation: allocation,
        });
        let pool = RetainInput {
            owner: owner.clone(),
        };
        records.claim(&pool);
        ids.claim(&pool);
        Ok(Self {
            records,
            ids,
            _owner: owner,
        })
    }
}

/// IMPORTANT: Neither reservation owner contains an Array/Buffer/RecordBatch.
/// Buffers -> BufferLease -> owner -> reservations/schema has no back edge.
struct BufferOwner {
    _imported: ImportedBatchAllocation,
    _allocation: OwnedRetainedReservation,
}
impl fmt::Debug for BufferOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PreadmittedInputOwner")
    }
}
#[derive(Debug)]
struct RetainInput {
    owner: Arc<BufferOwner>,
}
impl MemoryPool for RetainInput {
    fn reserve(&self, size: usize) -> Box<dyn MemoryReservation> {
        Box::new(BufferLease {
            bytes: size,
            _owner: self.owner.clone(),
        })
    }
    // This private hook only installs lifetime tokens on immutable preadmitted
    // inputs. It is never exposed as DataFusion's operator memory pool.
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
struct BufferLease {
    bytes: usize,
    _owner: Arc<BufferOwner>,
}
impl MemoryReservation for BufferLease {
    fn size(&self) -> usize {
        self.bytes
    }
    fn resize(&mut self, new_size: usize) {
        self.bytes = new_size;
    }
}
