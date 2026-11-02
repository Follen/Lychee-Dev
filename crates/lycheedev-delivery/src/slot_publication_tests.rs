//! Current delivery slot publication scenarios: install/publication isolation,
//! incomplete install and pending publication, ended-process and runtime
//! retirement of torn publications, bounded forward allocation preserving
//! unknown bytes, and actual foreign/empty route bytes. Additional faults cover
//! every before/after durable-write boundary, cancellation, exact field/digest
//! correlation, stale acknowledgements, and lease ownership.
use super::*;
use crate::installation::Resource;
use lycheedev_core::coordination::InstallationMaintenanceLease;
use std::{
    cell::RefCell,
    fs,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

fn envelope() -> SlotEnvelope {
    SlotEnvelope {
        schema: mailbox::SCHEMA.into(),
        index: 1,
        runtime: "1".repeat(32),
        owner: "2".repeat(32),
        nonce: "3".repeat(32),
        ticket: "4".repeat(32),
        fence: 1,
        action: "bind".into(),
        ..SlotEnvelope::default()
    }
}
fn empty_pool() -> SlotPool {
    SlotPool {
        schema: slots::SCHEMA.into(),
        version: "3.2.0".into(),
        state: "ready".into(),
        files: vec![
            SlotFile {
                payload_hash: storage::sha256(INERT),
                ..SlotFile::default()
            };
            slots::COUNT as usize
        ],
        ..SlotPool::default()
    }
}
#[derive(Clone, Copy, Debug)]
enum Fault {
    Before,
    After,
    CancelAfter,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Write {
    Manifest(SlotPool),
    Payload(i32, Vec<u8>),
}
struct MemoryStore {
    pool: SlotPool,
    payloads: Vec<Vec<u8>>,
    writes: Vec<Write>,
    fault: Option<(usize, Fault)>,
    attempts: usize,
    reads: usize,
    static_drift: bool,
}
impl MemoryStore {
    fn new() -> Self {
        Self {
            pool: empty_pool(),
            payloads: vec![INERT.to_vec(); slots::COUNT as usize],
            writes: vec![],
            fault: None,
            attempts: 0,
            reads: 0,
            static_drift: false,
        }
    }
    fn fault_error() -> ToolkitError {
        ToolkitError::new("fixture.injected_write", "injected uncertain write", 5)
    }
    fn write(&mut self, write: Write, control: &OperationControl) -> lycheedev_core::Result<()> {
        control.check()?;
        self.attempts += 1;
        let fault = self
            .fault
            .filter(|(at, _)| *at == self.attempts)
            .map(|(_, f)| f);
        if matches!(fault, Some(Fault::Before)) {
            return Err(Self::fault_error());
        }
        match &write {
            Write::Manifest(pool) => self.pool = pool.clone(),
            Write::Payload(index, bytes) => self.payloads[*index as usize - 1] = bytes.clone(),
        }
        self.writes.push(write);
        if matches!(fault, Some(Fault::After)) {
            return Err(Self::fault_error());
        }
        if matches!(fault, Some(Fault::CancelAfter)) {
            control.tracked_cancel_handle()?.cancel();
            control.check()?;
        }
        Ok(())
    }
    fn torn(replaced: bool) -> (Self, SlotEnvelope) {
        let mut store = Self::new();
        let e = envelope();
        let bytes = payload(&e).unwrap();
        let entry = &mut store.pool.files[0];
        entry.pending_hash = storage::sha256(&bytes);
        entry.nonce = e.nonce.clone();
        entry.runtime = e.runtime.clone();
        entry.consumer = "123/456".into();
        if replaced {
            store.payloads[0] = bytes;
        }
        (store, e)
    }
}
impl Store for MemoryStore {
    fn inspect(&mut self, control: &OperationControl) -> lycheedev_core::Result<SlotPool> {
        control.check()?;
        if self.static_drift
            || self.pool.schema != slots::SCHEMA
            || self.pool.state != "ready"
            || self.pool.files.len() != slots::COUNT as usize
        {
            return Err(invalid("fixture installation drift"));
        }
        for (entry, bytes) in self.pool.files.iter().zip(&self.payloads) {
            control.check()?;
            let hash = storage::sha256(bytes);
            if hash != entry.payload_hash && hash != entry.pending_hash {
                return Err(invalid("fixture payload drift"));
            }
        }
        Ok(self.pool.clone())
    }
    fn read_payload(
        &mut self,
        index: i32,
        control: &OperationControl,
    ) -> lycheedev_core::Result<Vec<u8>> {
        control.check()?;
        self.reads += 1;
        Ok(self.payloads[index as usize - 1].clone())
    }
    fn save(&mut self, pool: &SlotPool, control: &OperationControl) -> lycheedev_core::Result<()> {
        self.write(Write::Manifest(pool.clone()), control)
    }
    fn replace_payload(
        &mut self,
        index: i32,
        bytes: &[u8],
        control: &OperationControl,
    ) -> lycheedev_core::Result<()> {
        self.write(Write::Payload(index, bytes.to_vec()), control)
    }
}
fn code<T: std::fmt::Debug>(result: Result<T>, expected: &str) {
    let error = result.unwrap_err();
    assert!(error.is_code(expected), "expected {expected}: {error:?}");
}

#[test]
fn publication_is_intent_then_exact_core_payload_then_commit() {
    let mut store = MemoryStore::new();
    let control = OperationControl::default();
    let e = envelope();
    publish(&mut store, &control, "instance-a", &e).unwrap();
    assert_eq!(store.writes.len(), 3);
    let Write::Manifest(intent) = &store.writes[0] else {
        panic!("intent first")
    };
    assert_eq!(intent.files[0].payload_hash, storage::sha256(INERT));
    assert_eq!(
        intent.files[0].pending_hash,
        storage::sha256(&payload(&e).unwrap())
    );
    assert_eq!(intent.files[0].nonce, e.nonce);
    assert_eq!(intent.files[0].runtime, e.runtime);
    assert_eq!(intent.files[0].consumer, "instance-a");
    assert!(!intent.files[0].consumed);
    assert_eq!(
        store.writes[1],
        Write::Payload(1, mailbox::slot_payload(&e).unwrap())
    );
    assert_eq!(store.pool.files[0].pending_hash, "");
    assert_eq!(
        store.pool.files[0].payload_hash,
        intent.files[0].pending_hash
    );
    assert_eq!(store.pool.files[1..], empty_pool().files[1..]);
    verify(&mut store, &control, "instance-a", &e).unwrap();
    publish(&mut store, &control, "instance-a", &e).unwrap();
    assert_eq!(store.writes.len(), 3, "exact retry is read-only");
}
#[test]
fn reserved_preserves_both_causes_and_exact_captured_owner() {
    let mut store = MemoryStore::new();
    let c = OperationControl::default();
    let e = envelope();
    publish(&mut store, &c, "instance-a", &e).unwrap();
    for changed in ["nonce", "runtime", "consumer"] {
        let mut next = e.clone();
        let consumer = if changed == "consumer" {
            "instance-b"
        } else {
            "instance-a"
        };
        match changed {
            "nonce" => next.nonce = "5".repeat(32),
            "runtime" => next.runtime = "6".repeat(32),
            _ => (),
        }
        let error = publish(&mut store, &c, consumer, &next).unwrap_err();
        assert!(error.is_code("delivery.slot_reserved"));
        assert!(error.is_code("delivery.installation_conflict"));
        let PublicationError::Reserved { reservation } = error else {
            panic!("typed reserved")
        };
        assert_eq!(reservation, store.pool.files[0]);
    }
    assert_eq!(store.writes.len(), 3);
    // The existing reservation is checked before validating malformed tokens.
    let mut malformed = e.clone();
    malformed.nonce = "bad".into();
    code(
        publish(&mut store, &c, "instance-a", &malformed),
        "delivery.slot_reserved",
    );
}
#[test]
fn immutable_same_nonce_digest_includes_all_payload_fields() {
    let mut store = MemoryStore::new();
    let c = OperationControl::default();
    let e = envelope();
    publish(&mut store, &c, "a", &e).unwrap();
    for field in 0..13 {
        let mut next = e.clone();
        match field {
            0 => next.owner = "5".repeat(32),
            1 => next.fence = 2,
            2 => next.ticket = "6".repeat(32),
            3 => next.action = "prepare".into(),
            4 => next.code = "return '中文'".into(),
            5 => next.budget = 8,
            6 => next.challenge = "challenge".into(),
            7 => next.prepared_nonce = "7".repeat(32),
            8 => next.report_bytes = 100,
            9 => next.report_checksum = 88,
            10 => next.guid = "Player-1".into(),
            11 => next.build = "12.0.1.1".into(),
            _ => next.start_slot = 1,
        }
        code(
            publish(&mut store, &c, "a", &next),
            "delivery.installation_conflict",
        );
        code(
            verify(&mut store, &c, "a", &next),
            "delivery.installation_conflict",
        );
    }
    assert_eq!(store.writes.len(), 3);
}
#[test]
fn shared_encoder_validation_and_error_order_are_preserved() {
    let c = OperationControl::default();
    for case in 0..8 {
        let mut store = MemoryStore::new();
        let mut e = envelope();
        let want = match case {
            0 => {
                e.schema = "lycheedev.slot.v1".into();
                "delivery.invalid_installation"
            }
            1 => {
                e.index = 0;
                "delivery.invalid_installation"
            }
            2 => {
                e.index = 501;
                "delivery.invalid_installation"
            }
            3 => {
                e.fence = 0;
                "bridge.invalid_slot_envelope"
            }
            4 => {
                e.fence = mailbox::MAX_EXACT_INTEGER + 1;
                "bridge.invalid_slot_envelope"
            }
            5 => {
                e.owner = "A".repeat(32);
                "bridge.invalid_slot_token"
            }
            6 => {
                e.start_slot = -1;
                "bridge.invalid_slot_envelope"
            }
            _ => {
                e.code = "x".repeat(262145).into();
                "bridge.invalid_slot_envelope"
            }
        };
        code(publish(&mut store, &c, "a", &e), want);
        code(verify(&mut store, &c, "a", &e), want);
        assert!(store.writes.is_empty());
    }
    let mut store = MemoryStore::new();
    code(
        publish(&mut store, &c, "", &envelope()),
        "delivery.invalid_installation",
    );
    store.static_drift = true;
    let mut e = envelope();
    e.owner.clear();
    code(
        publish(&mut store, &c, "a", &e),
        "delivery.invalid_installation",
    );
}
#[test]
fn each_uncertain_write_boundary_retains_recoverable_exact_intent() {
    for stage in 1..=3 {
        for fault in [Fault::Before, Fault::After] {
            let mut store = MemoryStore::new();
            let c = OperationControl::default();
            let e = envelope();
            store.fault = Some((stage, fault));
            code(publish(&mut store, &c, "a", &e), "fixture.injected_write");
            let committed = stage == 3 && matches!(fault, Fault::After);
            let untouched = stage == 1 && matches!(fault, Fault::Before);
            assert_eq!(store.pool.files[0].nonce.is_empty(), untouched);
            assert_eq!(
                store.pool.files[0].pending_hash.is_empty(),
                untouched || committed
            );
            assert!(!store.pool.files[0].consumed);
            if !untouched {
                let mut other = e.clone();
                other.nonce = "5".repeat(32);
                code(
                    publish(&mut store, &c, "b", &other),
                    "delivery.slot_reserved",
                );
            }
            if !committed {
                code(
                    verify(&mut store, &c, "a", &e),
                    "delivery.installation_conflict",
                );
            }
            store.fault = None;
            publish(&mut store, &c, "a", &e).unwrap();
            verify(&mut store, &c, "a", &e).unwrap();
            assert_eq!(store.payloads[0], payload(&e).unwrap());
            assert_eq!(store.pool.files[0].pending_hash, "");
        }
    }
}
#[test]
fn cancellation_at_each_persistent_boundary_does_not_roll_back_or_continue() {
    for stage in 1..=3 {
        let mut store = MemoryStore::new();
        let c = OperationControl::default();
        let e = envelope();
        store.fault = Some((stage, Fault::CancelAfter));
        code(publish(&mut store, &c, "a", &e), "command.cancelled");
        assert_eq!(store.attempts, stage);
        assert_eq!(store.writes.len(), stage);
        assert!(!store.pool.files[0].consumed);
        store.fault = None;
        // Resume is an explicit new operation, not an internal reset of c.
        let resumed = OperationControl::default();
        publish(&mut store, &resumed, "a", &e).unwrap();
        verify(&mut store, &resumed, "a", &e).unwrap();
    }
}
#[test]
fn torn_same_nonce_changed_intent_never_overwrites_either_payload() {
    for replaced in [false, true] {
        let (mut store, mut e) = MemoryStore::torn(replaced);
        let before = store.pool.clone();
        let bytes = store.payloads[0].clone();
        e.fence += 1;
        code(
            publish(&mut store, &OperationControl::default(), "123/456", &e),
            "delivery.installation_conflict",
        );
        assert_eq!(store.pool, before);
        assert_eq!(store.payloads[0], bytes);
        assert!(store.writes.is_empty());
    }
}
#[test]
fn confirmation_requires_committed_exact_consumer_and_ignores_newer_nonce() {
    let mut store = MemoryStore::new();
    let c = OperationControl::default();
    let e = envelope();
    publish(&mut store, &c, "a", &e).unwrap();
    code(
        confirm_consumed(&mut store, &c, "b", &e.expected()),
        "delivery.installation_conflict",
    );
    confirm_consumed(&mut store, &c, "a", &e.expected()).unwrap();
    assert!(store.pool.files[0].consumed);
    code(
        verify(&mut store, &c, "a", &e),
        "delivery.installation_conflict",
    );
    let writes = store.writes.len();
    publish(&mut store, &c, "a", &e).unwrap();
    assert!(
        store.pool.files[0].consumed,
        "same nonce cannot resurrect consumption"
    );
    assert_eq!(store.writes.len(), writes);
    let mut other = e.clone();
    other.nonce = "5".repeat(32);
    other.runtime = "6".repeat(32);
    publish(&mut store, &c, "b", &other).unwrap();
    let before = store.pool.clone();
    confirm_consumed(&mut store, &c, "a", &e.expected()).unwrap();
    assert_eq!(store.pool, before);
    assert!(!store.pool.files[0].consumed);
}
#[test]
fn confirmation_rejects_pending_empty_identity_and_invalid_index() {
    let (mut store, e) = MemoryStore::torn(true);
    let c = OperationControl::default();
    code(
        confirm_consumed(&mut store, &c, "123/456", &e.expected()),
        "delivery.installation_conflict",
    );
    for case in 0..4 {
        let mut expected = e.expected();
        let consumer = if case == 0 { "" } else { "123/456" };
        let want = if case < 2 {
            "delivery.installation_conflict"
        } else {
            "delivery.invalid_installation"
        };
        match case {
            1 => expected.nonce.clear(),
            2 => expected.index = 0,
            3 => expected.index = 501,
            _ => (),
        }
        code(confirm_consumed(&mut store, &c, consumer, &expected), want);
    }
    assert!(store.writes.is_empty());
}
#[test]
fn lower_level_confirmation_keeps_nonce_contract_after_durable_receipt() {
    let mut store = MemoryStore::new();
    let c = OperationControl::default();
    let e = envelope();
    publish(&mut store, &c, "a", &e).unwrap();
    // The exact receipt has already been verified by the caller. Delivery does
    // not silently add a second schema/owner/fence gate absent from the retained contract.
    let expected = ExpectedEnvelope {
        index: e.index,
        nonce: e.nonce,
        ..Default::default()
    };
    confirm_consumed(&mut store, &c, "a", &expected).unwrap();
    assert!(store.pool.files[0].consumed);
}
#[test]
fn process_retirement_reconciles_both_torn_payload_states_without_ack() {
    for replaced in [false, true] {
        let (mut store, e) = MemoryStore::torn(replaced);
        let c = OperationControl::default();
        let before = store.pool.clone();
        let actual = storage::sha256(&store.payloads[0]);
        code(
            retire_process(&mut store, &c, "foreign", &e),
            "delivery.slot_retirement_conflict",
        );
        assert_eq!(store.pool, before);
        retire_process(&mut store, &c, "123/456", &e).unwrap();
        let row = store.pool.files[0].clone();
        assert_eq!(row.payload_hash, actual);
        assert!(row.pending_hash.is_empty());
        assert!(row.retired_process);
        assert!(!row.consumed);
        let writes = store.writes.len();
        retire_process(&mut store, &c, "123/456", &e).unwrap();
        assert_eq!(store.writes.len(), writes);
        let mut next = e.clone();
        next.nonce = "5".repeat(32);
        publish(&mut store, &c, "new", &next).unwrap();
        assert!(!store.pool.files[0].retired_process);
        let before = store.pool.clone();
        retire_process(&mut store, &c, "123/456", &e).unwrap();
        assert_eq!(store.pool, before);
    }
}
#[test]
fn runtime_retirement_persists_exact_successor_and_never_orders_tokens() {
    for replaced in [false, true] {
        let (mut store, e) = MemoryStore::torn(replaced);
        let c = OperationControl::default();
        let next = "0".repeat(32); // smaller than old, still a different runtime
        let actual = storage::sha256(&store.payloads[0]);
        let before = store.pool.clone();
        code(
            retire_runtime(&mut store, &c, "foreign", &e, &next),
            "delivery.slot_retirement_conflict",
        );
        assert_eq!(store.pool, before);
        for _ in 0..2 {
            retire_runtime(&mut store, &c, "123/456", &e, &next).unwrap();
        }
        let row = store.pool.files[0].clone();
        assert_eq!(row.payload_hash, actual);
        assert_eq!(row.pending_hash, "");
        assert_eq!(row.retired_runtime, next);
        assert!(!row.consumed && !row.retired_process);
        code(
            retire_runtime(&mut store, &c, "123/456", &e, &"9".repeat(32)),
            "delivery.slot_retirement_conflict",
        );
        assert_eq!(store.pool.files[0], row);
        let mut replacement = e.clone();
        replacement.runtime = next.clone();
        replacement.nonce = "5".repeat(32);
        publish(&mut store, &c, "another-instance", &replacement).unwrap();
        assert!(store.pool.files[0].retired_runtime.is_empty());
        let before = store.pool.clone();
        retire_runtime(&mut store, &c, "123/456", &e, &next).unwrap();
        assert_eq!(store.pool, before);
    }
}
#[test]
fn retirement_rejects_corrupt_payload_and_wrong_runtime_without_mutation() {
    for process in [false, true] {
        let (mut store, mut e) = MemoryStore::torn(true);
        let before = store.pool.clone();
        e.runtime = "8".repeat(32);
        let result = if process {
            retire_process(&mut store, &OperationControl::default(), "123/456", &e)
        } else {
            retire_runtime(
                &mut store,
                &OperationControl::default(),
                "123/456",
                &e,
                &"9".repeat(32),
            )
        };
        code(result, "delivery.slot_retirement_conflict");
        assert_eq!(store.pool, before);
        e.runtime = "1".repeat(32);
        store.payloads[0] = b"corrupt".to_vec();
        let result = if process {
            retire_process(&mut store, &OperationControl::default(), "123/456", &e)
        } else {
            retire_runtime(
                &mut store,
                &OperationControl::default(),
                "123/456",
                &e,
                &"9".repeat(32),
            )
        };
        code(result, "delivery.invalid_installation");
        assert_eq!(store.pool, before);
        assert!(store.writes.is_empty());
    }
}
#[test]
fn retirement_prechecks_and_lower_level_successor_length_match_reference() {
    let (mut store, e) = MemoryStore::torn(false);
    let c = OperationControl::default();
    for next in [
        String::new(),
        "f".repeat(31),
        e.runtime.clone(),
        "é".repeat(32),
    ] {
        code(
            retire_runtime(&mut store, &c, "123/456", &e, &next),
            "delivery.invalid_installation",
        );
    }
    for case in 0..4 {
        let mut changed = e.clone();
        let consumer = if case == 0 { "" } else { "123/456" };
        match case {
            1 => changed.nonce.clear(),
            2 => changed.index = 0,
            3 => changed.index = 501,
            _ => (),
        }
        code(
            retire_process(&mut store, &c, consumer, &changed),
            "delivery.invalid_installation",
        );
        code(
            retire_runtime(&mut store, &c, consumer, &changed, &"f".repeat(32)),
            "delivery.invalid_installation",
        );
    }
    assert!(store.writes.is_empty());
    retire_runtime(&mut store, &c, "123/456", &e, &"Z".repeat(32)).unwrap();
    assert_eq!(store.pool.files[0].retired_runtime, "Z".repeat(32));
}
#[test]
fn consumed_retirement_is_noop_but_foreign_consumer_still_conflicts() {
    let mut store = MemoryStore::new();
    let c = OperationControl::default();
    let e = envelope();
    publish(&mut store, &c, "a", &e).unwrap();
    confirm_consumed(&mut store, &c, "a", &e.expected()).unwrap();
    let before = store.pool.clone();
    let writes = store.writes.len();
    retire_process(&mut store, &c, "a", &e).unwrap();
    retire_runtime(&mut store, &c, "a", &e, &"9".repeat(32)).unwrap();
    assert_eq!(store.writes.len(), writes);
    assert_eq!(store.pool, before);
    code(
        retire_process(&mut store, &c, "foreign", &e),
        "delivery.slot_retirement_conflict",
    );
    code(
        retire_runtime(&mut store, &c, "foreign", &e, &"9".repeat(32)),
        "delivery.slot_retirement_conflict",
    );
}
#[test]
fn confirm_and_retire_uncertain_saves_are_idempotent_without_replay() {
    for operation in 0..3 {
        for fault in [Fault::Before, Fault::After, Fault::CancelAfter] {
            let mut store = MemoryStore::new();
            let initial = OperationControl::default();
            let e = envelope();
            publish(&mut store, &initial, "a", &e).unwrap();
            store.fault = Some((store.attempts + 1, fault));
            let c = OperationControl::default();
            let invoke = |store: &mut MemoryStore, c: &OperationControl| match operation {
                0 => confirm_consumed(store, c, "a", &e.expected()),
                1 => retire_process(store, c, "a", &e),
                _ => retire_runtime(store, c, "a", &e, &"9".repeat(32)),
            };
            assert!(invoke(&mut store, &c).is_err());
            store.fault = None;
            invoke(&mut store, &OperationControl::default()).unwrap();
            assert_eq!(store.pool.files[0].consumed, operation == 0);
            assert_eq!(store.payloads[0], payload(&e).unwrap());
            assert_eq!(
                store
                    .writes
                    .iter()
                    .filter(|w| matches!(w, Write::Payload(..)))
                    .count(),
                1
            );
        }
    }
}
#[test]
fn verify_rejects_every_reservation_retirement_and_hash_gate() {
    let c = OperationControl::default();
    for case in 0..8 {
        let mut store = MemoryStore::new();
        let e = envelope();
        publish(&mut store, &c, "a", &e).unwrap();
        let row = &mut store.pool.files[0];
        match case {
            0 => row.nonce = "5".repeat(32),
            1 => row.runtime = "6".repeat(32),
            2 => row.consumer = "b".into(),
            3 => row.pending_hash = row.payload_hash.clone(),
            4 => row.consumed = true,
            5 => row.retired_runtime = "7".repeat(32),
            6 => row.retired_process = true,
            _ => {
                store.payloads[0] = INERT.to_vec();
                row.payload_hash = storage::sha256(INERT);
            }
        }
        code(
            verify(&mut store, &c, "a", &e),
            "delivery.installation_conflict",
        );
        assert_eq!(store.writes.len(), 3);
    }
}
#[test]
fn forward_allocation_preserves_unknown_pending_and_does_not_wrap() {
    let mut pool = empty_pool();
    for row in &mut pool.files {
        row.nonce = "held".into();
    }
    assert_eq!(next_free_slot(&pool, 1), None);
    pool.files[499].consumed = true;
    assert_eq!(next_free_slot(&pool, 1), Some(500));
    pool.files[499].pending_hash = "interrupted".into();
    assert_eq!(next_free_slot(&pool, 1), None);
    pool.files[0] = SlotFile::default();
    assert_eq!(next_free_slot(&pool, 1), None);
    assert_eq!(next_free_slot(&pool, 0), Some(1));
    assert_eq!(next_free_slot(&pool, -1), None);
    assert_eq!(next_free_slot(&pool, 500), None);
    pool.files[499].pending_hash.clear();
    pool.files[499].consumed = false;
    pool.files[499].retired_runtime = "proof".into();
    assert_eq!(next_free_slot(&pool, 1), Some(500));
    pool.files[499].retired_runtime.clear();
    pool.files[499].retired_process = true;
    assert_eq!(next_free_slot(&pool, 1), Some(500));
}
#[test]
fn route_uses_actual_foreign_and_empty_bytes_not_pending_reservation() {
    let mut store = MemoryStore::new();
    let foreign = payload(&envelope()).unwrap();
    let mut e = envelope();
    e.index = 2;
    e.start_slot = 1;
    e.runtime = "5".repeat(32);
    for (bytes, allowed) in [
        (INERT.to_vec(), true),
        (foreign.clone(), true),
        (
            String::from_utf8(foreign.clone())
                .unwrap()
                .replace(&"1".repeat(32), &e.runtime)
                .into_bytes(),
            false,
        ),
        (
            String::from_utf8(foreign.clone())
                .unwrap()
                .replace(mailbox::SCHEMA, "lycheedev.slot.v1")
                .into_bytes(),
            false,
        ),
        (
            String::from_utf8(foreign.clone())
                .unwrap()
                .replace("index = 1,", "index = 7,")
                .into_bytes(),
            false,
        ),
        (
            String::from_utf8(foreign.clone())
                .unwrap()
                .replace("\"bind\"", "\"other\"")
                .into_bytes(),
            false,
        ),
    ] {
        store.payloads[0] = bytes;
        let before = store.pool.clone();
        assert_eq!(
            verify_route(&mut store, &OperationControl::default(), &e).is_ok(),
            allowed
        );
        assert_eq!(store.pool, before);
        assert!(store.writes.is_empty());
    }
    // A pending SAME-runtime intent does not make actual inert bytes unsafe.
    store.pool.files[0].runtime = e.runtime.clone();
    store.pool.files[0].pending_hash = storage::sha256(&payload(&e).unwrap());
    store.payloads[0] = INERT.to_vec();
    verify_route(&mut store, &OperationControl::default(), &e).unwrap();
}
#[test]
fn route_runtime_is_unique_anchored_ascii_and_checks_every_skipped_file() {
    let mut store = MemoryStore::new();
    let foreign = String::from_utf8(payload(&envelope()).unwrap()).unwrap();
    let mut e = envelope();
    e.index = 3;
    e.start_slot = 1;
    e.runtime = "5".repeat(32);
    for bytes in [
        format!("{foreign}  runtime = \"{}\",\n", "8".repeat(32)),
        foreign.replace("  runtime =", " runtime ="),
        foreign.replace(&"1".repeat(32), &"A".repeat(32)),
        foreign.replace('\n', "\r\n"),
    ] {
        store.payloads[0] = bytes.into_bytes();
        code(
            verify_route(&mut store, &OperationControl::default(), &e),
            "delivery.installation_conflict",
        );
    }
    store.payloads[0] = foreign.into_bytes();
    store.payloads[1] = b"unknown".to_vec();
    let error = verify_route(&mut store, &OperationControl::default(), &e).unwrap_err();
    assert!(error.to_string().contains("slot route blocked at 2"));
    store.payloads[1] = INERT.to_vec();
    verify_route(&mut store, &OperationControl::default(), &e).unwrap();
}
#[test]
fn route_range_defaults_and_schema_gate_match_reference() {
    let mut store = MemoryStore::new();
    let c = OperationControl::default();
    let mut e = envelope();
    e.schema.clear();
    verify_route(&mut store, &c, &e).unwrap(); // empty route does not validate envelope
    assert_eq!(store.reads, 0);
    for (start, index) in [(-1, 1), (2, 1), (0, 0), (501, 501), (1, 501), (1, 2)] {
        e.start_slot = start;
        e.index = index;
        code(
            verify_route(&mut store, &c, &e),
            "delivery.invalid_installation",
        );
    }
    assert_eq!(store.reads, 0);
    e.schema = mailbox::SCHEMA.into();
    e.index = 500;
    e.start_slot = 1;
    verify_route(&mut store, &c, &e).unwrap();
    assert_eq!(store.reads, 499);
}
#[test]
fn pre_cancelled_control_prevents_all_reads_writes_and_lock_attempts() {
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    let mut store = MemoryStore::new();
    let e = envelope();
    code(publish(&mut store, &c, "a", &e), "command.cancelled");
    code(verify(&mut store, &c, "a", &e), "command.cancelled");
    code(
        confirm_consumed(&mut store, &c, "a", &e.expected()),
        "command.cancelled",
    );
    code(retire_process(&mut store, &c, "a", &e), "command.cancelled");
    code(
        retire_runtime(&mut store, &c, "a", &e, &"9".repeat(32)),
        "command.cancelled",
    );
    let mut route = e;
    route.index = 2;
    route.start_slot = 1;
    code(verify_route(&mut store, &c, &route), "command.cancelled");
    let mut attempts = 0;
    let result = wait_lock(
        &c,
        || {
            attempts += 1;
            Ok(())
        },
        |_| panic!("no lock was acquired"),
    );
    assert_eq!(result.unwrap_err().0[0].code, "command.cancelled");
    assert_eq!(attempts, 0);
    assert_eq!(store.reads, 0);
    assert!(store.writes.is_empty());
}
#[test]
fn lock_wait_keeps_original_deadline_and_checks_success_after_cancel() {
    let c = OperationControl::uncancelable(Instant::now() + Duration::from_millis(5));
    let mut attempts = 0;
    let result: AcquisitionResult<()> = wait_lock(
        &c,
        || {
            attempts += 1;
            Err(ToolkitError::new("workspace.busy", "held", 6))
        },
        |_| panic!("a failed attempt owns no lock"),
    );
    assert_eq!(result.unwrap_err().0[0].code, "command.deadline");
    assert!(
        attempts <= 2,
        "an already-expired budget may admit no attempt"
    );
    let c = OperationControl::default();
    let mut closes = 0;
    let result = wait_lock(
        &c,
        || {
            c.tracked_cancel_handle().unwrap().cancel();
            Ok(())
        },
        |_| {
            closes += 1;
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().0[0].code, "command.cancelled");
    assert_eq!(closes, 1);
}

// Only helper-level close faults are injected here. They do not demonstrate an
// actual OS close failure; the core storage suite owns its consumed File tests.
#[derive(Debug)]
struct CloseProbe {
    name: &'static str,
    events: Rc<RefCell<Vec<String>>>,
    held: bool,
    fail_close: bool,
}
impl CloseProbe {
    fn new(name: &'static str, events: &Rc<RefCell<Vec<String>>>, fail_close: bool) -> Self {
        events.borrow_mut().push(format!("acquire:{name}"));
        Self {
            name,
            events: events.clone(),
            held: true,
            fail_close,
        }
    }
    fn close(mut self) -> lycheedev_core::Result<()> {
        assert!(self.held, "a consumed lock must never be closed twice");
        self.held = false;
        self.events
            .borrow_mut()
            .push(format!("close:{}", self.name));
        if self.fail_close {
            Err(ToolkitError::new(
                format!("fixture.{}_close", self.name),
                "helper-injected close failure",
                5,
            ))
        } else {
            Ok(())
        }
    }
}
impl Drop for CloseProbe {
    fn drop(&mut self) {
        if self.held {
            self.events
                .borrow_mut()
                .push(format!("fallback:{}", self.name));
        }
    }
}

#[test]
fn acquisition_failure_keeps_primary_and_ordered_cleanup_without_drop_retry() {
    for primary in ["fixture.path", "fixture.inspect"] {
        for publication_fails in [false, true] {
            for maintenance_fails in [false, true] {
                let events = Rc::new(RefCell::new(Vec::new()));
                let maintenance = CloseProbe::new("maintenance", &events, maintenance_fails);
                let result = with_acquired(
                    maintenance,
                    |_| {
                        let publication =
                            CloseProbe::new("publication", &events, publication_fails);
                        with_acquired(
                            publication,
                            |_| -> AcquisitionResult<()> {
                                Err(ToolkitError::new(primary, "original", 4)
                                    .with_details(serde_json::json!({"original": 17}))
                                    .into())
                            },
                            CloseProbe::close,
                        )
                    },
                    CloseProbe::close,
                );
                let error = PublicationError::from(result.unwrap_err());
                let causes = match &error {
                    PublicationError::Failure(cause) => std::slice::from_ref(cause),
                    PublicationError::CloseFailures(causes) => causes.as_slice(),
                    PublicationError::Reserved { .. } => panic!("not an acquisition error"),
                };
                let mut expected = vec![primary];
                if publication_fails {
                    expected.push("fixture.publication_close");
                }
                if maintenance_fails {
                    expected.push("fixture.maintenance_close");
                }
                assert_eq!(
                    causes
                        .iter()
                        .map(|cause| cause.code.as_str())
                        .collect::<Vec<_>>(),
                    expected
                );
                assert_eq!(causes[0].message, "original");
                assert_eq!(causes[0].exit_code, 4);
                assert_eq!(causes[0].details, serde_json::json!({"original": 17}));
                for code in expected {
                    assert!(error.is_code(code));
                }
                assert_eq!(
                    *events.borrow(),
                    [
                        "acquire:maintenance",
                        "acquire:publication",
                        "close:publication",
                        "close:maintenance"
                    ]
                );
            }
        }
    }
}

#[test]
fn acquisition_failure_before_second_lock_closes_only_maintenance() {
    for primary in ["fixture.path", "fixture.second_lock"] {
        let events = Rc::new(RefCell::new(Vec::new()));
        let result = with_acquired(
            CloseProbe::new("maintenance", &events, true),
            |_| -> AcquisitionResult<()> { Err(ToolkitError::new(primary, "original", 5).into()) },
            CloseProbe::close,
        );
        let error = PublicationError::from(result.unwrap_err());
        assert!(error.is_code(primary));
        assert!(error.is_code("fixture.maintenance_close"));
        assert!(!error.is_code("fixture.publication_close"));
        assert_eq!(
            *events.borrow(),
            ["acquire:maintenance", "close:maintenance"]
        );
    }
}

#[test]
fn acquisition_cleanup_ignores_cancelled_or_expired_control() {
    for cancelled in [false, true] {
        let events = Rc::new(RefCell::new(Vec::new()));
        let result = with_acquired(
            CloseProbe::new("maintenance", &events, true),
            |_| {
                with_acquired(
                    CloseProbe::new("publication", &events, true),
                    |_| {
                        let control = if cancelled {
                            let control = OperationControl::default();
                            control.tracked_cancel_handle()?.cancel();
                            control
                        } else {
                            OperationControl::uncancelable(Instant::now())
                        };
                        Ok(control.check()?)
                    },
                    CloseProbe::close,
                )
            },
            CloseProbe::close,
        );
        let PublicationError::CloseFailures(causes) = PublicationError::from(result.unwrap_err())
        else {
            panic!("primary plus both cleanup failures")
        };
        assert_eq!(causes.len(), 3);
        assert_eq!(
            causes[0].code,
            if cancelled {
                "command.cancelled"
            } else {
                "command.deadline"
            }
        );
        assert_eq!(causes[1].code, "fixture.publication_close");
        assert_eq!(causes[2].code, "fixture.maintenance_close");
        assert_eq!(
            *events.borrow(),
            [
                "acquire:maintenance",
                "acquire:publication",
                "close:publication",
                "close:maintenance"
            ]
        );
    }
}

#[test]
fn second_lock_post_acquire_cancel_closes_before_maintenance() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let control = OperationControl::default();
    let result = with_acquired(
        CloseProbe::new("maintenance", &events, true),
        |_| {
            wait_lock(
                &control,
                || {
                    let publication = CloseProbe::new("publication", &events, true);
                    control.tracked_cancel_handle()?.cancel();
                    Ok(publication)
                },
                CloseProbe::close,
            )
        },
        CloseProbe::close,
    );
    let PublicationError::CloseFailures(causes) = PublicationError::from(result.unwrap_err())
    else {
        panic!("post-acquire cancellation plus both close failures")
    };
    assert_eq!(
        causes
            .iter()
            .map(|cause| cause.code.as_str())
            .collect::<Vec<_>>(),
        [
            "command.cancelled",
            "fixture.publication_close",
            "fixture.maintenance_close"
        ]
    );
    assert_eq!(
        *events.borrow(),
        [
            "acquire:maintenance",
            "acquire:publication",
            "close:publication",
            "close:maintenance"
        ]
    );
}

#[test]
fn acquisition_success_transfers_ownership_without_early_close() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let (maintenance, (publication, value)) = with_acquired(
        CloseProbe::new("maintenance", &events, false),
        |_| {
            with_acquired(
                CloseProbe::new("publication", &events, false),
                |_| Ok(17),
                CloseProbe::close,
            )
        },
        CloseProbe::close,
    )
    .unwrap();
    assert_eq!(value, 17);
    assert_eq!(
        *events.borrow(),
        ["acquire:maintenance", "acquire:publication"]
    );
    publication.close().unwrap();
    maintenance.close().unwrap();
    assert_eq!(
        *events.borrow(),
        [
            "acquire:maintenance",
            "acquire:publication",
            "close:publication",
            "close:maintenance"
        ]
    );
}

static NEXT: AtomicU64 = AtomicU64::new(0);
fn independent_disk_root() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lychee-formal-publication-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed),
    ));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    root
}
struct LockRoot {
    root: PathBuf,
}
impl LockRoot {
    fn new() -> Self {
        Self {
            root: independent_disk_root(),
        }
    }
}
impl Drop for LockRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct DiskFixture {
    root: PathBuf,
    identity: VerifiedInstallation,
}
impl DiskFixture {
    fn new() -> Self {
        let fixture = Self::without_slot_pool();
        let root = &fixture.root;
        // Direct fixture setup is not a test of install or durable writes.
        for index in 1..=slots::COUNT {
            let path = slots::directory(root, index);
            fs::create_dir(&path).unwrap();
            for (name, bytes) in slots::files(index, "3.2.0") {
                fs::write(path.join(name), bytes).unwrap();
            }
        }
        fs::write(
            root.join(slots::MARKER),
            serde_json::to_vec(&empty_pool()).unwrap(),
        )
        .unwrap();
        fixture
    }
    // Acquisition failures below occur before DiskStore::inspect. Keep their
    // real canonical addon and receipt without constructing an unused pool.
    fn without_slot_pool() -> Self {
        let root = independent_disk_root();
        let addon = root.join("Lychee Dev");
        fs::create_dir(&addon).unwrap();
        fs::write(addon.join("Core.lua"), b"-- fixture\n").unwrap();
        let receipt = Receipt {
            schema: "lycheedev.installation.v1".into(),
            component: "addon".into(),
            version: "3.2.0".into(),
            commit: "a".repeat(40),
            resources: vec![Resource {
                path: "addon/Core.lua".into(),
                sha256: storage::sha256(b"-- fixture\n"),
                bytes: 11,
            }],
        };
        let raw = serde_json::to_vec(&receipt).unwrap();
        fs::write(addon.join(installation::MARKER), &raw).unwrap();
        Self {
            identity: VerifiedInstallation {
                canonical_path: addon,
                manifest_sha256: storage::sha256(&raw),
            },
            root,
        }
    }
}
impl Drop for DiskFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn assert_disk_locks_available(f: &DiskFixture) {
    InstallationMaintenanceLease::acquire(&f.identity.canonical_path)
        .unwrap()
        .close()
        .unwrap();
    ExclusiveLock::resource(&f.root.join(".lycheedev-slot-locks"), "publication")
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn lock_wait_post_acquire_cancel_checks_real_close_once_and_releases() {
    let f = LockRoot::new();
    let path = f.root.join("post-acquire.lock");
    for inject_close_failure in [false, true] {
        let control = OperationControl::default();
        let mut closes = 0;
        let result = wait_lock(
            &control,
            || {
                let lock = ExclusiveLock::acquire(&path)?;
                control.tracked_cancel_handle()?.cancel();
                Ok(lock)
            },
            |lock| {
                closes += 1;
                lock.close()?;
                if inject_close_failure {
                    // This is an error after a real successful close, not an
                    // OS close failure. Ownership must still not be retried.
                    Err(ToolkitError::new(
                        "fixture.post_close",
                        "helper-injected",
                        5,
                    ))
                } else {
                    Ok(())
                }
            },
        );
        let error = PublicationError::from(result.err().expect("post-acquire cancellation"));
        assert!(error.is_code("command.cancelled"));
        assert_eq!(error.is_code("fixture.post_close"), inject_close_failure);
        assert_eq!(closes, 1);
        assert!(path.is_file());
        ExclusiveLock::acquire(&path).unwrap().close().unwrap();
    }
}

#[test]
fn disk_acquire_canonical_path_failure_releases_maintenance() {
    let f = DiskFixture::without_slot_pool();
    fs::create_dir(f.root.join("alias")).unwrap();
    // PathBuf::join normalizes `..` for Windows verbatim paths. Use an
    // ordinary path spelling so the OS can resolve it, and append raw path
    // text so the fixture still reaches the API with a noncanonical identity.
    let mut raw_path = std::ffi::OsString::from(lycheedev_core::client::display_path(&f.root));
    for component in ["alias", "..", "Lychee Dev"] {
        raw_path.push(std::path::MAIN_SEPARATOR_STR);
        raw_path.push(component);
    }
    let noncanonical = PathBuf::from(raw_path);
    assert!(
        noncanonical
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    );
    let canonical = fs::canonicalize(&noncanonical).unwrap();
    assert_eq!(canonical, f.identity.canonical_path);
    assert_ne!(noncanonical, canonical);
    let identity = RecordedInstallationIdentity::from_authenticated_record(noncanonical).unwrap();
    let error =
        PublicationLease::acquire_recorded(&identity, "3.2.0", &OperationControl::default())
            .err()
            .expect("noncanonical recorded identity");
    assert!(error.is_code("delivery.invalid_installation"));
    assert!(f.root.join(".lycheedev-locks").is_dir());
    assert!(!f.root.join(".lycheedev-slot-locks").exists());
    assert_disk_locks_available(&f);
}

#[test]
fn disk_acquire_second_lock_io_failure_releases_maintenance() {
    let f = DiskFixture::without_slot_pool();
    let scope = f.root.join(".lycheedev-slot-locks");
    fs::write(&scope, b"a file cannot be a lock directory").unwrap();
    let error = PublicationLease::acquire(&f.identity, "3.2.0", &OperationControl::default())
        .err()
        .expect("publication lock directory cannot be created");
    assert!(error.is_code("external.io"));
    InstallationMaintenanceLease::acquire(&f.identity.canonical_path)
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(
        fs::read(&scope).unwrap(),
        b"a file cannot be a lock directory"
    );
    fs::remove_file(&scope).unwrap();
    assert_disk_locks_available(&f);
}

#[test]
fn disk_acquire_second_lock_deadline_releases_only_owned_maintenance() {
    let f = DiskFixture::without_slot_pool();
    let scope = f.root.join(".lycheedev-slot-locks");
    let foreign_publication = ExclusiveLock::resource(&scope, "publication").unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(25));
    let original_deadline = control.deadline();
    let error = PublicationLease::acquire(&f.identity, "3.2.0", &control)
        .err()
        .expect("foreign publication still held");
    assert!(error.is_code("command.deadline"));
    assert_eq!(control.deadline(), original_deadline);
    InstallationMaintenanceLease::acquire(&f.identity.canonical_path)
        .unwrap()
        .close()
        .unwrap();
    assert!(ExclusiveLock::resource(&scope, "publication").is_err());
    foreign_publication.close().unwrap();
    assert_disk_locks_available(&f);
}

