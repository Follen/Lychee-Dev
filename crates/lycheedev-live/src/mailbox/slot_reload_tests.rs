//! Go v3.2.2 (0bec60e) slot_reload_test.go translations plus failure boundaries.
//! Transport/clock are fixtures. Durable restart tests use the real journal.
//! Declarations are NOT_RUN until a controlled product runner executes them.
use super::driver::{Driver, error};
use super::journal::StateJournal;
use super::observations::*;
use super::recovery::has_code;
use super::recovery::test_support::{FakeClock, Journal, Peer, Schedule, Tokens, identity, target};
use super::runtime_retirement::RuntimeRetirementBackend;
use super::state::*;
use super::*;
use lycheedev_core::control::OperationControl;
use std::{cell::Cell, collections::BTreeSet, rc::Rc};

#[derive(Default)]
struct SlotPeer {
    peer: Peer,
    flags: BTreeSet<String>,
    publications: Vec<SlotEnvelope>,
    inputs: Vec<(InputKind, String)>,
    blocked: bool,
    uncertain: bool,
    invalid_readiness: bool,
    input_fault: Option<ChannelError>,
}
impl Backend for SlotPeer {
    fn observe(&mut self, c: &OperationControl, q: &ObservationQuery) -> ObservationDispatch {
        self.peer.observe(c, q)
    }
    fn publish(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.publications.push(e.clone());
        self.peer.publish(c, e)
    }
    fn observe_input(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        after: i64,
        cap: &str,
    ) -> ChannelResult<InputObservation> {
        if let Some(err) = &self.input_fault {
            return Err(err.clone());
        }
        let mut sample = self.peer.observe_input(c, e, after, cap)?;
        sample.slot_reload_required = self.flags.contains(&e.runtime);
        if self.blocked {
            sample.input_blocked = Some(true);
            sample.reason = "combat".into();
        }
        if self.invalid_readiness {
            sample.input_blocked = None;
        }
        Ok(sample)
    }
    fn input(&mut self, c: &OperationControl, a: &InputAction) -> InputDispatch {
        self.inputs.push((a.kind, a.envelope.runtime.clone()));
        if self.uncertain {
            return InputDispatch {
                outcome: InputOutcome {
                    disposition: InputDisposition::Uncertain,
                    messages_queued: 1,
                    at_millis: 1100,
                    reason: "fixture.unknown_reload".into(),
                    retryable: false,
                },
                error: Some(error("fixture.unknown_reload")),
            };
        }
        if a.kind == InputKind::Reload {
            let mut next = identity((a.envelope.runtime.as_bytes()[0] + 1) as char);
            next.next_slot = 1;
            self.peer.candidate = Some(next);
        }
        self.peer.input(c, a)
    }
    fn runtime_candidate(
        &mut self,
        c: &OperationControl,
        old: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        self.peer.runtime_candidate(c, old)
    }
    fn supersede(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        i: &Identity,
    ) -> ChannelResult<()> {
        self.peer.supersede(c, e, i)
    }
    fn consumed(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.peer.consumed(c, e)
    }
}
impl RuntimeRetirementBackend for SlotPeer {
    fn observe_runtime_replacement(
        &mut self,
        c: &OperationControl,
        old: &Identity,
    ) -> ChannelResult<Option<RuntimeReplacementProof>> {
        self.peer.observe_runtime_replacement(c, old)
    }
    fn retire_replaced_runtime(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        p: &RuntimeReplacementProof,
    ) -> ChannelResult<()> {
        self.peer.retire_replaced_runtime(c, e, p)
    }
}
type TestDriver = Driver<SlotPeer, Journal, FakeClock, Tokens>;
fn driver(flags: &[char]) -> TestDriver {
    let mut i = identity('1');
    i.next_slot = 1;
    let mut peer = SlotPeer::default();
    peer.flags = flags.iter().map(|c| c.to_string().repeat(32)).collect();
    Driver::new(
        i,
        peer,
        Journal::default(),
        FakeClock(Rc::new(Cell::new(1000))),
        Tokens(10),
    )
    .unwrap()
}
fn run<J: StateJournal>(d: &mut Driver<SlotPeer, J, FakeClock, Tokens>) -> ChannelResult<()> {
    d.continue_until(
        &OperationControl::default(),
        target(),
        &mut Schedule {
            stop_after: 30,
            ..Schedule::default()
        },
    )
}
fn restart(d: TestDriver) -> TestDriver {
    let state: State =
        serde_json::from_slice(&serde_json::to_vec(&d.journal.events.last().unwrap().1).unwrap())
            .unwrap();
    Driver::from_state(state, d.backend, d.journal, d.clock, d.tokens).unwrap()
}
fn input_attempt(e: &SlotEnvelope) -> InputAttempt {
    InputAttempt {
        id: "c".repeat(32),
        exchange: e.nonce.clone(),
        runtime: e.runtime.clone(),
        kind: InputKind::Invoke,
        after_millis: 0,
        recovery_after_millis: 0,

        observation: None,
        outcome: None,
        after_diagnostic: None,
    }
}

