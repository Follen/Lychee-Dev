//! Fixed Go scenario mapping (all runtime execution is NOT_RUN until CI):
//! slots_test.go: TestSlotPoolInstallPublicationAndIsolation,
//! TestSlotPoolIncompleteInstallAndPendingPublication,
//! TestEndedProcessRetirementReconcilesTornPublication,
//! TestRuntimeRetirementNeverClaimsConsumptionOrTouchesAnotherProcess.
//! slot_route_test.go: TestForwardAllocationIsBoundedAndPreservesUnknown,
//! TestRouteChecksActualForeignAndEmptyBytes.
//! slot_runtime_retirement_test.go: TestRuntimeRetirementReconcilesTornPublication,
//! TestRuntimeRetirementRejectsCorruptPayload.
//! Additional faults cover every before/after durable-write boundary, cancellation,
//! exact field/digest correlation, stale acknowledgements, and lease ownership.
use super::*;
use crate::installation::Resource;
use lycheedev_core::coordination::InstallationGuard;
use std::{
    fs,
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
fn reserved_preserves_both_go_causes_and_exact_captured_owner() {
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
    // Go checks the existing reservation before validating malformed tokens.
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
                e.code = "x".repeat(262145);
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
fn lower_level_confirmation_keeps_go_nonce_contract_after_durable_receipt() {
    let mut store = MemoryStore::new();
    let c = OperationControl::default();
    let e = envelope();
    publish(&mut store, &c, "a", &e).unwrap();
    // The exact receipt has already been verified by the caller. Delivery does
    // not silently add a second schema/owner/fence gate absent in Go.
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
fn retirement_prechecks_and_lower_level_successor_length_match_go() {
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
fn route_range_defaults_and_schema_gate_match_go() {
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
    let result = wait_lock(&c, || {
        attempts += 1;
        Ok(())
    });
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert_eq!(attempts, 0);
    assert_eq!(store.reads, 0);
    assert!(store.writes.is_empty());
}
#[test]
fn lock_wait_keeps_original_deadline_and_checks_success_after_cancel() {
    let c = OperationControl::uncancelable(Instant::now() + Duration::from_millis(5));
    let mut attempts = 0;
    let result: lycheedev_core::Result<()> = wait_lock(&c, || {
        attempts += 1;
        Err(ToolkitError::new("workspace.busy", "held", 6))
    });
    assert_eq!(result.unwrap_err().code, "command.deadline");
    assert!(
        attempts <= 2,
        "an already-expired budget may admit no attempt"
    );
    let c = OperationControl::default();
    let result = wait_lock(&c, || {
        c.tracked_cancel_handle().unwrap().cancel();
        Ok(())
    });
    assert_eq!(result.unwrap_err().code, "command.cancelled");
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct DiskFixture {
    root: PathBuf,
    identity: VerifiedInstallation,
}
impl DiskFixture {
    fn new() -> Self {
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
        // Direct fixture setup is not a test of install or durable writes.
        for index in 1..=slots::COUNT {
            let path = slots::directory(&root, index);
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
        Self {
            identity: VerifiedInstallation {
                canonical_path: addon,
                manifest_sha256: storage::sha256(&raw),
                generation: 1,
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
#[test]
fn disk_publication_roundtrip_holds_both_locks_and_releases_before_receipt_wait() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = envelope();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    assert!(InstallationGuard::acquire(&f.identity.canonical_path).is_err());
    assert!(ExclusiveLock::resource(&f.root.join(".lycheedev-slot-locks"), "publication").is_err());
    lease.publish("123/456", &e).unwrap();
    lease.verify("123/456", &e).unwrap();
    lease.verify_route(&e).unwrap();
    assert_eq!(
        fs::read(slots::directory(&f.root, 1).join("Payload.lua")).unwrap(),
        mailbox::slot_payload(&e).unwrap()
    );
    lease.close().unwrap(); // receipt wait begins only after checked lease close
    let gate = InstallationGuard::acquire(&f.identity.canonical_path).unwrap();
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
    assert!(InstallationGuard::acquire(&f.identity.canonical_path).is_ok());
    let loader = slots::directory(&f.root, 500).join("Loader.lua");
    let original = fs::read(&loader).unwrap();
    fs::write(&loader, b"-- changed").unwrap();
    assert!(
        PublicationLease::acquire(&f.identity, "3.2.0", &c)
            .err()
            .unwrap()
            .is_code("delivery.invalid_installation")
    );
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
}

#[test]
fn disk_cleanup_uses_recorded_path_and_pool_even_when_addon_or_receipt_changed() {
    // Go Consumed/native_windows.go and RetireSlotProcess/process_end_windows.go
    // do not call addon Guard or read its installation receipt during cleanup.
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
    assert!(!slots::read_pool(&f.root).unwrap().files[0].consumed);
}

#[test]
fn disk_formal_cleanup_does_not_load_legacy_claims_but_shares_maintenance_lock() {
    let f = DiskFixture::new();
    let c = OperationControl::default();
    let e = envelope();
    let recorded =
        RecordedInstallationIdentity::from_authenticated_record(f.identity.canonical_path.clone())
            .unwrap();
    let mut lease = PublicationLease::acquire(&f.identity, "3.2.0", &c).unwrap();
    lease.publish("123/456", &e).unwrap();
    drop(lease);
    fs::write(
        f.root.join(".lycheedev-claims.json"),
        b"broken historical registry",
    )
    .unwrap();
    let mut cleanup = PublicationLease::acquire_recorded(&recorded, "3.2.0", &c).unwrap();
    assert_eq!(
        InstallationGuard::acquire(&f.identity.canonical_path)
            .err()
            .unwrap()
            .code,
        "workspace.busy"
    );
    cleanup.retire_process("123/456", &e).unwrap();
    assert!(cleanup.inspect().unwrap().files[0].retired_process);
    drop(cleanup);
    // The legacy API retains its former registry-validation behavior.
    assert_eq!(
        InstallationGuard::acquire(&f.identity.canonical_path)
            .err()
            .unwrap()
            .code,
        "input.invalid"
    );
    assert_eq!(
        fs::read(f.root.join(".lycheedev-claims.json")).unwrap(),
        b"broken historical registry"
    );
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