#[test]
fn disk_publication_roundtrip_holds_both_locks_and_releases_before_receipt_wait() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = envelope();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    assert!(InstallationMaintenanceLease::acquire(&f.identity.canonical_path).is_err());
    assert!(ExclusiveLock::resource(&f.root.join(".lycheedev-slot-locks"), "publication").is_err());
    lease.publish("123/456", &e).unwrap();
    lease.verify("123/456", &e).unwrap();
    lease.verify_route(&e).unwrap();
    assert_eq!(
        fs::read(slots::directory(&f.root, 1).join("Payload.lua")).unwrap(),
        mailbox::slot_payload(&e).unwrap()
    );
    lease.close().unwrap(); // receipt wait begins only after checked lease close
    let gate = InstallationMaintenanceLease::acquire(&f.identity.canonical_path).unwrap();
    drop(gate);
    let mut resumed = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    let mut foreign = e.clone();
    foreign.nonce = "9".repeat(32);
    code(resumed.publish("other", &foreign), "delivery.slot_reserved");
    resumed.confirm_consumed("123/456", &e.expected()).unwrap();
    resumed.publish("other", &foreign).unwrap();
    resumed.confirm_consumed("123/456", &e.expected()).unwrap();
    assert!(!resumed.inspect().unwrap().files[0].consumed);
    resumed.close().unwrap();
    assert!(
        f.root
            .join(".lycheedev-slot-locks")
            .join(format!("{}.lock", storage::sha256(b"publication")))
            .is_file()
    );
}
#[test]
fn disk_publication_rejects_changed_receipt_and_admission_rejects_pool_drift() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let mut wrong = f.identity.clone();
    wrong.manifest_sha256 = "0".repeat(64);
    let mut lease = PublicationLease::acquire(&wrong, "3.2.0", &c).unwrap();
    code(
        lease.publish("a", &envelope()),
        "delivery.invalid_installation",
    );
    drop(lease);
    assert!(InstallationMaintenanceLease::acquire(&f.identity.canonical_path).is_ok());
    let loader = slots::directory(&f.root, 500).join("Loader.lua");
    let original = fs::read(&loader).unwrap();
    fs::write(&loader, b"-- changed").unwrap();
    assert!(
        PublicationLease::acquire(&f.identity, "3.2.0", &c)
            .err()
            .unwrap()
            .is_code("delivery.invalid_installation")
    );
    assert_disk_locks_available(&f);
    fs::write(&loader, original).unwrap();
    fs::write(
        slots::directory(&f.root, 499).join("Payload.lua"),
        b"corrupt",
    )
    .unwrap();
    assert!(
        PublicationLease::acquire(&f.identity, "3.2.0", &c)
            .err()
            .unwrap()
            .is_code("delivery.invalid_installation")
    );
    assert_disk_locks_available(&f);
}

