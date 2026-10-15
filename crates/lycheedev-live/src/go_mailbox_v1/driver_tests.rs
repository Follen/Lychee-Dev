//! In-memory orchestration fixtures only. These do not prove filesystem flush,
//! publication leases, native key dispatch, Lua traversal or runtime replacement.
use super::super::observations::{InputAction, InputDispatch, ObservationDispatch};
use super::super::state::{Blocker, InputKind, TransactionPhase};
use super::super::transactions::{ReleaseRecoveryDecision, missing_release_decision};
use super::super::{
    InputBindings, InputDiagnostic, InputDisposition, InputObservation, InputOutcome, Receipt,
};
use super::*;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
    sync::atomic::Ordering,
};

type Trace = Rc<RefCell<Vec<String>>>;
#[derive(Clone)]
struct FakeClock(Rc<Cell<i64>>);
impl Clock for FakeClock {
    fn unix_millis(&self) -> i64 {
        self.0.get()
    }
}
struct Tokens(u64);
impl TokenSource for Tokens {
    fn token(&mut self) -> ChannelResult<String> {
        self.0 += 1;
        Ok(format!("{:032x}", self.0))
    }
}

struct FakeJournal {
    trace: Trace,
    events: Vec<(String, State)>,
    results: Vec<Operation>,
    fail_event: Option<&'static str>,
    fail_after_append: bool,
    fail_as_deadline: bool,
    fail_result: bool,
    fail_history: bool,
    known_zero_checkpoints: usize,
}
impl StateJournal for FakeJournal {
    fn append(
        &mut self,
        control: &OperationControl,
        event: &str,
        state: &State,
    ) -> ChannelResult<()> {
        check_control(control)?;
        self.trace.borrow_mut().push(format!("save:{event}"));
        if self.fail_event == Some(event) && !self.fail_after_append {
            return Err(if self.fail_as_deadline {
                ChannelError::DeadlineExceeded
            } else {
                error("fixture.journal_write_failed")
            });
        }
        // Model a journal reload, including the deliberate loss of host-uptime
        // freshness from saved observations. Raw result bytes remain identical.
        let saved = serde_json::from_slice(&serde_json::to_vec(state).unwrap()).unwrap();
        self.events.push((event.into(), saved));
        if self.fail_event == Some(event) {
            return Err(if self.fail_as_deadline {
                ChannelError::DeadlineExceeded
            } else {
                error("fixture.journal_write_failed")
            });
        }
        Ok(())
    }
    fn historical_request(
        &mut self,
        control: &OperationControl,
        request: &str,
    ) -> ChannelResult<Option<State>> {
        check_control(control)?;
        if self.fail_history {
            return Err(error("journal.memory_log_corrupt"));
        }
        Ok(self
            .events
            .iter()
            .rev()
            .find(|(_, s)| s.operation.as_ref().is_some_and(|o| o.request == request))
            .map(|(_, s)| s.clone()))
    }
    fn write_result(
        &mut self,
        control: &OperationControl,
        operation: &Operation,
    ) -> ChannelResult<()> {
        check_control(control)?;
        self.trace.borrow_mut().push("write_result".into());
        if self.fail_result {
            return Err(error("fixture.result_write_failed"));
        }
        self.results.push(operation.clone());
        Ok(())
    }
    fn checkpoint(
        &mut self,
        control: &OperationControl,
        state: &mut State,
        known_zero: bool,
    ) -> ChannelResult<()> {
        check_control(control)?;
        state.validate()?;
        self.trace
            .borrow_mut()
            .push(format!("checkpoint:{known_zero}"));
        if known_zero {
            self.known_zero_checkpoints += 1;
        }
        Ok(())
    }
}

