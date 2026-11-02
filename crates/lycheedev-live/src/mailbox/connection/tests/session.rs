//! Public service entry points, real durable journals and execution leases.
//! Native failures are explicit models, not Windows handle/input evidence.
use super::*;
use crate::mailbox::{
    driver::budget,
    durable_journal::tests::fail_lease_close,
    state::{RecoveryPhase, RuntimeRecovery},
};

fn probe_close_under_lease(service: &mut Service, fixture: &Fixture) {
    service.environment.close_lease_probe = Some((fixture.root.clone(), fixture.id.clone()));
}

fn assert_lease_released(fixture: &Fixture) {
    let (_, mut next) = fixture.book().acquire(&fixture.id, &control()).unwrap();
    next.close().unwrap();
}

#[test]
fn session_owner_success_closes_native_under_lease_on_acquiring_thread() {
    let fixture = Fixture::new();
    fixture.edit(|state| state.bound = true);
    let mut service = fixture.service();
    probe_close_under_lease(&mut service, &fixture);
    let outcome = service.resume(&fixture.id, &control());
    assert!(outcome.error().is_none(), "{outcome:?}");
    let result = outcome.into_report().unwrap();
    assert!(result.bound && result.complete && !result.closed);
    let counts = service.environment.counts.borrow();
    assert_eq!(
        (counts.opens, counts.closes, counts.closes_under_lease),
        (1, 1, 1)
    );
    assert_eq!(counts.input, 0);
    drop(counts);
    assert_lease_released(&fixture);
}

#[test]
fn session_owner_partial_open_joins_native_and_lease_failures_once() {
    let fixture = Fixture::new();
    let original = fixture.state();
    let log = fs::read(fixture.log()).unwrap();
    let mut service = fixture.service();
    probe_close_under_lease(&mut service, &fixture);
    service.environment.partial_open_error = Some(error("fixture.partial_open_failed"));
    service.environment.close_error = Some(error("fixture.native_close_failed"));
    service.journal_fault = Some(fail_lease_close);

    let outcome = service.resume(&fixture.id, &control());
    assert_eq!(
        outcome.error(),
        Some(&AdmissionError::Joined(vec![
            ChannelError::Joined {
                causes: vec![
                    error("fixture.partial_open_failed"),
                    error("fixture.native_close_failed"),
                ],
            }
            .into(),
            error("fixture.journal_io_failure").into(),
        ])),
    );
    let result = outcome.into_report().unwrap();
    assert_eq!(result.session, fixture.id);
    assert_eq!(fixture.state(), original);
    assert_eq!(fs::read(fixture.log()).unwrap(), log);
    let counts = service.environment.counts.borrow();
    assert_eq!(
        (counts.opens, counts.closes, counts.closes_under_lease),
        (1, 1, 1)
    );
    assert_eq!(counts.input, 0);
    drop(counts);
    assert_lease_released(&fixture);
}

#[test]
fn session_owner_unknown_result_preserves_ordered_close_errors_without_replay() {
    let fixture = Fixture::new();
    fixture.operation(OperationStage::ExecutionUnknown, "unknown-close-errors");
    let original = fixture.state().operation.unwrap();
    let mut service = fixture.service();
    probe_close_under_lease(&mut service, &fixture);
    service.environment.close_error = Some(error("fixture.native_close_failed"));
    service.journal_fault = Some(fail_lease_close);

    let outcome = service.execute(&fixture.id, request("unknown-close-errors"), &control());
    assert_eq!(
        outcome.error(),
        Some(&AdmissionError::Joined(vec![
            AdmissionError::Joined(vec![
                error("live.channel_execution_unknown").into(),
                error("fixture.native_close_failed").into(),
            ]),
            error("fixture.journal_io_failure").into(),
        ])),
    );
    let result = outcome.into_report().unwrap();
    assert!(!result.complete && !result.closed);
    assert_eq!(result.report_state, "unavailable");
    assert_eq!(fixture.state().operation.unwrap(), original);
    let counts = service.environment.counts.borrow();
    assert_eq!(
        (counts.opens, counts.closes, counts.closes_under_lease),
        (1, 1, 1)
    );
    assert_eq!(counts.input, 0);
    assert!(counts.published.is_empty());
    drop(counts);
    assert_lease_released(&fixture);
}