#[test]
fn startup_slots_reload_before_any_bind_publication() {
    for required in [false, true] {
        let mut d = driver(if required { &['1'] } else { &[] });
        let original_id = d.state.id.clone();
        let deadline = d.state.connect_budget.as_ref().unwrap().deadline_ms;
        run(&mut d).unwrap();
        let runtime = if required { "2" } else { "1" }.repeat(32);
        let mut inputs = Vec::new();
        if required {
            inputs.push((InputKind::Reload, "1".repeat(32)));
        }
        inputs.push((InputKind::Invoke, runtime.clone()));
        assert_eq!(d.backend.inputs, inputs);
        assert_eq!(d.backend.publications.len(), 1);
        assert_eq!(d.backend.publications[0].runtime, runtime);
        assert_eq!(d.backend.publications[0].index, 1);
        assert_eq!(d.state.id, original_id);
        assert!(d.state.bound);
        if required {
            let r = d.state.reload.as_ref().unwrap();
            assert!(r.automatic && r.slot_enablement);
            assert_eq!(r.phase, ReloadPhase::Complete);
            assert_eq!(r.recovery_budget.as_ref().unwrap().deadline_ms, deadline);
            assert!(
                d.journal
                    .events
                    .iter()
                    .any(|(event, s)| event == "slot_enable_reload_intent"
                        && s.transaction.is_none()
                        && s.input.is_none())
            );
        } else {
            assert!(d.state.reload.is_none());
        }
    }
}

