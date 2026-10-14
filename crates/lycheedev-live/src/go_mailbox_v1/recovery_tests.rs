use super::super::state::TransactionPhase;
use super::test_support::*;
use super::*;

#[test]
fn durable_budget_exhaustion_and_rollback_never_revive() {
    let mut b = budget(1000, 100).unwrap();
    assert_eq!(observe_budget(&mut b, 1050), (true, Ok(50)));
    assert!(has_code(
        &observe_budget(&mut b, 1049).1.unwrap_err(),
        "live.channel_budget_clock_rollback"
    ));
    assert!(b.exhausted && b.clock_rollback);
    assert!(observe_budget(&mut b, 1060).1.is_err());
    let mut b = budget(1000, 100).unwrap();
    assert!(observe_budget(&mut b, 1100).1.is_err());
    assert!(observe_budget(&mut b, 1001).1.is_err());
}
#[test]
fn legacy_budget_is_created_once_only_when_driven() {
    let mut d = driver(OperationStage::Prepared, OperationPolicy::Opaque);
    d.state.operation.as_mut().unwrap().recovery_budget = None;
    assert_eq!(d.continuation().remaining_budget_ms, None);
    d.budget_control(&OperationControl::default()).unwrap();
    let b = d
        .state
        .operation
        .as_ref()
        .unwrap()
        .recovery_budget
        .clone()
        .unwrap();
    assert!(b.legacy);
    d.clock.0.set(2000);
    d.budget_control(&OperationControl::default()).unwrap();
    assert_eq!(
        d.state
            .operation
            .as_ref()
            .unwrap()
            .recovery_budget
            .as_ref()
            .unwrap()
            .deadline_ms,
        b.deadline_ms
    );
    assert_eq!(
        d.journal
            .events
            .iter()
            .filter(|(e, _)| e == "budget_migrated")
            .count(),
        1
    );
}
#[test]
fn recovery_opaque_keeps_unknown_evidence_observation_gets_new_attempt() {
    for policy in [OperationPolicy::Opaque, OperationPolicy::Observation] {
        let mut d = driver(OperationStage::ConfirmReady, policy);
        transaction(&mut d, "confirm", TransactionPhase::InputAttempted);
        let before = d.state.operation.clone().unwrap();
        d.backend.supersede_error = Some(error("fixture.retirement_failed"));
        assert!(
            d.recover_runtime(&OperationControl::default(), identity('2'))
                .is_err()
        );
        assert_eq!(
            d.state.operation.as_ref().unwrap().stage,
            OperationStage::ConfirmReady
        );
        let mut d = restart(d);
        let result = d.finish_recovery(&OperationControl::default());
        let op = d.state.operation.as_ref().unwrap();
        assert_eq!(op.id, before.id);
        assert_eq!(op.request, before.request);
        assert_eq!(op.recovery_budget, before.recovery_budget);
        assert_eq!(d.backend.input_kinds.len(), 1);
        if policy == OperationPolicy::Opaque {
            assert!(has_code(
                &result.unwrap_err(),
                "live.channel_execution_unknown"
            ));
            assert_eq!(op.stage, OperationStage::ExecutionUnknown);
            assert_eq!(op.ticket, before.ticket);
            assert_eq!(op.result_bytes, before.result_bytes);
        } else {
            result.unwrap();
            assert_eq!(op.stage, OperationStage::Prepared);
            assert_ne!(op.ticket, before.ticket);
            assert_eq!(op.attempt, 2);
            assert!(op.result_bytes.0.is_empty());
        }
    }
}
#[test]
fn commit_intent_already_means_unknown_even_without_its_own_input() {
    for phase in [TransactionPhase::Intent, TransactionPhase::Published] {
        let mut d = driver(OperationStage::CommitReady, OperationPolicy::Opaque);
        transaction(&mut d, "commit", phase);
        assert!(has_code(
            &d.recover_runtime(&OperationControl::default(), identity('2'))
                .unwrap_err(),
            "live.channel_execution_unknown"
        ));
        assert_eq!(
            d.state.operation.as_ref().unwrap().stage,
            OperationStage::ExecutionUnknown
        );
        assert_eq!(d.backend.actions, ["bind"]);
    }
}
#[test]
fn verified_result_survives_replacement_with_distinct_cleanup() {
    let mut d = driver(OperationStage::ReleaseReady, OperationPolicy::Opaque);
    transaction(&mut d, "release", TransactionPhase::InputAttempted);
    let bytes = d.state.operation.as_ref().unwrap().result_bytes.clone();
    d.recover_runtime(&OperationControl::default(), identity('2'))
        .unwrap();
    let op = d.state.operation.as_ref().unwrap();
    assert_eq!(op.stage, OperationStage::Complete);
    assert_eq!(op.cleanup_method, CleanupMethod::RuntimeDestroyed);
    assert_eq!(op.result_bytes, bytes);
    assert_eq!(d.journal.results.len(), 1);
}
#[test]
fn candidate_is_not_retirement_and_third_runtime_does_not_overwrite_claims() {
    let mut d = driver(OperationStage::Running, OperationPolicy::Opaque);
    transaction(&mut d, "confirm", TransactionPhase::InputAttempted);
    d.backend.pending_bind = true;
    assert!(
        d.recover_runtime(&OperationControl::default(), identity('2'))
            .unwrap_err()
            .is_pending()
    );
    let original = d.state.recovery.as_ref().unwrap().transaction.clone();
    let binding = d.state.transaction.clone();
    assert!(
        d.recover_runtime(&OperationControl::default(), identity('3'))
            .unwrap_err()
            .is_blocked()
    );
    assert_eq!(d.state.recovery.as_ref().unwrap().transaction, original);
    assert_eq!(d.state.transaction, binding);
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::Running
    );
}
#[test]
fn mismatched_or_incomplete_candidates_never_send_input() {
    for kind in 0..4 {
        let mut d = driver(OperationStage::Running, OperationPolicy::Opaque);
        let mut candidate = identity('2');
        match kind {
            0 => candidate.guid = "other".into(),
            1 => candidate.owner = "f".repeat(32),
            2 => candidate.runtime = identity('1').runtime,
            _ => candidate.inventory = Some(499),
        }
        assert!(
            d.recover_runtime(&OperationControl::default(), candidate)
                .is_err()
        );
        assert!(d.backend.actions.is_empty());
        assert!(d.state.recovery.is_none());
    }
}
#[test]
fn observation_retry_limit_preserves_original_attempt_and_deadline() {
    let mut d = driver(OperationStage::Running, OperationPolicy::Observation);
    d.state.operation.as_mut().unwrap().attempt = 3;
    let before = d.state.operation.clone();
    assert!(has_code(
        &d.recover_runtime(&OperationControl::default(), identity('2'))
            .unwrap_err(),
        "live.channel_recovery_attempt_limit"
    ));
    assert_eq!(d.state.operation, before);
}
#[test]
fn terminal_projection_overrides_stale_blockers_and_unknown_requires_decision() {
    let mut d = driver(OperationStage::Complete, OperationPolicy::Opaque);
    d.state.blocker = Some(Blocker {
        kind: "input_combat".into(),
        ..Blocker::default()
    });
    assert_eq!(d.continuation().kind, "completed");
    assert!(d.continuation().blocker.is_none());
    d.state.operation.as_mut().unwrap().stage = OperationStage::ExecutionUnknown;
    assert_eq!(d.continuation().kind, "needs_decision");
    d.state.closed = true;
    d.state.bound = false;
    assert_eq!(d.continuation().kind, "needs_decision");
}
#[test]
fn repeated_same_blocker_is_not_progress() {
    let mut d = driver(OperationStage::Prepared, OperationPolicy::Opaque);
    let version = d.state.progress_version;
    d.record_blocker(&Err(pending())).unwrap();
    let count = d.journal.events.len();
    d.record_blocker(&Err(pending())).unwrap();
    assert_eq!(count, d.journal.events.len());
    assert_eq!(version, d.state.progress_version);
}
