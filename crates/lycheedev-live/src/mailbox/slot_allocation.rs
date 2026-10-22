//! Fixed-Go slot routing and generated Lua data. No filesystem reservation is
//! acquired here. A real publisher must hold its short publication lease and
//! recheck the selected process, exact reservation and complete skipped route.
//! Oracle: bridge/slot.go, channel/driver.go and delivery/slot_route.go at
//! 3057a9666ab078270b63c7f06de748069b0d1f4b.
use super::state::{InputAttempt, SlotEnvelope};
use super::{SCHEMA, SLOT_COUNT};
pub use lycheedev_core::mailbox::slot_payload;

/// Only the fields used by fixed-Go NextFreeSlot. An allocation hint never
/// authorizes overwrite, consumption or retirement of somebody else's claim.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SlotAllocationEntry {
    pub pending_hash: String,
    pub nonce: String,
    pub consumed: bool,
    pub retired_runtime: String,
    pub retired_process: bool,
}

/// Search strictly after the one-based occupied index; zero starts at slot 1.
/// Fixed-size input prevents inventing slots outside the managed 500-slot pool.
pub fn next_free_slot(
    slots: &[SlotAllocationEntry; SLOT_COUNT as usize],
    after: i32,
) -> Option<i32> {
    if !(0..SLOT_COUNT).contains(&after) {
        return None;
    }
    slots
        .iter()
        .enumerate()
        .skip(after as usize)
        .find_map(|(n, s)| {
            (s.pending_hash.is_empty()
                && (s.nonce.is_empty()
                    || s.consumed
                    || !s.retired_runtime.is_empty()
                    || s.retired_process))
                .then_some(n as i32 + 1)
        })
}

/// Native.Publish's extra prepare headroom gate. The backend must obtain these
/// facts under the publication lease, before changing any file.
pub fn forward_slot_suggestion(
    slots: &[SlotAllocationEntry; SLOT_COUNT as usize],
    envelope: &SlotEnvelope,
) -> Option<i32> {
    if envelope.schema != SCHEMA {
        return None;
    }
    next_free_slot(slots, envelope.index)
        .filter(|next| envelope.action != "prepare" || *next <= SLOT_COUNT - 4)
}

/// Driver.advance only moves an unpublished intent with no recorded input for
/// this nonce. Keep StartSlot, nonce, request, ticket and deadline unchanged.
pub fn may_reallocate(
    envelope: &SlotEnvelope,
    input: Option<&InputAttempt>,
    destination: i32,
) -> bool {
    envelope.schema == SCHEMA
        && destination > envelope.index
        && destination <= SLOT_COUNT
        && input.is_none_or(|a| a.exchange != envelope.nonce)
}

/// Run's conservative complete-operation gate; allocation races have a separate
/// four-slot gate above. The final 15-slot reserve is deliberately not reduced
/// to the ordinary prepare/commit/confirm/release count.
pub fn operation_needs_reload(next_slot: i32, slots: i32) -> bool {
    next_slot > slots - 15
}

#[cfg(test)]
#[path = "slot_allocation_tests.rs"]
mod tests;