#[test]
fn disk_cleanup_uses_recorded_path_and_pool_even_when_addon_or_receipt_changed() {
    // Recorded cleanup does not call the addon Guard or read its installation
    // receipt; it uses the recorded path and pool only.
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = envelope();
    let recorded =
        RecordedInstallationIdentity::from_authenticated_record(f.identity.canonical_path.clone())
            .unwrap();
    let marker = f.identity.canonical_path.join(installation::MARKER);
    let receipt = fs::read(&marker).unwrap();
    let core = f.identity.canonical_path.join("Core.lua");
    for operation in 0..3 {
        // Restore owned fixture bytes and create a fresh durable reservation.
        fs::write(&marker, &receipt).unwrap();
        fs::write(&core, b"-- fixture\n").unwrap();
        fs::write(slots::directory(&f.root, 1).join("Payload.lua"), INERT).unwrap();
        fs::write(
            f.root.join(slots::MARKER),
            serde_json::to_vec(&empty_pool()).unwrap(),
        )
        .unwrap();
        let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
        lease.publish("123/456", &e).unwrap();
        drop(lease);
        match operation {
            0 => {
                fs::remove_file(&marker).unwrap();
            }
            1 => {
                fs::write(&marker, b"updated or corrupt addon receipt").unwrap();
            }
            _ => {
                fs::remove_dir_all(&f.identity.canonical_path).unwrap();
            }
        }
        if core.exists() {
            fs::write(&core, b"-- externally edited addon\n").unwrap();
        }
        let mut cleanup = PublicationLease::acquire_recorded(&recorded, "3.2.0", &c).unwrap();
        // A path-only recovery identity is not a counterfeit clean snapshot.
        code(
            cleanup.publish("123/456", &e),
            "delivery.installation_not_clean",
        );
        match operation {
            0 => cleanup.confirm_consumed("123/456", &e.expected()).unwrap(),
            1 => cleanup.retire_process("123/456", &e).unwrap(),
            _ => cleanup
                .retire_runtime("123/456", &e, &"9".repeat(32))
                .unwrap(),
        }
        let row = cleanup.inspect().unwrap().files[0].clone();
        assert_eq!(row.consumed, operation == 0);
        assert_eq!(row.retired_process, operation == 1);
        assert_eq!(!row.retired_runtime.is_empty(), operation == 2);
        drop(cleanup);
    }
}
#[test]
fn disk_pre_input_rechecks_addon_inventory_with_original_control() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = envelope();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    lease.publish("a", &e).unwrap();
    fs::write(f.identity.canonical_path.join("Core.lua"), b"-- changed\n").unwrap();
    code(lease.verify("a", &e), "delivery.invalid_payload");
    let cancelled = c.tracked_cancel_handle().unwrap();
    cancelled.cancel();
    code(lease.verify("a", &e), "command.cancelled");
    assert!(!slots::read_pool_controlled(&f.root, &Default::default()).unwrap().files[0].consumed);
}

