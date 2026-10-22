use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct InputBindings {
    pub wake: String,
    pub reload: String,
}
impl InputBindings {
    fn current(&self) -> bool {
        fn chord(s: &str) -> bool {
            (1..=12).any(|n| s == format!("ALT-CTRL-F{n}"))
        }
        self.wake != self.reload && chord(&self.wake) && chord(&self.reload)
    }
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InputObservation {
    pub bindings: Option<InputBindings>,
    pub schema: String,
    pub runtime: String,
    pub owner: String,
    pub fence: u64,
    pub next_slot: i32,
    pub guid: String,
    pub build: String,
    pub sample_millis: i64,
    pub input_blocked: Option<bool>,
    #[serde(skip_serializing_if = "is_false")]
    pub slot_reload_required: bool,
    pub reason: String,
    pub escape_limit: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_attempt: Option<InputDiagnostic>,
    /// Host uptime from this read. Never restored from addon JSON.
    #[serde(skip)]
    pub observed_at_millis: i64,
}

impl<'de> Deserialize<'de> for InputObservation {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        // Go InputObservation.UnmarshalJSON applies the diagnostic's raw 512-byte
        // limit to both wire and journal JSON. The outer wire's 2048-byte limit
        // belongs only to decode_input, not this reusable DTO deserializer.
        #[derive(Default, Deserialize)]
        #[serde(default, rename_all = "camelCase")]
        struct Wire {
            bindings: Option<InputBindings>,
            schema: String,
            runtime: String,
            owner: String,
            fence: u64,
            next_slot: i32,
            guid: String,
            build: String,
            sample_millis: i64,
            input_blocked: Option<bool>,
            slot_reload_required: Option<bool>,
            reason: String,
            escape_limit: i32,
            input_attempt: Option<Box<serde_json::value::RawValue>>,
        }
        let w = Wire::deserialize(deserializer)?;
        let input_attempt = w.input_attempt.as_deref().and_then(|raw| {
            parse_input_diagnostic(raw.get().as_bytes(), &w.runtime, w.sample_millis)
        });
        Ok(Self {
            bindings: w.bindings,
            schema: w.schema,
            runtime: w.runtime,
            owner: w.owner,
            fence: w.fence,
            next_slot: w.next_slot,
            guid: w.guid,
            build: w.build,
            sample_millis: w.sample_millis,
            input_blocked: w.input_blocked,
            slot_reload_required: w.slot_reload_required.unwrap_or(false),
            reason: w.reason,
            escape_limit: w.escape_limit,
            input_attempt,
            observed_at_millis: 0,
        })
    }
}
impl InputObservation {
    /// An equal timestamp does not authorize a changed activation/key profile.
    pub fn same_input_profile(&self, stored: &Self) -> bool {
        self.valid_bindings()
            && stored.valid_bindings()
            && self.bindings.is_some()
            && self.bindings == stored.bindings
            && self.slot_reload_required == stored.slot_reload_required
    }
    pub fn valid_readiness(&self) -> bool {
        self.input_blocked
            .is_some_and(|blocked| blocked != self.reason.is_empty())
            && self.escape_limit == 0
    }
    pub fn valid_bindings(&self) -> bool {
        let (Some(b), Some(blocked)) = (&self.bindings, self.input_blocked) else {
            return false;
        };
        if b == &InputBindings::default() {
            return blocked && self.reason == "input_binding_unavailable";
        }
        self.reason != "input_binding_unavailable" && b.current()
    }
}

/// A structurally valid sample may be stale. Keep its two-key capability
/// evidence for read-only upgrade inspection, without granting input authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFreshness {
    Current,
    Stale,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputInspection {
    pub observation: InputObservation,
    pub freshness: InputFreshness,
}
/// Freshness-requiring wrapper for existing consumers. For upgrade capability
/// inspection use inspect_input so a stale sample is not mistaken for absence.
pub fn decode_input(
    record: &Record<'_>,
    e: &ExpectedEnvelope,
    after: i64,
    now: i64,
) -> Result<InputObservation> {
    let inspected = inspect_input(record, e, after, now)?;
    if inspected.freshness == InputFreshness::Stale {
        Err(ContractError("live.channel_input_state_stale"))
    } else {
        Ok(inspected.observation)
    }
}

