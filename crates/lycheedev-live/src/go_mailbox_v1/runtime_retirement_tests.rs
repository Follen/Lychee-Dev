use super::super::recovery::test_support::*;
use super::super::state::{OperationPolicy, TransactionPhase};
use super::*;

#[test]
fn target_mismatch_never_saves_proof_or_retires_claims() {
    let mut d = driver(OperationStage::ReleaseReady, OperationPolicy::Opaque);
    transaction(&mut d, "release", TransactionPhase::InputAttempted);
    d.backend.proof = Some(proof());
    let wrong = RetirementTarget {
        process_started_at: 35,
        ..target()
    };
    assert!(
        d.close_replaced_runtime(&OperationControl::default(), wrong)
            .is_err()
    );
    assert!(d.state.runtime_end.is_none());
    assert!(d.backend.retired.is_empty());
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::ReleaseReady
    );
}
#[test]
fn missing_proof_is_not_destruction_or_report_success() {
    let mut d = driver(OperationStage::ConfirmReady, OperationPolicy::Opaque);
    assert!(
        !d.close_replaced_runtime(&OperationControl::default(), target())
            .unwrap()
    );
    assert!(!d.state.closed);
    assert_eq!(
        d.state.operation.as_ref().unwrap().stage,
        OperationStage::ConfirmReady
    );
    assert!(d.state.runtime_end.is_none());
}
#[test]
fn saved_proof_retries_exact_retirement_without_another_observation() {
    let mut d = driver(OperationStage::ReleaseReady, OperationPolicy::Opaque);
    transaction(&mut d, "release", TransactionPhase::InputAttempted);
    let original_identity = d.state.identity.clone();
    d.backend.proof = Some(proof());
    d.backend.retirement_error = Some(error("fixture.cleanup_failed"));
    assert!(
        d.close_replaced_runtime(&OperationControl::default(), target())
            .is_err()
    );
    assert!(d.state.runtime_end.is_some());
    assert!(!d.state.closed);
    assert_eq!(
        d.continuation().blocker.unwrap().kind,
        "runtime_retirement_pending"
    );
    assert_eq!(
        d.state.operation.as_ref().unwrap().cleanup_method,
        CleanupMethod::None
    );
    let mut d = restart(d);
    d.backend.proof = None;
    assert!(
        d.close_replaced_runtime(&OperationControl::default(), target())
            .unwrap()
    );
    assert_eq!(d.backend.proof_reads, 1);
    assert_eq!(d.backend.retired.len(), 1);
    assert_eq!(d.state.identity, original_identity);
    assert!(d.state.closed);
    assert_eq!(
        d.state.operation.as_ref().unwrap().cleanup_method,
        CleanupMethod::RuntimeDestroyed
    );
    assert!(d.backend.input_kinds.is_empty());
}
#[test]
fn candidate_result_keeps_bytes_but_becomes_unknown_after_verified_retirement() {
    let mut d = driver(OperationStage::ConfirmReady, OperationPolicy::Observation);
    transaction(&mut d, "confirm", TransactionPhase::InputAttempted);
    let candidate = d.state.operation.as_ref().unwrap().result_bytes.clone();
    d.backend.proof = Some(proof());
    assert!(
        d.close_replaced_runtime(&OperationControl::default(), target())
            .unwrap()
    );
    let op = d.state.operation.as_ref().unwrap();
    assert_eq!(op.stage, OperationStage::ExecutionUnknown);
    assert_eq!(op.cleanup_method, CleanupMethod::None);
    assert_eq!(op.result_bytes, candidate);
    assert_eq!(op.attempt, 1);
    assert!(d.journal.results.is_empty());
    assert!(d.backend.input_kinds.is_empty());
}
#[test]
fn failed_terminal_save_preserves_claims_and_blocks_in_memory_reuse() {
    let mut d = driver(OperationStage::ReleaseReady, OperationPolicy::Opaque);
    transaction(&mut d, "release", TransactionPhase::InputAttempted);
    d.backend.proof = Some(proof());
    d.journal.fail = Some("runtime_replacement_closed");
    assert!(
        d.close_replaced_runtime(&OperationControl::default(), target())
            .is_err()
    );
    assert!(!d.state.closed);
    assert!(d.state.transaction.is_some());
    assert!(d.state.runtime_end.is_some());
    assert!(has_code(
        &d.close_replaced_runtime(&OperationControl::default(), target())
            .unwrap_err(),
        "live.channel_journal_reload_required"
    ));
    let mut d = restart(d);
    d.journal.fail = None;
    assert!(
        d.close_replaced_runtime(&OperationControl::default(), target())
            .unwrap()
    );
    assert_eq!(d.backend.proof_reads, 1);
    assert_eq!(d.backend.retired.len(), 2);
}
#[test]
fn failed_proof_save_cannot_retire_or_allow_following_input() {
    let mut d = driver(OperationStage::ReleaseReady, OperationPolicy::Opaque);
    transaction(&mut d, "release", TransactionPhase::InputAttempted);
    d.backend.proof = Some(proof());
    d.journal.fail = Some("runtime_replacement_observed");
    assert!(
        d.close_replaced_runtime(&OperationControl::default(), target())
            .is_err()
    );
    assert!(d.state.runtime_end.is_none());
    assert!(d.backend.retired.is_empty());
    assert!(
        d.request_reload(&OperationControl::default(), "later")
            .is_err()
    );
}
#[test]
fn optional_probe_timeout_does_not_swallow_cancel_identity_or_io() {
    for extra in [
        ChannelError::Cancelled,
        error("fixture.identity_changed"),
        ChannelError::External {
            causes: vec!["io".into()],
        },
    ] {
        let mixed = ChannelError::Joined {
            causes: vec![ChannelError::DeadlineExceeded, extra],
        };
        assert!(!mixed.optional_observation_timeout());
        let mut d = driver(OperationStage::Running, OperationPolicy::Opaque);
        d.backend.observe_error = Some(mixed.clone());
        assert_eq!(
            d.continue_or_retire(
                &OperationControl::default(),
                target(),
                true,
                &mut Schedule::default()
            )
            .unwrap_err(),
            mixed
        );
        assert!(d.backend.input_kinds.is_empty());
    }
}
#[test]
fn replacement_proof_cannot_retire_a_claim_for_its_own_runtime() {
    let mut d = driver(OperationStage::Running, OperationPolicy::Opaque);
    transaction(&mut d, "confirm", TransactionPhase::InputAttempted);
    let p = proof();
    d.state.transaction.as_mut().unwrap().envelope.runtime = p.current.runtime.clone();
    assert!(retirement_proof_matches_state(&d.state, &p).is_err());
}