#[test]
fn explicit_close_preserves_both_ordered_failure_causes() {
    assert!(close_results(Ok(()), Ok(())).is_ok());
    let publication = || ToolkitError::new("fixture.publication_close", "first", 5);
    let maintenance = || ToolkitError::new("fixture.maintenance_close", "second", 5);
    code(
        close_results(Err(publication()), Ok(())),
        "fixture.publication_close",
    );
    code(
        close_results(Ok(()), Err(maintenance())),
        "fixture.maintenance_close",
    );
    let error = close_results(Err(publication()), Err(maintenance())).unwrap_err();
    assert!(error.is_code("fixture.publication_close"));
    assert!(error.is_code("fixture.maintenance_close"));
    let PublicationError::CloseFailures(causes) = error else {
        panic!("joined close causes")
    };
    assert_eq!(causes[0].message, "first");
    assert_eq!(causes[1].message, "second");
}

fn unbind() -> SlotEnvelope {
    SlotEnvelope {
        action: "unbind".into(),
        ..envelope()
    }
}
fn recorded(f: &DiskFixture) -> RecordedInstallationIdentity {
    RecordedInstallationIdentity::from_authenticated_record(f.identity.canonical_path.clone())
        .unwrap()
}

#[test]
fn disk_retirement_lost_manifest_preserves_unknown_bytes_without_authorizing_publication() {
    for process in [false, true] {
        for damaged in [
            None,
            Some(b"{broken".to_vec()),
            Some(b"{\"schema\":4}".to_vec()),
            Some(b"{}".to_vec()),
            Some(vec![b' '; (256 << 10) + 1]),
        ] {
            let f = DiskFixture::without_slot_pool();
            let c = OperationControl::default();
            let marker = f.root.join(slots::MARKER);
            if let Some(bytes) = &damaged {
                fs::write(&marker, bytes).unwrap();
            }
            fs::create_dir(slots::directory(&f.root, 1)).unwrap();
            let path = slots::directory(&f.root, 1).join("Payload.lua");
            let unknown = b"unknown payload\0\xff\n";
            fs::write(&path, unknown).unwrap();
            for _ in 0..2 {
                let mut lease =
                    PublicationLease::acquire_retirement_recorded(&recorded(&f), "3.2.0", &c)
                        .unwrap();
                if process {
                    lease.retire_process("123/456", &envelope()).unwrap();
                } else {
                    lease
                        .retire_runtime("123/456", &envelope(), &"9".repeat(32))
                        .unwrap();
                }
                lease.close().unwrap();
            }
            if let Some(bytes) = damaged {
                assert_eq!(fs::read(&marker).unwrap(), bytes);
            } else {
                assert!(!marker.exists());
            }
            assert_eq!(fs::read(path).unwrap(), unknown);
            assert!(PublicationLease::acquire_recorded(&recorded(&f), "3.2.0", &c).is_err());
            assert_disk_locks_available(&f);
        }
    }
}