struct FakeBackend {
    trace: Trace,
    identity: Identity,
    published: Vec<SlotEnvelope>,
    inputs: Vec<InputAction>,
    observations: usize,
    input_reads: usize,
    consumed: Vec<SlotEnvelope>,
    receipts: BTreeMap<String, Receipt>,
    pending_actions: BTreeSet<String>,
    publish_errors: VecDeque<ChannelError>,
    consume_error: Option<ChannelError>,
    observe_error: Option<ChannelError>,
    input_error: Option<ChannelError>,
    input_outcomes: VecDeque<InputOutcome>,
    cancel_in_input: bool,
    diagnostic: Option<InputDiagnostic>,
    receipt_mutation: Option<fn(&mut Receipt)>,
    result_mutation: Option<fn(&mut Receipt, &mut Vec<u8>)>,
    input_blocked: Option<String>,
    sample_override: Option<(i64, i64)>,
    uptime: i64,
    prepared_nonce: String,
    prepare_reported: bool,
    result: Vec<u8>,
}
impl FakeBackend {
    fn receipt(&self, e: &SlotEnvelope, state: &str) -> Receipt {
        let mut identity = self.identity.clone();
        identity.runtime = e.runtime.clone();
        identity.owner = e.owner.clone();
        identity.fence = e.fence;
        identity.guid = e.guid.clone();
        identity.build = e.build.clone();
        identity.next_slot = e.index + 1;
        Receipt {
            identity,
            nonce: e.nonce.clone(),
            ticket: e.ticket.clone(),
            action: e.action.clone(),
            state: state.into(),
            challenge: "c".repeat(32),
            prepared_nonce: self.prepared_nonce.clone(),
            report_bytes: self.result.len() as u32,
            report_checksum: checksum(&self.result),
            ..Receipt::default()
        }
    }
}
impl Backend for FakeBackend {
    fn observe(&mut self, control: &OperationControl, q: &ObservationQuery) -> ObservationDispatch {
        if let Err(err) = check_control(control) {
            return Err(err).into();
        }
        self.observations += 1;
        self.trace
            .borrow_mut()
            .push(format!("observe:{:?}:{}", q.kind, q.envelope.action));
        let observation = if q.kind == ObservationKind::Result {
            let mut receipt = self.receipt(&q.envelope, "reported");
            receipt.action = "prepare".into();
            receipt.identity.next_slot = self.identity.next_slot;
            let mut payload = self.result.clone();
            if let Some(edit) = self.result_mutation {
                edit(&mut receipt, &mut payload);
            }
            Observation::Reported { receipt, payload }
        } else if self.pending_actions.contains(&q.envelope.action) {
            Observation::Pending {
                diagnostic: self.diagnostic.clone(),
            }
        } else if let Some(receipt) = self.receipts.get(&q.envelope.nonce) {
            let mut receipt = receipt.clone();
            if let Some(edit) = self.receipt_mutation {
                edit(&mut receipt);
            }
            if receipt.state == "rejected" {
                Observation::Rejected { receipt }
            } else {
                Observation::Receipt(receipt)
            }
        } else {
            Observation::Pending {
                diagnostic: self.diagnostic.clone(),
            }
        };
        ObservationDispatch {
            observation: Some(observation),
            error: self.observe_error.clone(),
        }
    }
    fn publish(&mut self, control: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        check_control(control)?;
        self.trace
            .borrow_mut()
            .push(format!("publish:{}:{}", e.action, e.index));
        if let Some(err) = self.publish_errors.pop_front() {
            return Err(err);
        }
        e.validate()?;
        self.published.push(e.clone());
        Ok(())
    }
    fn observe_input(
        &mut self,
        control: &OperationControl,
        e: &SlotEnvelope,
        after: i64,
        _capability: &str,
    ) -> ChannelResult<InputObservation> {
        check_control(control)?;
        self.input_reads += 1;
        self.uptime += 100;
        let (sample, now) = self.sample_override.unwrap_or((self.uptime, self.uptime));
        if i128::from(sample) < i128::from(after) + 100 || sample < now - 500 {
            return Err(ChannelError::Pending {
                causes: vec!["live.channel_input_observation_stale"],
            });
        }
        Ok(InputObservation {
            bindings: Some(InputBindings {
                wake: "ALT-CTRL-F12".into(),
                reload: "ALT-CTRL-F11".into(),
                ..InputBindings::default()
            }),
            schema: SCHEMA.into(),
            runtime: e.runtime.clone(),
            owner: e.owner.clone(),
            fence: e.fence,
            next_slot: e.slot_start(),
            guid: e.guid.clone(),
            build: e.build.clone(),
            sample_millis: sample,
            input_blocked: Some(self.input_blocked.is_some()),
            reason: self.input_blocked.clone().unwrap_or_default(),
            escape_limit: 0,
            input_attempt: None,
            observed_at_millis: now,
        })
    }
    fn input(&mut self, control: &OperationControl, a: &InputAction) -> InputDispatch {
        self.trace
            .borrow_mut()
            .push(format!("input:{}", a.envelope.action));
        self.inputs.push(a.clone());
        let outcome = self.input_outcomes.pop_front().unwrap_or(InputOutcome {
            disposition: InputDisposition::Submitted,
            messages_queued: 4,
            at_millis: self.uptime,
            reason: String::new(),
            retryable: false,
        });
        if outcome.disposition != InputDisposition::NotSent {
            if a.envelope.action == "prepare" {
                self.prepared_nonce = a.envelope.nonce.clone();
            }
            let state = match a.envelope.action.as_str() {
                "bind" => "bound",
                "prepare" if self.prepare_reported => "reported",
                "prepare" => "prepared",
                "commit" => "accepted",
                "confirm" => "reported",
                "release" => "released",
                "unbind" => "unbound",
                _ => panic!("unexpected fixture action"),
            };
            let receipt = self.receipt(&a.envelope, state);
            self.identity = receipt.identity.clone();
            self.receipts.insert(a.envelope.nonce.clone(), receipt);
        }
        if self.cancel_in_input {
            control.cancel_handle().store(true, Ordering::Release);
        }
        InputDispatch {
            outcome,
            error: self.input_error.take(),
        }
    }
    fn runtime_candidate(
        &mut self,
        _control: &OperationControl,
        _identity: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        panic!("B must not discover/rebind a runtime")
    }
    fn supersede(
        &mut self,
        _control: &OperationControl,
        _e: &SlotEnvelope,
        _replacement: &Identity,
    ) -> ChannelResult<()> {
        panic!("B must not retire runtime claims")
    }
    fn consumed(&mut self, control: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        check_control(control)?;
        self.trace
            .borrow_mut()
            .push(format!("consumed:{}", e.action));
        if let Some(err) = &self.consume_error {
            return Err(err.clone());
        }
        self.consumed.push(e.clone());
        Ok(())
    }
}

