//! Fixed Go command classification. The original typed error tree is retained
//! in ServiceOutcome; this is only its public code/exit projection.
use super::{AdmissionError, ChannelError};
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PublicFailure {
    pub code: String,
    pub exit_code: i32,
    pub details: serde_json::Value,
    pub message: String,
}
fn channel_any(error: &AdmissionError, predicate: &impl Fn(&ChannelError) -> bool) -> bool {
    match error {
        AdmissionError::Channel(e) => e.any_cause(predicate),
        AdmissionError::Joined(es) => es.iter().any(|e| channel_any(e, predicate)),
        AdmissionError::Occupied { .. } => false,
    }
}
fn has(error: &AdmissionError, wanted: &str) -> bool {
    channel_any(error, &|e| match e {
        ChannelError::Contract(c) => c.0 == wanted,
        ChannelError::Toolkit { code, .. } => code == wanted,
        ChannelError::External { causes } => causes
            .iter()
            .any(|c| c == wanted || c.starts_with(&format!("{wanted}:"))),
        _ => false,
    })
}
fn first(error: &AdmissionError) -> String {
    match error {
        AdmissionError::Channel(ChannelError::Joined { causes }) => causes
            .first()
            .map(|e| first(&AdmissionError::Channel(e.clone())))
            .unwrap_or_else(|| "command.failed".into()),
        AdmissionError::Joined(es) => es
            .first()
            .map(first)
            .unwrap_or_else(|| "command.failed".into()),
        other => other
            .to_string()
            .split([':', '\n'])
            .next()
            .unwrap_or("command.failed")
            .into(),
    }
}
fn toolkit_channel(error: &ChannelError) -> Option<(String, i32, serde_json::Value)> {
    match error {
        ChannelError::Toolkit {
            code,
            exit_code,
            details,
            ..
        } => Some((code.clone(), *exit_code, details.clone())),
        ChannelError::Joined { causes } => causes.iter().find_map(toolkit_channel),
        _ => None,
    }
}
fn toolkit(error: &AdmissionError) -> Option<(String, i32, serde_json::Value)> {
    match error {
        AdmissionError::Channel(e) => toolkit_channel(e),
        AdmissionError::Joined(es) => es.iter().find_map(toolkit),
        AdmissionError::Occupied { .. } => None,
    }
}
pub fn classify_error(error: &AdmissionError) -> Option<PublicFailure> {
    let (code, exit_code) = if has(error, "live.channel_trace_flush") {
        ("live.channel_trace_flush".into(), 5)
    } else if channel_any(error, &|e| matches!(e, ChannelError::Cancelled)) {
        ("live.channel_wait_cancelled".into(), 7)
    } else if channel_any(error, &|e| matches!(e, ChannelError::Pending { .. })) {
        ("live.channel_pending".into(), 6)
    } else {
        let code = first(error);
        let exit = match code.as_str() {
            "live.channel_pending"
            | "live.channel_discovery_incomplete"
            | "live.channel_transaction_pending" => 6,
            "live.channel_request_key_invalid"
            | "live.channel_request_invalid"
            | "live.channel_request_conflict"
            | "live.channel_connection_id_required"
            | "live.channel_target_mismatch" => 2,
            "live.channel_execution_unknown" | "live.channel_recovery_attempt_limit" => 5,
            "live.channel_activation_required"
            | "live.channel_closed"
            | "live.channel_not_idle"
            | "live.channel_clean_current_addon_required"
            | "live.channel_slot_installation_required"
            | "live.channel_ownership_changed"
            | "live.channel_reload_required"
            | "live.channel_client_restart_required"
            | "live.channel_input_capability_unsupported"
            | "live.channel_mailbox_build_unsupported"
            | "live.channel_mailbox_required"
            | "live.channel_mailbox_unavailable" => 3,
            _ if code.starts_with("live.channel_") => 4,
            _ => {
                let (code, exit_code, details) = toolkit(error)?;
                return Some(PublicFailure {
                    code,
                    exit_code,
                    details,
                    message: error.to_string(),
                });
            }
        };
        (code, exit)
    };
    Some(PublicFailure {
        code,
        exit_code,
        message: error.to_string(),
        details: toolkit(error).map_or(serde_json::Value::Null, |(_, _, details)| details),
    })
}