#[test]
fn disk_retirement_ignores_unrelated_static_damage_and_keeps_exact_reservation_checks() {
    for process in [false, true] {
        for damage in [
            "static",
            "consumer",
            "runtime",
            "payload",
            "new-nonce",
            "version",
        ] {
            let f = DiskFixture::without_slot_pool();
            let c = OperationControl::default();
            let e = envelope();
            let bytes = payload(&e).unwrap();
            let mut pool = empty_pool();
            pool.files[0] = SlotFile {
                payload_hash: storage::sha256(&bytes),
                consumer: "123/456".into(),
                nonce: e.nonce.clone(),
                runtime: e.runtime.clone(),
                ..Default::default()
            };
            match damage {
                "consumer" => pool.files[0].consumer = "foreign".into(),
                "runtime" => pool.files[0].runtime = "8".repeat(32),
                "new-nonce" => pool.files[0].nonce = "7".repeat(32),
                "version" => pool.version = "other-version".into(),
                _ => {}
            }
            slots::save_pool(&f.root, &pool, &c).unwrap();
            fs::create_dir(slots::directory(&f.root, 1)).unwrap();
            let path = slots::directory(&f.root, 1).join("Payload.lua");
            fs::write(
                path,
                if damage == "payload" {
                    b"unjournaled".as_slice()
                } else {
                    &bytes
                },
            )
            .unwrap();
            fs::create_dir(slots::directory(&f.root, 2)).unwrap();
            let unrelated = slots::directory(&f.root, 2).join("Loader.lua");
            fs::write(&unrelated, b"unknown static damage").unwrap();
            let before = fs::read(f.root.join(slots::MARKER)).unwrap();
            let mut lease =
                PublicationLease::acquire_retirement_recorded(&recorded(&f), "3.2.0", &c).unwrap();
            let result = if process {
                lease.retire_process("123/456", &e)
            } else {
                lease.retire_runtime("123/456", &e, &"9".repeat(32))
            };
            assert_eq!(
                result.is_ok(),
                matches!(damage, "static" | "new-nonce"),
                "{damage}: {result:?}"
            );
            lease.close().unwrap();
            let after = fs::read(f.root.join(slots::MARKER)).unwrap();
            if damage == "static" {
                let retired = slots::read_pool_controlled(&f.root, &Default::default()).unwrap();
                assert_eq!(retired.files[0].retired_process, process);
                assert_eq!(!retired.files[0].retired_runtime.is_empty(), !process);
                assert!(!retired.files[0].consumed);
            } else {
                assert_eq!(before, after);
            }
            assert_eq!(fs::read(unrelated).unwrap(), b"unknown static damage");
            assert!(slots::inspect_typed(&f.root, "3.2.0", &c).is_err());
        }
    }
}