type Fixture = Driver<FakeBackend, FakeJournal, FakeClock, Tokens>;
fn fixture() -> Fixture {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let identity = Identity {
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        next_slot: 1,
        slots: 500,
        input_state: SCHEMA.into(),
        character: "c".into(),
        realm: "r".into(),
        guid: "g".into(),
        build: "120100".into(),
        product: "retail".into(),
        release: "3.2.0".into(),
        ..Identity::default()
    };
    let backend = FakeBackend {
        trace: trace.clone(),
        identity: identity.clone(),
        published: Vec::new(),
        inputs: Vec::new(),
        observations: 0,
        input_reads: 0,
        consumed: Vec::new(),
        receipts: BTreeMap::new(),
        pending_actions: BTreeSet::new(),
        publish_errors: VecDeque::new(),
        consume_error: None,
        observe_error: None,
        input_error: None,
        input_outcomes: VecDeque::new(),
        cancel_in_input: false,
        diagnostic: None,
        receipt_mutation: None,
        result_mutation: None,
        input_blocked: None,
        sample_override: None,
        uptime: 1000,
        prepared_nonce: String::new(),
        prepare_reported: false,
        result: b"{ \"ok\": true, \"text\": \"<>&\", \"n\": 1e+02 }\n".to_vec(),
    };
    let journal = FakeJournal {
        trace,
        events: Vec::new(),
        results: Vec::new(),
        fail_event: None,
        fail_after_append: false,
        fail_as_deadline: false,
        fail_result: false,
        fail_history: false,
        known_zero_checkpoints: 0,
    };
    Driver::new(
        identity,
        backend,
        journal,
        FakeClock(Rc::new(Cell::new(1_800_000_000_000))),
        Tokens(0),
    )
    .unwrap()
}
fn connected() -> Fixture {
    let mut d = fixture();
    d.connect(&OperationControl::default()).unwrap();
    d.backend.trace.borrow_mut().clear();
    d
}
fn prepared() -> Fixture {
    let mut d = connected();
    d.prepare_request(
        &OperationControl::default(),
        "request-1",
        "return 1",
        5,
        OperationPolicy::Opaque,
    )
    .unwrap();
    d
}
fn reload(mut d: Fixture) -> Fixture {
    let state = d.journal.events.last().unwrap().1.clone();
    d.journal.fail_event = None;
    Driver::from_state(state, d.backend, d.journal, d.clock, d.tokens).unwrap()
}
fn error_is(err: &ChannelError, code: &str) -> bool {
    err.any_cause(&|e| matches!(e, ChannelError::Contract(ContractError(c)) if *c == code))
}

#[test]
fn go_mailbox_v1_driver_full_cycle_exact_order_and_result_bytes() {
    let mut d = prepared();
    let original = d.state.operation.clone().unwrap();
    let ctx = OperationControl::default();
    d.run(&ctx).unwrap();
    let op = d.state.operation.as_ref().unwrap();
    assert_eq!(op.stage, OperationStage::Complete);
    assert_eq!(op.cleanup_method, CleanupMethod::Released);
    assert_eq!(op.result_bytes.0, d.backend.result);
    assert_eq!(op.ticket, original.ticket);
    assert_eq!(op.recovery_budget, original.recovery_budget);
    assert_eq!(
        d.backend
            .published
            .iter()
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["bind", "prepare", "commit", "confirm", "release"]
    );
    assert_eq!(d.state.identity.next_slot, 6);
    let trace = d.backend.trace.borrow();
    let at = |s: &str| trace.iter().position(|v| v == s).unwrap();
    assert!(at("save:operation_created") < at("publish:prepare:2"));
    assert!(at("save:slot_intent") < at("publish:prepare:2"));
    assert!(at("publish:prepare:2") < at("save:slot_published"));
    assert!(at("save:input_intent") < at("input:prepare"));
    assert!(at("input:prepare") < at("save:input_observed"));
    assert!(at("save:receipt_verified") < at("consumed:prepare"));
    assert!(at("save:report_candidate") < at("input:confirm"));
    assert!(at("consumed:confirm") < at("write_result"));
    assert!(at("write_result") < at("save:result_durable"));
    assert!(at("save:result_durable") < at("publish:release:5"));
    drop(trace);
    assert_eq!(d.journal.results.len(), 1);
    assert_eq!(d.journal.results[0].stage, OperationStage::ResultVerified);
    assert_eq!(d.journal.results[0].result_bytes.0, d.backend.result);
    let sends = d.backend.inputs.len();
    let mut d = reload(d);
    d.run(&ctx).unwrap();
    assert_eq!(d.backend.inputs.len(), sends);
    d.disconnect(&ctx).unwrap();
    assert!(!d.state.bound);
    assert_eq!(d.state.identity.next_slot, 7);
    let sends = d.backend.inputs.len();
    d.disconnect(&ctx).unwrap();
    assert_eq!(d.backend.inputs.len(), sends);
}

#[test]
fn go_mailbox_v1_driver_reported_prepare_skips_commit() {
    let mut d = prepared();
    d.backend.prepare_reported = true;
    d.run(&OperationControl::default()).unwrap();
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::Complete
    );
    assert!(!d.backend.published.iter().any(|e| e.action == "commit"));
}