#[test]
fn explicit_recovery_two_to_three_retains_original_transaction_and_policy() {
    for policy in [OperationPolicy::Opaque, OperationPolicy::Observation] {
        let mut d = driver(&['2']);
        d.state.bound = true;
        d.state.identity.owner = d.state.owner.clone();
        d.state.identity.fence = 1;
        d.prepare_request(
            &OperationControl::default(),
            "original-business",
            "return 1",
            5,
            policy,
        )
        .unwrap();
        let op = d.state.operation.as_mut().unwrap();
        op.stage = OperationStage::Running;
        op.prepared_nonce = "a".repeat(32);
        op.challenge = "b".repeat(32);
        let original_operation = op.clone();
        d.begin(
            &OperationControl::default(),
            "confirm",
            original_operation.ticket.clone(),
            |_| {},
        )
        .unwrap();
        d.state.transaction.as_mut().unwrap().phase = TransactionPhase::InputAttempted;
        let original = d.state.transaction.as_ref().unwrap().envelope.clone();
        d.state.input = Some(input_attempt(&original));
        d.request_reload(&OperationControl::default(), "original-reload")
            .unwrap();
        let deadline = d
            .state
            .reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .deadline_ms;
        run(&mut d).unwrap();
        let d = restart(d);
        let reload = d.state.reload.as_ref().unwrap();
        assert_eq!(reload.request, "original-reload");
        assert!(!reload.automatic);
        assert!(reload.slot_enablement);
        assert_eq!(reload.phase, ReloadPhase::Complete);
        assert_eq!(
            reload.recovery_budget.as_ref().unwrap().deadline_ms,
            deadline
        );
        assert_eq!(
            d.backend.inputs,
            vec![
                (InputKind::Reload, "1".repeat(32)),
                (InputKind::Reload, "2".repeat(32)),
                (InputKind::Invoke, "3".repeat(32))
            ]
        );
        assert_eq!(d.backend.publications.len(), 1);
        assert_eq!(d.backend.publications[0].runtime, "3".repeat(32));
        assert_eq!(d.backend.peer.superseded, vec![original.clone()]);
        assert_eq!(
            d.state
                .recovery
                .as_ref()
                .unwrap()
                .transaction
                .as_ref()
                .unwrap()
                .envelope,
            original
        );
        let op = d.state.operation.as_ref().unwrap();
        assert_eq!(
            (&op.id, &op.request, op.budget),
            (
                &original_operation.id,
                &original_operation.request,
                original_operation.budget
            )
        );
        assert_eq!(
            op.recovery_budget.as_ref().unwrap().deadline_ms,
            original_operation
                .recovery_budget
                .as_ref()
                .unwrap()
                .deadline_ms
        );
        assert_eq!(
            (op.stage, op.attempt),
            if policy == OperationPolicy::Opaque {
                (OperationStage::ExecutionUnknown, 1)
            } else {
                (OperationStage::Prepared, 2)
            }
        );
    }
}

#[test]
fn durable_submitted_uncertain_or_missing_reload_outcome_never_replays() {
    durable_reload_unknown_never_replays();
}

fn durable_reload_unknown_never_replays() {
    use super::durable_journal::{DurableChannelJournal, tests::Fixture};
    for disposition in [
        Some(InputDisposition::Submitted),
        Some(InputDisposition::Uncertain),
        None,
    ] {
        let fixture = Fixture::new();
        let mut initial = driver(&['1']);
        initial.state.id = format!("CON-{}", "1".repeat(32));
        initial.state.owner = "1".repeat(32);
        fixture.seed([initial.state.clone()]);
        let mut d = Driver::from_state(
            initial.state,
            initial.backend,
            fixture.open(),
            initial.clock,
            initial.tokens,
        )
        .unwrap();
        d.backend.uncertain = true;
        assert!(has_code(
            &run(&mut d).unwrap_err(),
            "fixture.unknown_reload"
        ));
        assert_eq!(d.state.reload.as_ref().unwrap().phase, ReloadPhase::Intent);
        let deadline = d
            .state
            .reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .deadline_ms;
        match disposition {
            Some(value) => {
                d.state
                    .input
                    .as_mut()
                    .unwrap()
                    .outcome
                    .as_mut()
                    .unwrap()
                    .disposition = value
            }
            None => d.state.input.as_mut().unwrap().outcome = None,
        }
        d.save(
            &OperationControl::default(),
            "reload_outcome_crash_boundary_fixture",
        )
        .unwrap();
        for _ in 0..2 {
            d.journal.close().unwrap();
            let journal: DurableChannelJournal = fixture.open();
            let state = journal.load(&OperationControl::default()).unwrap();
            d = Driver::from_state(state, d.backend, journal, d.clock, d.tokens).unwrap();
            assert!(run(&mut d).is_err());
            assert_eq!(d.backend.inputs.len(), 1);
            assert!(d.backend.publications.is_empty());
            assert_eq!(
                d.state
                    .reload
                    .as_ref()
                    .unwrap()
                    .recovery_budget
                    .as_ref()
                    .unwrap()
                    .deadline_ms,
                deadline
            );
        }
        d.journal.close().unwrap();
    }
}