#[test]
fn session_owner_lease_close_failure_defers_claim_retirement_on_both_routes() {
    for process_ended in [false, true] {
        let fixture = Fixture::new();
        fixture.operation(OperationStage::Complete, "closed-report");
        let original = fixture.state().operation.unwrap();
        let mut service = fixture.service();
        probe_close_under_lease(&mut service, &fixture);
        if process_ended {
            service.environment.end = ProcessEnd::ProcessExited;
        }
        service.journal_fault = Some(fail_lease_close);
        let outcome = service.disconnect(&fixture.id, &control());
        assert_eq!(
            outcome.error(),
            Some(&error("fixture.journal_io_failure").into())
        );
        let result = outcome.into_report().unwrap();
        assert!(result.closed && result.complete && !result.bound);
        assert_eq!(result.report_state, "verified");
        assert_eq!(result.report.unwrap().0, original.result_bytes.0);
        assert!(fixture.state().closed);
        assert_eq!(
            fixture.state().operation.unwrap().result_bytes,
            original.result_bytes
        );
        let owner = inspect_window_owner(
            &fixture.target.addon_parent(),
            &fixture.target.resource(),
            &control(),
        );
        assert!(owner.error.is_none(), "{:?}", owner.error);
        assert_eq!(owner.owner.unwrap().operation_id, fixture.id);
        assert_lease_released(&fixture);
        let counts = service.environment.counts.borrow();
        let expected_native = usize::from(!process_ended);
        assert_eq!(
            (counts.opens, counts.closes),
            (expected_native, expected_native)
        );
        assert_eq!(counts.closes_under_lease, expected_native);
        drop(counts);

        // A separately constructed invocation observes the saved closed state
        // and retires the retained claim without opening a native backend.
        let log = fs::read(fixture.log()).unwrap();
        let mut recovery = fixture.service();
        let outcome = recovery.resume(&fixture.id, &control());
        assert!(outcome.error().is_none(), "{outcome:?}");
        assert!(outcome.into_report().unwrap().closed);
        assert_eq!(fs::read(fixture.log()).unwrap(), log);
        assert_eq!(recovery.environment.counts.borrow().opens, 0);
        let owner = inspect_window_owner(
            &fixture.target.addon_parent(),
            &fixture.target.resource(),
            &control(),
        );
        assert!(owner.error.is_none(), "{:?}", owner.error);
        assert!(!owner.occupied && owner.owner.is_none());
    }
}

#[test]
fn session_owner_cancelled_submitted_input_keeps_deadline_and_resumes_without_replay() {
    let fixture = Fixture::new();
    let original_budget = fixture.state().connect_budget.unwrap();
    let mut service = fixture.service();
    probe_close_under_lease(&mut service, &fixture);
    service.environment.cancel_input = true;
    service.environment.close_error = Some(error("fixture.native_close_failed"));
    service.journal_fault = Some(fail_lease_close);

    let outcome = service.resume(&fixture.id, &control());
    assert!(code(&outcome, "command.cancelled"));
    assert!(code(&outcome, "fixture.native_close_failed"));
    assert!(code(&outcome, "fixture.journal_io_failure"));
    let saved = fixture.state();
    assert!(!saved.closed);
    let transaction = saved.transaction.as_ref().unwrap();
    assert_eq!(transaction.envelope.action, "bind");
    // The input outcome is durably flushed, but cancellation prevents the
    // following transaction-phase save. Published is deliberately retained.
    assert_eq!(transaction.phase, TransactionPhase::Published);
    let input = saved.input.as_ref().unwrap();
    assert_eq!(input.exchange, transaction.envelope.nonce);
    assert_eq!(
        input.outcome.as_ref().unwrap().disposition,
        crate::mailbox::InputDisposition::Submitted,
    );
    assert_eq!(
        saved.connect_budget.as_ref().unwrap().deadline_ms,
        original_budget.deadline_ms
    );
    let counts = service.environment.counts.borrow();
    assert_eq!(
        (counts.input, counts.closes, counts.closes_under_lease),
        (1, 1, 1)
    );
    drop(counts);
    assert_lease_released(&fixture);

    let mut recovery = fixture.service();
    probe_close_under_lease(&mut recovery, &fixture);
    let outcome = recovery.resume(&fixture.id, &control());
    assert!(outcome.error().is_none(), "{outcome:?}");
    assert!(outcome.into_report().unwrap().bound);
    let after = fixture.state();
    assert!(after.bound && after.transaction.is_none());
    assert_eq!(after.input, saved.input);
    assert_eq!(
        after.connect_budget.unwrap().deadline_ms,
        original_budget.deadline_ms
    );
    let counts = recovery.environment.counts.borrow();
    assert_eq!(
        (counts.input, counts.closes, counts.closes_under_lease),
        (0, 1, 1)
    );
    assert!(counts.published.is_empty());
    drop(counts);
    assert_lease_released(&fixture);
}