#[test]
fn go_mailbox_v1_driver_failed_intents_never_reach_dependent_effects() {
    for event in [
        "slot_intent",
        "slot_published",
        "input_intent",
        "input_observed",
        "receipt_verified",
        "slot_consumed",
        "report_candidate",
        "result_durable",
    ] {
        let mut d = prepared();
        d.journal.fail_event = Some(event);
        let before = d.backend.inputs.len();
        assert!(d.run(&OperationControl::default()).is_err(), "{event}");
        assert!(d.journal_failed);
        if event == "slot_intent" {
            assert_eq!(d.backend.published.len(), 1);
        }
        if matches!(event, "slot_intent" | "slot_published" | "input_intent") {
            assert_eq!(d.backend.inputs.len(), before);
        }
        if event == "receipt_verified" {
            assert_eq!(d.backend.consumed.len(), 1);
        }
        if matches!(event, "report_candidate" | "result_durable") {
            assert!(
                !d.backend
                    .inputs
                    .iter()
                    .any(|a| a.envelope.action == "release")
            );
        }
        let effects = (
            d.backend.inputs.len(),
            d.backend.published.len(),
            d.backend.consumed.len(),
        );
        assert!(error_is(
            &d.run(&OperationControl::default()).unwrap_err(),
            "live.channel_journal_reload_required"
        ));
        assert_eq!(
            effects,
            (
                d.backend.inputs.len(),
                d.backend.published.len(),
                d.backend.consumed.len()
            )
        );
    }
}

#[test]
fn go_mailbox_v1_driver_unknown_intent_and_uncertain_outcomes_never_replay() {
    for missing in [true, false] {
        let mut d = prepared();
        d.backend.pending_actions.insert("prepare".into());
        if missing {
            d.journal.fail_event = Some("input_observed");
        } else {
            d.backend.input_outcomes.push_back(InputOutcome {
                disposition: InputDisposition::Uncertain,
                messages_queued: 1,
                at_millis: 0,
                reason: "fixture_uncertain".into(),
                retryable: false,
            });
            d.backend.input_error = Some(error("fixture.uncertain"));
        }
        assert!(d.run(&OperationControl::default()).is_err());
        let sends = d.backend.inputs.len();
        let mut d = reload(d);
        let input_id = d.state.input.as_ref().unwrap().id.clone();
        let nonce = d.state.transaction.as_ref().unwrap().envelope.nonce.clone();
        for _ in 0..3 {
            assert!(
                d.run(&OperationControl::default())
                    .unwrap_err()
                    .is_pending()
            );
            assert_eq!(d.backend.inputs.len(), sends);
            assert_eq!(d.state.input.as_ref().unwrap().id, input_id);
            assert_eq!(d.state.transaction.as_ref().unwrap().envelope.nonce, nonce);
        }
        assert_eq!(
            d.state.transaction.as_ref().unwrap().phase,
            TransactionPhase::InputAttempted
        );
    }
}

#[test]
fn go_mailbox_v1_driver_known_zero_retry_keeps_nonce_and_deadline() {
    let mut d = prepared();
    let budget = d.state.operation.as_ref().unwrap().recovery_budget.clone();
    for _ in 0..3 {
        d.backend.input_outcomes.push_back(InputOutcome {
            disposition: InputDisposition::NotSent,
            messages_queued: 0,
            at_millis: 0,
            reason: "physical_key_held".into(),
            retryable: true,
        });
    }
    let mut nonce = None;
    for _ in 0..3 {
        assert!(
            d.run(&OperationControl::default())
                .unwrap_err()
                .is_pending()
        );
        let e = &d.state.transaction.as_ref().unwrap().envelope;
        assert_eq!(nonce.get_or_insert_with(|| e.nonce.clone()), &e.nonce);
        assert_eq!(
            d.state.transaction.as_ref().unwrap().phase,
            TransactionPhase::Published
        );
        assert_eq!(d.state.operation.as_ref().unwrap().recovery_budget, budget);
        d = reload(d);
    }
    assert_eq!(d.journal.known_zero_checkpoints, 2);
    assert_eq!(
        d.backend
            .published
            .iter()
            .filter(|e| e.action == "prepare")
            .count(),
        1
    );
    d.run(&OperationControl::default()).unwrap();
    assert_eq!(d.journal.known_zero_checkpoints, 3);
    assert_eq!(d.state.operation.as_ref().unwrap().recovery_budget, budget);
}

#[test]
fn go_mailbox_v1_driver_cancellation_still_flushes_input_outcome() {
    let mut d = prepared();
    d.backend.cancel_in_input = true;
    let ctx = OperationControl::default();
    assert!(matches!(d.run(&ctx).unwrap_err(), ChannelError::Cancelled));
    let persisted = &d.journal.events.last().unwrap();
    assert_eq!(persisted.0, "input_observed");
    assert_eq!(
        persisted
            .1
            .input
            .as_ref()
            .unwrap()
            .outcome
            .as_ref()
            .unwrap()
            .disposition,
        InputDisposition::Submitted
    );
    let sends = d.backend.inputs.len();
    let mut d = reload(d);
    d.backend.cancel_in_input = false;
    d.backend.pending_actions.insert("prepare".into());
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(d.backend.inputs.len(), sends);
}

#[test]
fn go_mailbox_v1_driver_invalid_transport_outcome_becomes_unknown() {
    let mut d = prepared();
    d.backend.input_outcomes.push_back(InputOutcome {
        disposition: InputDisposition::Submitted,
        messages_queued: 0,
        at_millis: -1,
        reason: String::new(),
        retryable: true,
    });
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_input_outcome_invalid"
    ));
    let outcome = d.state.input.as_ref().unwrap().outcome.as_ref().unwrap();
    assert_eq!(outcome.disposition, InputDisposition::Uncertain);
    assert_eq!(outcome.reason, "invalid_transport_outcome");
    let sends = d.backend.inputs.len();
    let mut d = reload(d);
    d.backend.pending_actions.insert("prepare".into());
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(d.backend.inputs.len(), sends);
}

