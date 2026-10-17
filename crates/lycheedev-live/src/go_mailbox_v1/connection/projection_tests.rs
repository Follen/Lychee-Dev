//! The complete pre-optimization projection at bbfe882 remains the independent
//! wire reference. Ownership assertions measure pointer/capacity, not estimates.
use super::*;
use crate::go_mailbox_v1::{
    ContractError,
    connection::ServiceOutcome,
    observations::ChannelError,
    state::{Operation, OperationPolicy, ResultBytes, SlotEnvelope, Transaction, TransactionPhase},
};
use lycheedev_core::public_json::{write_pretty_serializable_line, write_serializable_line};

fn old_present(
    state: &State,
    journal: &Path,
    now_ms: i64,
    waiting: &str,
    scans: &str,
) -> ProjectResult {
    let mut result = ProjectResult {
        continuation: continuation(state, now_ms),
        process_end: match state.process_end {
            ProcessEnd::None => "",
            ProcessEnd::ProcessAbsent => "process_absent",
            ProcessEnd::ProcessExited => "process_exited",
            ProcessEnd::PidReused => "pid_reused",
        }
        .into(),
        runtime_end: state.runtime_end.clone(),
        input: state.input.clone(),
        waiting: waiting.into(),
        session: state.id.clone(),
        operation: String::new(),
        operation_state: String::new(),
        scans: scans.into(),
        identity: state.identity.clone(),
        bound: state.bound,
        closed: state.closed,
        stage: if state.closed {
            "closed"
        } else if state.bound {
            "connected"
        } else {
            "connecting"
        }
        .into(),
        complete: state.bound || state.closed,
        report_state: "unavailable".into(),
        cleanup: "none".into(),
        cleanup_method: String::new(),
        report: None,
        journal: journal.display().to_string(),
        reload: state.reload.clone(),
        report_origin: None,
    };
    if let Some(operation) = &state.operation {
        result.cleanup_method = match operation.cleanup_method {
            CleanupMethod::None => "",
            CleanupMethod::Released => "released",
            CleanupMethod::RuntimeDestroyed => "runtime_destroyed",
            CleanupMethod::ReloadRequired => "reload_required",
        }
        .into();
        result.operation_state = stage(operation.stage).into();
        result.stage = result.operation_state.clone();
        result.report_origin = operation.origin.clone();
        result.operation = operation.id.clone();
        result.complete = operation.stage == OperationStage::Complete;
        result.cleanup = "pending".into();
        if matches!(
            operation.stage,
            OperationStage::ResultVerified
                | OperationStage::ReleaseReady
                | OperationStage::Complete
        ) {
            result.report_state = "verified".into();
            result.report = Some(RawReport(operation.result_bytes.0.clone()));
        }
        match operation.stage {
            OperationStage::Complete => result.cleanup = "complete".into(),
            OperationStage::Cancelled => {
                result.cleanup = "complete".into();
                result.cleanup_method = "not_executed".into();
            }
            OperationStage::ExecutionUnknown => result.cleanup = "runtime_lost".into(),
            _ => (),
        }
    }
    if state.closed {
        result.stage = "closed".into();
        result.complete = state
            .operation
            .as_ref()
            .is_none_or(|o| o.stage == OperationStage::Complete);
    }
    if let Some(reload) = state
        .reload
        .as_ref()
        .filter(|r| r.phase != ReloadPhase::Complete)
    {
        result.stage = match reload.phase {
            ReloadPhase::Intent => "reload_intent",
            ReloadPhase::InputAttempted => "reload_input_attempted",
            ReloadPhase::Binding => "reload_binding",
            ReloadPhase::Complete => unreachable!(),
        }
        .into();
        result.complete = false;
    }
    if state.transaction.is_some()
        || state.closing
        || state
            .recovery
            .as_ref()
            .is_some_and(|r| r.phase != RecoveryPhase::Complete)
    {
        result.complete = false;
    }
    if state.closing
        || state
            .transaction
            .as_ref()
            .is_some_and(|t| t.envelope.action == "unbind")
    {
        result.stage = "closing".into();
    }
    result
}

fn state(stage: OperationStage, bytes: &[u8]) -> State {
    let mut operation = Operation::prepared(
        "LMO-owned".into(),
        "ticket".into(),
        "return 1".into(),
        5,
        OperationPolicy::Observation,
    );
    operation.stage = stage;
    operation.request = "original request".into();
    operation.cleanup_method = if stage == OperationStage::Complete {
        CleanupMethod::Released
    } else {
        CleanupMethod::ReloadRequired
    };
    let mut report = Vec::with_capacity(bytes.len() + 1234);
    report.extend_from_slice(bytes);
    operation.report_bytes = report.len() as u32;
    operation.report_checksum = crate::go_mailbox_v1::checksum(&report);
    operation.result_bytes = ResultBytes(report);
    State {
        id: "CON-owned".into(),
        bound: true,
        operation: Some(operation),
        ..State::default()
    }
}

fn wire(result: &ProjectResult, pretty: bool) -> Vec<u8> {
    let mut output = Vec::new();
    if pretty {
        write_pretty_serializable_line(&mut output, result).unwrap();
    } else {
        write_serializable_line(&mut output, result).unwrap();
    }
    output
}