#[test]
fn disk_retirement_recognized_previous_generations_fail_without_changing_original_bytes() {
    let mut generations = vec![
        ("lycheedev.slots.v1", 64, "ready", "", ""),
        ("lycheedev.slots.v2", 200, "ready", "", ""),
        (slots::SCHEMA, 200, "ready", "", ""),
        (slots::SCHEMA, 200, "installing", "", ""),
        (slots::SCHEMA, 500, "unrecognized-state", "", ""),
    ];
    for schema in [
        "lycheedev.slots.migration.v3",
        "lycheedev.slots.migration.v4",
    ] {
        for (source, count) in [
            ("lycheedev.slots.v1", 64),
            ("lycheedev.slots.v2", 200),
            (slots::SCHEMA, 200),
            (slots::SCHEMA, 500),
        ] {
            generations.push((schema, count, "upgrading", source, "next-version"));
        }
    }
    for process in [false, true] {
        for (schema, count, state, source, pending) in &generations {
            let f = DiskFixture::without_slot_pool();
            let control = OperationControl::default();
            let e = envelope();
            let bytes = payload(&e).unwrap();
            let mut pool = empty_pool();
            pool.schema = (*schema).into();
            pool.files.truncate(*count);
            pool.state = (*state).into();
            pool.source_schema = (*source).into();
            pool.pending_version = (*pending).into();
            pool.files[0] = SlotFile {
                payload_hash: storage::sha256(&bytes),
                nonce: e.nonce.clone(),
                runtime: e.runtime.clone(),
                consumer: "123/456".into(),
                ..Default::default()
            };
            let marker = f.root.join(slots::MARKER);
            let original = serde_json::to_vec(&pool).unwrap();
            fs::write(&marker, &original).unwrap();
            fs::create_dir(slots::directory(&f.root, 1)).unwrap();
            let payload_path = slots::directory(&f.root, 1).join("Payload.lua");
            fs::write(&payload_path, &bytes).unwrap();
            let mut lease =
                PublicationLease::acquire_retirement_recorded(&recorded(&f), "3.2.0", &control)
                    .unwrap();
            let result = if process {
                lease.retire_process("123/456", &e)
            } else {
                lease.retire_runtime("123/456", &e, &"9".repeat(32))
            };
            code(result, "delivery.invalid_installation");
            lease.close().unwrap();
            assert_eq!(
                fs::read(&marker).unwrap(),
                original,
                "{schema}/{count}/{state}/{source}"
            );
            assert_eq!(fs::read(payload_path).unwrap(), bytes);
            assert_disk_locks_available(&f);
        }
    }
}

#[test]
fn disk_retirement_malformed_previous_generation_metadata_is_left_unavailable() {
    for process in [false, true] {
        for (schema, count, state, source, pending) in [
            ("lycheedev.slots.v1", 65, "ready", "", ""),
            ("lycheedev.slots.v1", 64, "installing", "", ""),
            ("lycheedev.slots.v2", 64, "ready", "", ""),
            (
                "lycheedev.slots.migration.v3",
                64,
                "upgrading",
                "lycheedev.slots.v1",
                "",
            ),
            (
                "lycheedev.slots.migration.v4",
                200,
                "upgrading",
                "unknown-schema",
                "next-version",
            ),
        ] {
            let f = DiskFixture::without_slot_pool();
            let control = OperationControl::default();
            let mut pool = empty_pool();
            pool.schema = schema.into();
            pool.files.truncate(count);
            pool.state = state.into();
            pool.source_schema = source.into();
            pool.pending_version = pending.into();
            let marker = f.root.join(slots::MARKER);
            let original = serde_json::to_vec(&pool).unwrap();
            fs::write(&marker, &original).unwrap();
            fs::create_dir(slots::directory(&f.root, 1)).unwrap();
            let payload_path = slots::directory(&f.root, 1).join("Payload.lua");
            let unknown = b"unknown original payload\0\xff";
            fs::write(&payload_path, unknown).unwrap();
            let mut lease =
                PublicationLease::acquire_retirement_recorded(&recorded(&f), "3.2.0", &control)
                    .unwrap();
            if process {
                lease.retire_process("123/456", &envelope()).unwrap();
            } else {
                lease
                    .retire_runtime("123/456", &envelope(), &"9".repeat(32))
                    .unwrap();
            }
            lease.close().unwrap();
            assert_eq!(fs::read(&marker).unwrap(), original);
            assert_eq!(fs::read(payload_path).unwrap(), unknown);
        }
    }
}

#[test]
fn disk_retirement_preserves_cancellation_and_non_file_metadata_errors() {
    let f = DiskFixture::without_slot_pool();
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    match PublicationLease::acquire_retirement_recorded(&recorded(&f), "3.2.0", &c) {
        Err(error) => assert!(error.is_code("command.cancelled")),
        Ok(_) => panic!("cancelled retirement acquired a lease"),
    }
    let c = OperationControl::default();
    fs::create_dir(f.root.join(slots::MARKER)).unwrap();
    let mut lease =
        PublicationLease::acquire_retirement_recorded(&recorded(&f), "3.2.0", &c).unwrap();
    assert!(lease.retire_process("123/456", &envelope()).is_err());
    lease.close().unwrap();
    let path = f.root.join("regular-parent");
    fs::write(&path, b"not a directory").unwrap();
    assert!(slots::retirement_pool(&path, "3.2.0", &c).is_err());
}
fn repair(f: &DiskFixture, e: &SlotEnvelope) -> RepairPublication {
    RepairPublication::from_authenticated_record(&recorded(f), "3.2.0", "123/456", e).unwrap()
}
fn assert_receipt_held(path: &std::path::Path) {
    #[cfg(windows)]
    {
        assert!(fs::write(path, b"must remain locked").is_err());
        assert!(fs::remove_file(path).is_err());
    }
    #[cfg(not(windows))]
    let _ = path;
}
fn receipt_handle(file: &File) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        file.as_raw_handle() as usize
    }
    #[cfg(unix)]
    {
        use std::os::fd::AsRawFd;
        file.as_raw_fd() as usize
    }
}