#[test]
fn go_mailbox_v1_driver_consumption_failure_keeps_exact_received_transaction() {
    let mut d = prepared();
    d.backend.consume_error = Some(error("fixture.consume_failed"));
    assert!(d.run(&OperationControl::default()).is_err());
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::Prepared
    );
    assert_eq!(d.state.identity.next_slot, 2);
    assert_eq!(
        d.state.transaction.as_ref().unwrap().phase,
        TransactionPhase::Received
    );
    let sends = d.backend.inputs.len();
    let mut d = reload(d);
    d.backend.consume_error = None;
    d.run(&OperationControl::default()).unwrap();
    assert_eq!(
        d.backend.inputs.len(),
        sends + 3,
        "prepare must not be sent twice"
    );
}

#[test]
fn go_mailbox_v1_driver_bad_receipt_target_never_consumes() {
    let mutations: [fn(&mut Receipt); 10] = [
        |r| r.nonce = "f".repeat(32),
        |r| r.ticket = "f".repeat(32),
        |r| r.action = "commit".into(),
        |r| r.identity.runtime = "f".repeat(32),
        |r| r.identity.owner = "f".repeat(32),
        |r| r.identity.fence += 1,
        |r| r.identity.guid = "other".into(),
        |r| r.identity.product = "classic".into(),
        |r| r.identity.release = "other".into(),
        |r| r.identity.next_slot = 2,
    ];
    for edit in mutations {
        let mut d = prepared();
        d.backend.receipt_mutation = Some(edit);
        assert!(d.run(&OperationControl::default()).is_err());
        assert_eq!(d.backend.consumed.len(), 1);
        assert!(d.state.transaction.as_ref().unwrap().receipt.is_none());
    }
}

#[test]
fn go_mailbox_v1_driver_report_target_length_checksum_json_are_requalified() {
    let mutations: [fn(&mut Receipt, &mut Vec<u8>); 5] = [
        |r, _| r.identity.product = "classic".into(),
        |r, _| r.report_bytes += 1,
        |r, _| r.report_checksum ^= 1,
        |r, b| {
            *b = b"broken".to_vec();
            r.report_bytes = b.len() as u32;
            r.report_checksum = checksum(b);
        },
        |r, b| {
            b.clear();
            r.report_bytes = 0;
            r.report_checksum = checksum(b);
        },
    ];
    for edit in mutations {
        let mut d = prepared();
        d.backend.result_mutation = Some(edit);
        assert!(d.run(&OperationControl::default()).is_err());
        assert_eq!(
            d.state.operation.as_ref().unwrap().stage,
            OperationStage::Running
        );
        assert!(
            !d.backend
                .published
                .iter()
                .any(|e| e.action == "confirm" || e.action == "release")
        );
    }
}

#[test]
fn go_mailbox_v1_driver_confirmation_must_match_original_report() {
    for field in 0..3 {
        let mut d = prepared();
        d.backend.pending_actions.insert("confirm".into());
        assert!(
            d.run(&OperationControl::default())
                .unwrap_err()
                .is_pending()
        );
        d.backend.pending_actions.clear();
        let nonce = d.state.transaction.as_ref().unwrap().envelope.nonce.clone();
        let receipt = d.backend.receipts.get_mut(&nonce).unwrap();
        match field {
            0 => receipt.prepared_nonce = "f".repeat(32),
            1 => receipt.report_bytes += 1,
            _ => receipt.report_checksum ^= 1,
        }
        assert!(error_is(
            &d.run(&OperationControl::default()).unwrap_err(),
            "live.channel_report_not_current"
        ));
        assert_eq!(
            d.state.operation.as_ref().unwrap().stage,
            OperationStage::ConfirmReady
        );
        assert!(d.journal.results.is_empty());
        assert!(!d.backend.published.iter().any(|e| e.action == "release"));
    }
}

#[test]
fn go_mailbox_v1_driver_result_write_failure_cannot_admit_release() {
    let mut d = prepared();
    d.journal.fail_result = true;
    assert!(d.run(&OperationControl::default()).is_err());
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::ResultVerified
    );
    assert!(!d.backend.published.iter().any(|e| e.action == "release"));
    let sends = d.backend.inputs.len();
    d.journal.fail_result = false;
    d.run(&OperationControl::default()).unwrap();
    assert_eq!(d.backend.inputs.len(), sends + 1);
}

#[test]
fn go_mailbox_v1_driver_reallocation_keeps_nonce_route_and_persists_before_publish() {
    let mut d = fixture();
    d.backend
        .publish_errors
        .push_back(ChannelError::SlotAvailable {
            index: 200,
            blocker: Blocker {
                kind: "slot_reservation".into(),
                slot: 1,
                ..Blocker::default()
            },
        });
    d.connect(&OperationControl::default()).unwrap();
    let e = &d.backend.published[0];
    assert_eq!((e.index, e.start_slot), (200, 1));
    let states: Vec<_> = d
        .journal
        .events
        .iter()
        .filter(|(kind, _)| matches!(kind.as_str(), "slot_intent" | "slot_reallocated"))
        .collect();
    assert_eq!(
        states[0].1.transaction.as_ref().unwrap().envelope.nonce,
        states[1].1.transaction.as_ref().unwrap().envelope.nonce
    );
    assert_eq!(
        d.backend.inputs[0].observation.as_ref().unwrap().next_slot,
        1
    );
    let trace = d.backend.trace.borrow();
    let saved = trace
        .iter()
        .position(|s| s == "save:slot_reallocated")
        .unwrap();
    let published = trace.iter().position(|s| s == "publish:bind:200").unwrap();
    assert!(saved < published);
}

