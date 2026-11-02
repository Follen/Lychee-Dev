//! Orchestration fixtures, with one explicit real DurableChannelJournal regression.
//! FakeJournal tests do not prove filesystem flush or publication leases; none of
//! these fixtures prove native key dispatch, Lua traversal or runtime replacement.
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
            slot_reload_required: false,
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
            control.tracked_cancel_handle().unwrap().cancel();
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
fn mailbox_driver_full_cycle_exact_order_and_result_bytes() {
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
fn mailbox_driver_reported_prepare_skips_commit() {
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
fn mailbox_driver_failed_intents_never_reach_dependent_effects() {
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
fn mailbox_driver_unknown_intent_and_uncertain_outcomes_never_replay() {
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
fn mailbox_driver_known_zero_retry_keeps_nonce_and_deadline() {
    let mut d = prepared();
    let budget = d.state.operation.as_ref().unwrap().recovery_budget.clone();
    for attempt in 0..3 {
        d.backend.input_outcomes.push_back(InputOutcome {
            disposition: InputDisposition::NotSent,
            messages_queued: 0,
            at_millis: 0,
            reason: if attempt % 2 == 0 {
                "desktop.keyboard_active"
            } else {
                "desktop.key_already_held"
            }
            .into(),
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
fn mailbox_driver_cancellation_still_flushes_input_outcome() {
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

fn assert_input_fact_flush_failure(cancel: bool) {
    let mut d = prepared();
    let original = d.state.operation.clone().unwrap();
    let input_error = ChannelError::from(
        lycheedev_core::ToolkitError::new("external.io", "original input dispatch failed", 5)
            .with_details(serde_json::json!({"queued":2,"stage":"post_message"})),
    );
    d.backend.input_error = Some(if cancel {
        ChannelError::Joined {
            causes: vec![input_error.clone(), ChannelError::Cancelled],
        }
    } else {
        input_error.clone()
    });
    d.backend.cancel_in_input = cancel;
    d.backend.input_outcomes.push_back(InputOutcome {
        disposition: InputDisposition::Uncertain,
        messages_queued: 2,
        at_millis: 0,
        reason: "original input dispatch failed".into(),
        retryable: false,
    });
    d.journal.fail_event = Some("input_observed");
    let control = OperationControl::default();
    let before = d.backend.inputs.len();
    let error = d.run(&control).unwrap_err();
    assert_eq!(d.backend.inputs.len(), before + 1);
    let durable = &d.journal.events.last().unwrap();
    assert_eq!(durable.0, "input_intent");
    assert!(durable.1.input.as_ref().unwrap().outcome.is_none());
    assert_eq!(
        durable.1.operation.as_ref().unwrap().recovery_budget,
        original.recovery_budget
    );
    let nonce = durable
        .1
        .transaction
        .as_ref()
        .unwrap()
        .envelope
        .nonce
        .clone();
    let mut resumed = reload(d);
    resumed.backend.cancel_in_input = false;
    resumed.backend.pending_actions.insert("prepare".into());
    assert!(
        resumed
            .run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(
        resumed.backend.inputs.len(),
        before + 1,
        "unknown durable intent never replays"
    );
    assert_eq!(
        resumed.state.transaction.as_ref().unwrap().envelope.nonce,
        nonce
    );
    assert_eq!(
        resumed.state.operation.as_ref().unwrap().recovery_budget,
        original.recovery_budget
    );
    assert!(error_is(&error, "fixture.journal_write_failed"));
    assert!(
        error.any_cause(&|cause| cause == &input_error),
        "original structured dispatch error was lost: {error:?}"
    );
    if cancel {
        assert!(control.check().is_err());
        assert!(
            error.any_cause(&|cause| matches!(cause, ChannelError::Cancelled)),
            "original cancellation was lost: {error:?}"
        );
    }
}
#[test]
fn mailbox_driver_input_fact_flush_failure_keeps_dispatch_error() {
    assert_input_fact_flush_failure(false);
}
#[test]
fn mailbox_driver_input_fact_flush_failure_keeps_cancel_and_dispatch_error() {
    assert_input_fact_flush_failure(true);
}

#[test]
fn mailbox_driver_invalid_transport_outcome_becomes_unknown() {
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
fn mailbox_driver_consumption_failure_keeps_exact_received_transaction() {
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
fn mailbox_driver_bad_receipt_target_never_consumes() {
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
fn mailbox_driver_report_target_length_checksum_json_are_requalified() {
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
fn mailbox_driver_confirmation_must_match_original_report() {
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
fn mailbox_driver_result_write_failure_cannot_admit_release() {
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
fn mailbox_driver_reallocation_keeps_nonce_route_and_persists_before_publish() {
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
fn mailbox_driver_reallocation_failure_does_not_write_new_destination() {
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
fn mailbox_driver_capacity_reserve_starts_at_486_and_preserves_request() {
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
fn mailbox_driver_capacity_rejection_carries_original_recovery_budget() {
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
fn mailbox_driver_release_timeout_records_one_reload_intent_without_replay() {
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
fn mailbox_driver_release_resource_rejection_keeps_verified_report() {
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
fn mailbox_driver_closing_never_publishes_pending_business() {
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
fn mailbox_driver_input_diagnostic_remains_pending_not_execution() {
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
fn mailbox_driver_readiness_blocker_issues_no_input_or_escape() {
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
fn mailbox_driver_post_receipt_unknown_time_bound_is_persisted_once() {
    let mut d = prepared();
    d.backend.input_outcomes.push_back(InputOutcome {
        disposition: InputDisposition::Submitted,
        messages_queued: 4,
        at_millis: 0,
        reason: String::new(),
        retryable: false,
    });
    // Go 3.2.2 observes input readiness before bind, advancing fixture uptime.
    // Start after the actual bind input; do not accidentally test stale prepare.
    let recovered_at = d.backend.uptime + 100;
    d.backend.sample_override = Some((recovered_at, recovered_at));
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
    assert_eq!(
        d.state.input.as_ref().unwrap().recovery_after_millis,
        recovered_at
    );
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
    d.backend.sample_override = Some((recovered_at + 99, recovered_at + 100));
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    assert_eq!(
        d.state.input.as_ref().unwrap().recovery_after_millis,
        recovered_at
    );
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
    d.backend.uptime = recovered_at + 100;
    d.run(&OperationControl::default()).unwrap();
}

#[test]
fn mailbox_driver_history_lookup_read_only_and_conflicts_fail_closed() {
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
fn mailbox_driver_observation_error_cannot_become_receipt_authority() {
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
fn mailbox_driver_ambiguous_saved_input_intent_stays_unknown() {
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
fn mailbox_driver_bad_nonce_source_and_request_fail_before_publication() {
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
fn mailbox_driver_preappend_control_stop_allows_only_bounded_fact_flush() {
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
fn mailbox_driver_entered_append_deadline_is_still_ambiguous() {
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
fn mailbox_driver_invalid_hex_challenge_keeps_consumption_error_order() {
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
fn mailbox_driver_raw_byte_report_survives_complete_and_readonly_recovery() {
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

#[test]
fn driver_keyboard_wait_rechecks_input_without_replacing_exchange_or_deadline() {
    let mut d = prepared();
    let budget = d.state.operation.as_ref().unwrap().recovery_budget.clone();
    d.backend.input_outcomes.push_back(InputOutcome {
        disposition: InputDisposition::NotSent,
        messages_queued: 0,
        at_millis: 0,
        reason: "desktop.keyboard_active".into(),
        retryable: true,
    });
    assert!(
        d.run(&OperationControl::default())
            .unwrap_err()
            .is_pending()
    );
    let original = d.state.transaction.as_ref().unwrap().envelope.clone();
    let first = d.backend.inputs.last().unwrap().clone();
    let reads = d.backend.input_reads;
    d = reload(d);
    d.run(&OperationControl::default()).unwrap();
    let retry = d
        .backend
        .inputs
        .iter()
        .filter(|a| a.envelope.nonce == original.nonce)
        .nth(1)
        .unwrap();
    assert_eq!(retry.envelope, original);
    assert!(
        retry.observation.as_ref().unwrap().sample_millis
            > first.observation.as_ref().unwrap().sample_millis
    );
    assert!(d.backend.input_reads > reads);
    assert_eq!(d.state.operation.as_ref().unwrap().recovery_budget, budget);
    assert_eq!(
        d.backend
            .published
            .iter()
            .filter(|e| e.nonce == original.nonce)
            .count(),
        1
    );
}

#[test]
fn driver_busy_keyboard_does_not_delay_or_replay_submitted_uncertain_or_unknown_receipt() {
    for state in ["submitted", "uncertain", "missing"] {
        let mut d = fixture();
        d.backend.pending_actions.insert("bind".into());
        if state == "missing" {
            d.journal.fail_event = Some("input_observed");
        } else if state == "uncertain" {
            d.backend.input_outcomes.push_back(InputOutcome {
                disposition: InputDisposition::Uncertain,
                messages_queued: 1,
                at_millis: 0,
                reason: "fixture.uncertain".into(),
                retryable: false,
            });
        }
        assert!(d.connect(&OperationControl::default()).is_err());
        let original = d.state.transaction.as_ref().unwrap().envelope.clone();
        let sends = d.backend.inputs.len();
        let reads = d.backend.input_reads;
        d = reload(d);
        // Any new INPUT admission would be blocked. The original receipt path
        // must remain independent of the physical-keyboard readiness gate.
        d.backend.input_blocked = Some("desktop.keyboard_active".into());
        d.backend.pending_actions.remove("bind");
        d.connect(&OperationControl::default()).unwrap();
        assert!(d.state.bound);
        assert_eq!(d.backend.inputs.len(), sends);
        assert_eq!(d.backend.input_reads, reads);
        assert_eq!(
            d.backend
                .published
                .iter()
                .filter(|e| e.nonce == original.nonce)
                .count(),
            1
        );
        assert!(d.backend.consumed.iter().any(|e| e == &original));
    }
}

// Go 3.2.1 input_journal_test.go: TestHeldKeyRetriesKeepJournalCapacityAndOriginalDeadline.
// Reuse the driver peer, clock, tokens, scheduler and durable filesystem fixture.
// Only the transport remains fake; every journal append, archive, close and reload
// below goes through the production DurableChannelJournal.
struct HeldKeyJournalPeer(FakeBackend);
impl Backend for HeldKeyJournalPeer {
    fn observe(
        &mut self,
        control: &OperationControl,
        query: &ObservationQuery,
    ) -> ObservationDispatch {
        let observed = self.0.observe(control, query);
        control.tracked_cancel_handle().unwrap().cancel();
        observed
    }
    fn publish(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()> {
        self.0.publish(control, envelope)
    }
    fn observe_input(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        after: i64,
        capability: &str,
    ) -> ChannelResult<InputObservation> {
        use super::super::{Header, Kind, ZERO_TOKEN, decode_input, decode_record, encode_record};
        let sample = self.0.observe_input(control, envelope, after, capability)?;
        let runtime = parse_token(&sample.runtime)?;
        let wire = encode_record(
            Header {
                nonce: runtime,
                runtime,
                ticket: ZERO_TOKEN,
                kind: Kind::Input,
                state: 1,
                sequence: self.0.input_reads.try_into().unwrap(),
                length: 0,
                checksum: 0,
            },
            &serde_json::to_vec(&sample).unwrap(),
        )?;
        // Freshness is qualified from a checksum-valid INPUT on every turn,
        // not from a journal-restored host observed_at_millis value.
        Ok(decode_input(
            &decode_record(&wire)?,
            &envelope.expected(),
            after,
            sample.observed_at_millis,
        )?)
    }
    fn input(&mut self, control: &OperationControl, action: &InputAction) -> InputDispatch {
        let dispatched = self.0.input(control, action);
        control.tracked_cancel_handle().unwrap().cancel();
        dispatched
    }
    fn runtime_candidate(
        &mut self,
        _: &OperationControl,
        _: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        panic!("bounded held-key turns must not discover another runtime")
    }
    fn supersede(
        &mut self,
        _: &OperationControl,
        _: &SlotEnvelope,
        _: &Identity,
    ) -> ChannelResult<()> {
        panic!("held-key retry must not supersede the original exchange")
    }
    fn consumed(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()> {
        self.0.consumed(control, envelope)
    }
}
impl super::super::runtime_retirement::RuntimeRetirementBackend for HeldKeyJournalPeer {
    fn observe_runtime_replacement(
        &mut self,
        _: &OperationControl,
        _: &Identity,
    ) -> ChannelResult<Option<super::super::state::RuntimeReplacementProof>> {
        panic!("held-key retry must not request runtime replacement")
    }
    fn retire_replaced_runtime(
        &mut self,
        _: &OperationControl,
        _: &SlotEnvelope,
        _: &super::super::state::RuntimeReplacementProof,
    ) -> ChannelResult<()> {
        panic!("held-key retry must not retire the original runtime")
    }
}
type DurableHeldKeyDriver = Driver<
    HeldKeyJournalPeer,
    super::super::durable_journal::DurableChannelJournal,
    FakeClock,
    Tokens,
>;
fn reload_held_key_journal(
    mut driver: DurableHeldKeyDriver,
    filesystem: &super::super::durable_journal::tests::Fixture,
) -> DurableHeldKeyDriver {
    driver.journal.close().unwrap();
    let journal = filesystem.open();
    let state = journal.load(&OperationControl::default()).unwrap();
    if let Some(input) = &state.input {
        assert_eq!(input.observation.as_ref().unwrap().observed_at_millis, 0);
    }
    Driver::from_state(state, driver.backend, journal, driver.clock, driver.tokens).unwrap()
}
fn held_key_journal_turn(driver: &mut DurableHeldKeyDriver) -> ChannelResult<()> {
    use super::super::recovery::test_support::{Schedule, target};
    let control = OperationControl::uncancelable(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
    );
    let mut scheduler = Schedule::default();
    let result = driver.continue_until(&control, target(), &mut scheduler);
    assert_eq!(scheduler.waits, 0, "the peer ends each bounded CLI turn");
    result
}

#[test]
fn driver_durable_keyboard_retries_preserve_archive_capacity_and_original_deadline() {
    use super::super::durable_journal::tests::Fixture as JournalFixture;
    use super::super::journal::{MAX_EVENTS, event_hash, hydrate, read_log_bytes};
    use super::super::state::InputAttempt;
    use std::{fs, path::PathBuf};

    let filesystem = JournalFixture::new();
    let mut initial = fixture();
    // Match the existing journal fixture's connection owner. No bind input is
    // sent: the pinned Go regression also starts with an already-bound state.
    initial.state.id = format!("CON-{}", "1".repeat(32));
    initial.state.owner = "1".repeat(32);
    initial.state.bound = true;
    initial.state.identity.owner = initial.state.owner.clone();
    initial.state.identity.fence = 1;
    initial.backend.identity = initial.state.identity.clone();
    initial.backend.pending_actions.insert("prepare".into());
    filesystem.seed([initial.state.clone()]);
    let mut d = Driver::from_state(
        initial.state,
        HeldKeyJournalPeer(initial.backend),
        filesystem.open(),
        initial.clock,
        initial.tokens,
    )
    .unwrap();
    let start = d.clock.unix_millis();
    d.prepare_request(
        &OperationControl::default(),
        "held-key-original",
        "return 1",
        5,
        OperationPolicy::Opaque,
    )
    .unwrap();
    let original = d.state.operation.clone().unwrap();
    let original_budget = original.recovery_budget.clone().unwrap();
    let deadline = original_budget.deadline_ms;
    assert_eq!(deadline - start, 600_000);
    let mut envelope = None;
    let mut expected_inputs = BTreeMap::<String, InputAttempt>::new();
    let mut immutable_archives = BTreeMap::<PathBuf, Vec<u8>>::new();
    let mut previous_sample = 0;

    for turn in 0..=600 {
        d.clock.0.set(start + (i64::from(turn) + 1) * 500);
        d = reload_held_key_journal(d, &filesystem);
        d.backend.0.uptime = 1_000_000 + d.clock.unix_millis() - start - 100;
        let reason = if turn == 600 {
            "fixture_submission_unknown"
        } else if turn % 2 == 0 {
            "desktop.keyboard_active"
        } else {
            "desktop.key_already_held"
        };
        let expected_outcome = InputOutcome {
            disposition: if turn == 600 {
                InputDisposition::Uncertain
            } else {
                InputDisposition::NotSent
            },
            messages_queued: if turn == 600 { 1 } else { 0 },
            at_millis: d.backend.0.uptime + 100,
            reason: reason.into(),
            retryable: turn != 600,
        };
        d.backend
            .0
            .input_outcomes
            .push_back(expected_outcome.clone());
        d.backend.0.input_error = Some(error(reason));
        let err = held_key_journal_turn(&mut d).unwrap_err();
        // Go input_journal_test.go checks ErrPending only for known-zero
        // retries. The final uncertain submission keeps its original error;
        // subsequent resumes below must wait without sending it again.
        if turn == 600 {
            assert!(
                !err.is_pending(),
                "uncertain submission was rewritten: {err:?}"
            );
        } else {
            assert!(err.is_pending(), "turn {turn}: {err:?}");
        }
        assert!(error_is(&err, reason), "turn {turn}: {err:?}");
        let operation = d.state.operation.as_ref().unwrap();
        assert_eq!(
            (&operation.id, &operation.ticket, &operation.request),
            (&original.id, &original.ticket, &original.request)
        );
        let budget = operation.recovery_budget.as_ref().unwrap();
        assert_eq!(
            (budget.started_at_ms, budget.deadline_ms, budget.exhausted),
            (original_budget.started_at_ms, deadline, false)
        );
        let transaction = d.state.transaction.as_ref().unwrap();
        assert_eq!(transaction.phase, TransactionPhase::Published);
        assert_eq!(
            envelope.get_or_insert_with(|| transaction.envelope.clone()),
            &transaction.envelope
        );
        let input = d.state.input.as_ref().unwrap();
        assert_eq!(input.outcome.as_ref(), Some(&expected_outcome));
        let sample = input.observation.as_ref().unwrap();
        assert!(sample.sample_millis > previous_sample);
        assert_eq!(sample.observed_at_millis, sample.sample_millis);
        assert!(sample.sample_millis >= input.after_millis + 100);
        previous_sample = sample.sample_millis;
        // Persisted observations intentionally lose host freshness; compare the
        // entire remaining intent and exact outcome after independent decoding.
        let saved_input = serde_json::from_slice(&serde_json::to_vec(input).unwrap()).unwrap();
        assert!(
            expected_inputs
                .insert(input.id.clone(), saved_input)
                .is_none()
        );
        assert_eq!(d.backend.0.inputs.len(), turn as usize + 1);
        assert_eq!(d.backend.0.input_reads, turn as usize + 1);
        assert_eq!(d.backend.0.published.len(), 1);
        if !d.state.archive.is_empty() {
            let path = d
                .journal
                .paths()
                .history_directory()
                .join(format!("{}.jsonl", d.state.archive));
            let bytes = fs::read(&path).unwrap();
            if let Some(prior) = immutable_archives.get(&path) {
                assert_eq!(&bytes, prior, "an immutable predecessor changed");
            } else {
                immutable_archives.insert(path, bytes);
            }
        }
    }
    let input_id = d.state.input.as_ref().unwrap().id.clone();
    for _ in 0..2 {
        d = reload_held_key_journal(d, &filesystem);
        assert!(held_key_journal_turn(&mut d).unwrap_err().is_pending());
        assert_eq!(d.backend.0.inputs.len(), 601);
        assert_eq!(d.backend.0.input_reads, 601);
        assert_eq!(d.state.input.as_ref().unwrap().id, input_id);
    }
    assert_eq!(d.backend.0.observations, 2);
    d.clock.0.set(deadline);
    let exhausted = held_key_journal_turn(&mut d).unwrap_err();
    assert!(exhausted.is_pending(), "{exhausted:?}");
    // Durable budget exhaustion is a Pending cause, not a Contract variant.
    assert!(
        super::super::recovery::has_code(&exhausted, "live.channel_budget_exhausted"),
        "{exhausted:?}"
    );
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
    assert_eq!(d.backend.0.inputs.len(), 601);
    d.request_close(&OperationControl::default()).unwrap();
    d = reload_held_key_journal(d, &filesystem);
    assert!(d.state.closing, "close intent was not durable");
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
    assert_eq!(
        d.state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .deadline_ms,
        deadline
    );
    assert!(held_key_journal_turn(&mut d).unwrap_err().is_pending());
    assert_eq!(d.waiting, "closing_exchange_unconfirmed");
    assert_eq!(d.backend.0.inputs.len(), 601);
    assert_eq!(d.backend.0.input_reads, 601);
    assert_eq!(d.backend.0.published.len(), 1);
    assert_eq!(d.backend.0.observations, 3);

    let paths = d.journal.paths().clone();
    d.journal.close().unwrap();
    let mut path = paths.log_path().to_owned();
    let mut expected_predecessor = String::new();
    let mut visited = BTreeSet::new();
    let mut intents = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    let mut zero_outcomes = 0;
    loop {
        assert!(visited.insert(path.clone()), "archive chain loop");
        assert!(visited.len() <= 16, "unexpected archive chain length");
        let bytes = fs::read(&path).unwrap();
        if !expected_predecessor.is_empty() {
            assert_eq!(
                &bytes,
                immutable_archives.get(&path).unwrap(),
                "archived bytes changed after retry/resume/close"
            );
        }
        let log = read_log_bytes(&bytes);
        assert!(log.failure.is_none(), "{path:?}: {:?}", log.failure);
        assert!(!log.events.is_empty());
        assert!(
            log.events.len() < MAX_EVENTS,
            "active/archived segment exhausted its hard limit"
        );
        let last = log.events.last().unwrap();
        if !expected_predecessor.is_empty() {
            assert_eq!(last.checksum, expected_predecessor);
        }
        for event in &log.events {
            assert_eq!(event_hash(event).unwrap(), event.checksum);
            if !matches!(event.kind.as_str(), "input_intent" | "input_observed") {
                continue;
            }
            let snapshot: State = serde_json::from_str(event.data.get()).unwrap();
            let state = hydrate(snapshot, |hash| {
                Ok(fs::read(paths.artifact_directory().join(hash)).unwrap())
            })
            .unwrap();
            state.validate().unwrap();
            let operation = state.operation.as_ref().unwrap();
            assert_eq!(
                (
                    &operation.id,
                    &operation.ticket,
                    &operation.request,
                    &operation.code
                ),
                (
                    &original.id,
                    &original.ticket,
                    &original.request,
                    &original.code
                )
            );
            let budget = operation.recovery_budget.as_ref().unwrap();
            assert_eq!(
                (budget.started_at_ms, budget.deadline_ms, budget.exhausted),
                (original_budget.started_at_ms, deadline, false)
            );
            assert_eq!(
                &state.transaction.as_ref().unwrap().envelope,
                envelope.as_ref().unwrap()
            );
            let input = state.input.as_ref().unwrap();
            assert_eq!(input.exchange, envelope.as_ref().unwrap().nonce);
            let mut expected = expected_inputs.get(&input.id).unwrap().clone();
            if event.kind == "input_intent" {
                expected.outcome = None;
                assert!(intents.insert(input.id.clone()), "duplicate input intent");
            } else {
                assert!(outcomes.insert(input.id.clone()), "duplicate input outcome");
                let outcome = input.outcome.as_ref().unwrap();
                if outcome.disposition == InputDisposition::NotSent {
                    assert_eq!(outcome.messages_queued, 0);
                    zero_outcomes += 1;
                } else {
                    assert_eq!(input.id, input_id);
                    assert_eq!(outcome.disposition, InputDisposition::Uncertain);
                    assert_eq!(outcome.messages_queued, 1);
                }
            }
            assert_eq!(input, &expected, "archive lost an exact intent/outcome");
        }
        let latest: State = serde_json::from_str(last.data.get()).unwrap();
        if latest.archive.is_empty() {
            break;
        }
        expected_predecessor = latest.archive;
        path = paths
            .history_directory()
            .join(format!("{expected_predecessor}.jsonl"));
    }
    assert_eq!(intents.len(), 601);
    assert_eq!(outcomes, intents);
    assert_eq!(zero_outcomes, 600);
    assert!(visited.len() >= 2, "the real journal never checkpointed");
    assert_eq!(visited.len(), immutable_archives.len() + 1);
    assert_eq!(
        fs::read_dir(paths.history_directory()).unwrap().count(),
        immutable_archives.len()
    );
}