#[test]
fn repair_capability_only_admits_exact_unbind_without_invented_wire_restrictions() {
    let identity =
        RecordedInstallationIdentity::from_authenticated_record(std::env::temp_dir()).unwrap();
    for action in [
        "bind", "prepare", "commit", "confirm", "release", "", "execute",
    ] {
        let e = SlotEnvelope {
            action: action.into(),
            ..envelope()
        };
        code(
            RepairPublication::from_authenticated_record(&identity, "3.2.0", "123/456", &e),
            "delivery.invalid_installation",
        );
    }
    for (release, consumer) in [("", "123/456"), ("3.2.0", "")] {
        code(
            RepairPublication::from_authenticated_record(&identity, release, consumer, &unbind()),
            "delivery.invalid_installation",
        );
    }
    let mut e = unbind();
    e.nonce = "bad".into();
    code(
        RepairPublication::from_authenticated_record(&identity, "3.2.0", "123/456", &e),
        "bridge.invalid_slot_token",
    );
    // Shared envelope admission does not forbid ignored unbind fields or
    // demand a newly invented consumer/version format. Preserve them exactly.
    e = unbind();
    e.code = "-- ignored for unbind".into();
    e.budget = 7;
    e.guid = "original-guid".into();
    let cap = RepairPublication::from_authenticated_record(
        &identity,
        "original-release",
        "original-consumer",
        &e,
    )
    .unwrap();
    assert_eq!(cap.envelope(), &e);
    assert_eq!(cap.consumer(), "original-consumer");
    e.nonce = "5".repeat(32);
    e.action = "commit".into();
    assert_eq!(cap.envelope().action, "unbind");
    assert_ne!(
        cap.envelope().nonce,
        e.nonce,
        "no mutable alias into the capability"
    );
}

#[test]
fn disk_repair_unbind_survives_missing_addon_but_ordinary_publication_stays_closed() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = unbind();
    let cap = repair(&f, &e);
    fs::remove_dir_all(&f.identity.canonical_path).unwrap();
    let mut lease = PublicationLease::acquire_recorded(&recorded(&f), "3.2.0", &c).unwrap();
    code(
        lease.publish("123/456", &e),
        "delivery.installation_not_clean",
    );
    code(
        lease.verify("123/456", &e),
        "delivery.installation_not_clean",
    );
    lease.publish_repair(&cap).unwrap();
    lease.verify_repair(&cap).unwrap();
    lease.verify_repair_route(&cap).unwrap();
    assert!(lease.store._receipt_file.is_none());
    assert_eq!(
        fs::read(slots::directory(&f.root, 1).join("Payload.lua")).unwrap(),
        payload(&e).unwrap()
    );
    assert!(InstallationMaintenanceLease::acquire(&f.identity.canonical_path).is_err());
    let row = lease.inspect().unwrap().files[0].clone();
    assert_eq!(row.consumer, "123/456");
    assert_eq!(row.runtime, e.runtime);
    assert_eq!(row.nonce, e.nonce);
    lease.close().unwrap();
    assert_disk_locks_available(&f);
    let mut resumed = PublicationLease::acquire_recorded(&recorded(&f), "3.2.0", &c).unwrap();
    resumed.verify_repair(&cap).unwrap();
    resumed.confirm_consumed("123/456", &e.expected()).unwrap();
    code(
        resumed.verify_repair(&cap),
        "delivery.installation_conflict",
    );
    resumed.close().unwrap();
}

#[test]
fn disk_repair_modified_addon_uses_original_release_and_exact_consumer() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let original_release = "3.1.9";
    // This fixture models a valid recorded older pool; no older release is an
    // oracle for the current behavior.
    for index in 1..=slots::COUNT {
        for (name, bytes) in slots::files(index, original_release) {
            fs::write(slots::directory(&f.root, index).join(name), bytes).unwrap();
        }
    }
    let mut pool = empty_pool();
    pool.version = original_release.into();
    fs::write(
        f.root.join(slots::MARKER),
        serde_json::to_vec(&pool).unwrap(),
    )
    .unwrap();
    fs::write(
        f.identity.canonical_path.join("Core.lua"),
        b"modified addon",
    )
    .unwrap();
    fs::write(
        f.identity.canonical_path.join(installation::MARKER),
        b"modified receipt",
    )
    .unwrap();
    let e = unbind();
    let cap = RepairPublication::from_authenticated_record(
        &recorded(&f),
        original_release,
        "123/456",
        &e,
    )
    .unwrap();
    let mut lease =
        PublicationLease::acquire_recorded(&recorded(&f), original_release, &c).unwrap();
    lease.publish_repair(&cap).unwrap();
    lease.verify_repair(&cap).unwrap();
    code(
        verify(&mut lease.store, &c, "123/999", &e),
        "delivery.installation_conflict",
    );
    let wrong_release = repair(&f, &e);
    code(
        lease.publish_repair(&wrong_release),
        "delivery.installation_conflict",
    );
    code(
        lease.verify_repair(&wrong_release),
        "delivery.installation_conflict",
    );
    code(
        lease.verify_repair_route(&wrong_release),
        "delivery.installation_conflict",
    );
    lease.close().unwrap();
    code(
        PublicationLease::acquire_recorded(&recorded(&f), "3.2.0", &c)
            .map(|lease| lease.close().unwrap()),
        "delivery.invalid_installation",
    );
}

#[test]
fn disk_repair_rejects_other_target_and_preserves_foreign_reserved_metadata() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    lease.publish("other-instance", &envelope()).unwrap();
    let mut e = unbind();
    e.nonce = "9".repeat(32);
    let wrong = RecordedInstallationIdentity::from_authenticated_record(f.root.join("Other addon"))
        .unwrap();
    let wrong_cap =
        RepairPublication::from_authenticated_record(&wrong, "3.2.0", "123/456", &e).unwrap();
    code(
        lease.publish_repair(&wrong_cap),
        "delivery.installation_conflict",
    );
    code(
        lease.verify_repair(&wrong_cap),
        "delivery.installation_conflict",
    );
    code(
        lease.verify_repair_route(&wrong_cap),
        "delivery.installation_conflict",
    );
    let primary = lease.publish_repair(&repair(&f, &e)).unwrap_err();
    assert!(primary.is_code("delivery.slot_reserved"));
    assert!(primary.is_code("delivery.installation_conflict"));
    let PublicationError::Reserved { reservation } = &primary else {
        panic!("captured reservation")
    };
    assert_eq!(reservation.consumer, "other-instance");
    assert_eq!(reservation.nonce, envelope().nonce);
    // Operation and checked-close results remain separate: a cleanup failure
    // cannot erase or re-read the exact Reserved metadata. Native joins these.
    let close = lease
        .close_with(
            |file, control| storage::finish_file(file, Ok(()), control),
            |lock| {
                lock.close()?;
                Err(ToolkitError::new(
                    "fixture.close",
                    "model after real close",
                    5,
                ))
            },
            InstallationMaintenanceLease::close,
        )
        .unwrap_err();
    assert!(close.is_code("fixture.close"));
    assert!(primary.is_code("delivery.slot_reserved"));
    assert_eq!(reservation.consumer, "other-instance");
    assert_disk_locks_available(&f);
}

#[test]
fn disk_repair_rechecks_actual_pool_payload_route_and_original_control() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = SlotEnvelope {
        index: 2,
        start_slot: 1,
        ..unbind()
    };
    let cap = repair(&f, &e);
    let mut lease = PublicationLease::acquire_recorded(&recorded(&f), "3.2.0", &c).unwrap();
    lease.publish_repair(&cap).unwrap();
    lease.verify_repair(&cap).unwrap();
    lease.verify_repair_route(&cap).unwrap();
    // A same-runtime earlier envelope blocks the route even with exact hashes.
    let prior = envelope();
    publish(&mut lease.store, &c, "123/456", &prior).unwrap();
    code(
        lease.verify_repair_route(&cap),
        "delivery.installation_conflict",
    );
    let payload_path = slots::directory(&f.root, 2).join("Payload.lua");
    fs::write(&payload_path, b"corrupt payload").unwrap();
    code(lease.verify_repair(&cap), "delivery.invalid_installation");
    fs::write(&payload_path, payload(&e).unwrap()).unwrap();
    let loader = slots::directory(&f.root, 500).join("Loader.lua");
    let original_loader = fs::read(&loader).unwrap();
    fs::write(&loader, b"modified loader").unwrap();
    code(lease.publish_repair(&cap), "delivery.invalid_installation");
    fs::write(loader, original_loader).unwrap();
    c.tracked_cancel_handle().unwrap().cancel();
    code(lease.publish_repair(&cap), "command.cancelled");
    code(lease.verify_repair(&cap), "command.cancelled");
    code(lease.verify_repair_route(&cap), "command.cancelled");
    lease.close().unwrap();
    assert_disk_locks_available(&f);
}

#[test]
fn receipt_success_retains_original_handle_through_revalidation_and_checked_close() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let path = f.identity.canonical_path.join(installation::MARKER);
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    let mut finishes = 0;
    lease
        .store
        .verify_addon_with_finish(&c, |file, result| {
            finishes += 1;
            storage::finish_file(file, result, &c)
        })
        .unwrap();
    assert_eq!(finishes, 0, "first success transfers its live owner");
    let original = receipt_handle(lease.store._receipt_file.as_ref().unwrap());
    assert_receipt_held(&path);
    for _ in 0..2 {
        lease
            .store
            .verify_addon_with_finish(&c, |file, result| {
                finishes += 1;
                assert_ne!(receipt_handle(&file), original);
                assert_receipt_held(&path);
                storage::finish_file(file, result, &c)
            })
            .unwrap();
        assert_eq!(
            receipt_handle(lease.store._receipt_file.as_ref().unwrap()),
            original
        );
        assert_receipt_held(&path);
    }
    assert_eq!(finishes, 2);
    let mut retained_finishes = 0;
    lease
        .close_with(
            |file, control| {
                retained_finishes += 1;
                assert_eq!(receipt_handle(&file), original);
                assert_receipt_held(&path);
                storage::finish_file(file, Ok(()), control)
            },
            ExclusiveLock::close,
            InstallationMaintenanceLease::close,
        )
        .unwrap();
    assert_eq!(retained_finishes, 1);
    fs::write(path, b"released after burst").unwrap();
    assert_disk_locks_available(&f);
}

