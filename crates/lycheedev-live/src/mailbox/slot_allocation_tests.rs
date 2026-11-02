use super::super::state::InputKind;
use super::super::{ContractError, InputDisposition, InputOutcome};
use super::*;

fn envelope() -> SlotEnvelope {
    SlotEnvelope {
        schema: SCHEMA.into(),
        index: 1,
        start_slot: 1,
        runtime: "2".repeat(32),
        owner: "1".repeat(32),
        fence: 1,
        nonce: "3".repeat(32),
        ticket: "4".repeat(32),
        action: "prepare".into(),
        guid: "g".into(),
        build: "b".into(),
        ..SlotEnvelope::default()
    }
}

#[test]
fn mailbox_allocation_foreign_pending_and_retired_claims() {
    let mut slots: [SlotAllocationEntry; 500] = std::array::from_fn(|_| SlotAllocationEntry {
        nonce: "reserved".into(),
        ..SlotAllocationEntry::default()
    });
    assert_eq!(next_free_slot(&slots, 0), None);
    slots[0].nonce.clear();
    slots[1].consumed = true;
    slots[2].retired_runtime = "replacement".into();
    slots[3].retired_process = true;
    assert_eq!(next_free_slot(&slots, 0), Some(1));
    assert_eq!(next_free_slot(&slots, 1), Some(2));
    assert_eq!(next_free_slot(&slots, 2), Some(3));
    assert_eq!(next_free_slot(&slots, 3), Some(4));
    for slot in &mut slots[..4] {
        slot.pending_hash = "in-flight".into();
    }
    assert_eq!(next_free_slot(&slots, 0), None);
    slots[499].nonce.clear();
    assert_eq!(next_free_slot(&slots, 498), Some(500));
    assert_eq!(next_free_slot(&slots, 500), None);
    assert_eq!(next_free_slot(&slots, -1), None);
    assert_eq!(next_free_slot(&slots, 501), None);
}

#[test]
fn mailbox_allocation_distinguishes_operation_and_publish_reserves() {
    assert!(!operation_needs_reload(485, 500));
    assert!(operation_needs_reload(486, 500));
    let slots = std::array::from_fn(|_| SlotAllocationEntry::default());
    let mut e = envelope();
    e.index = 495;
    assert_eq!(forward_slot_suggestion(&slots, &e), Some(496));
    e.index = 496;
    assert_eq!(forward_slot_suggestion(&slots, &e), None);
    for action in ["bind", "commit", "confirm", "release", "unbind"] {
        e.action = action.into();
        e.index = 499;
        assert_eq!(forward_slot_suggestion(&slots, &e), Some(500));
    }
    e.schema = "lycheedev.mailbox.v1".into();
    assert_eq!(forward_slot_suggestion(&slots, &e), None);
}

#[test]
fn mailbox_allocation_never_moves_an_attempted_nonce_even_known_zero() {
    let e = envelope();
    assert!(may_reallocate(&e, None, 500));
    assert!(!may_reallocate(&e, None, 1));
    assert!(!may_reallocate(&e, None, 501));
    let mut input = InputAttempt {
        id: "5".repeat(32),
        exchange: e.nonce.clone(),
        runtime: e.runtime.clone(),
        kind: InputKind::Invoke,
        after_millis: 0,
        recovery_after_millis: 0,

        observation: None,
        outcome: None,
        after_diagnostic: None,
    };
    assert!(!may_reallocate(&e, Some(&input), 2));
    input.outcome = Some(InputOutcome {
        disposition: InputDisposition::NotSent,
        messages_queued: 0,
        at_millis: 0,
        reason: "held".into(),
        retryable: true,
    });
    assert!(!may_reallocate(&e, Some(&input), 2));
    input.exchange = "6".repeat(32);
    assert!(may_reallocate(&e, Some(&input), 2));
}

#[test]
fn mailbox_slot_payload_exact_text_and_byte_escaping() {
    let mut e = envelope();
    e.code = "return \"中\"\n--\\\0\u{7f}".into();
    e.code_bytes = 999;
    e.code_checksum = 999;
    e.budget = 5;
    let bytes = slot_payload(&e).unwrap();
    let expected = format!(
        "LycheeDevSlotEnvelope = {{\n  startSlot = 1,\n  schema = \"Lycheedev mailbox protocol v1\",\n  runtime = \"{}\",\n  owner = \"{}\",\n  nonce = \"{}\",\n  ticket = \"{}\",\n  action = \"prepare\",\n  guid = \"g\",\n  build = \"b\",\n  code = \"return \\\"\\228\\184\\173\\\"\\010--\\\\\\000\\127\",\n  challenge = \"\",\n  preparedNonce = \"\",\n  index = 1, fence = 1, codeBytes = {}, codeChecksum = {}, budget = 5, reportBytes = 0, reportChecksum = 0,\n}}\n",
        "2".repeat(32),
        "1".repeat(32),
        "3".repeat(32),
        "4".repeat(32),
        18,
        1_153_959_565u32
    );
    assert_eq!(bytes, expected.as_bytes());
    assert_eq!(e.code_bytes, 999, "payload must not mutate journal input");
    e.start_slot = 0;
    e.code.clear();
    let text = String::from_utf8(slot_payload(&e).unwrap()).unwrap();
    assert!(!text.contains("startSlot"));
    assert!(text.contains("codeBytes = 999, codeChecksum = 999"));
    e.index = 501;
    assert_eq!(
        slot_payload(&e),
        Err(ContractError("bridge.invalid_slot_envelope"))
    );
    e.index = 500;
    e.runtime = "A".repeat(32);
    assert_eq!(
        slot_payload(&e),
        Err(ContractError("bridge.invalid_slot_token"))
    );
}
