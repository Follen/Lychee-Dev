use super::super::recovery::test_support::*;
use super::super::state::{InputAttempt, TransactionPhase};
use super::super::{InputDisposition, InputOutcome};
use super::*;

#[test]
fn automatic_cleanup_reload_keeps_original_exchange_and_deadline() {
    let mut d = driver(
        OperationStage::ReleaseReady,
        super::super::state::OperationPolicy::Opaque,
    );
    transaction(&mut d, "release", TransactionPhase::InputAttempted);
    let envelope = d.state.transaction.as_ref().unwrap().envelope.clone();
    d.state
        .transaction
        .as_mut()
        .unwrap()
        .release_wait_started_at_ms = 1000;
    d.clock.0.set(6000);
    let deadline = d.state.operation.as_ref().unwrap().recovery_budget.clone();
    assert!(has_code(
        &d.recover_missing_release(&OperationControl::default(), pending())
            .unwrap_err(),
        "live.channel_progress"
    ));
    let r = d.state.reload.as_ref().unwrap();
    assert!(r.automatic && r.reconcile_pending);
    assert_eq!(r.recovery_budget, deadline);
    assert_eq!(d.state.transaction.as_ref().unwrap().envelope, envelope);
    assert_eq!(
        d.state.operation.as_ref().unwrap().cleanup_method,
        CleanupMethod::None
    );
    assert!(d.backend.input_kinds.is_empty());
}
#[test]
fn unknown_reload_input_is_never_replayed_on_resume() {
    for outcome in [
        None,
        Some(InputOutcome {
            disposition: InputDisposition::Uncertain,
            messages_queued: 1,
            at_millis: 0,
            reason: "interrupted".into(),
            retryable: false,
        }),
    ] {
        let mut d = driver(
            OperationStage::Running,
            super::super::state::OperationPolicy::Opaque,
        );
        d.request_reload(&OperationControl::default(), "recover")
            .unwrap();
        d.state.input = Some(InputAttempt {
            id: "c".repeat(32),
            exchange: "reload:recover".into(),
            runtime: d.state.identity.runtime.clone(),
            kind: InputKind::Reload,
            after_millis: 0,
            recovery_after_millis: 0,
            escape_attempts: 0,
            observation: None,
            outcome,
            after_diagnostic: None,
        });
        assert!(has_code(
            &d.step_reload(&OperationControl::default(), target())
                .unwrap_err(),
            "live.channel_progress"
        ));
        assert_eq!(
            d.state.reload.as_ref().unwrap().phase,
            ReloadPhase::InputAttempted
        );
        assert!(
            d.step_reload(&OperationControl::default(), target())
                .unwrap_err()
                .is_pending()
        );
        assert!(d.backend.input_kinds.is_empty());
        assert_eq!(d.backend.input_reads, 0);
    }
}
#[test]
fn reload_control_completion_does_not_promote_unknown_business_result() {
    let mut d = driver(
        OperationStage::ConfirmReady,
        super::super::state::OperationPolicy::Opaque,
    );
    transaction(&mut d, "confirm", TransactionPhase::InputAttempted);
    d.backend.candidate = Some(identity('2'));
    let result = d.state.operation.as_ref().unwrap().result_bytes.clone();
    d.request_reload(&OperationControl::default(), "recover")
        .unwrap();
    d.continue_until(
        &OperationControl::default(),
        target(),
        &mut Schedule::default(),
    )
    .unwrap();
    assert_eq!(
        d.state.reload.as_ref().unwrap().phase,
        ReloadPhase::Complete
    );
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::ExecutionUnknown
    );
    assert_eq!(d.state.operation.as_ref().unwrap().result_bytes, result);
    assert_eq!(
        d.backend.input_kinds,
        [InputKind::Reload, InputKind::Invoke]
    );
    assert!(has_code(
        &d.continue_until(
            &OperationControl::default(),
            target(),
            &mut Schedule::default()
        )
        .unwrap_err(),
        "live.channel_execution_unknown"
    ));
    assert_eq!(d.backend.input_kinds.len(), 2);
}
#[test]
fn closing_initial_bind_uses_reload_and_proof_without_binding_replacement() {
    let mut d = driver(
        OperationStage::Prepared,
        super::super::state::OperationPolicy::Opaque,
    );
    d.state.operation = None;
    d.state.bound = false;
    d.begin(
        &OperationControl::default(),
        "bind",
        d.state.owner.clone(),
        |_| {},
    )
    .unwrap();
    d.state.transaction.as_mut().unwrap().phase = TransactionPhase::InputAttempted;
    d.request_close(&OperationControl::default()).unwrap();
    d.request_reload(&OperationControl::default(), "close-initial")
        .unwrap();
    d.backend.proof = Some(proof());
    d.continue_until(
        &OperationControl::default(),
        target(),
        &mut Schedule::default(),
    )
    .unwrap();
    assert!(d.state.closed);
    assert!(!d.state.bound);
    assert_eq!(
        d.state.reload.as_ref().unwrap().phase,
        ReloadPhase::Complete
    );
    assert!(d.backend.actions.is_empty());
    assert!(d.backend.input_kinds.is_empty());
    assert_eq!(d.backend.retired.len(), 1);
}
#[test]
fn expired_auto_reload_never_gets_new_budget_or_input() {
    let mut d = driver(
        OperationStage::ReleaseReady,
        super::super::state::OperationPolicy::Opaque,
    );
    d.state.operation.as_mut().unwrap().cleanup_method = CleanupMethod::ReloadRequired;
    let deadline = d
        .state
        .operation
        .as_ref()
        .unwrap()
        .recovery_budget
        .as_ref()
        .unwrap()
        .deadline_ms;
    assert!(has_code(
        &d.next_step(&OperationControl::default(), target())
            .unwrap_err(),
        "live.channel_progress"
    ));
    d.clock.0.set(deadline);
    assert!(has_code(
        &d.continue_until(
            &OperationControl::default(),
            target(),
            &mut Schedule::default()
        )
        .unwrap_err(),
        "live.channel_budget_exhausted"
    ));
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
    d.clock.0.set(1001);
    assert!(
        d.continue_until(
            &OperationControl::default(),
            target(),
            &mut Schedule::default()
        )
        .is_err()
    );
    assert!(d.backend.input_kinds.is_empty());
}
#[test]
fn explicit_reload_owns_new_control_budget_but_keeps_business_budget() {
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    let old = d.state.operation.as_ref().unwrap().recovery_budget.clone();
    d.clock.0.set(2000);
    d.request_close(&OperationControl::default()).unwrap();
    d.request_reload(&OperationControl::default(), "control")
        .unwrap();
    assert_eq!(goal(&d.state), Some(super::super::recovery::Goal::Reload));
    assert_eq!(d.state.operation.as_ref().unwrap().recovery_budget, old);
    assert_eq!(
        d.state
            .reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .started_at_ms,
        2000
    );
}
#[test]
fn publication_gaps_retry_only_in_defined_phase_and_preserve_other_faults() {
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    for code in [
        "memory.mailbox_unavailable: lua_state_null",
        "memory.mailbox_unavailable: string_layout",
    ] {
        let err = pending_observation(&d.state, error(code));
        assert!(!err.is_pending());
    }
    d.request_reload(&OperationControl::default(), "reload")
        .unwrap();
    d.state.reload.as_mut().unwrap().phase = ReloadPhase::InputAttempted;
    for code in [
        "memory.mailbox_unavailable: path_changed",
        "memory.mailbox_unavailable: lua_state_null",
        "memory.mailbox_unavailable: string_layout",
    ] {
        let err = pending_observation(&d.state, error(code));
        assert!(retryable_pending(&d.state, &err));
        let joined = pending_observation(
            &d.state,
            join_errors(error(code), error("fixture.identity_changed")),
        );
        assert!(!retryable_pending(&d.state, &joined));
        assert!(has_code(&joined, "fixture.identity_changed"));
    }
}
#[test]
fn discovery_cooldown_and_wait_are_bounded_and_injected() {
    assert_eq!(discovery_cooldown_millis(0), 1000);
    assert_eq!(discovery_cooldown_millis(2345), 2345);
    assert_eq!(discovery_cooldown_millis(90000), 30000);
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    let mut schedule = Schedule {
        stop_after: 9,
        ..Schedule::default()
    };
    let err = d
        .continue_until(&OperationControl::default(), target(), &mut schedule)
        .unwrap_err();
    assert!(err.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
    assert_eq!(schedule.waits, 9);
    assert_eq!(d.backend.candidate_reads, 3);
    assert!(d.backend.input_kinds.is_empty());
}
#[test]
fn completed_close_bookkeeping_works_after_its_budget_expires() {
    let mut d = driver(
        OperationStage::Cancelled,
        super::super::state::OperationPolicy::Opaque,
    );
    d.state.bound = false;
    d.request_close(&OperationControl::default()).unwrap();
    d.clock
        .0
        .set(d.state.close_budget.as_ref().unwrap().deadline_ms);
    d.continue_until(
        &OperationControl::default(),
        target(),
        &mut Schedule::default(),
    )
    .unwrap();
    assert!(d.state.closed);
    assert!(d.backend.input_kinds.is_empty());
    assert_eq!(d.continuation().kind, "completed");
}

#[test]
fn cleanup_reload_recovers_missing_input_time_once_without_moving_bound() {
    let mut d = driver(
        OperationStage::ReleaseReady,
        super::super::state::OperationPolicy::Opaque,
    );
    transaction(&mut d, "release", TransactionPhase::InputAttempted);
    let tx = d.state.transaction.as_ref().unwrap();
    d.state.input = Some(InputAttempt {
        id: "c".repeat(32),
        exchange: tx.envelope.nonce.clone(),
        runtime: tx.envelope.runtime.clone(),
        kind: InputKind::Invoke,
        after_millis: 0,
        recovery_after_millis: 0,
        escape_attempts: 0,
        observation: None,
        outcome: None,
        after_diagnostic: None,
    });
    d.state
        .transaction
        .as_mut()
        .unwrap()
        .release_wait_started_at_ms = 1000;
    d.clock.0.set(6000);
    assert!(has_code(
        &d.recover_missing_release(&OperationControl::default(), pending())
            .unwrap_err(),
        "live.channel_progress"
    ));
    assert!(has_code(
        &d.step_reload(&OperationControl::default(), target())
            .unwrap_err(),
        "live.channel_input_observation_stale"
    ));
    assert_eq!(d.state.input.as_ref().unwrap().recovery_after_millis, 200);
    assert!(d.backend.input_kinds.is_empty());
    let mut d = restart(d);
    assert!(has_code(
        &d.step_reload(&OperationControl::default(), target())
            .unwrap_err(),
        "live.channel_progress"
    ));
    assert_eq!(d.backend.input_kinds, [InputKind::Reload]);
    assert_eq!(
        d.journal
            .events
            .iter()
            .filter(|(kind, _)| kind == "input_time_recovered")
            .count(),
        1
    );
    let saved = d
        .journal
        .events
        .iter()
        .find(|(kind, _)| kind == "input_time_recovered")
        .unwrap();
    assert_eq!(saved.1.input.as_ref().unwrap().recovery_after_millis, 200);
}

#[test]
fn monotonic_goal_expiry_flushes_sticky_budget_after_preappend_stop() {
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    d.request_reload(&OperationControl::default(), "deadline-control")
        .unwrap();
    d.state.reload.as_mut().unwrap().recovery_budget = Some(budget(1000, 200).unwrap());
    d.backend.expire_after_input = true;
    let result = d.continue_until(
        &OperationControl::default(),
        target(),
        &mut Schedule::default(),
    );
    assert!(has_code(
        &result.unwrap_err(),
        "live.channel_budget_exhausted"
    ));
    assert_eq!(
        d.clock.0.get(),
        1000,
        "wall clock intentionally did not advance"
    );
    assert_eq!(d.backend.input_kinds, [InputKind::Reload]);
    assert!(
        d.journal
            .events
            .iter()
            .any(|(event, _)| event == "input_observed")
    );
    assert!(
        !d.journal
            .events
            .iter()
            .any(|(event, _)| event == "reload_input_attempted")
    );
    let last = &d.journal.events.last().unwrap().1;
    assert!(
        last.reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .exhausted
    );
    assert_eq!(
        last.reload.as_ref().unwrap().phase,
        ReloadPhase::InputAttempted
    );
    let mut d = restart(d);
    d.backend.expire_after_input = false;
    assert!(has_code(
        &d.continue_until(
            &OperationControl::default(),
            target(),
            &mut Schedule::default()
        )
        .unwrap_err(),
        "live.channel_budget_exhausted"
    ));
    assert_eq!(d.backend.input_kinds, [InputKind::Reload]);
}

fn automatic_reload(path: usize) -> TestDriver {
    let mut d = driver(
        if path == 2 {
            OperationStage::ReleaseReady
        } else {
            OperationStage::Prepared
        },
        super::super::state::OperationPolicy::Opaque,
    );
    match path {
        0 => {
            d.state.identity.next_slot = 486;
            assert!(has_code(
                &d.next_step(&OperationControl::default(), target())
                    .unwrap_err(),
                "live.channel_progress"
            ));
        }
        1 => {
            d.backend.reject_capacity = true;
            assert!(has_code(
                &d.run(&OperationControl::default()).unwrap_err(),
                "live.channel_reload_required"
            ));
        }
        _ => {
            transaction(&mut d, "release", TransactionPhase::InputAttempted);
            d.state
                .transaction
                .as_mut()
                .unwrap()
                .release_wait_started_at_ms = 1000;
            d.clock.0.set(6000);
            assert!(has_code(
                &d.recover_missing_release(&OperationControl::default(), pending())
                    .unwrap_err(),
                "live.channel_progress"
            ));
        }
    }
    d
}

#[test]
fn all_automatic_reload_paths_preserve_shared_budget_facts_after_roundtrip() {
    for path in 0..3 {
        let mut d = automatic_reload(path);
        let original = d
            .state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .clone()
            .unwrap();
        assert_eq!(
            d.state.reload.as_ref().unwrap().recovery_budget.as_ref(),
            Some(&original)
        );
        d.clock.0.set(7000);
        d.budget_control(&OperationControl::default()).unwrap();
        let op_budget = d
            .state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap();
        assert_eq!(op_budget.last_observed_ms, 7000);
        assert_eq!(op_budget.deadline_ms, original.deadline_ms);
        assert_eq!(
            d.state.reload.as_ref().unwrap().recovery_budget.as_ref(),
            Some(op_budget)
        );
        let mut d = restart(d);
        d.state.reload.as_mut().unwrap().phase = ReloadPhase::Complete;
        d.save(&OperationControl::default(), "fixture_reload_complete")
            .unwrap();
        let mut d = restart(d);
        let inputs = d.backend.input_kinds.len();
        d.clock.0.set(6500);
        let err = d.budget_control(&OperationControl::default()).unwrap_err();
        assert!(has_code(&err, "live.channel_budget_clock_rollback"));
        for b in [
            d.state
                .operation
                .as_ref()
                .unwrap()
                .recovery_budget
                .as_ref()
                .unwrap(),
            d.state
                .reload
                .as_ref()
                .unwrap()
                .recovery_budget
                .as_ref()
                .unwrap(),
        ] {
            assert_eq!(b.last_observed_ms, 7000);
            assert!(b.exhausted && b.clock_rollback);
            assert_eq!(b.deadline_ms, original.deadline_ms);
        }
        let mut d = restart(d);
        d.clock.0.set(8000);
        assert!(has_code(
            &d.budget_control(&OperationControl::default()).unwrap_err(),
            "live.channel_budget_clock_rollback"
        ));
        assert_eq!(d.backend.input_kinds.len(), inputs);
    }
}

#[test]
fn stale_persisted_automatic_view_recovers_maximum_and_sticky_facts_on_load() {
    let d = automatic_reload(0);
    for sticky in [false, true] {
        let mut state = d.state.clone();
        let reload = state.reload.as_mut().unwrap();
        reload.recovery_budget.as_mut().unwrap().last_observed_ms = 7000;
        reload.recovery_budget.as_mut().unwrap().exhausted = sticky;
        reload.recovery_budget.as_mut().unwrap().clock_rollback = sticky;
        let hydrated = serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap();
        let loaded = Driver::from_state(
            hydrated,
            Peer::default(),
            Journal::default(),
            d.clock.clone(),
            Tokens(100),
        )
        .unwrap();
        let b = loaded
            .state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap();
        assert_eq!(b.last_observed_ms, 7000);
        assert_eq!(b.exhausted, sticky);
        assert_eq!(b.clock_rollback, sticky);
        assert_eq!(
            loaded
                .state
                .reload
                .as_ref()
                .unwrap()
                .recovery_budget
                .as_ref(),
            Some(b)
        );
    }
}

#[test]
fn unrelated_explicit_or_previous_operation_budget_is_never_aliased() {
    let mut d = automatic_reload(0);
    d.state.operation.as_mut().unwrap().id = format!("LMO-{}", "f".repeat(32));
    let old = d.state.operation.as_ref().unwrap().recovery_budget.clone();
    d.state
        .reload
        .as_mut()
        .unwrap()
        .recovery_budget
        .as_mut()
        .unwrap()
        .last_observed_ms = 7000;
    assert!(!super::super::recovery::synchronize_automatic_budget(&mut d.state).unwrap());
    assert_eq!(d.state.operation.as_ref().unwrap().recovery_budget, old);
    let mut d = automatic_reload(0);
    d.state.reload.as_mut().unwrap().automatic = false;
    d.state
        .reload
        .as_mut()
        .unwrap()
        .recovery_budget
        .as_mut()
        .unwrap()
        .last_observed_ms = 7000;
    assert!(!super::super::recovery::synchronize_automatic_budget(&mut d.state).unwrap());
    assert_eq!(d.state.operation.as_ref().unwrap().recovery_budget, old);
}

#[test]
fn caller_cancellation_before_delayed_fact_flush_does_not_exhaust_goal() {
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    d.request_reload(&OperationControl::default(), "cancel-control")
        .unwrap();
    d.state.reload.as_mut().unwrap().recovery_budget = Some(budget(1000, 200).unwrap());
    let signal = lycheedev_core::control::TrackedCancellation::new();
    let caller = OperationControl::tracked(
        std::time::Instant::now() + Duration::from_secs(10),
        signal.clone(),
    );
    d.backend.cancel_parent_on_input = Some(signal);
    d.journal.fact_flush_delay = Some(Duration::from_millis(250));
    let result = d
        .continue_until(&caller, target(), &mut Schedule::default())
        .unwrap_err();
    assert!(result.any_cause(&|c| matches!(c, ChannelError::Cancelled)));
    assert!(!has_code(&result, "live.channel_budget_exhausted"));
    assert_eq!(d.invocation_stop.get(), Some(InvocationStop::Cancelled));
    assert!(d.invocation_control.is_none());
    assert_eq!(d.clock.0.get(), 1000);
    assert!(
        d.journal
            .events
            .iter()
            .any(|(kind, _)| kind == "input_observed")
    );
    let mut d = restart(d);
    assert!(
        !d.state
            .reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .exhausted
    );
    d.backend.cancel_after_input = false;
    d.journal.fact_flush_delay = None;
    let _ = d.continue_until(
        &OperationControl::default(),
        target(),
        &mut Schedule::default(),
    );
    assert!(
        !d.state
            .reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .exhausted
    );
    assert_eq!(d.backend.input_kinds, [InputKind::Reload]);
}

#[test]
fn backend_shorter_deadline_does_not_exhaust_original_goal() {
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    d.backend.observe_error = Some(ChannelError::DeadlineExceeded);
    let err = d
        .continue_until(
            &OperationControl::default(),
            target(),
            &mut Schedule::default(),
        )
        .unwrap_err();
    assert!(err.any_cause(&|c| matches!(c, ChannelError::DeadlineExceeded)));
    assert!(!has_code(&err, "live.channel_budget_exhausted"));
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

#[test]
fn all_automatic_reload_paths_keep_exhaustion_after_return_to_business() {
    for path in 0..3 {
        let mut d = automatic_reload(path);
        let deadline = d
            .state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .deadline_ms;
        d.clock.0.set(deadline);
        assert!(has_code(
            &d.budget_control(&OperationControl::default()).unwrap_err(),
            "live.channel_budget_exhausted"
        ));
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
        d.state.reload.as_mut().unwrap().phase = ReloadPhase::Complete;
        d.save(&OperationControl::default(), "fixture_reload_complete")
            .unwrap();
        let mut d = restart(d);
        d.clock.0.set(1001);
        assert!(has_code(
            &d.budget_control(&OperationControl::default()).unwrap_err(),
            "live.channel_budget_exhausted"
        ));
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
    }
}

#[test]
fn automatic_reload_conflicting_deadline_fails_without_effects() {
    let mut d = automatic_reload(0);
    d.state
        .reload
        .as_mut()
        .unwrap()
        .recovery_budget
        .as_mut()
        .unwrap()
        .deadline_ms += 1000;
    assert!(has_code(
        &d.budget_control(&OperationControl::default()).unwrap_err(),
        "live.channel_budget_invalid"
    ));
    assert!(d.backend.input_kinds.is_empty());
}

#[test]
fn goal_deadline_wins_over_late_parent_cancel_while_backend_is_blocked() {
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    d.request_reload(&OperationControl::default(), "deadline-before-cancel")
        .unwrap();
    d.state.reload.as_mut().unwrap().recovery_budget = Some(budget(1000, 200).unwrap());
    let signal = lycheedev_core::control::TrackedCancellation::new();
    let caller = OperationControl::tracked(
        std::time::Instant::now() + Duration::from_secs(10),
        signal.clone(),
    );
    d.backend.cancel_parent_after_deadline = Some(signal.clone());
    let result = d
        .continue_until(&caller, target(), &mut Schedule::default())
        .unwrap_err();
    assert!(signal.is_cancelled());
    assert_eq!(
        caller.stop_cause().unwrap(),
        Some(lycheedev_core::control::StopCause::Cancelled)
    );
    assert_eq!(
        d.invocation_stop.get(),
        Some(InvocationStop::DeadlineExceeded)
    );
    assert!(d.invocation_control.is_none());
    assert!(has_code(&result, "live.channel_budget_exhausted"));
    assert_eq!(d.backend.input_kinds, [InputKind::Reload]);
    assert!(
        d.state
            .reload
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .exhausted
    );
    assert_eq!(d.clock.0.get(), 1000);
    let mut d = restart(d);
    assert!(has_code(
        &d.continue_until(
            &OperationControl::default(),
            target(),
            &mut Schedule::default()
        )
        .unwrap_err(),
        "live.channel_budget_exhausted"
    ));
    assert_eq!(d.backend.input_kinds, [InputKind::Reload]);
}

#[test]
fn untracked_cancellation_order_is_rejected_before_input() {
    let mut d = driver(
        OperationStage::Running,
        super::super::state::OperationPolicy::Opaque,
    );
    d.request_reload(&OperationControl::default(), "raw-control")
        .unwrap();
    let caller = OperationControl::new(
        std::time::Instant::now() + Duration::from_secs(10),
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let result = d
        .continue_until(&caller, target(), &mut Schedule::default())
        .unwrap_err();
    assert!(has_code(
        &result,
        "live.channel_cancellation_order_unavailable"
    ));
    assert!(d.backend.input_kinds.is_empty());
    assert_eq!(d.state.reload.as_ref().unwrap().phase, ReloadPhase::Intent);
}