#[test]
fn go_mailbox_v1_driver_reallocation_failure_does_not_write_new_destination() {
    let mut d = fixture();
    d.backend
        .publish_errors
        .push_back(ChannelError::SlotAvailable {
            index: 2,
            blocker: Blocker {
                kind: "slot_reservation".into(),
                slot: 1,
                ..Blocker::default()
            },
        });
    d.journal.fail_event = Some("slot_reallocated");
    assert!(d.connect(&OperationControl::default()).is_err());
    assert!(d.backend.published.is_empty());
    assert!(d.backend.inputs.is_empty());
    let mut d = reload(d);
    assert_eq!(d.state.transaction.as_ref().unwrap().envelope.index, 1);
    d.backend
        .publish_errors
        .push_back(ChannelError::SlotAvailable {
            index: 2,
            blocker: Blocker {
                slot: 1,
                ..Blocker::default()
            },
        });
    d.connect(&OperationControl::default()).unwrap();
    assert_eq!(d.backend.published[0].index, 2);
}

#[test]
fn go_mailbox_v1_driver_capacity_reserve_starts_at_486_and_preserves_request() {
    for next in [485, 486, 500, 501] {
        let mut d = prepared();
        d.state.identity.next_slot = next;
        d.backend.identity.next_slot = next;
        let request = d.state.operation.clone();
        let result = d.run(&OperationControl::default());
        if next == 485 {
            result.unwrap();
        } else {
            assert!(error_is(
                &result.unwrap_err(),
                "live.channel_reload_required"
            ));
            assert_eq!(d.state.operation, request);
            assert!(d.state.transaction.is_none());
            assert_eq!(d.backend.inputs.len(), 1);
        }
    }
}

#[test]
fn go_mailbox_v1_driver_capacity_rejection_carries_original_recovery_budget() {
    let mut d = prepared();
    let original = d.state.operation.clone().unwrap();
    d.backend.receipt_mutation = Some(|r| {
        r.state = "rejected".into();
        r.reason = "slot_capacity".into();
    });
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_reload_required"
    ));
    let reload = d.state.reload.as_ref().unwrap();
    assert_eq!(reload.request, format!("capacity-{}", original.id));
    assert_eq!(reload.recovery_budget, original.recovery_budget);
    assert!(reload.automatic);
    assert!(!reload.reconcile_pending);
    assert_eq!(d.state.operation.as_ref().unwrap().ticket, original.ticket);
    assert!(d.state.transaction.is_none());
    assert_eq!(d.backend.inputs.len(), 2);
}

#[test]
fn go_mailbox_v1_driver_release_timeout_records_one_reload_intent_without_replay() {
    let mut d = prepared();
    d.backend.pending_actions.insert("release".into());
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    let original = d.state.operation.clone().unwrap();
    let tx = d.state.transaction.clone().unwrap();
    let sends = d.backend.inputs.len();
    assert_eq!(original.stage, OperationStage::ReleaseReady);
    assert_eq!(original.cleanup_method, CleanupMethod::None);
    assert_eq!(d.journal.results[0].result_bytes.0, original.result_bytes.0);
    let start = tx.release_wait_started_at_ms;
    assert_eq!(
        missing_release_decision(&d.state, start - 1).unwrap(),
        ReleaseRecoveryDecision::Pending
    );
    d.clock.0.set(start + 4999);
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    d.clock.0.set(start + 5000);
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_progress"
    ));
    let reload = d.state.reload.as_ref().unwrap();
    assert_eq!(reload.recovery_budget, original.recovery_budget);
    assert!(reload.automatic && reload.reconcile_pending);
    assert_eq!(d.state.transaction.as_ref().unwrap().envelope, tx.envelope);
    assert_eq!(
        d.state.operation.as_ref().unwrap().cleanup_method,
        CleanupMethod::None
    );
    let mut d = reload_fixture(d);
    d.clock.0.set(start + 9000);
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(d.backend.inputs.len(), sends);
    assert_eq!(
        d.journal
            .events
            .iter()
            .filter(|(k, _)| k == "release_recovery_reload_intent")
            .count(),
        1
    );
}
fn reload_fixture(d: Fixture) -> Fixture {
    reload(d)
}

#[test]
fn go_mailbox_v1_driver_release_resource_rejection_keeps_verified_report() {
    let mut d = prepared();
    d.backend.pending_actions.insert("release".into());
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    d.backend.pending_actions.clear();
    let nonce = d.state.transaction.as_ref().unwrap().envelope.nonce.clone();
    let r = d.backend.receipts.get_mut(&nonce).unwrap();
    r.state = "rejected".into();
    r.reason = "slot_resources_pending".into();
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_cleanup_reload_required"
    ));
    let op = d.state.operation.as_ref().unwrap();
    assert_eq!(op.stage, OperationStage::ReleaseReady);
    assert_eq!(op.cleanup_method, CleanupMethod::ReloadRequired);
    assert_eq!(op.result_bytes.0, d.backend.result);
    assert!(d.state.transaction.is_none());
    let sends = d.backend.inputs.len();
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_cleanup_reload_required"
    ));
    assert_eq!(d.backend.inputs.len(), sends);
}

