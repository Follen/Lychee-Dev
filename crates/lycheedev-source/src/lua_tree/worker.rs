//! Optional speculative admission only; never an operation budget or public error.
use lycheedev_core::control::OperationControl;
use std::{cell::Cell, mem::size_of, time::Instant};

/// The caller holds 32 MiB of scratch separately from the owned Facts allowance.
/// 16 MiB covers child-node vectors (at most 65,536 observed nodes, including
/// unnamed nodes, with eight Node-sized slots per node for overlapping vectors
/// and growth). The other 16 MiB covers source/grammar/line/diagnostic scratch
/// (8 MiB, including old/new Vec growth overlap) and checked per-walk temporary
/// name/literal buffers (8 MiB). Scope strings and their vector are charged to
/// `owned`, as are every retained Facts buffer.
/// These are requested Rust allocations, not allocator overhead or an RSS cap.
/// Tree-sitter's opaque parser/tree/cursor C allocations have no safe allocation
/// cap; input <=64 KiB and the caller's worker-count limit bound their exposure.
pub(super) struct WorkerGuard<'a> {
    control: &'a OperationControl,
    limit: usize,
    owned: Cell<usize>,
    abandoned: Cell<bool>,
    nodes: Cell<usize>,
    scratch: Cell<usize>,
}
impl<'a> WorkerGuard<'a> {
    pub(super) fn new(control: &'a OperationControl, limit: usize) -> Self {
        Self {
            control,
            limit,
            owned: Cell::new(0),
            abandoned: Cell::new(false),
            nodes: Cell::new(0),
            scratch: Cell::new(8 << 20),
        }
    }
    pub(super) fn check(&self) -> bool {
        if self.abandoned.get()
            || self.control.is_cancelled()
            || Instant::now() >= self.control.deadline()
        {
            self.abandoned.set(true);
            false
        } else {
            true
        }
    }
    pub(super) fn charge(&self, bytes: usize) -> bool {
        if !self.check() {
            return false;
        }
        if let Some(next) = self
            .owned
            .get()
            .checked_add(bytes)
            .filter(|next| *next <= self.limit)
        {
            self.owned.set(next);
            true
        } else {
            self.abandoned.set(true);
            false
        }
    }
    pub(super) fn observe_node(&self) -> bool {
        let nodes = self.nodes.get() + 1;
        self.nodes.set(nodes);
        if nodes > (16 << 20) / (8 * size_of::<tree_sitter::Node<'_>>()) {
            self.abandoned.set(true);
        }
        self.check()
    }
    pub(super) fn frame(&self, raw_len: usize) -> Option<ScratchFrame<'_>> {
        // A frame can hold converted/decoded target names, a temporary UTF-8
        // replacement, and pending assignment names while descending. Each is
        // bounded by source bytes; growth/UTF-8 replacement fits this factor.
        let bytes = raw_len * 8 + 1024;
        let next = self.scratch.get() + bytes;
        if next > 16 << 20 || !self.check() {
            self.abandoned.set(true);
            return None;
        }
        self.scratch.set(next);
        Some(ScratchFrame {
            used: &self.scratch,
            bytes,
        })
    }
    pub(super) fn raw_target(&self, previous_len: usize, capacity: usize) -> bool {
        // Conservative BTreeMap node/split allowance: each possible spine
        // level gets 1 KiB, exceeding a usize/Vec<u8> leaf or internal node on
        // the supported 32/64-bit Rust targets. Even a binary tree needs at
        // most this many levels; charge before insert, never after allocation.
        let levels = usize::BITS as usize - previous_len.leading_zeros() as usize + 2;
        self.charge(capacity.saturating_add(levels * 1024))
    }
}
pub(super) struct ScratchFrame<'a> {
    used: &'a Cell<usize>,
    bytes: usize,
}
impl Drop for ScratchFrame<'_> {
    fn drop(&mut self) {
        self.used.set(self.used.get() - self.bytes);
    }
}
pub(super) fn check(guard: Option<&WorkerGuard<'_>>) -> bool {
    guard.is_none_or(WorkerGuard::check)
}
pub(super) fn charge(guard: Option<&WorkerGuard<'_>>, bytes: usize) -> bool {
    guard.is_none_or(|guard| guard.charge(bytes))
}
pub(super) fn push<T>(guard: Option<&WorkerGuard<'_>>, values: &mut Vec<T>, value: T) {
    if prepare_push(guard, values) {
        values.push(value);
    }
}
pub(super) fn prepare_push<T>(guard: Option<&WorkerGuard<'_>>, values: &mut Vec<T>) -> bool {
    let Some(guard) = guard else { return true };
    if !guard.check() {
        return false;
    }
    if values.len() == values.capacity() {
        let capacity = values.capacity().saturating_mul(2).max(4);
        // Charge the whole new buffer, including old/new overlap at growth.
        // Old reservations are deliberately never reused during speculation.
        if !guard.charge(capacity.saturating_mul(size_of::<T>())) {
            return false;
        }
        values.reserve_exact(capacity - values.len());
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn worker_vec_capacity_is_admitted_before_growth() {
        let control = OperationControl::default();
        let guard = WorkerGuard::new(&control, 3 * size_of::<u64>());
        let mut values = Vec::<u64>::new();
        assert!(!prepare_push(Some(&guard), &mut values));
        assert_eq!(values.capacity(), 0);
        assert_eq!(guard.owned.get(), 0);
        assert!(!guard.check());
    }
    #[test]
    fn worker_node_vector_scratch_is_admitted_by_the_shared_syntax_pass() {
        let control = OperationControl::default();
        let guard = WorkerGuard::new(&control, usize::MAX);
        let limit = (16 << 20) / (8 * size_of::<tree_sitter::Node<'_>>());
        for _ in 0..limit {
            assert!(guard.observe_node());
        }
        assert!(!guard.observe_node());
        assert!(!guard.check());
        assert!(control.check().is_ok());
    }
    #[test]
    fn worker_scratch_frames_release_and_exhaustion_is_sticky() {
        let control = OperationControl::default();
        let guard = WorkerGuard::new(&control, usize::MAX);
        let before = guard.scratch.get();
        {
            let _frame = guard.frame(64 << 10).unwrap();
            assert!(guard.scratch.get() > before);
        }
        assert_eq!(guard.scratch.get(), before);
        let mut frames = Vec::new();
        while let Some(frame) = guard.frame(64 << 10) {
            frames.push(frame);
        }
        assert!(guard.scratch.get() <= 16 << 20);
        assert!(!guard.check());
        drop(frames);
        assert_eq!(guard.scratch.get(), before);
        assert!(!guard.check());
    }
}
