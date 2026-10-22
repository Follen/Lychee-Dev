//! Fixed Go ProjectResult is presentation only, never a second persisted state.
use super::super::{
    Identity,
    recovery::{Continuation, continuation},
    state::{
        CleanupMethod, InputAttempt, OperationStage, ProcessEnd, RecoveryPhase, ReloadAttempt,
        ReloadPhase, RuntimeReplacementProof, State,
    },
};
use serde::{Serialize, Serializer};
use std::path::Path;

/// Keep original report lexemes/duplicate fields/large integers until the public
/// encoder. This is raw JSON, not ResultBytes' persisted base64 representation.
#[derive(Debug, Clone)]
pub struct RawReport(pub Vec<u8>);
impl Serialize for RawReport {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.0.len() > super::super::MAX_PAYLOAD {
            return Err(serde::ser::Error::custom("live.channel_report_length"));
        }
        let bytes = lycheedev_core::raw_json::RawJsonBytes::new(&self.0)
            .map_err(serde::ser::Error::custom)?;
        // Preserve ordinary serde_json behavior for UTF-8 reports. Native Go
        // RawMessage also accepts non-UTF-8 string bytes; those require the
        // typed public writer and fail closed through ordinary serde_json.
        if let Ok(text) = std::str::from_utf8(bytes.as_bytes()) {
            let raw: &serde_json::value::RawValue =
                serde_json::from_str(text).map_err(serde::ser::Error::custom)?;
            raw.serialize(serializer)
        } else {
            bytes.serialize(serializer)
        }
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectResult {
    pub continuation: Continuation,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub process_end: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_end: Option<RuntimeReplacementProof>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InputAttempt>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub waiting: String,
    pub session: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub operation: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub operation_state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scans: String,
    pub identity: Identity,
    pub bound: bool,
    pub closed: bool,
    pub stage: String,
    pub complete: bool,
    pub report_state: String,
    pub cleanup: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub cleanup_method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<RawReport>,
    pub journal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload: Option<ReloadAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_origin: Option<Identity>,
}
fn stage(value: OperationStage) -> &'static str {
    match value {
        OperationStage::Prepared => "prepared",
        OperationStage::CommitReady => "commit_ready",
        OperationStage::Running => "running",
        OperationStage::ConfirmReady => "confirm_ready",
        OperationStage::ResultVerified => "result_verified",
        OperationStage::ReleaseReady => "release_ready",
        OperationStage::Complete => "complete",
        OperationStage::ExecutionUnknown => "execution_unknown",
        OperationStage::Cancelled => "cancelled",
    }
}
pub fn present(
    state: &State,
    journal: &Path,
    now_ms: i64,
    waiting: &str,
    scans: &str,
) -> ProjectResult {
    let mut result = present_metadata(state, journal, now_ms, waiting, scans);
    if let Some(operation) = state.operation.as_ref().filter(|o| reported(o.stage)) {
        result.report = Some(RawReport(operation.result_bytes.0.clone()));
    }
    result
}

/// The invocation has finished using State. Transfer the verified report's
/// existing allocation into the public result, including when cleanup fails.
/// No persistent or borrowed State is emptied to manufacture this ownership.
pub(super) fn present_owned(
    state: State,
    journal: &Path,
    now_ms: i64,
    waiting: &str,
    scans: &str,
) -> ProjectResult {
    let mut result = present_metadata(&state, journal, now_ms, waiting, scans);
    if let Some(operation) = state.operation.filter(|o| reported(o.stage)) {
        result.report = Some(RawReport(operation.result_bytes.0));
    }
    result
}

fn reported(stage: OperationStage) -> bool {
    matches!(stage, OperationStage::ResultVerified | OperationStage::ReleaseReady | OperationStage::Complete)
}

fn present_metadata(
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
        if reported(operation.stage) {
            result.report_state = "verified".into();
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

#[cfg(test)]
#[path = "projection_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "offline_performance_tests.rs"]
mod offline_performance;