#[test]
fn go_mailbox_v1_driver_closing_never_publishes_pending_business() {
    let mut d = prepared();
    d.state.closing = true;
    d.run(&OperationControl::default()).unwrap();
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::Cancelled
    );
    assert_eq!(d.backend.inputs.len(), 1);
    let mut d = prepared();
    let ticket = d.state.operation.as_ref().unwrap().ticket.clone();
    d.begin(&OperationControl::default(), "prepare", ticket, |_| {})
        .unwrap();
    d.state.closing = true;
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(d.waiting, "closing_exchange_unconfirmed");
    assert_eq!(d.backend.published.len(), 1);
    assert_eq!(d.backend.inputs.len(), 1);
}

#[test]
fn go_mailbox_v1_driver_input_diagnostic_remains_pending_not_execution() {
    let mut d = prepared();
    d.backend.pending_actions.insert("prepare".into());
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    let e = &d.state.transaction.as_ref().unwrap().envelope;
    let sample = d
        .state
        .input
        .as_ref()
        .unwrap()
        .observation
        .as_ref()
        .unwrap()
        .sample_millis;
    d.backend.diagnostic = Some(InputDiagnostic {
        schema: SCHEMA.into(),
        runtime: e.runtime.clone(),
        attempt_seq: 1,
        slot: e.index,
        stage: "load_failed".into(),
        reason: "slot_addon_disabled".into(),
        received: false,
        sample_millis: sample,
    });
    let sends = d.backend.inputs.len();
    for _ in 0..2 {
        assert!(
            d.run(&OperationControl::default())
                .unwrap_err()
                .is_pending()
        );
    }
    assert_eq!(
        d.journal
            .events
            .iter()
            .filter(|(k, _)| k == "input_attempt_diagnostic")
            .count(),
        1
    );
    assert_eq!(
        d.state
            .input
            .as_ref()
            .unwrap()
            .after_diagnostic
            .as_ref()
            .unwrap()
            .reason,
        "slot_addon_disabled"
    );
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::Prepared
    );
    assert!(d.state.transaction.as_ref().unwrap().receipt.is_none());
    assert_eq!(d.backend.inputs.len(), sends);
}

#[test]
fn go_mailbox_v1_driver_readiness_blocker_issues_no_input_or_escape() {
    let mut d = prepared();
    d.backend.input_blocked = Some("input_protection_busy".into());
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(d.waiting, "input_protection_busy");
    assert_eq!(d.backend.inputs.len(), 1);
    assert_eq!(
        d.state.transaction.as_ref().unwrap().phase,
        TransactionPhase::Published
    );
    assert!(d.backend.inputs.iter().all(|a| a.kind == InputKind::Invoke));
}

#[test]
fn go_mailbox_v1_driver_post_receipt_unknown_time_bound_is_persisted_once() {
    let mut d = prepared();
    d.backend.input_outcomes.push_back(InputOutcome {
        disposition: InputDisposition::Submitted,
        messages_queued: 4,
        at_millis: 0,
        reason: String::new(),
        retryable: false,
    });
    d.backend.sample_override = Some((1200, 1200));
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(d.waiting, "input_observation_stale");
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::CommitReady
    );
    assert_eq!(d.state.input.as_ref().unwrap().recovery_after_millis, 1200);
    let sends = d.backend.inputs.len();
    let mut d = reload(d);
    assert_eq!(
        d.state
            .input
            .as_ref()
            .unwrap()
            .observation
            .as_ref()
            .unwrap()
            .observed_at_millis,
        0
    );
    d.backend.sample_override = Some((1299, 1300));
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(d.state.input.as_ref().unwrap().recovery_after_millis, 1200);
    assert_eq!(d.backend.inputs.len(), sends);
    assert_eq!(
        d.journal
            .events
            .iter()
            .filter(|(k, _)| k == "input_time_recovered")
            .count(),
        1
    );
    d.backend.sample_override = None;
    d.backend.uptime = 1300;
    d.run(&OperationControl::default()).unwrap();
}

#[test]
fn go_mailbox_v1_driver_history_lookup_read_only_and_conflicts_fail_closed() {
    let mut d = prepared();
    let ctx = OperationControl::default();
    d.run(&ctx).unwrap();
    let old = d.state.operation.clone().unwrap();
    d.prepare_request(&ctx, "request-2", "return 2", 5, OperationPolicy::Opaque)
        .unwrap();
    let current = d.state.clone();
    let trace_before = d.backend.trace.borrow().len();
    let historical = d
        .historical_request(&ctx, "request-1", "return 1", 5, OperationPolicy::Opaque)
        .unwrap()
        .unwrap();
    assert_eq!(historical.operation.unwrap(), old);
    assert_eq!(d.state, current);
    assert_eq!(d.backend.trace.borrow().len(), trace_before);
    assert!(error_is(
        &d.historical_request(&ctx, "request-1", "return 2", 5, OperationPolicy::Opaque)
            .unwrap_err(),
        "live.channel_request_conflict"
    ));
    d.journal.fail_history = true;
    assert!(
        d.historical_request(&ctx, "missing", "return 1", 5, OperationPolicy::Opaque)
            .is_err()
    );
    assert_eq!(d.state, current);
}

#[test]
fn go_mailbox_v1_driver_observation_error_cannot_become_receipt_authority() {
    let mut d = prepared();
    d.backend.observe_error = Some(ChannelError::Joined {
        causes: vec![pending(), error("fixture.identity_changed")],
    });
    let err = d.run(&OperationControl::default()).unwrap_err();
    assert!(err.is_pending());
    assert!(error_is(&err, "fixture.identity_changed"));
    assert!(!err.optional_observation_timeout());
    assert!(d.state.transaction.as_ref().unwrap().receipt.is_none());
    assert_eq!(d.backend.consumed.len(), 1);
}

