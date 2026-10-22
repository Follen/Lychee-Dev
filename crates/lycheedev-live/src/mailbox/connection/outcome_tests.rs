use super::*;
use crate::mailbox::{
    ContractError,
    connection::{RawReport, present},
    observations::ChannelError,
    state::State,
};
use std::path::Path;

fn report() -> ProjectResult {
    let mut result = present(
        &State {
            id: "CON-outcome".into(),
            ..State::default()
        },
        Path::new("fixture/con.jsonl"),
        100,
        "",
        "",
    );
    result.stage = "result_verified".into();
    result.report_state = "verified".into();
    result.cleanup = "pending".into();
    result.report = Some(RawReport(
        br#"{"same":1,"same":2,"big":9007199254740993123456789}"#.to_vec(),
    ));
    result
}
fn close_error() -> AdmissionError {
    ChannelError::Contract(ContractError("live.channel_trace_flush")).into()
}
fn ok() -> Result<(), AdmissionError> {
    Ok(())
}

#[test]
fn completed_join_ok_keeps_the_owned_report_without_cloning() {
    let report = report();
    let bytes = report.report.as_ref().unwrap().0.as_ptr();
    let ServiceOutcome::Completed(result) = ServiceOutcome::Completed(report).join(ok()) else {
        panic!("successful checked close changed the outcome");
    };
    assert_eq!(result.report.unwrap().0.as_ptr(), bytes);
    assert!(
        !result.complete,
        "call completion does not imply operation completion"
    );
    assert_eq!(result.cleanup, "pending");
}

#[test]
fn completed_join_failure_preserves_verified_report_and_cleanup_obligation() {
    let report = report();
    let bytes = report.report.as_ref().unwrap().0.as_ptr();
    let ServiceOutcome::ReportedWithFailure { result, error } =
        ServiceOutcome::Completed(report).join(Err(close_error()))
    else {
        panic!("close failure must retain the existing report");
    };
    assert_eq!(error, close_error());
    assert_eq!(result.report.unwrap().0.as_ptr(), bytes);
    assert_eq!(result.report_state, "verified");
    assert_eq!(result.cleanup, "pending");
    assert!(!result.complete);
}

#[test]
fn failure_without_report_stays_failed_after_successful_or_failed_close() {
    let primary: AdmissionError = ChannelError::Cancelled.into();
    let ServiceOutcome::Failed(error) = ServiceOutcome::Failed(primary.clone()).join(ok()) else {
        panic!("successful cleanup cannot invent a report");
    };
    assert_eq!(error, primary);
    let ServiceOutcome::Failed(error) =
        ServiceOutcome::Failed(primary.clone()).join(Err(close_error()))
    else {
        panic!("failed cleanup cannot invent a report");
    };
    assert_eq!(error, AdmissionError::Joined(vec![primary, close_error()]));
}

#[test]
fn reported_failure_keeps_primary_then_each_checked_close_in_original_tree_order() {
    let primary: AdmissionError = ChannelError::Pending { causes: vec![] }.into();
    let cancelled: AdmissionError = ChannelError::Cancelled.into();
    let value = report();
    let bytes = value.report.as_ref().unwrap().0.as_ptr();
    let outcome = ServiceOutcome::ReportedWithFailure {
        result: value,
        error: primary.clone(),
    }
    .join(ok())
    .join(Err(cancelled.clone()))
    .join(Err(close_error()));
    let ServiceOutcome::ReportedWithFailure { result, error } = outcome else {
        panic!("error joins cannot discard committed facts");
    };
    assert_eq!(
        error,
        AdmissionError::Joined(vec![
            AdmissionError::Joined(vec![primary, cancelled]),
            close_error()
        ])
    );
    assert_eq!(result.report.unwrap().0.as_ptr(), bytes);
    assert_eq!(
        super::super::classify_error(&error).unwrap().code,
        "live.channel_trace_flush"
    );
}

#[test]
fn operation_result_selects_only_completed_or_reported_failure() {
    assert!(matches!(
        ServiceOutcome::with_result(report(), ok()),
        ServiceOutcome::Completed(_)
    ));
    let ServiceOutcome::ReportedWithFailure { error, .. } =
        ServiceOutcome::with_result(report(), Err(close_error()))
    else {
        panic!("failed operation with a projection must keep both");
    };
    assert_eq!(error, close_error());
}

#[test]
fn replacing_committed_facts_preserves_each_outcomes_failure_state() {
    for outcome in [
        ServiceOutcome::Completed(report()),
        ServiceOutcome::Failed(close_error()),
        ServiceOutcome::ReportedWithFailure {
            result: report(),
            error: close_error(),
        },
    ] {
        let had_error = outcome.error().is_some();
        let mut newer = report();
        newer.session = "CON-durable-channel".into();
        let outcome = outcome.with_report(newer);
        assert_eq!(outcome.error().is_some(), had_error);
        if had_error {
            assert_eq!(outcome.error().unwrap(), &close_error());
        }
        assert_eq!(
            outcome.into_report().unwrap().session,
            "CON-durable-channel"
        );
    }
}
