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
/// XML/TOC use the same conservative prerequisite for the whole document,
/// including every inline Lua handler. XML persistent buffers and event scratch
/// compete with Lua frames in the remaining 8 MiB; no nested parser receives
/// another allowance. TOC does not parse Lua, but keeps the same admission.
/// These are requested Rust allocations, not allocator overhead or an RSS cap.
/// Tree-sitter's opaque parser/tree/cursor C allocations have no safe allocation
/// cap; input <=64 KiB and the caller's worker-count limit bound their exposure.
pub(crate) struct WorkerGuard<'a> {
    control: &'a OperationControl,
    limit: usize,
    owned: Cell<usize>,
    abandoned: Cell<bool>,
    nodes: Cell<usize>,
    scratch: Cell<usize>,
}
impl<'a> WorkerGuard<'a> {
    pub(crate) fn new(control: &'a OperationControl, limit: usize) -> Self {
        Self {
            control,
            limit,
            owned: Cell::new(0),
            abandoned: Cell::new(false),
            nodes: Cell::new(0),
            scratch: Cell::new(8 << 20),
        }
    }
    pub(crate) fn abandon(&self) {
        self.abandoned.set(true);
    }
    pub(crate) fn check(&self) -> bool {
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
    pub(crate) fn charge(&self, bytes: usize) -> bool {
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
    pub(crate) fn observe_node(&self) -> bool {
        let nodes = self.nodes.get() + 1;
        self.nodes.set(nodes);
        if nodes > (16 << 20) / (8 * size_of::<tree_sitter::Node<'_>>()) {
            self.abandoned.set(true);
        }
        self.check()
    }
    pub(crate) fn frame(&self, raw_len: usize) -> Option<ScratchFrame<'_>> {
        // A frame can hold converted/decoded target names, a temporary UTF-8
        // replacement, and pending assignment names while descending. Each is
        // bounded by source bytes; growth/UTF-8 replacement fits this factor.
        let Some(bytes) = raw_len
            .checked_mul(8)
            .and_then(|bytes| bytes.checked_add(1024))
        else {
            self.abandon();
            return None;
        };
        self.reserve_scratch(bytes)
    }
    pub(crate) fn reserve_scratch(&self, bytes: usize) -> Option<ScratchFrame<'_>> {
        if !self.charge_scratch(bytes) {
            return None;
        }
        Some(ScratchFrame {
            used: &self.scratch,
            bytes,
        })
    }
    pub(crate) fn charge_scratch(&self, bytes: usize) -> bool {
        let Some(next) = self.scratch.get().checked_add(bytes) else {
            self.abandon();
            return false;
        };
        if next > 16 << 20 || !self.check() {
            self.abandon();
            return false;
        }
        // Persistent vector/String capacities keep their charges until the
        // document ends. Scoped frames release only the bytes they own.
        self.scratch.set(next);
        true
    }
    pub(crate) fn raw_target(&self, previous_len: usize, capacity: usize) -> bool {
        // Conservative BTreeMap node/split allowance: each possible spine
        // level gets 1 KiB, exceeding a usize/Vec<u8> leaf or internal node on
        // the supported 32/64-bit Rust targets. Even a binary tree needs at
        // most this many levels; charge before insert, never after allocation.
        let levels = usize::BITS as usize - previous_len.leading_zeros() as usize + 2;
        self.charge(capacity.saturating_add(levels * 1024))
    }
}
pub(crate) struct ScratchFrame<'a> {
    used: &'a Cell<usize>,
    bytes: usize,
}
impl Drop for ScratchFrame<'_> {
    fn drop(&mut self) {
        self.used.set(self.used.get() - self.bytes);
    }
}
pub(crate) fn check(guard: Option<&WorkerGuard<'_>>) -> bool {
    guard.is_none_or(WorkerGuard::check)
}
pub(crate) fn charge(guard: Option<&WorkerGuard<'_>>, bytes: usize) -> bool {
    guard.is_none_or(|guard| guard.charge(bytes))
}
pub(crate) fn push<T>(guard: Option<&WorkerGuard<'_>>, values: &mut Vec<T>, value: T) {
    if prepare_push(guard, values) {
        values.push(value);
    }
}
pub(crate) fn prepare_push<T>(guard: Option<&WorkerGuard<'_>>, values: &mut Vec<T>) -> bool {
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
    #[test]
    fn xml_and_lua_frames_share_the_exact_scratch_boundary() {
        let control = OperationControl::default();
        let guard = WorkerGuard::new(&control, 4 << 20);
        let frame_bytes = (64 << 10) * 8 + 1024;
        let outer = guard.reserve_scratch((8 << 20) - frame_bytes).unwrap();
        let inner = guard.frame(64 << 10).unwrap();
        assert_eq!(guard.scratch.get(), 16 << 20);
        assert!(guard.reserve_scratch(1).is_none());
        drop(inner);
        drop(outer);
        assert_eq!(guard.scratch.get(), 8 << 20);
        assert!(!guard.check(), "abandonment remains sticky after release");
        assert!(control.check().is_ok());
    }
    #[test]
    fn inline_lua_observes_the_live_xml_scratch_owner() {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn_scoped(scope, || {
                    let control = OperationControl::default();
                    let guard = WorkerGuard::new(&control, 4 << 20);
                    let outer = guard.reserve_scratch((8 << 20) - 1000).unwrap();
                    assert!(crate::lua_tree::facts_with_worker_guard(b"Call()", &guard).is_none());
                    assert!(!guard.check());
                    drop(outer);
                    assert_eq!(guard.scratch.get(), 8 << 20);
                    assert!(control.check().is_ok());
                    assert_eq!(crate::facts::lua(b"Call()", false).relationships.len(), 1);
                })
                .unwrap()
                .join()
                .unwrap();
        });
    }
    #[test]
    fn xml_scratch_growth_is_admitted_before_allocation_and_reuses_live_capacity() {
        let control = OperationControl::default();
        let guard = WorkerGuard::new(&control, 4 << 20);
        let mut text = String::new();
        assert!(append_scratch(Some(&guard), &mut text, "abc"));
        let charged = guard.scratch.get();
        text.clear();
        assert!(append_scratch(Some(&guard), &mut text, "x"));
        assert_eq!(guard.scratch.get(), charged);
        let outer = guard.reserve_scratch((16 << 20) - charged).unwrap();
        let mut stack = Vec::<u64>::new();
        assert!(!prepare_scratch_push(Some(&guard), &mut stack));
        assert_eq!(stack.capacity(), 0);
        assert_eq!(text, "x");
        drop(outer);
        assert!(!guard.check());
    }
    #[test]
    fn inherited_xml_owner_pressure_abandons_without_a_public_parse_limit() {
        std::thread::scope(|scope| {
            std::thread::Builder::new()
                .stack_size(2 << 20)
                .spawn_scoped(scope, || {
                    let control = OperationControl::default();
                    let owner = "X".repeat(8192);
                    let raw = format!(
                        "<Ui><Frame name=\"{owner}\">{}{}</Frame></Ui>",
                        "<N>".repeat(1200),
                        "</N>".repeat(1200)
                    );
                    assert!(raw.len() < 64 << 10);
                    assert!(
                        crate::facts::for_worker(
                            raw.as_bytes(),
                            crate::facts::ParseMode::Xml,
                            &control,
                            4 << 20
                        )
                        .is_none()
                    );
                    assert!(control.check().is_ok());
                    let complete = crate::facts::xml(raw.as_bytes());
                    assert!(complete.diagnostics.is_empty());
                    assert_eq!(complete.declarations.len(), 1);
                    assert_eq!(complete.declarations[0].name, owner);
                })
                .unwrap()
                .join()
                .unwrap();
        });
    }
    #[test]
    fn overflowing_scratch_requests_abandon_without_allocating() {
        let control = OperationControl::default();
        for as_frame in [false, true] {
            let guard = WorkerGuard::new(&control, 4 << 20);
            let result = if as_frame {
                guard.frame(usize::MAX)
            } else {
                guard.reserve_scratch(usize::MAX)
            };
            assert!(result.is_none());
            assert_eq!(guard.scratch.get(), 8 << 20);
            assert!(!guard.check());
        }
    }
}