#[test]
fn owned_projection_matches_old_full_wire_for_all_operation_and_control_stages() {
    for stage in [
        OperationStage::Prepared,
        OperationStage::CommitReady,
        OperationStage::Running,
        OperationStage::ConfirmReady,
        OperationStage::ResultVerified,
        OperationStage::ReleaseReady,
        OperationStage::Complete,
        OperationStage::ExecutionUnknown,
        OperationStage::Cancelled,
    ] {
        for shape in 0..6 {
            let mut s = state(
                stage,
                b" {\"s\":\"\xff\",\"x\":1e400,\"x\":-0,\"esc\":\"\\u003c\"}\n",
            );
            match shape {
                1 => {
                    s.closed = true;
                    s.bound = false;
                }
                2 => {
                    s.reload = Some(ReloadAttempt {
                        automatic: false,
                        slot_enablement: false,
                        slot_budget_source: None,
                        recovery_budget: None,
                        reconcile_pending: true,
                        request: "reload".into(),
                        from: "runtime".into(),
                        phase: ReloadPhase::Binding,
                        input_step: 1,
                        patch_transitions: 2,
                    })
                }
                3 => s.closing = true,
                4 => {
                    s.transaction = Some(Transaction {
                        release_wait_started_at_ms: 9,
                        envelope: SlotEnvelope {
                            action: "unbind".into(),
                            ..SlotEnvelope::default()
                        },
                        phase: TransactionPhase::InputAttempted,
                        receipt: None,
                        escape_attempts: 0,
                        input_observation: None,
                    })
                }
                5 => {
                    s.recovery = Some(crate::go_mailbox_v1::state::RuntimeRecovery {
                        from: s.identity.clone(),
                        transaction: None,
                        phase: RecoveryPhase::Binding,
                    })
                }
                _ => (),
            }
            let original = &s.operation.as_ref().unwrap().result_bytes.0;
            let pointer = original.as_ptr();
            let capacity = original.capacity();
            let path = Path::new("fixture/con.jsonl");
            let expected = old_present(&s, path, 100, "pending", "scans");
            let borrowed = present(&s, path, 100, "pending", "scans");
            let actual = present_owned(s, path, 100, "pending", "scans");
            for pretty in [false, true] {
                assert_eq!(
                    wire(&actual, pretty),
                    wire(&expected, pretty),
                    "owned {stage:?}/{shape}"
                );
                assert_eq!(
                    wire(&borrowed, pretty),
                    wire(&expected, pretty),
                    "borrowed {stage:?}/{shape}"
                );
            }
            if let Some(report) = actual.report {
                assert_eq!(report.0.as_ptr(), pointer);
                assert_eq!(report.0.capacity(), capacity);
            } else {
                assert!(!matches!(
                    stage,
                    OperationStage::ResultVerified
                        | OperationStage::ReleaseReady
                        | OperationStage::Complete
                ));
            }
        }
    }
}

#[test]
fn owned_projection_keeps_original_report_allocation_through_checked_close_failure() {
    let mut bytes = b"{\"s\":\"".to_vec();
    bytes.resize(crate::go_mailbox_v1::MAX_PAYLOAD - 2, b'x');
    bytes.extend_from_slice(b"\"}");
    let s = state(OperationStage::ReleaseReady, &bytes);
    let pointer = s.operation.as_ref().unwrap().result_bytes.0.as_ptr();
    let capacity = s.operation.as_ref().unwrap().result_bytes.0.capacity();
    let projected = present_owned(s, Path::new("fixture/con.jsonl"), 100, "", "");
    let cleanup: super::super::AdmissionError =
        ChannelError::Contract(ContractError("fixture.close_failed")).into();
    let ServiceOutcome::ReportedWithFailure { result, error } =
        ServiceOutcome::Completed(projected).join(Err(cleanup.clone()))
    else {
        panic!("cleanup failure erased verified report");
    };
    assert_eq!(error, cleanup);
    assert_eq!(result.report_state, "verified");
    assert_eq!(result.cleanup, "pending");
    assert!(!result.complete);
    let moved = result.report.unwrap().0;
    assert_eq!(moved, bytes);
    assert_eq!(moved.as_ptr(), pointer);
    assert_eq!(moved.capacity(), capacity);
}

#[test]
fn owned_projection_does_not_make_invalid_report_serializable_or_write_partial_output() {
    for bytes in [
        b"{\"s\":\"\xff\"} trailing".to_vec(),
        b"{".to_vec(),
        vec![b' '; crate::go_mailbox_v1::MAX_PAYLOAD + 1],
    ] {
        let s = state(OperationStage::Complete, &bytes);
        let path = Path::new("fixture/con.jsonl");
        let expected = old_present(&s, path, 100, "", "");
        let actual = present_owned(s, path, 100, "", "");
        for result in [expected, actual] {
            let mut output = b"prior\n".to_vec();
            assert!(write_serializable_line(&mut output, &result).is_err());
            assert_eq!(output, b"prior\n");
        }
    }
}