/// Qualifies sample data only. Native publication rechecks, selected target,
/// current physical-key state, persisted intent and user authorization remain
/// separate gates. Current samples can still report input_blocked=true.
pub fn inspect_input(
    record: &Record<'_>,
    e: &ExpectedEnvelope,
    after: i64,
    now: i64,
) -> Result<InputInspection> {
    let invalid = ContractError("live.channel_input_state_invalid");
    let h = &record.header;
    if record.payload.len() > 2048
        || h.kind != Kind::Input
        || h.state != 1
        || h.sequence == 0
        || h.sequence == u32::MAX
        || h.ticket != ZERO_TOKEN
    {
        return Err(invalid);
    }
    let mut s: InputObservation = serde_json::from_slice(record.payload).map_err(|_| invalid)?;
    if s.schema != SCHEMA || !s.valid_readiness() || !s.valid_bindings() {
        return Err(invalid);
    }
    let runtime = parse_token(&s.runtime).map_err(|_| invalid)?;
    if runtime == ZERO_TOKEN || h.runtime != runtime || h.nonce != runtime {
        return Err(invalid);
    }
    if s.runtime != e.runtime
        || s.guid != e.guid
        || s.build != e.build
        || (e.action != "reload" && s.next_slot != e.slot_start())
        || (s.owner != e.owner && !(e.action == "bind" && s.owner.is_empty()))
        || (!s.owner.is_empty() && s.fence != e.fence)
    {
        return Err(ContractError("live.channel_input_target_changed"));
    }
    // Wider arithmetic avoids overflow at hostile boundary values.
    let sample = i128::from(s.sample_millis);
    if sample < 0 || sample > i128::from(now) + 100 {
        return Err(ContractError("live.channel_input_clock_invalid"));
    }
    let freshness = if sample < i128::from(after) + 100 || sample < i128::from(now) - 500 {
        InputFreshness::Stale
    } else {
        s.observed_at_millis = now;
        InputFreshness::Current
    };
    Ok(InputInspection {
        observation: s,
        freshness,
    })
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct InputDiagnostic {
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub runtime: String,
    #[serde(default)]
    pub attempt_seq: u64,
    #[serde(default)]
    pub slot: i32,
    #[serde(default)]
    pub stage: String,
    pub reason: String,
    pub received: bool,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub sample_millis: i64,
}
fn is_zero(n: &i64) -> bool {
    *n == 0
}
impl InputDiagnostic {
    pub fn valid(&self) -> bool {
        if parse_token(&self.runtime).map_or(true, |t| t == ZERO_TOKEN)
            || self.schema != SCHEMA
            || self.attempt_seq == 0
            || self.attempt_seq > MAX_EXACT_INTEGER
            || !(1..=SLOT_COUNT + 1).contains(&self.slot)
            || self.sample_millis < 0
        {
            return false;
        }
        match self.stage.as_str() {
            "entered" | "loading" => self.reason.is_empty(),
            "received" => self.received && self.reason.is_empty(),
            "load_failed" => matches!(
                self.reason.as_str(),
                "slot_load_failed" | "slot_addon_disabled"
            ),
            "no_dispatch" => self.reason == "slot_loader_no_dispatch",
            "exception" => self.reason == "slot_wake_failed",
            "rejected" => matches!(
                self.reason.as_str(),
                "slot_input_not_ready"
                    | "input_protection_not_ready"
                    | "input_protection_busy"
                    | "input_protection_unavailable"
                    | "input_protection_timer_unavailable"
                    | "slot_exhausted"
                    | "slot_inventory_incomplete"
                    | "slot_wake_rejected"
            ),
            _ => false,
        }
    }
}
pub fn parse_input_diagnostic(raw: &[u8], runtime: &str, sample: i64) -> Option<InputDiagnostic> {
    if raw.is_empty() || raw.len() > 512 {
        return None;
    }
    let mut d: InputDiagnostic = serde_json::from_slice(raw).ok()?;
    if !d.valid() || d.runtime != runtime {
        return None;
    }
    d.sample_millis = sample;
    Some(d)
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputDisposition {
    NotSent,
    Submitted,
    Uncertain,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InputOutcome {
    pub disposition: InputDisposition,
    #[serde(default)]
    pub messages_queued: i32,
    #[serde(default)]
    pub at_millis: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retryable: bool,
}
fn is_false(value: &bool) -> bool {
    !value
}
impl InputOutcome {
    pub fn validate(&self) -> Result<()> {
        if self.messages_queued < 0
            || self.at_millis < 0
            || (self.disposition == InputDisposition::NotSent && self.messages_queued != 0)
            || (self.disposition == InputDisposition::Submitted && self.messages_queued == 0)
            || (self.retryable && self.disposition != InputDisposition::NotSent)
        {
            return Err(ContractError("live.channel_input_outcome_invalid"));
        }
        Ok(())
    }
}
/// The persisted intent's outcome, not diagnostics/readiness, determines whether
/// the same exchange may be prepared again. None means unknown after a crash.
/// true still requires a separate current-slot proof and fresh native checks.
pub fn may_prepare_again(outcome: Option<&InputOutcome>) -> bool {
    outcome.is_some_and(|o| {
        o.validate().is_ok() && o.disposition == InputDisposition::NotSent && o.messages_queued == 0
    })
}