#[test]
fn session_owner_policy_recovery_retains_observation_attempt_and_opaque_unknown_contracts() {
    for policy in [OperationPolicy::Opaque, OperationPolicy::Observation] {
        let fixture = Fixture::new();
        fixture.operation(OperationStage::ConfirmReady, "recovered");
        fixture.edit(|state| {
            let operation = state.operation.as_mut().unwrap();
            operation.policy = policy;
            operation.recovery_budget = Some(budget(1000, 8000).unwrap());
            let prior = Transaction {
                envelope: SlotEnvelope {
                    schema: SCHEMA.into(),
                    runtime: state.identity.runtime.clone(),
                    owner: state.owner.clone(),
                    guid: state.identity.guid.clone(),
                    build: state.identity.build.clone(),
                    index: 1,
                    nonce: "9".repeat(32),
                    ticket: operation.ticket.clone(),
                    fence: 1,
                    action: "confirm".into(),
                    ..Default::default()
                },
                phase: TransactionPhase::InputAttempted,
                release_wait_started_at_ms: 0,
                receipt: None,

                input_observation: None,
            };
            state.recovery = Some(RuntimeRecovery {
                from: state.identity.clone(),
                transaction: Some(prior),
                phase: RecoveryPhase::Bound,
            });
            state.identity = identity('3');
        });
        let before = fixture.state();
        let original = before.operation.unwrap();
        let prior = before.recovery.unwrap().transaction.unwrap().envelope;
        let mut service = fixture.service();
        service.environment.prepare_pending = policy == OperationPolicy::Observation;
        probe_close_under_lease(&mut service, &fixture);
        let outcome = service.resume(&fixture.id, &control());
        let after = fixture.state();
        assert_eq!(after.recovery.unwrap().phase, RecoveryPhase::Complete);
        let operation = after.operation.unwrap();
        assert_eq!(operation.id, original.id);
        assert_eq!(operation.request, original.request);
        assert_eq!(operation.recovery_budget, original.recovery_budget);
        let counts = service.environment.counts.borrow();
        assert_eq!(counts.superseded, [prior]);
        assert_eq!(
            (counts.opens, counts.closes, counts.closes_under_lease),
            (1, 1, 1)
        );
        if policy == OperationPolicy::Opaque {
            assert!(code(&outcome, "live.channel_execution_unknown"));
            assert_eq!(operation.stage, OperationStage::ExecutionUnknown);
            assert_eq!(operation.ticket, original.ticket);
            assert_eq!(operation.result_bytes, original.result_bytes);
            assert_eq!(counts.input, 0);
            assert!(counts.published.is_empty());
        } else {
            // The existing protocol peer has no business prepare receipt. The
            // bounded scheduler cancels that wait after the permitted retry;
            // it must not turn Observation recovery into opaque unknown.
            assert!(code(&outcome, "command.cancelled"));
            assert_eq!(operation.stage, OperationStage::Prepared);
            assert_eq!(operation.attempt, original.attempt + 1);
            assert_ne!(operation.ticket, original.ticket);
            assert_eq!(operation.origin.unwrap().runtime, identity('3').runtime);
            assert!(operation.result_bytes.0.is_empty());
            assert_eq!(counts.input, 1);
            assert_eq!(counts.published.len(), 1);
            assert_eq!(counts.published[0].action, "prepare");
            assert_eq!(counts.published[0].ticket, operation.ticket);
            assert_eq!(counts.published[0].runtime, identity('3').runtime);
        }
        drop(counts);
        assert_lease_released(&fixture);
    }
}