#[test]
fn go_mailbox_v1_driver_ambiguous_saved_input_intent_stays_unknown() {
    let mut d = prepared();
    d.journal.fail_event = Some("input_intent");
    d.journal.fail_after_append = true;
    assert!(d.run(&OperationControl::default()).is_err());
    assert_eq!(d.backend.inputs.len(), 1, "only the earlier bind was sent");
    let mut d = reload(d);
    assert!(d.state.input.as_ref().unwrap().outcome.is_none());
    for _ in 0..3 {
        assert!(
            d.run(&OperationControl::default())
                .unwrap_err()
                .is_pending()
        );
    }
    assert_eq!(
        d.backend.inputs.len(),
        1,
        "durable intent is not proof of no send"
    );
}

#[test]
fn go_mailbox_v1_driver_bad_nonce_source_and_request_fail_before_publication() {
    struct BadTokens;
    impl TokenSource for BadTokens {
        fn token(&mut self) -> ChannelResult<String> {
            Ok("A".repeat(32))
        }
    }
    let d = fixture();
    assert!(Driver::new(d.state.identity, d.backend, d.journal, d.clock, BadTokens).is_err());
    for (code, budget) in [
        ("", 5),
        ("\u{1b}Lua", 5),
        ("return 1", 0),
        ("return 1", 121),
    ] {
        let mut d = connected();
        assert!(error_is(
            &d.prepare_request(
                &OperationControl::default(),
                "request",
                code,
                budget,
                OperationPolicy::Opaque
            )
            .unwrap_err(),
            "live.channel_request_invalid"
        ));
        assert!(d.state.operation.is_none());
        assert_eq!(d.backend.inputs.len(), 1);
        assert!(d.backend.trace.borrow().is_empty());
    }
}

#[test]
fn go_mailbox_v1_driver_preappend_control_stop_allows_only_bounded_fact_flush() {
    let mut d = prepared();
    d.backend.cancel_in_input = true;
    let ctx = OperationControl::default();
    assert!(matches!(d.run(&ctx).unwrap_err(), ChannelError::Cancelled));
    assert!(d.save_pending);
    assert!(
        !d.journal_failed,
        "no journal call was entered for input_attempted"
    );
    assert_eq!(d.journal.events.last().unwrap().0, "input_observed");
    let sends = d.backend.inputs.len();
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_journal_reload_required"
    ));
    assert_eq!(d.backend.inputs.len(), sends);
    // The continuation's bounded finalizer preserves sticky exhaustion with
    // the already-observed facts, without performing another external effect.
    d.state
        .operation
        .as_mut()
        .unwrap()
        .recovery_budget
        .as_mut()
        .unwrap()
        .exhausted = true;
    d.save(&OperationControl::default(), "budget_observed")
        .unwrap();
    assert!(!d.save_pending);
    assert!(!d.journal_failed);
    let d = reload(d);
    assert!(
        d.state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .exhausted
    );
    assert_eq!(d.backend.inputs.len(), sends);
}

#[test]
fn go_mailbox_v1_driver_entered_append_deadline_is_still_ambiguous() {
    for completed in [false, true] {
        let mut d = prepared();
        d.journal.fail_event = Some("input_intent");
        d.journal.fail_as_deadline = true;
        d.journal.fail_after_append = completed;
        assert!(matches!(
            d.run(&OperationControl::default()).unwrap_err(),
            ChannelError::DeadlineExceeded
        ));
        assert!(d.journal_failed);
        assert!(error_is(
            &d.save(&OperationControl::default(), "budget_observed")
                .unwrap_err(),
            "live.channel_journal_reload_required"
        ));
        assert_eq!(d.backend.inputs.len(), 1);
    }
}

#[test]
fn go_mailbox_v1_driver_invalid_hex_challenge_keeps_go_consumption_error_order() {
    let mut d = prepared();
    d.backend.receipt_mutation = Some(|r| r.challenge = "G".repeat(32));
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_journal_invalid"
    ));
    assert_eq!(
        d.backend
            .consumed
            .iter()
            .map(|e| e.action.as_str())
            .collect::<Vec<_>>(),
        ["bind", "prepare"]
    );
    assert_eq!(d.journal.events.last().unwrap().0, "receipt_verified");
    assert!(d.journal_failed);
    assert!(!d.backend.published.iter().any(|e| e.action == "commit"));
    assert!(error_is(
        &d.run(&OperationControl::default()).unwrap_err(),
        "live.channel_journal_reload_required"
    ));
}

#[test]
fn go_mailbox_v1_driver_raw_byte_report_survives_complete_and_readonly_recovery() {
    let mut d = prepared();
    d.backend.result = b" { \"s\":\"\xff\", \"x\":1e400, \"x\":-0 } \n".to_vec();
    let original = d.backend.result.clone();
    let control = OperationControl::default();
    d.run(&control).unwrap();
    let op = d.state.operation.as_ref().unwrap();
    assert_eq!(op.stage, OperationStage::Complete);
    assert_eq!(op.result_bytes.0, original);
    assert_eq!(op.report_checksum, checksum(&original));
    assert_eq!(d.journal.results[0].result_bytes.0, original);
    let sends = d.backend.inputs.len();
    let mut d = reload(d);
    d.run(&control).unwrap();
    assert_eq!(d.backend.inputs.len(), sends);
    assert_eq!(d.state.operation.as_ref().unwrap().result_bytes.0, original);
}