#[test]
fn combat_reload_resume_keeps_original_connection_deadline() {
    let mut d = driver(&['1']);
    d.backend.blocked = true;
    let deadline = d.state.connect_budget.as_ref().unwrap().deadline_ms;
    assert!(run(&mut d).is_err());
    assert_eq!(d.waiting, "combat");
    assert!(d.backend.inputs.is_empty());
    assert!(d.backend.publications.is_empty());
    let mut d = restart(d);
    d.backend.blocked = false;
    run(&mut d).unwrap();
    assert_eq!(
        d.state
            .reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .deadline_ms,
        deadline
    );
    assert_eq!(d.backend.inputs[0], (InputKind::Reload, "1".repeat(32)));
}

#[test]
fn unknown_invoke_precedes_slot_flag_and_does_not_observe_again() {
    for disposition in [
        None,
        Some(InputDisposition::Submitted),
        Some(InputDisposition::Uncertain),
    ] {
        let mut d = driver(&['1']);
        let e = SlotEnvelope {
            runtime: d.state.identity.runtime.clone(),
            nonce: "d".repeat(32),
            action: "bind".into(),
            ..SlotEnvelope::default()
        };
        let mut attempt = input_attempt(&e);
        attempt.outcome = disposition.map(|disposition| InputOutcome {
            disposition,
            messages_queued: 1,
            at_millis: 1100,
            reason: String::new(),
            retryable: false,
        });
        d.state.input = Some(attempt);
        d.submit_input(
            &OperationControl::default(),
            &e.nonce,
            &e.runtime,
            InputKind::Invoke,
            &e,
        )
        .unwrap();
        assert!(d.backend.inputs.is_empty());
        assert!(d.state.reload.is_none());
        assert_eq!(d.backend.peer.input_reads, 0);
    }
}

#[test]
fn invoke_flag_preserves_existing_transaction_before_reload_intent() {
    let mut d = driver(&['1']);
    d.state.bound = true;
    d.state.identity.owner = d.state.owner.clone();
    d.state.identity.fence = 1;
    d.prepare_request(
        &OperationControl::default(),
        "original-business",
        "return 1",
        5,
        OperationPolicy::Opaque,
    )
    .unwrap();
    let op = d.state.operation.as_ref().unwrap().clone();
    d.begin(
        &OperationControl::default(),
        "confirm",
        op.ticket.clone(),
        |_| {},
    )
    .unwrap();
    let e = d.state.transaction.as_ref().unwrap().envelope.clone();
    assert!(has_code(
        &d.submit_input(
            &OperationControl::default(),
            &e.nonce,
            &e.runtime,
            InputKind::Invoke,
            &e
        )
        .unwrap_err(),
        "live.channel_progress"
    ));
    assert_eq!(d.state.transaction.as_ref().unwrap().envelope, e);
    assert_eq!(d.state.operation.as_ref().unwrap(), &op);
    assert_eq!(
        d.state.reload.as_ref().unwrap().recovery_budget,
        op.recovery_budget
    );
    assert!(d.backend.inputs.is_empty());
}

#[test]
fn startup_faults_and_intent_save_failure_do_not_publish_or_send() {
    for fault in 0..4 {
        let mut d = driver(&['1']);
        match fault {
            0 => d.backend.input_fault = Some(error("fixture.read_failed")),
            1 => d.backend.invalid_readiness = true,
            2 => {
                d.backend.input_fault = Some(ChannelError::Pending {
                    causes: vec!["live.channel_input_observation_stale"],
                })
            }
            _ => d.journal.fail = Some("slot_enable_reload_intent"),
        }
        assert!(d.connect(&OperationControl::default()).is_err());
        assert!(d.backend.inputs.is_empty());
        assert!(d.backend.publications.is_empty());
        assert!(d.state.transaction.is_none());
        if fault == 3 {
            assert!(d.connect(&OperationControl::default()).is_err());
            assert!(d.backend.inputs.is_empty());
        }
    }
}