#[test]
fn receipt_validation_failures_finish_once_without_retaining_or_hiding_errors() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let path = f.identity.canonical_path.join(installation::MARKER);
    let original = fs::read(&path).unwrap();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    for failure in [
        "digest",
        "json",
        "schema",
        "component",
        "version",
        "commit",
        "inventory",
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        match failure {
            "schema" | "component" | "version" | "commit" => {
                value[failure] = serde_json::json!("wrong")
            }
            "inventory" => value["resources"][0]["sha256"] = serde_json::json!("0".repeat(64)),
            _ => (),
        }
        let bytes = if failure == "json" {
            b"{".to_vec()
        } else {
            serde_json::to_vec(&value).unwrap()
        };
        fs::write(&path, &bytes).unwrap();
        lease.store.manifest_sha256 = Some(if failure == "digest" {
            "0".repeat(64)
        } else {
            storage::sha256(&bytes)
        });
        let mut finishes = 0;
        let error = lease
            .store
            .verify_addon_with_finish(&c, |file, result| {
                finishes += 1;
                assert!(result.is_err());
                assert_receipt_held(&path);
                storage::finish_file(file, result, &c)
            })
            .unwrap_err();
        assert_eq!(finishes, 1, "{failure}");
        assert!(lease.store._receipt_file.is_none());
        assert!(
            matches!(
                error.code.as_str(),
                "delivery.invalid_installation" | "delivery.invalid_payload"
            ),
            "{failure}: {error:?}"
        );
        fs::write(&path, &original).unwrap();
    }
    lease.close().unwrap();
}

#[test]
fn receipt_opened_read_error_and_control_stop_are_checked_before_return() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let path = f.identity.canonical_path.join(installation::MARKER);
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    let digest = f.identity.manifest_sha256.clone();
    let mut finishes = 0;
    // Real File, real read error: seek succeeds on a valid write-only file.
    let write_only = fs::OpenOptions::new().write(true).open(&path).unwrap();
    let error = lease
        .store
        .verify_opened_receipt(write_only, &digest, &c, |file, result| {
            finishes += 1;
            assert_eq!(result.as_ref().unwrap_err().code, "external.io");
            storage::finish_file(file, result, &c)
        })
        .unwrap_err();
    assert_eq!(error.code, "external.io");
    assert_eq!(finishes, 1);
    let (file, digest) = storage::locked_sha256(&path, RECEIPT_LIMIT as u64, &c).unwrap();
    c.tracked_cancel_handle().unwrap().cancel();
    let stopped = lease
        .store
        .verify_opened_receipt(file, &digest, &c, |file, result| {
            finishes += 1;
            storage::finish_file(file, result, &c)
        })
        .unwrap_err();
    assert_eq!(stopped.code, "command.cancelled");
    assert_eq!(finishes, 2);
    assert!(lease.store._receipt_file.is_none());
    lease.close().unwrap();
    fs::write(path, b"released").unwrap();
}

struct ReceiptReadFault {
    cursor: std::io::Cursor<Vec<u8>>,
    seek_fault: bool,
    read_fault: bool,
}
impl Read for ReceiptReadFault {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.read_fault {
            Err(std::io::Error::other("model read fault"))
        } else {
            self.cursor.read(bytes)
        }
    }
}
impl Seek for ReceiptReadFault {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        if self.seek_fault {
            Err(std::io::Error::other("model seek fault"))
        } else {
            self.cursor.seek(position)
        }
    }
}
#[test]
fn receipt_seek_read_size_and_second_digest_validation_preserve_original_checks() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    let bytes = fs::read(f.identity.canonical_path.join(installation::MARKER)).unwrap();
    for failure in ["seek", "read", "size", "second_digest"] {
        let mut raw = bytes.clone();
        if failure == "size" {
            raw = vec![b' '; RECEIPT_LIMIT + 1];
        }
        if failure == "second_digest" {
            raw.push(b' ');
        }
        let mut reader = ReceiptReadFault {
            cursor: std::io::Cursor::new(raw),
            seek_fault: failure == "seek",
            read_fault: failure == "read",
        };
        let error = lease
            .store
            .validate_receipt(&mut reader, &f.identity.manifest_sha256, &c)
            .unwrap_err();
        assert_eq!(
            error.code,
            if matches!(failure, "seek" | "read") {
                "external.io"
            } else {
                "delivery.invalid_installation"
            }
        );
    }
    lease.close().unwrap();
}

#[test]
fn receipt_revalidation_finish_failure_retains_original_owner_until_burst_close() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let path = f.identity.canonical_path.join(installation::MARKER);
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    lease.publish("123/456", &envelope()).unwrap();
    let original = receipt_handle(lease.store._receipt_file.as_ref().unwrap());
    let injected = ToolkitError::new(
        "fixture.checked_close",
        "modeled close result, not kernel failure",
        5,
    );
    let error = lease
        .store
        .verify_addon_with_finish(&c, |file, result| {
            storage::finish_file(file, result, &c)?;
            Err(injected.clone())
        })
        .unwrap_err();
    assert_eq!(
        serde_json::to_value(error).unwrap(),
        serde_json::to_value(&injected).unwrap()
    );
    assert_eq!(
        receipt_handle(lease.store._receipt_file.as_ref().unwrap()),
        original
    );
    assert_receipt_held(&path);
    lease.close().unwrap();
    fs::write(path, b"released").unwrap();
}

#[test]
fn receipt_failure_preserves_complete_nested_primary_cleanup_and_control_error() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    lease.store.manifest_sha256 = Some("0".repeat(64));
    let mut expected = None;
    let actual = lease
        .store
        .verify_addon_with_finish(&c, |file, result| {
            let primary = result.unwrap_err();
            storage::finish_file(file, Ok(()), &c)?;
            // Injection at the checked-finish seam, after real successful close.
            // Core separately tests the OS-close adapter's structured combiner.
            let cleanup = ToolkitError::new("fixture.close", "modeled OS-close outcome", 5);
            let mut joined = primary.clone();
            joined.details = serde_json::json!({
                "schema": storage::FILE_CLEANUP_SCHEMA,
                "primary": primary,
                "cleanup": cleanup,
                "control": ToolkitError::new("command.cancelled", "model late stop", 7),
            });
            expected = Some(serde_json::to_value(&joined).unwrap());
            Err(joined)
        })
        .unwrap_err();
    assert_eq!(Some(serde_json::to_value(actual).unwrap()), expected);
    assert!(lease.store._receipt_file.is_none());
    lease.close().unwrap();
}

#[test]
fn receipt_and_both_lock_close_faults_are_ordered_consumed_once_and_never_short_circuit() {
    let f = DiskFixture::new();
    for mask in 0u8..8 {
        let c = OperationControl::default();
        let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
        lease.store.verify_addon(&c).unwrap();
        let events = RefCell::new(Vec::new());
        let injected = |bit: u8, label: &str| {
            if mask & bit != 0 {
                Err(ToolkitError::new(
                    format!("fixture.{label}"),
                    "model after real checked close",
                    5,
                ))
            } else {
                Ok(())
            }
        };
        let result = lease.close_with(
            |file, control| {
                events.borrow_mut().push("receipt");
                storage::finish_file(file, Ok(()), control)?;
                injected(1, "receipt")
            },
            |lock| {
                events.borrow_mut().push("publication");
                lock.close()?;
                injected(2, "publication")
            },
            |lock| {
                events.borrow_mut().push("maintenance");
                lock.close()?;
                injected(4, "maintenance")
            },
        );
        assert_eq!(*events.borrow(), ["receipt", "publication", "maintenance"]);
        match result {
            Ok(()) => assert_eq!(mask, 0),
            Err(error) => {
                for (bit, label) in [(1, "receipt"), (2, "publication"), (4, "maintenance")] {
                    assert_eq!(error.is_code(&format!("fixture.{label}")), mask & bit != 0);
                }
                let causes = match &error {
                    PublicationError::Failure(e) => vec![e],
                    PublicationError::CloseFailures(es) => es.iter().collect(),
                    _ => panic!("close cannot produce Reserved"),
                };
                let expected: Vec<_> = [(1, "receipt"), (2, "publication"), (4, "maintenance")]
                    .into_iter()
                    .filter(|(bit, _)| mask & bit != 0)
                    .map(|(_, label)| format!("fixture.{label}"))
                    .collect();
                assert_eq!(
                    causes.iter().map(|e| e.code.clone()).collect::<Vec<_>>(),
                    expected
                );
            }
        }
        assert_disk_locks_available(&f);
    }
}

#[test]
fn cancelled_receipt_close_still_releases_both_locks_and_preserves_control() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    lease.store.verify_addon(&c).unwrap();
    c.tracked_cancel_handle().unwrap().cancel();
    code(lease.close(), "command.cancelled");
    assert_disk_locks_available(&f);
    fs::write(
        f.identity.canonical_path.join(installation::MARKER),
        b"released",
    )
    .unwrap();
}

#[test]
fn repair_finalization_capability_rejects_business_and_copies_exact_envelope() {
    let identity =
        RecordedInstallationIdentity::from_authenticated_record(std::env::temp_dir()).unwrap();
    for action in ["bind", "prepare", "commit", "unbind", "", "execute"] {
        let e = SlotEnvelope {
            action: action.into(),
            ..envelope()
        };
        assert!(
            RepairPublication::from_authenticated_finalization(&identity, "3.2.0", "123/456", &e)
                .is_err()
        );
    }
    for action in ["confirm", "release"] {
        let e = SlotEnvelope {
            action: action.into(),
            ..envelope()
        };
        let cap =
            RepairPublication::from_authenticated_finalization(&identity, "3.2.0", "123/456", &e)
                .unwrap();
        assert_eq!(cap.envelope(), &e);
        for (release, consumer) in [("", "123/456"), ("3.2.0", "")] {
            assert!(
                RepairPublication::from_authenticated_finalization(
                    &identity, release, consumer, &e
                )
                .is_err()
            );
        }
    }
}

#[test]
fn recorded_cleanup_shares_current_installation_maintenance_lock() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = envelope();
    let recorded =
        RecordedInstallationIdentity::from_authenticated_record(f.identity.canonical_path.clone())
            .unwrap();
    let mut publication = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    publication.publish("123/456", &e).unwrap();
    publication.close().unwrap();
    let mut cleanup = PublicationLease::acquire_recorded(&recorded, "3.2.0", &c).unwrap();
    assert_eq!(
        InstallationMaintenanceLease::acquire(&f.identity.canonical_path)
            .err()
            .unwrap()
            .code,
        "workspace.busy"
    );
    cleanup.retire_process("123/456", &e).unwrap();
    assert!(cleanup.inspect().unwrap().files[0].retired_process);
    cleanup.close().unwrap();
    InstallationMaintenanceLease::acquire(&f.identity.canonical_path)
        .unwrap()
        .close()
        .unwrap();
}
