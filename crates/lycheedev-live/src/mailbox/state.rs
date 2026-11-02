//! Persisted fixed-Go channel.v2 data. These are facts, never input authority.
//! Oracle: channel/{driver,state,input,budget,request,runtime_replacement}.go
//! at 3057a9666ab078270b63c7f06de748069b0d1f4b.
use super::{
    ContractError, Identity, InputDiagnostic, InputObservation, InputOutcome, MAX_EXACT_INTEGER,
    MAX_PAYLOAD, Receipt, Result, SCHEMA, SLOT_COUNT, ZERO_TOKEN, checksum, parse_token,
};
pub use lycheedev_core::mailbox::{CodeBytes, SlotEnvelope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[path = "state_snapshot.rs"]
mod snapshot;

pub const CHANNEL_SCHEMA: &str = "lycheedev.channel.v2";
pub const DEFAULT_CONNECT_MILLIS: i64 = 120_000;
pub const DEFAULT_CLOSE_MILLIS: i64 = 120_000;
pub const DEFAULT_RECOVERY_MILLIS: i64 = 600_000;
fn zero_i32(n: &i32) -> bool {
    *n == 0
}
fn zero_i64(n: &i64) -> bool {
    *n == 0
}
fn zero_u32(n: &u32) -> bool {
    *n == 0
}
fn zero_u64(n: &u64) -> bool {
    *n == 0
}
fn no(b: &bool) -> bool {
    !*b
}
fn bad() -> ContractError {
    ContractError("live.channel_journal_invalid")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionPhase {
    Intent,
    Published,
    InputAttempted,
    Received,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationStage {
    Prepared,
    CommitReady,
    Running,
    ConfirmReady,
    ResultVerified,
    ReleaseReady,
    Complete,
    ExecutionUnknown,
    Cancelled,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPolicy {
    Observation,
    Opaque,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupMethod {
    #[default]
    #[serde(rename = "")]
    None,
    Released,
    RuntimeDestroyed,
    ReloadRequired,
}
impl CleanupMethod {
    fn absent(&self) -> bool {
        *self == Self::None
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadPhase {
    Intent,
    InputAttempted,
    Binding,
    Complete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryPhase {
    Binding,
    Bound,
    Complete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputKind {
    Invoke,
    Reload,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessEnd {
    #[default]
    #[serde(rename = "")]
    None,
    ProcessAbsent,
    ProcessExited,
    PidReused,
}
impl ProcessEnd {
    fn absent(&self) -> bool {
        *self == Self::None
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DurableBudget {
    #[serde(rename = "startedAtMs")]
    pub started_at_ms: i64,
    #[serde(rename = "deadlineMs")]
    pub deadline_ms: i64,
    #[serde(rename = "lastObservedMs")]
    pub last_observed_ms: i64,
    #[serde(skip_serializing_if = "no")]
    pub exhausted: bool,
    #[serde(skip_serializing_if = "no")]
    pub clock_rollback: bool,
}
impl DurableBudget {
    pub fn validate(&self) -> Result<()> {
        if self.started_at_ms <= 0
            || self.deadline_ms <= self.started_at_ms
            || i128::from(self.deadline_ms) - i128::from(self.started_at_ms)
                > i128::from(i64::MAX / 1_000_000)
            || self.last_observed_ms < self.started_at_ms
            || (self.clock_rollback && !self.exhausted)
            || (!self.exhausted && self.last_observed_ms >= self.deadline_ms)
        {
            Err(ContractError("live.channel_budget_invalid"))
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Blocker {
    pub kind: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub installation: String,
    #[serde(skip_serializing_if = "zero_i32")]
    pub slot: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub consumer: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub runtime: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub nonce: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub condition: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transaction {
    #[serde(
        default,
        rename = "releaseWaitStartedAtMs",
        skip_serializing_if = "zero_i64"
    )]
    pub release_wait_started_at_ms: i64,
    pub envelope: SlotEnvelope,
    pub phase: TransactionPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Receipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_observation: Option<InputObservation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InputAttempt {
    pub id: String,
    pub exchange: String,
    pub runtime: String,
    pub kind: InputKind,
    #[serde(default, skip_serializing_if = "zero_i64")]
    pub after_millis: i64,
    #[serde(default, skip_serializing_if = "zero_i64")]
    pub recovery_after_millis: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observation: Option<InputObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<InputOutcome>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_diagnostic: Option<InputDiagnostic>,
}
/// Persist the actual Rust budget link; equal timestamps are not identity.
/// Absent on Go records: Go Load restores independent budget values, not aliases.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SlotBudgetSource {
    Connect,
    Close,
    Operation { operation_id: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadAttempt {
    #[serde(default, skip_serializing_if = "no")]
    pub automatic: bool,
    #[serde(default, skip_serializing_if = "no")]
    pub slot_enablement: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub slot_budget_source: Option<SlotBudgetSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_budget: Option<DurableBudget>,
    #[serde(default, skip_serializing_if = "no")]
    pub reconcile_pending: bool,
    pub request: String,
    pub from: String,
    pub phase: ReloadPhase,
    #[serde(default)]
    pub input_step: i32,
    #[serde(default)]
    pub patch_transitions: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRecovery {
    pub from: Identity,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction: Option<Transaction>,
    pub phase: RecoveryPhase,
}

/// Go []byte JSON is standard padded base64. Preserve original report bytes;
/// serializing the parsed JSON would change whitespace/HTML and its checksum.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResultBytes(pub Vec<u8>);
impl ResultBytes {
    fn empty(&self) -> bool {
        self.0.is_empty()
    }
}
impl Serialize for ResultBytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        const DIGITS: &[u8; 64] =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut text = String::with_capacity(self.0.len().div_ceil(3) * 4);
        for c in self.0.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            text.push(DIGITS[((n >> 18) & 63) as usize] as char);
            text.push(DIGITS[((n >> 12) & 63) as usize] as char);
            text.push(if c.len() > 1 {
                DIGITS[((n >> 6) & 63) as usize] as char
            } else {
                '='
            });
            text.push(if c.len() > 2 {
                DIGITS[(n & 63) as usize] as char
            } else {
                '='
            });
        }
        s.serialize_str(&text)
    }
}
impl<'de> Deserialize<'de> for ResultBytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let raw = Option::<String>::deserialize(d)?.unwrap_or_default();
        let b: Vec<_> = raw.bytes().filter(|c| *c != b'\r' && *c != b'\n').collect();
        let invalid =
            || <D::Error as serde::de::Error>::custom("live.channel_result_bytes_invalid");
        if b.len() % 4 != 0 || b.len() > MAX_PAYLOAD.div_ceil(3) * 4 {
            return Err(invalid());
        }
        let value = |c: u8| match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        };
        let mut out = Vec::with_capacity(b.len() / 4 * 3);
        for (i, c) in b.chunks_exact(4).enumerate() {
            let last = i + 1 == b.len() / 4;
            let a = value(c[0]).ok_or_else(invalid)?;
            let b = value(c[1]).ok_or_else(invalid)?;
            let pad2 = c[2] == b'=';
            let pad3 = c[3] == b'=';
            if (pad2 && !pad3) || ((pad2 || pad3) && !last) {
                return Err(invalid());
            }
            let cc = if pad2 {
                0
            } else {
                value(c[2]).ok_or_else(invalid)?
            };
            let dd = if pad3 {
                0
            } else {
                value(c[3]).ok_or_else(invalid)?
            };
            out.push((a << 2) | (b >> 4));
            if !pad2 {
                out.push((b << 4) | (cc >> 2));
            }
            if !pad3 {
                out.push((cc << 6) | dd);
            }
        }
        if out.len() > MAX_PAYLOAD {
            return Err(invalid());
        }
        Ok(Self(out))
    }
}
fn reject_legacy_result<'de, D: serde::Deserializer<'de>>(
    _: D,
) -> std::result::Result<(), D::Error> {
    Err(serde::de::Error::custom(
        "live.channel_operation_schema_invalid",
    ))
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_budget: Option<DurableBudget>,
    #[serde(default, skip_serializing_if = "CleanupMethod::absent")]
    pub cleanup_method: CleanupMethod,
    #[serde(default, skip_serializing_if = "zero_i32")]
    pub attempt: i32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Identity>,
    pub id: String,
    pub ticket: String,
    pub code: CodeBytes,
    pub budget: i32,
    pub policy: OperationPolicy,
    pub stage: OperationStage,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub prepared_nonce: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub challenge: String,
    #[serde(default, skip_serializing_if = "zero_u32")]
    pub report_bytes: u32,
    #[serde(default, skip_serializing_if = "zero_u32")]
    pub report_checksum: u32,
    #[serde(default, skip_serializing_if = "ResultBytes::empty")]
    pub result_bytes: ResultBytes,
    // Private sentinel rejects even explicit result:null, as Go does. Use the
    // constructor for new operations; serialized output never includes this.
    #[serde(
        rename = "result",
        default,
        skip_serializing,
        deserialize_with = "reject_legacy_result"
    )]
    legacy_result: (),
}
impl Operation {
    pub fn prepared(
        id: String,
        ticket: String,
        code: String,
        budget: i32,
        policy: OperationPolicy,
    ) -> Self {
        Self::prepared_bytes(id, ticket, code.into(), budget, policy)
    }
    pub fn prepared_bytes(
        id: String,
        ticket: String,
        code: CodeBytes,
        budget: i32,
        policy: OperationPolicy,
    ) -> Self {
        Self {
            recovery_budget: None,
            cleanup_method: CleanupMethod::None,
            attempt: 0,
            request: String::new(),
            origin: None,
            id,
            ticket,
            code,
            budget,
            policy,
            stage: OperationStage::Prepared,
            prepared_nonce: String::new(),
            challenge: String::new(),
            report_bytes: 0,
            report_checksum: 0,
            result_bytes: ResultBytes::default(),
            legacy_result: (),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReplacementWitness {
    pub first: InputObservation,
    pub first_sequence: u32,
    #[serde(with = "decimal_address")]
    pub address: u64,
    pub length: u32,
    #[serde(rename = "beforeSHA256")]
    pub before_sha256: String,
    #[serde(rename = "afterSHA256")]
    pub after_sha256: String,
    pub sequence: u32,
    pub observation: InputObservation,
}
mod decimal_address {
    use serde::{Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(n: &u64, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&n.to_string())
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
        String::deserialize(d)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeReplacementProof {
    pub schema: String,
    pub process_id: u32,
    pub process_started_at: u64,
    pub current: Identity,
    pub witness: RuntimeReplacementWitness,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct State {
    #[serde(skip_serializing_if = "zero_u64")]
    pub progress_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<Blocker>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_budget: Option<DurableBudget>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_budget: Option<DurableBudget>,
    #[serde(skip_serializing_if = "ProcessEnd::absent")]
    pub process_end: ProcessEnd,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_end: Option<RuntimeReplacementProof>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub artifacts: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub archive: String,
    pub schema: String,
    pub id: String,
    pub owner: String,
    pub identity: Identity,
    pub bound: bool,
    #[serde(skip_serializing_if = "no")]
    pub closed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<Transaction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload: Option<ReloadAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<RuntimeRecovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<InputAttempt>,
    #[serde(skip_serializing_if = "no")]
    pub closing: bool,
}

pub fn validate_request(key: &str) -> Result<()> {
    if key.is_empty() || key.len() > 256 || key.chars().any(|c| c < ' ' || c == '\x7f') {
        Err(ContractError("live.channel_request_key_invalid"))
    } else {
        Ok(())
    }
}
fn digest(s: &str, lower: bool) -> bool {
    s.len() == 64 && hex::decode(s).is_ok() && (!lower || s == s.to_ascii_lowercase())
}
impl RuntimeReplacementProof {
    pub fn validate(&self, old: &Identity) -> Result<()> {
        let bad = || ContractError("live.channel_runtime_replacement_invalid");
        if self.schema != SCHEMA
            || old.validate().is_err()
            || self.current.validate().is_err()
            || self.process_id == 0
            || self.process_started_at == 0
            || self.current.runtime == old.runtime
            || self.current.runtime == "0".repeat(32)
            || self.current.input_state != SCHEMA
            || self.current.build != old.build
            || self.current.product != old.product
            || self.current.release != old.release
        {
            return Err(bad());
        }
        let w = &self.witness;
        if w.address == 0
            || w.length <= 120
            || w.length > 2168
            || w.address.checked_add(u64::from(w.length)).is_none()
            || w.first_sequence == 0
            || w.first_sequence == u32::MAX
            || w.sequence <= w.first_sequence
            || w.sequence == u32::MAX
            || !digest(&w.before_sha256, true)
            || !digest(&w.after_sha256, true)
            || w.before_sha256 == w.after_sha256
            || w.observation.sample_millis <= w.first.sample_millis
        {
            return Err(bad());
        }
        for o in [&w.first, &w.observation] {
            if o.schema != SCHEMA
                || o.runtime != self.current.runtime
                || o.owner != self.current.owner
                || o.fence != self.current.fence
                || o.guid != self.current.guid
                || o.build != self.current.build
                || o.next_slot != self.current.next_slot
                || o.sample_millis < 0
                || o.sample_millis as u64 > MAX_EXACT_INTEGER
                || !(1..=SLOT_COUNT + 1).contains(&o.next_slot)
                || o.guid.is_empty()
                || o.build.is_empty()
                || !o.valid_readiness()
                || !o.valid_bindings()
                || (if o.owner.is_empty() {
                    o.fence != 0
                } else {
                    parse_token(&o.owner).map_or(true, |t| t == ZERO_TOKEN)
                        || o.fence == 0
                        || o.fence > MAX_EXACT_INTEGER
                })
                || serde_json::to_vec(o)
                    .ok()
                    .and_then(|b| super::journal::raw_json_bytes(&b).ok())
                    .is_none_or(|b| b.len() > 2048)
            {
                return Err(bad());
            }
        }
        Ok(())
    }
}
impl State {
    pub fn validate(&self) -> Result<()> {
        for b in [
            self.connect_budget.as_ref(),
            self.close_budget.as_ref(),
            self.operation
                .as_ref()
                .and_then(|o| o.recovery_budget.as_ref()),
            self.reload
                .as_ref()
                .and_then(|r| r.recovery_budget.as_ref()),
        ]
        .into_iter()
        .flatten()
        {
            b.validate().map_err(|_| bad())?;
        }
        if let Some(p) = &self.runtime_end {
            p.validate(&self.identity).map_err(|_| bad())?;
            if self
                .recovery
                .as_ref()
                .is_some_and(|r| r.from.runtime == p.current.runtime)
            {
                return Err(bad());
            }
            for tx in [
                self.transaction.as_ref(),
                self.recovery.as_ref().and_then(|r| r.transaction.as_ref()),
            ]
            .into_iter()
            .flatten()
            {
                if tx.envelope.runtime == p.current.runtime || tx.envelope.build != p.current.build
                {
                    return Err(bad());
                }
            }
        }
        if let Some(i) = &self.input {
            parse_token(&i.id).map_err(|_| bad())?;
            parse_token(&i.runtime).map_err(|_| bad())?;
            if i.exchange.is_empty()
                || i.after_millis < 0
                || i.recovery_after_millis < 0
                || i.outcome.as_ref().is_some_and(|o| o.validate().is_err())
                || i.after_diagnostic
                    .as_ref()
                    .is_some_and(|d| !d.valid() || d.runtime != i.runtime)
                || i.observation.as_ref().is_some_and(|o| {
                    o.input_attempt
                        .as_ref()
                        .is_some_and(|d| !d.valid() || d.runtime != o.runtime)
                })
            {
                return Err(bad());
            }
        }
        if !self.archive.is_empty() && !digest(&self.archive, false) {
            return Err(bad());
        }
        if self.schema != CHANNEL_SCHEMA
            || self.id != format!("CON-{}", self.owner)
            || self.identity.validate().is_err()
        {
            return Err(bad());
        }
        parse_token(&self.owner).map_err(|_| bad())?;
        if self.closed
            && (self.bound
                || self.transaction.is_some()
                || self.operation.as_ref().is_some_and(|o| {
                    !matches!(
                        o.stage,
                        OperationStage::Complete
                            | OperationStage::ExecutionUnknown
                            | OperationStage::Cancelled
                    )
                }))
        {
            return Err(bad());
        }
        if let Some(r) = &self.recovery {
            if r.from.validate().is_err() || r.from.runtime >= self.identity.runtime {
                return Err(bad());
            }
            if let Some(tx) = &r.transaction {
                let e = &tx.envelope;
                e.validate().map_err(|_| bad())?;
                if e.runtime != r.from.runtime
                    || e.owner != self.owner
                    || e.guid != r.from.guid
                    || e.build != r.from.build
                {
                    return Err(bad());
                }
            }
        }
        if let Some(r) = &self.reload {
            validate_request(&r.request).map_err(|_| bad())?;
            parse_token(&r.from).map_err(|_| bad())?;
            if let Some(source) = &r.slot_budget_source {
                if !r.automatic || !r.slot_enablement || !r.request.starts_with("slots-") {
                    return Err(bad());
                }
                if let SlotBudgetSource::Operation { operation_id } = source {
                    parse_token(operation_id.strip_prefix("LMO-").ok_or_else(bad)?)
                        .map_err(|_| bad())?;
                }
            }
            if !(0..=16).contains(&r.input_step) || !(0..=2).contains(&r.patch_transitions) {
                return Err(bad());
            }
            if r.phase != ReloadPhase::Complete
                && (self.closed
                    || (!r.reconcile_pending
                        && self.operation.as_ref().is_some_and(|o| {
                            o.stage != OperationStage::Complete
                                && (o.stage != OperationStage::Prepared
                                    || !o.prepared_nonce.is_empty())
                                && !(o.stage == OperationStage::ReleaseReady
                                    && o.cleanup_method == CleanupMethod::ReloadRequired)
                        })))
            {
                return Err(bad());
            }
        }
        if let Some(tx) = &self.transaction {
            if tx.release_wait_started_at_ms < 0 {
                return Err(bad());
            }
            let e = &tx.envelope;
            e.validate().map_err(|_| bad())?;
            if e.owner != self.owner
                || e.runtime != self.identity.runtime
                || e.guid != self.identity.guid
                || e.build != self.identity.build
            {
                return Err(bad());
            }
            match tx.phase {
                TransactionPhase::Received => {
                    let r = tx.receipt.as_ref().ok_or_else(bad)?;
                    if r.validate().is_err()
                        || r.identity.schema != e.schema
                        || r.nonce != e.nonce
                        || r.ticket != e.ticket
                        || r.action != e.action
                    {
                        return Err(bad());
                    }
                }
                _ => {
                    if tx.receipt.is_some() {
                        return Err(bad());
                    }
                }
            }
        }
        if let Some(o) = &self.operation {
            if match o.cleanup_method {
                CleanupMethod::None => false,
                CleanupMethod::Released | CleanupMethod::RuntimeDestroyed => {
                    o.stage != OperationStage::Complete
                }
                CleanupMethod::ReloadRequired => o.stage != OperationStage::ReleaseReady,
            } {
                return Err(bad());
            }
            if !(0..=3).contains(&o.attempt)
                || o.origin.as_ref().is_some_and(|i| i.validate().is_err())
            {
                return Err(bad());
            }
            if !o.request.is_empty() {
                validate_request(&o.request).map_err(|_| bad())?;
            }
            parse_token(o.id.strip_prefix("LMO-").ok_or_else(bad)?).map_err(|_| bad())?;
            parse_token(&o.ticket).map_err(|_| bad())?;
            if o.code.is_empty() || o.code.len() > 262144 || !(1..=120).contains(&o.budget) {
                return Err(bad());
            }
            if !matches!(
                o.stage,
                OperationStage::Prepared
                    | OperationStage::ExecutionUnknown
                    | OperationStage::Cancelled
            ) {
                parse_token(&o.prepared_nonce).map_err(|_| bad())?;
                parse_token(&o.challenge).map_err(|_| bad())?;
            }
            if matches!(
                o.stage,
                OperationStage::ConfirmReady
                    | OperationStage::ResultVerified
                    | OperationStage::ReleaseReady
                    | OperationStage::Complete
            ) && (o.result_bytes.0.len() > MAX_PAYLOAD
                || o.result_bytes.0.len() != o.report_bytes as usize
                || checksum(&o.result_bytes.0) != o.report_checksum
                || !lycheedev_core::raw_json::is_valid(&o.result_bytes.0))
            {
                return Err(bad());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod tests;