#[test]
fn flag_roundtrip_defaults_omits_false_and_changes_profile() {
    let original = super::recovery::test_support::input("1".repeat(32), String::new(), 0, 1, 1000);
    let raw = serde_json::to_value(&original).unwrap();
    assert!(raw.get("slotReloadRequired").is_none());
    let mut null_flag = raw.clone();
    null_flag["slotReloadRequired"] = serde_json::Value::Null;
    assert!(
        !serde_json::from_value::<InputObservation>(null_flag)
            .unwrap()
            .slot_reload_required
    );
    let mut flagged = original.clone();
    flagged.slot_reload_required = true;
    assert!(!flagged.same_input_profile(&original));
    assert!(!original.same_input_profile(&flagged));
    let raw = serde_json::to_value(&flagged).unwrap();
    assert_eq!(raw["slotReloadRequired"], true);
    let copy: InputObservation = serde_json::from_value(raw).unwrap();
    assert!(copy.slot_reload_required);
    assert_eq!(copy.observed_at_millis, 0);
    let mut d = driver(&['1']);
    assert!(d.connect(&OperationControl::default()).is_err());
    let mut raw = serde_json::to_value(&d.state).unwrap();
    assert_eq!(raw["reload"]["slotEnablement"], true);
    raw["reload"]
        .as_object_mut()
        .unwrap()
        .remove("slotEnablement");
    let unflagged: State = serde_json::from_value(raw).unwrap();
    assert!(!unflagged.reload.unwrap().slot_enablement);
}

#[test]
fn doctor_slot_activation_is_separate_read_only_diagnosis() {
    let mut sample =
        super::recovery::test_support::input("1".repeat(32), String::new(), 0, 1, 1000);
    sample.slot_reload_required = true;
    let mut report = super::doctor::DoctorResult::default();
    report.inspect_input(sample.clone());
    assert!(!report.healthy);
    assert!(
        report
            .checks
            .iter()
            .any(|c| c.id == "live.input" && c.state == "ok")
    );
    assert!(report.checks.iter().any(|c| c.id == "live.slot_activation"
        && c.state == "blocked"
        && c.next_step.contains("original CON")));
    assert_eq!(report.input, Some(sample));
}

#[test]
fn automatic_slot_reload_preserves_sticky_original_budget_after_restart() {
    let mut d = driver(&['1']);
    assert!(has_code(
        &d.connect(&OperationControl::default()).unwrap_err(),
        "live.channel_progress"
    ));
    let mut exhausted = d
        .state
        .reload
        .as_ref()
        .unwrap()
        .recovery_budget
        .clone()
        .unwrap();
    exhausted.last_observed_ms += 500;
    exhausted.exhausted = true;
    exhausted.clock_rollback = true;
    d.state.reload.as_mut().unwrap().recovery_budget = Some(exhausted.clone());
    d.save(
        &OperationControl::default(),
        "slot_budget_exhaustion_fixture",
    )
    .unwrap();
    let mut d = restart(d);
    assert_eq!(d.state.connect_budget, Some(exhausted));
    d.clock.0.set(1000);
    assert!(has_code(
        &run(&mut d).unwrap_err(),
        "live.channel_budget_clock_rollback"
    ));
    assert!(d.backend.inputs.is_empty());
    assert!(d.backend.publications.is_empty());
}

