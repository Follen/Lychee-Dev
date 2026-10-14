//! Fixed-Go slot routing and generated Lua data. No filesystem reservation is
//! acquired here. A real publisher must hold its short publication lease and
//! recheck the selected process, exact reservation and complete skipped route.
//! Oracle: bridge/slot.go, channel/driver.go and delivery/slot_route.go at
//! 3057a9666ab078270b63c7f06de748069b0d1f4b.
use super::state::{InputAttempt, SlotEnvelope};
use super::{Result, SCHEMA, SLOT_COUNT, checksum};
use std::fmt::Write;

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

fn lua_string(out: &mut String, value: &str) {
    out.push('"');
    for byte in value.bytes() {
        match byte {
            b'"' | b'\\' => {
                out.push('\\');
                out.push(byte as char);
            }
            32..=126 => out.push(byte as char),
            _ => {
                // Writing to a String cannot fail; fixed-width decimal Lua 5.1
                // byte escapes avoid both Unicode loss and delimiter injection.
                let _ = write!(out, "\\{byte:03}");
            }
        }
    }
    out.push('"');
}

/// Exact bridge.SlotPayload bytes. This emits data, never top-level user code.
/// Static payload validation does not prove action/readiness/input authority.
pub fn slot_payload(envelope: &SlotEnvelope) -> Result<Vec<u8>> {
    envelope.validate()?;
    let mut out = String::with_capacity(envelope.code.len() + 512);
    out.push_str("LycheeDevSlotEnvelope = {\n");
    if envelope.start_slot != 0 {
        let _ = writeln!(out, "  startSlot = {},", envelope.start_slot);
    }
    for (name, value) in [
        ("schema", &envelope.schema),
        ("runtime", &envelope.runtime),
        ("owner", &envelope.owner),
        ("nonce", &envelope.nonce),
        ("ticket", &envelope.ticket),
        ("action", &envelope.action),
        ("guid", &envelope.guid),
        ("build", &envelope.build),
        ("code", &envelope.code),
        ("challenge", &envelope.challenge),
        ("preparedNonce", &envelope.prepared_nonce),
    ] {
        let _ = write!(out, "  {name} = ");
        lua_string(&mut out, value);
        out.push_str(",\n");
    }
    let (code_bytes, code_checksum) = if envelope.code.is_empty() {
        (envelope.code_bytes, envelope.code_checksum)
    } else {
        (
            envelope.code.len() as i32,
            checksum(envelope.code.as_bytes()),
        )
    };
    let _ = writeln!(
        out,
        "  index = {}, fence = {}, codeBytes = {}, codeChecksum = {}, budget = {}, reportBytes = {}, reportChecksum = {},\n}}",
        envelope.index,
        envelope.fence,
        code_bytes,
        code_checksum,
        envelope.budget,
        envelope.report_bytes,
        envelope.report_checksum
    );
    Ok(out.into_bytes())
}

#[cfg(test)]
#[path = "slot_allocation_tests.rs"]
mod tests;
