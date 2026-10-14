//! Fixed-Go channel snapshot and append-chain contracts. No filesystem writer is
//! wired here: production writes require the selected connection's owning lease,
//! durable artifact-before-log ordering, fsync/close checks and retained history.
use super::observations::{ChannelResult, check_control};
use super::state::{Operation, ResultBytes, State};
use super::{ContractError, MAX_PAYLOAD, Result};
use lycheedev_core::control::OperationControl;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_LOG_BYTES: usize = 64 << 20;
pub const MAX_EVENT_BYTES: usize = 2 << 20;
pub const MAX_EVENTS: usize = 1024;
pub const CHECKPOINT_EVENTS: usize = 256;
pub const CHECKPOINT_BYTES: usize = 8 << 20;
pub const MAX_HISTORY_SEGMENTS: usize = 1024;

/// Implementors own the exclusive driver lease for all mutating calls. An Err
/// never permits the driver to proceed with the external effect being journaled.
/// historical_request is read-only and must check the complete retained chain;
/// missing/corrupt history cannot make an old request look new.
pub trait StateJournal {
    fn append(
        &mut self,
        control: &OperationControl,
        event: &str,
        state: &State,
    ) -> ChannelResult<()>;
    fn historical_request(
        &mut self,
        control: &OperationControl,
        request: &str,
    ) -> ChannelResult<Option<State>>;
    fn write_result(
        &mut self,
        control: &OperationControl,
        operation: &Operation,
    ) -> ChannelResult<()>;
    fn checkpoint(
        &mut self,
        control: &OperationControl,
        state: &mut State,
        allow_pending_known_zero: bool,
    ) -> ChannelResult<()>;
}
/// Go Driver.Save's progress boundary. Budget/blocker observations do not advance
/// business progress. A failed save must stop the dependent side effect.
pub fn save_state(
    journal: &mut dyn StateJournal,
    control: &OperationControl,
    state: &mut State,
    event: &str,
) -> ChannelResult<()> {
    check_control(control)?;
    if state.schema == "lycheedev.channel.v1" {
        state.schema = super::state::CHANNEL_SCHEMA.into();
    }
    if !matches!(
        event,
        "budget_observed" | "budget_migrated" | "blocker_observed"
    ) {
        state.progress_version = state
            .progress_version
            .checked_add(1)
            .ok_or(ContractError("live.channel_progress_overflow"))?;
    }
    state.validate()?;
    journal.append(control, event, state)
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Contains digest references and cleared code/report fields. Validate only
    /// after hydration, as Go does; this is not a ready-to-run channel state.
    pub state: State,
    /// SHA256 => exact original bytes; commit durably before appending state.
    pub artifacts: BTreeMap<String, Vec<u8>>,
}
fn hash(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn artifact_digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn snapshot(state: &State) -> Result<Snapshot> {
    // Match Go's JSON copy: host observedAtMillis is not restored as freshness.
    let bytes =
        serde_json::to_vec(state).map_err(|_| ContractError("live.channel_journal_invalid"))?;
    let mut copy: State = serde_json::from_slice(&bytes)
        .map_err(|_| ContractError("live.channel_journal_invalid"))?;
    copy.artifacts.clear();
    let mut artifacts = BTreeMap::new();
    let mut put = |key: &str, bytes: Vec<u8>| {
        if bytes.is_empty() {
            return;
        }
        let digest = hash(&bytes);
        copy.artifacts.insert(key.into(), digest.clone());
        artifacts.entry(digest).or_insert(bytes);
    };
    if let Some(o) = &mut copy.operation {
        put("code", std::mem::take(&mut o.code).into_bytes());
        put("result", std::mem::take(&mut o.result_bytes.0));
    }
    if let Some(t) = &mut copy.transaction {
        put(
            "transaction",
            std::mem::take(&mut t.envelope.code).into_bytes(),
        );
    }
    if let Some(t) = copy.recovery.as_mut().and_then(|r| r.transaction.as_mut()) {
        put(
            "recovery",
            std::mem::take(&mut t.envelope.code).into_bytes(),
        );
    }
    Ok(Snapshot {
        state: copy,
        artifacts,
    })
}
pub fn hydrate(mut state: State, mut read: impl FnMut(&str) -> Result<Vec<u8>>) -> Result<State> {
    for (key, digest) in std::mem::take(&mut state.artifacts) {
        if !artifact_digest(&digest) {
            return Err(ContractError("live.channel_artifact_invalid"));
        }
        let bytes = read(&digest)?;
        if bytes.len() > MAX_PAYLOAD {
            return Err(ContractError("live.channel_artifact_invalid"));
        }
        if hash(&bytes) != digest {
            return Err(ContractError("live.channel_artifact_corrupt"));
        }
        match key.as_str() {
            "result" => {
                state
                    .operation
                    .as_mut()
                    .ok_or(ContractError("live.channel_artifact_invalid"))?
                    .result_bytes = ResultBytes(bytes)
            }
            "code" => {
                state
                    .operation
                    .as_mut()
                    .ok_or(ContractError("live.channel_artifact_invalid"))?
                    .code = String::from_utf8(bytes)
                    .map_err(|_| ContractError("live.channel_artifact_invalid"))?
            }
            "transaction" => {
                state
                    .transaction
                    .as_mut()
                    .ok_or(ContractError("live.channel_artifact_invalid"))?
                    .envelope
                    .code = String::from_utf8(bytes)
                    .map_err(|_| ContractError("live.channel_artifact_invalid"))?
            }
            "recovery" => {
                state
                    .recovery
                    .as_mut()
                    .and_then(|r| r.transaction.as_mut())
                    .ok_or(ContractError("live.channel_artifact_invalid"))?
                    .envelope
                    .code = String::from_utf8(bytes)
                    .map_err(|_| ContractError("live.channel_artifact_invalid"))?
            }
            _ => return Err(ContractError("live.channel_artifact_invalid")),
        }
    }
    state.validate()?;
    Ok(state)
}

/// Go json.Marshal compacts RawMessage without sorting its keys, escapes HTML,
/// and escapes literal U+2028/U+2029. Preserve raw numeric spellings and escapes.
/// Parsing into Value and serializing it would change historical chain hashes.
pub fn go_json_bytes(raw: &[u8]) -> Result<Vec<u8>> {
    let text = std::str::from_utf8(raw).map_err(|_| ContractError("journal.memory_log_corrupt"))?;
    serde_json::from_str::<Box<serde_json::value::RawValue>>(text)
        .map_err(|_| ContractError("journal.memory_log_corrupt"))?;
    let mut out = String::with_capacity(text.len());
    let mut quoted = false;
    let mut escape = false;
    for c in text.chars() {
        if quoted {
            if escape {
                out.push(c);
                escape = false;
                continue;
            }
            match c {
                '\\' => {
                    out.push(c);
                    escape = true;
                }
                '"' => {
                    out.push(c);
                    quoted = false;
                }
                '<' => out.push_str("\\u003c"),
                '>' => out.push_str("\\u003e"),
                '&' => out.push_str("\\u0026"),
                '\u{2028}' => out.push_str("\\u2028"),
                '\u{2029}' => out.push_str("\\u2029"),
                _ => out.push(c),
            }
        } else if c == '"' {
            out.push(c);
            quoted = true;
        } else if !matches!(c, ' ' | '\n' | '\r' | '\t') {
            out.push(c);
        }
    }
    Ok(out.into_bytes())
}
fn json_text(text: &str) -> Result<String> {
    let encoded =
        serde_json::to_vec(text).map_err(|_| ContractError("journal.memory_log_corrupt"))?;
    String::from_utf8(go_json_bytes(&encoded)?)
        .map_err(|_| ContractError("journal.memory_log_corrupt"))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEvent {
    pub sequence: i64,
    pub previous: String,
    pub kind: String,
    pub data: Box<serde_json::value::RawValue>,
    pub checksum: String,
}
fn event_bytes(event: &MemoryEvent, empty_checksum: bool) -> Result<Vec<u8>> {
    let data = String::from_utf8(go_json_bytes(event.data.get().as_bytes())?)
        .map_err(|_| ContractError("journal.memory_log_corrupt"))?;
    Ok(format!(
        "{{\"sequence\":{},\"previous\":{},\"kind\":{},\"data\":{},\"checksum\":{}}}",
        event.sequence,
        json_text(&event.previous)?,
        json_text(&event.kind)?,
        data,
        json_text(if empty_checksum { "" } else { &event.checksum })?
    )
    .into_bytes())
}
pub fn event_hash(event: &MemoryEvent) -> Result<String> {
    Ok(hash(&event_bytes(event, true)?))
}
#[derive(Debug)]
pub struct LogRead {
    /// Complete verified prefix. A nonempty prefix plus torn_tail still cannot
    /// authorize automatic replay or silent repair.
    pub events: Vec<MemoryEvent>,
    pub failure: Option<ContractError>,
}
pub fn read_log_bytes(bytes: &[u8]) -> LogRead {
    let mut out = LogRead {
        events: Vec::new(),
        failure: None,
    };
    let fail = |mut out: LogRead, code| {
        out.failure = Some(ContractError(code));
        out
    };
    if bytes.len() > MAX_LOG_BYTES {
        return fail(out, "journal.memory_log_limit");
    }
    let mut start = 0;
    while start < bytes.len() {
        let Some(relative) = bytes[start..].iter().position(|c| *c == b'\n') else {
            return fail(
                out,
                if bytes.len() - start > MAX_EVENT_BYTES {
                    "journal.memory_log_limit"
                } else {
                    "journal.memory_torn_tail"
                },
            );
        };
        let end = start + relative + 1;
        if end - start > MAX_EVENT_BYTES || out.events.len() >= MAX_EVENTS {
            return fail(out, "journal.memory_log_limit");
        }
        let event: MemoryEvent = match serde_json::from_slice(&bytes[start..end]) {
            Ok(e) => e,
            Err(_) => return fail(out, "journal.memory_log_corrupt"),
        };
        let previous = out.events.last().map_or("", |e| e.checksum.as_str());
        if event.sequence != out.events.len() as i64 + 1
            || event.previous != previous
            || event_hash(&event).as_ref() != Ok(&event.checksum)
        {
            return fail(out, "journal.memory_log_corrupt");
        }
        out.events.push(event);
        start = end;
    }
    out
}
/// Prepare bytes, not a durable append. The owning writer must validate the
/// original whole chain, size, lease and expected predecessor before writing.
pub fn prepare_event(previous: &[MemoryEvent], kind: &str, state: &State) -> Result<Vec<u8>> {
    if previous.len() >= MAX_EVENTS {
        return Err(ContractError("journal.memory_log_limit"));
    }
    let data = serde_json::value::to_raw_value(state)
        .map_err(|_| ContractError("journal.memory_log_corrupt"))?;
    let mut event = MemoryEvent {
        sequence: previous.len() as i64 + 1,
        previous: previous
            .last()
            .map_or(String::new(), |e| e.checksum.clone()),
        kind: kind.into(),
        data,
        checksum: String::new(),
    };
    event.checksum = event_hash(&event)?;
    let mut bytes = event_bytes(&event, false)?;
    if bytes.len() > MAX_EVENT_BYTES {
        return Err(ContractError("journal.memory_log_limit"));
    }
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
#[path = "journal_tests.rs"]
mod tests;