#[test]
fn unrelated_second_runtime_with_bind_reservation_is_still_blocked() {
    let mut d = driver(&[]);
    d.state.bound = true;
    d.state.identity.owner = d.state.owner.clone();
    d.state.identity.fence = 1;
    d.prepare_request(
        &OperationControl::default(),
        "business",
        "return 1",
        5,
        OperationPolicy::Opaque,
    )
    .unwrap();
    let ticket = d.state.operation.as_ref().unwrap().ticket.clone();
    d.begin(&OperationControl::default(), "confirm", ticket, |_| {})
        .unwrap();
    let original = d.state.transaction.as_ref().unwrap().clone();
    d.request_reload(&OperationControl::default(), "explicit")
        .unwrap();
    d.backend.peer.pending_bind = true;
    assert!(
        d.recover_runtime(&OperationControl::default(), identity('2'))
            .unwrap_err()
            .is_pending()
    );
    let bind = d.state.transaction.as_ref().unwrap().clone();
    // Even a slot-flagged reload may not discard an existing fresh bind.
    let reload = d.state.reload.as_mut().unwrap();
    reload.slot_enablement = true;
    reload.from = "2".repeat(32);
    reload.phase = ReloadPhase::InputAttempted;
    let result = d.recover_runtime(&OperationControl::default(), identity('3'));
    assert!(
        matches!(result, Err(ChannelError::Blocked(ref b)) if b.kind == "runtime_changed_during_recovery")
    );
    assert_eq!(d.state.transaction.as_ref().unwrap(), &bind);
    assert_eq!(
        d.state
            .recovery
            .as_ref()
            .unwrap()
            .transaction
            .as_ref()
            .unwrap(),
        &original
    );
}

#[test]
fn automatic_slot_reload_merges_operation_sticky_budget_only() {
    let mut d = driver(&[]);
    d.state.bound = true;
    d.prepare_request(
        &OperationControl::default(),
        "business",
        "return 1",
        5,
        OperationPolicy::Opaque,
    )
    .unwrap();
    let connect = d.state.connect_budget.clone();
    assert!(has_code(
        &d.request_slot_reload(&OperationControl::default())
            .unwrap_err(),
        "live.channel_progress"
    ));
    let mut sticky = d
        .state
        .reload
        .as_ref()
        .unwrap()
        .recovery_budget
        .clone()
        .unwrap();
    sticky.last_observed_ms += 500;
    sticky.exhausted = true;
    sticky.clock_rollback = true;
    d.state.reload.as_mut().unwrap().recovery_budget = Some(sticky.clone());
    assert!(super::recovery::synchronize_automatic_budget(&mut d.state).unwrap());
    assert_eq!(
        d.state.operation.as_ref().unwrap().recovery_budget,
        Some(sticky.clone())
    );
    assert_eq!(
        d.state.reload.as_ref().unwrap().recovery_budget,
        Some(sticky)
    );
    assert_eq!(d.state.connect_budget, connect);
    assert!(d.state.close_budget.is_none());
    d.save(&OperationControl::default(), "sticky_operation_fixture")
        .unwrap();
    let mut d = restart(d);
    d.clock.0.set(1000);
    assert!(has_code(
        &run(&mut d).unwrap_err(),
        "live.channel_budget_clock_rollback"
    ));
    assert!(d.backend.inputs.is_empty());
}

#[test]
fn automatic_slot_reload_merges_close_sticky_budget_only() {
    let mut d = driver(&[]);
    d.state.bound = true;
    d.request_close(&OperationControl::default()).unwrap();
    let connect = d.state.connect_budget.clone();
    assert!(has_code(
        &d.request_slot_reload(&OperationControl::default())
            .unwrap_err(),
        "live.channel_progress"
    ));
    let mut sticky = d.state.close_budget.clone().unwrap();
    sticky.last_observed_ms += 500;
    sticky.exhausted = true;
    sticky.clock_rollback = true;
    d.state.close_budget = Some(sticky.clone());
    assert!(super::recovery::synchronize_automatic_budget(&mut d.state).unwrap());
    assert_eq!(
        d.state.reload.as_ref().unwrap().recovery_budget,
        Some(sticky.clone())
    );
    assert_eq!(d.state.close_budget, Some(sticky));
    assert_eq!(d.state.connect_budget, connect);
    assert!(d.state.operation.is_none());
    d.save(&OperationControl::default(), "sticky_close_fixture")
        .unwrap();
    let mut d = restart(d);
    d.clock.0.set(1000);
    assert!(has_code(
        &run(&mut d).unwrap_err(),
        "live.channel_budget_clock_rollback"
    ));
    assert!(d.backend.inputs.is_empty());
}

