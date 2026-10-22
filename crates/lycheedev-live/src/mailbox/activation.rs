//! Persistent activation facts. No keystrokes are emitted by this module.
//! Oracle: Go 3.2.0 channel/activation_windows.go. A missing outcome is never
//! proof of zero input; manual waiting is valid only before any input intent.
#![forbid(unsafe_code)]
use super::{
    InputDisposition, InputOutcome,
    connection::{ProjectResult, TargetRequest, present},
    driver::{budget, error},
    observations::{ChannelError, ChannelResult},
    recovery::observe_budget,
    state::{Blocker, DEFAULT_RECOVERY_MILLIS, DurableBudget, State, validate_request},
};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const ACTIVATION_SCHEMA: &str = "lycheedev.channel-activation.v3";
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivationPhase {
    Prepared,
    WaitingManualReload,
    InputAttempted,
    RuntimeSelected,
    Cancelled,
    #[default]
    #[serde(other)]
    Invalid,
}
impl ActivationPhase {
    fn name(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Invalid => "",
            Self::WaitingManualReload => "waiting_manual_reload",
            Self::InputAttempted => "input_attempted",
            Self::RuntimeSelected => "runtime_selected",
            Self::Cancelled => "cancelled",
        }
    }
}
/// Go TargetRequest has exported, untagged names. This is its persisted shape,
/// separate from the application selector so its casing cannot drift.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct ActivationSelection {
    pub installation: String,
    #[serde(rename = "PID")]
    pub pid: u32,
    pub character: String,
    pub realm: String,
    pub build: String,
    pub product: String,
}
impl From<&TargetRequest> for ActivationSelection {
    fn from(value: &TargetRequest) -> Self {
        Self {
            installation: value.installation.clone(),
            pid: value.pid,
            character: value.filter.character.clone(),
            realm: value.filter.realm.clone(),
            build: value.filter.build.clone(),
            product: value.filter.product.clone(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Activation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub budget: Option<DurableBudget>,
    #[serde(rename = "inputOutcome", skip_serializing_if = "Option::is_none")]
    pub outcome: Option<InputOutcome>,
    pub schema: String,
    pub request: String,
    pub selection: ActivationSelection,
    pub phase: ActivationPhase,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prior_runtime: String,
    pub input_step: i32,
    pub patch_transitions: i32,
}
impl Activation {
    pub fn prepared(request: &TargetRequest, key: &str, now_ms: i64) -> ChannelResult<Self> {
        validate_request(key)?;
        Ok(Self {
            schema: ACTIVATION_SCHEMA.into(),
            request: key.into(),
            selection: request.into(),
            phase: ActivationPhase::Prepared,
            budget: Some(budget(now_ms, DEFAULT_RECOVERY_MILLIS)?),
            ..Self::default()
        })
    }
    pub fn validate(&self) -> ChannelResult<()> {
        let valid = self.phase != ActivationPhase::Invalid
            && self.schema == ACTIVATION_SCHEMA
            && validate_request(&self.request).is_ok()
            && (0..=6).contains(&self.input_step)
            && self.budget.as_ref().is_some_and(|b| b.validate().is_ok())
            && self.outcome.as_ref().is_none_or(|o| o.validate().is_ok())
            && (!matches!(
                self.phase,
                ActivationPhase::WaitingManualReload | ActivationPhase::Cancelled
            ) || self.input_free());
        if valid {
            Ok(())
        } else {
            Err(error("live.channel_activation_invalid"))
        }
    }
    pub fn input_free(&self) -> bool {
        self.input_step == 0
            && self.outcome.as_ref().is_none_or(|o| {
                o.disposition == InputDisposition::NotSent && o.messages_queued == 0
            })
    }
    pub fn cancel(&mut self) -> ChannelResult<()> {
        self.validate()?;
        if self.phase == ActivationPhase::Cancelled {
            return Ok(());
        }
        if !matches!(
            self.phase,
            ActivationPhase::Prepared | ActivationPhase::WaitingManualReload
        ) || !self.input_free()
        {
            return Err(error("live.channel_journal_missing"));
        }
        self.phase = ActivationPhase::Cancelled;
        Ok(())
    }
    /// Project a clone. Read-only status must never persist wall-clock changes.
    pub fn present(&self, id: &str, path: &Path, now_ms: i64) -> ProjectResult {
        let mut r = present(
            &State {
                id: id.into(),
                ..State::default()
            },
            path,
            now_ms,
            "",
            "",
        );
        r.stage = format!("activation_{}", self.phase.name());
        r.complete = false;
        r.continuation.kind = "continue".into();
        r.continuation.goal = "activation".into();
        r.continuation.request_id = self.request.clone();
        r.continuation.remaining_budget_ms = None;
        if self.phase == ActivationPhase::Cancelled {
            r.closed = true;
            r.continuation.kind = "completed".into();
        } else if self.phase == ActivationPhase::WaitingManualReload {
            r.continuation.kind = "wait_external".into();
            r.continuation.blocker = Some(manual_reload_blocker());
        } else if let Some(mut budget) = self.budget.clone() {
            let (_, remaining) = observe_budget(&mut budget, now_ms);
            r.continuation.remaining_budget_ms = Some(remaining.as_ref().copied().unwrap_or(0));
            if remaining.is_err() {
                r.continuation.kind = "budget_exhausted".into();
                r.continuation.blocker = Some(Blocker {
                    kind: if budget.clock_rollback {
                        "budget_clock_rollback"
                    } else {
                        "budget_exhausted"
                    }
                    .into(),
                    condition: "explicit_budget_decision".into(),
                    ..Default::default()
                });
            }
        }
        r
    }
}
pub fn manual_reload_blocker() -> Blocker {
    Blocker {
        kind: "manual_reload_required".into(),
        condition: "reload_selected_client_then_resume".into(),
        ..Default::default()
    }
}
pub fn manual_reload_required() -> ChannelError {
    ChannelError::Blocked(manual_reload_blocker())
}

#[cfg(test)]
#[path = "activation_tests.rs"]
mod tests;