/// Grow an XML stack/script within the same scratch ledger used by inline Lua.
/// Requested old buffers remain conservatively charged for the whole document.
pub(crate) fn prepare_scratch_push<T>(
    guard: Option<&WorkerGuard<'_>>,
    values: &mut Vec<T>,
) -> bool {
    let Some(guard) = guard else { return true };
    if !guard.check() {
        return false;
    }
    if values.len() == values.capacity() {
        let capacity = values.capacity().saturating_mul(2).max(4);
        if !guard.charge_scratch(capacity.saturating_mul(size_of::<T>())) {
            return false;
        }
        values.reserve_exact(capacity - values.len());
    }
    true
}
pub(crate) fn append_scratch(
    guard: Option<&WorkerGuard<'_>>,
    target: &mut String,
    text: &str,
) -> bool {
    let Some(guard) = guard else {
        target.push_str(text);
        return true;
    };
    if !guard.check() {
        return false;
    }
    let Some(required) = target.len().checked_add(text.len()) else {
        guard.abandon();
        return false;
    };
    if required > target.capacity() {
        let capacity = target.capacity().saturating_mul(2).max(required);
        if !guard.charge_scratch(capacity) {
            return false;
        }
        target.reserve_exact(capacity - target.len());
    }
    target.push_str(text);
    true
}