#[test]
fn slot_budget_mismatch_or_missing_origin_cannot_pollute_other_goals() {
    for mismatch in 0..3 {
        let mut d = driver(&[]);
        assert!(has_code(
            &d.request_slot_reload(&OperationControl::default())
                .unwrap_err(),
            "live.channel_progress"
        ));
        let b = d
            .state
            .reload
            .as_mut()
            .unwrap()
            .recovery_budget
            .as_mut()
            .unwrap();
        b.exhausted = true;
        b.clock_rollback = true;
        match mismatch {
            0 => {
                b.started_at_ms += 1;
                b.last_observed_ms += 1;
            }
            1 => b.deadline_ms += 1,
            _ => d.state.connect_budget = None,
        }
        let before = d.state.clone();
        assert!(!super::recovery::synchronize_automatic_budget(&mut d.state).unwrap());
        assert_eq!(d.state, before);
        d.state.validate().unwrap();
    }
}

#[test]
fn connect_slot_source_does_not_alias_same_millisecond_close() {
    let mut d = driver(&[]);
    assert!(has_code(
        &d.request_slot_reload(&OperationControl::default())
            .unwrap_err(),
        "live.channel_progress"
    ));
    d.request_close(&OperationControl::default()).unwrap();
    assert_eq!(d.state.connect_budget, d.state.close_budget);
    let close = d.state.close_budget.clone();
    let mut sticky = d
        .state
        .reload
        .as_ref()
        .unwrap()
        .recovery_budget
        .clone()
        .unwrap();
    sticky.last_observed_ms += 500;
    sticky.exhausted = true;
    sticky.clock_rollback = true;
    d.state.reload.as_mut().unwrap().recovery_budget = Some(sticky.clone());
    assert!(super::recovery::synchronize_automatic_budget(&mut d.state).unwrap());
    assert_eq!(d.state.connect_budget, Some(sticky));
    assert_eq!(d.state.close_budget, close);
    d.save(&OperationControl::default(), "source_collision_fixture")
        .unwrap();
    let d = restart(d);
    assert_eq!(
        d.state.reload.as_ref().unwrap().slot_budget_source,
        Some(SlotBudgetSource::Connect)
    );
    assert_eq!(d.state.close_budget, close);
    assert_ne!(d.state.connect_budget, d.state.close_budget);
}

#[test]
fn slot_source_cannot_link_replacement_operation_with_equal_budget() {
    let mut d = driver(&[]);
    d.state.bound = true;
    d.prepare_request(
        &OperationControl::default(),
        "original",
        "return 1",
        5,
        OperationPolicy::Opaque,
    )
    .unwrap();
    let original = d.state.operation.as_ref().unwrap().id.clone();
    assert!(has_code(
        &d.request_slot_reload(&OperationControl::default())
            .unwrap_err(),
        "live.channel_progress"
    ));
    assert_eq!(
        d.state.reload.as_ref().unwrap().slot_budget_source,
        Some(SlotBudgetSource::Operation {
            operation_id: original
        })
    );
    // A later operation can have equal wall-clock values, but is not the source.
    d.state.operation.as_mut().unwrap().id = format!("LMO-{}", "e".repeat(32));
    let b = d
        .state
        .reload
        .as_mut()
        .unwrap()
        .recovery_budget
        .as_mut()
        .unwrap();
    b.exhausted = true;
    b.clock_rollback = true;
    let before = d.state.clone();
    assert!(!super::recovery::synchronize_automatic_budget(&mut d.state).unwrap());
    assert_eq!(d.state, before);
    d.save(
        &OperationControl::default(),
        "replaced_operation_source_fixture",
    )
    .unwrap();
    let d = restart(d);
    assert_eq!(d.state, d.journal.events.last().unwrap().1);
    assert!(
        !d.state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .exhausted
    );
}
