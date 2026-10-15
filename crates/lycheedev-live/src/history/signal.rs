//! Read-only historical Go 3.2.0 signal and persisted-report verification.
//!
//! Oracle: 3057a9666ab078270b63c7f06de748069b0d1f4b,
//! internal/bridge/{signal,checkpoint,json,report,history_labels}.go and
//! internal/desktop/receiver_bindings.go. No observation, input, ACK or replay.
//! This module must remain outside production until separately integrated and
//! verified. Tests for this staged implementation are NOT_RUN.
#![forbid(unsafe_code)]

pub use super::bootstrap::Signal;
use crate::go_mailbox_v1::admission::{WireRecord, ignored, scalar};
use lycheedev_core::{Result, ToolkitError, control::OperationControl};
use serde::{
    Serialize,
    de::{DeserializeSeed, MapAccess},
};
use sha2::{Digest, Sha256};
use std::borrow::Cow;

mod json;
#[cfg(test)]
#[path = "signal_tests.rs"]
mod tests;
mod validation;

const LUA_INTEGER_MAX: u64 = 9_007_199_254_740_991;
const SIGNAL_LIMIT: usize = 4096;
const CODE_LIMIT: usize = 256 << 10;
const REPORT_LIMIT: usize = 512 << 10;
const MAX_RECEIVER_WIRE_BYTES: u32 = 256;

/// Deliberately has no GUID field: fixed Go Match does not compare GUID.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SignalExpectation {
    #[serde(rename = "Release")]
    pub release: String,
    #[serde(rename = "Kind")]
    pub kind: String,
    #[serde(rename = "SessionNonce")]
    pub session_nonce: String,
    #[serde(rename = "RequestID")]
    pub request_id: String,
    #[serde(rename = "ReloadNonce")]
    pub reload_nonce: String,
    #[serde(rename = "CleanupNonce")]
    pub cleanup_nonce: String,
    #[serde(rename = "ProbeNonce")]
    pub probe_nonce: String,
    #[serde(rename = "ActorState")]
    pub actor_state: String,
    #[serde(rename = "Character")]
    pub character: String,
    #[serde(rename = "Realm")]
    pub realm: String,
    #[serde(rename = "Product")]
    pub product: String,
    #[serde(rename = "Build")]
    pub build: String,
    #[serde(rename = "AfterSequence")]
    pub after_sequence: u64,
    #[serde(rename = "RuntimeEpoch")]
    pub runtime_epoch: u64,
    #[serde(rename = "RequireInputReady")]
    pub require_input_ready: bool,
    #[serde(rename = "ReceiverNonce")]
    pub receiver_nonce: String,
    #[serde(rename = "AttemptID")]
    pub attempt_id: String,
    #[serde(rename = "BodyAdler32")]
    pub body_adler32: String,
    #[serde(rename = "CommitNonce")]
    pub commit_nonce: String,
    #[serde(rename = "BodyBytes")]
    pub body_bytes: u32,
    #[serde(rename = "RequireAccepted")]
    pub require_accepted: bool,
    #[serde(rename = "AllowReportError")]
    pub allow_report_error: bool,
}

impl WireRecord for SignalExpectation {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        match key {
            "release" => scalar(&mut self.release, map),
            "kind" => scalar(&mut self.kind, map),
            "sessionnonce" => scalar(&mut self.session_nonce, map),
            "requestid" => scalar(&mut self.request_id, map),
            "reloadnonce" => scalar(&mut self.reload_nonce, map),
            "cleanupnonce" => scalar(&mut self.cleanup_nonce, map),
            "probenonce" => scalar(&mut self.probe_nonce, map),
            "actorstate" => scalar(&mut self.actor_state, map),
            "character" => scalar(&mut self.character, map),
            "realm" => scalar(&mut self.realm, map),
            "product" => scalar(&mut self.product, map),
            "build" => scalar(&mut self.build, map),
            "aftersequence" => scalar(&mut self.after_sequence, map),
            "runtimeepoch" => scalar(&mut self.runtime_epoch, map),
            "requireinputready" => scalar(&mut self.require_input_ready, map),
            "receivernonce" => scalar(&mut self.receiver_nonce, map),
            "attemptid" => scalar(&mut self.attempt_id, map),
            "bodyadler32" => scalar(&mut self.body_adler32, map),
            "commitnonce" => scalar(&mut self.commit_nonce, map),
            "bodybytes" => scalar(&mut self.body_bytes, map),
            "requireaccepted" => scalar(&mut self.require_accepted, map),
            "allowreporterror" => scalar(&mut self.allow_report_error, map),
            _ => ignored(map),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SignalIdentity {
    pub release: String,
    pub character: String,
    pub realm: String,
    pub guid: String,
    pub product: String,
    pub build: String,
    pub session_nonce: String,
    /// Retained for parity with Go's DTO; FillSignalIdentity never copies it.
    pub runtime_epoch: u64,
}

/// Integrity of archived transport bytes, never authentication or an ACK.
#[derive(Debug, Clone)]
pub struct VerifiedReport {
    pub receipt: Signal,
    pub receipt_bytes: Vec<u8>,
    pub body: Vec<u8>,
    pub sha256: String,
}

fn error(code: &'static str) -> ToolkitError {
    ToolkitError::new(code, code, 4)
}
fn syntax_error(detail: impl std::fmt::Display) -> ToolkitError {
    ToolkitError::new("bridge.invalid_json", detail.to_string(), 4)
}

/// Fill only omitted strings; contradictions, runtime epoch and other fields
/// remain untouched. Call matches() separately against independent identity.
pub fn fill_signal_identity(mut signal: Signal, baseline: &SignalIdentity) -> Signal {
    for (field, value) in [
        (&mut signal.release, &baseline.release),
        (&mut signal.character, &baseline.character),
        (&mut signal.realm, &baseline.realm),
        (&mut signal.guid, &baseline.guid),
        (&mut signal.product, &baseline.product),
        (&mut signal.build, &baseline.build),
        (&mut signal.session_nonce, &baseline.session_nonce),
    ] {
        if field.is_empty() {
            field.clone_from(value);
        }
    }
    signal
}

/// Accept plain UTF-8 JSON plus both archived raw-DEFLATE markers. The same
/// original control is checked through inflation and nested checkpoint parsing.
pub fn parse_signal(data: &[u8], control: &OperationControl) -> Result<Signal> {
    control.check()?;
    if data.is_empty() || data.len() > SIGNAL_LIMIT {
        return Err(error("bridge.signal_budget_or_encoding"));
    }
    let plain = match data[0] {
        0x1f => {
            if data.len() < 2 {
                return Err(error("bridge.signal_budget_or_encoding"));
            }
            Cow::Owned(inflate_signal(&data[1..], control)?)
        }
        0x1e => {
            if data.len() < 2 {
                return Err(error("bridge.signal_budget_or_encoding"));
            }
            let compressed = decode_base64(&data[1..]).map_err(|detail| {
                ToolkitError::new(
                    "bridge.signal_budget_or_encoding",
                    format!("bridge.signal_budget_or_encoding: {detail}"),
                    4,
                )
            })?;
            Cow::Owned(inflate_signal(&compressed, control)?)
        }
        _ => Cow::Borrowed(data),
    };
    if plain.is_empty() || plain.len() > SIGNAL_LIMIT || std::str::from_utf8(&plain).is_err() {
        return Err(error("bridge.signal_budget_or_encoding"));
    }
    json::validate(&plain, 2, 32, control)?;
    // Reuse the existing Go-compatible decoder (case-folding and scalar null
    // semantics), adding strict field admission only at this wire boundary.
    let normalized = lycheedev_core::evidence::normalize_go_json_strings(&plain, control)?;
    let mut signal = Signal::default();
    let mut decoder = serde_json::Deserializer::from_slice(&normalized);
    let mut unknown = None;
    StrictSignalSeed {
        signal: &mut signal,
        unknown: &mut unknown,
    }
    .deserialize(&mut decoder)
    .map_err(|detail| match unknown {
        Some(key) => syntax_error(format!("json: unknown field {key:?}")),
        None => syntax_error(detail),
    })?;
    decoder
        .end()
        .map_err(|_| error("bridge.signal_trailing_data"))?;
    control.check()?;
    validation::validate(&signal, control)?;
    Ok(signal)
}

// Preserve original key spelling for DisallowUnknownFields diagnostics while
// retaining the same Go case-fold/null/type semantics for known DTO fields.
struct StrictSignalSeed<'a> {
    signal: &'a mut Signal,
    unknown: &'a mut Option<String>,
}
impl<'de> DeserializeSeed<'de> for StrictSignalSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        decoder: D,
    ) -> std::result::Result<(), D::Error> {
        impl<'de> serde::de::Visitor<'de> for StrictSignalSeed<'_> {
            type Value = ();
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
                Ok(())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<(), A::Error> {
                while let Some(key) = map.next_key::<String>()? {
                    let folded: String = key
                        .chars()
                        .map(|c| match c {
                            'ſ' => 's',
                            'K' => 'k',
                            _ => c.to_ascii_lowercase(),
                        })
                        .collect();
                    if let Err(error) = StrictSignal(self.signal).field(&folded, &mut map) {
                        if error.to_string().starts_with("json: unknown field ") {
                            *self.unknown = Some(key);
                        }
                        return Err(error);
                    }
                }
                Ok(())
            }
        }
        decoder.deserialize_any(self)
    }
}

struct StrictSignal<'a>(&'a mut Signal);
impl WireRecord for StrictSignal<'_> {
    fn field<'de, A: MapAccess<'de>>(
        &mut self,
        key: &str,
        map: &mut A,
    ) -> std::result::Result<(), A::Error> {
        // Keep the DTO and decoder in bootstrap.rs as the single source of
        // field types. This allow-list only mirrors DisallowUnknownFields.
        match key {
            "schema"
            | "release"
            | "kind"
            | "sessionnonce"
            | "requestid"
            | "reloadnonce"
            | "cleanupnonce"
            | "probenonce"
            | "actorstate"
            | "inputreason"
            | "character"
            | "realm"
            | "guid"
            | "product"
            | "build"
            | "sequence"
            | "runtimeepoch"
            | "inputready"
            | "codebytes"
            | "codeadler32"
            | "reportbytes"
            | "reportadler32"
            | "receiverprotocol"
            | "receivernonce"
            | "attemptid"
            | "bodybytes"
            | "bodyadler32"
            | "commitnonce"
            | "accepted"
            | "errorcode"
            | "wakebinding"
            | "submitbinding"
            | "closebinding"
            | "priorsessionsequence"
            | "priorsessionepoch"
            | "reportscope"
            | "workstate"
            | "codesha256"
            | "receipt"
            | "resourcesreleased" => self.0.field(key, map),
            _ => Err(serde::de::Error::custom(format!(
                "json: unknown field {key:?}"
            ))),
        }
    }
}

/// Go StdEncoding: ignore CR/LF, require padding, permit nonzero pad bits.
/// Callers bound their own input before requesting this allocation.
#[derive(Debug, Clone, Copy)]
pub(super) struct Base64Error(usize);
impl std::fmt::Display for Base64Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal base64 data at input byte {}", self.0)
    }
}
impl std::error::Error for Base64Error {}
// Go StdEncoding.decodeQuantum: keep indices in the original input (including
// CR/LF); the crate decoder reports indices after whitespace removal instead.
pub(super) fn decode_base64(data: &[u8]) -> std::result::Result<Vec<u8>, Base64Error> {
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut at = 0;
    loop {
        let mut quantum = [0u32; 4];
        let mut j = 0;
        let mut length = 4;
        while j < 4 {
            if at == data.len() {
                if j == 0 {
                    return Ok(out);
                }
                return Err(Base64Error(at - j));
            }
            let byte = data[at];
            at += 1;
            let value = match byte {
                b'A'..=b'Z' => Some(byte - b'A'),
                b'a'..=b'z' => Some(byte - b'a' + 26),
                b'0'..=b'9' => Some(byte - b'0' + 52),
                b'+' => Some(62),
                b'/' => Some(63),
                _ => None,
            };
            if let Some(value) = value {
                quantum[j] = u32::from(value);
                j += 1;
                continue;
            }
            if matches!(byte, b'\r' | b'\n') {
                continue;
            }
            if byte != b'=' || j < 2 {
                return Err(Base64Error(at - 1));
            }
            if j == 2 {
                while at < data.len() && matches!(data[at], b'\r' | b'\n') {
                    at += 1;
                }
                if at == data.len() {
                    return Err(Base64Error(at));
                }
                if data[at] != b'=' {
                    return Err(Base64Error(at - 1));
                }
                at += 1;
            }
            while at < data.len() && matches!(data[at], b'\r' | b'\n') {
                at += 1;
            }
            if at < data.len() {
                return Err(Base64Error(at));
            }
            length = j;
            break;
        }
        let value = (quantum[0] << 18) | (quantum[1] << 12) | (quantum[2] << 6) | quantum[3];
        out.push((value >> 16) as u8);
        if length >= 3 {
            out.push((value >> 8) as u8);
        }
        if length == 4 {
            out.push(value as u8);
        }
        if length < 4 {
            return Ok(out);
        }
    }
}

pub(super) fn validate_receiver_bindings(
    bindings: &super::bootstrap::ReceiverBindings,
) -> Result<()> {
    validation::receiver_bindings(
        &bindings.wake_binding,
        &bindings.submit_binding,
        &bindings.close_binding,
    )
}

fn inflate_signal(compressed: &[u8], control: &OperationControl) -> Result<Vec<u8>> {
    use flate2::{Decompress, FlushDecompress, Status};
    let unexpected_eof = || {
        ToolkitError::new(
            "bridge.signal_budget_or_encoding",
            "bridge.signal_budget_or_encoding: unexpected EOF",
            4,
        )
    };
    if compressed.is_empty() {
        return Err(unexpected_eof());
    }
    let mut decoder = Decompress::new(false); // raw DEFLATE, not zlib/gzip
    let mut out = Vec::with_capacity(SIGNAL_LIMIT + 1);
    let mut consumed = 0usize;
    loop {
        control.check()?;
        let end = compressed.len().min(consumed.saturating_add(128));
        let mut buffer = [0u8; 512];
        let remaining = (SIGNAL_LIMIT + 1 - out.len()).min(buffer.len());
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &compressed[consumed..end],
                &mut buffer[..remaining],
                if end == compressed.len() {
                    FlushDecompress::Finish
                } else {
                    FlushDecompress::None
                },
            )
            .map_err(|_| error("bridge.signal_budget_or_encoding"))?;
        let read = (decoder.total_in() - before_in) as usize;
        let written = (decoder.total_out() - before_out) as usize;
        consumed += read;
        out.extend_from_slice(&buffer[..written]);
        if out.len() > SIGNAL_LIMIT {
            return Err(error("bridge.signal_budget_or_encoding"));
        }
        if status == Status::StreamEnd {
            // Go flate stops at the first stream and ignores trailing bytes.
            control.check()?;
            return Ok(out);
        }
        if read == 0 && written == 0 {
            return Err(if consumed == compressed.len() {
                unexpected_eof()
            } else {
                error("bridge.signal_budget_or_encoding")
            });
        }
    }
}

impl Signal {
    pub fn matches(&self, expected: &SignalExpectation) -> Result<()> {
        let mismatch = || error("bridge.signal_identity_mismatch");
        if self.sequence > LUA_INTEGER_MAX
            || (self.kind == "identity") != (expected.kind == "identity")
            || self.kind.starts_with("receiver_") != expected.kind.starts_with("receiver_")
            || (expected.kind == "identity" && !validation::hex(&expected.probe_nonce, 32))
            || (expected.runtime_epoch != 0 && self.runtime_epoch != expected.runtime_epoch)
        {
            return Err(mismatch());
        }
        let kind = if expected.allow_report_error
            && expected.kind == "reported"
            && self.kind == "report_error"
        {
            "reported"
        } else {
            &self.kind
        };
        for (actual, wanted) in [
            (self.release.as_str(), expected.release.as_str()),
            (kind, &expected.kind),
            (&self.session_nonce, &expected.session_nonce),
            (&self.request_id, &expected.request_id),
            (&self.reload_nonce, &expected.reload_nonce),
            (&self.cleanup_nonce, &expected.cleanup_nonce),
            (&self.probe_nonce, &expected.probe_nonce),
            (&self.actor_state, &expected.actor_state),
            (&self.character, &expected.character),
            (&self.realm, &expected.realm),
            (&self.product, &expected.product),
            (&self.build, &expected.build),
            (&self.receiver_nonce, &expected.receiver_nonce),
            (&self.attempt_id, &expected.attempt_id),
            (&self.body_adler32, &expected.body_adler32),
            (&self.commit_nonce, &expected.commit_nonce),
        ] {
            if !wanted.is_empty() && actual != wanted {
                return Err(mismatch());
            }
        }
        if (expected.body_bytes != 0 && self.body_bytes != expected.body_bytes)
            || (expected.require_accepted && !self.accepted)
        {
            return Err(mismatch());
        }
        if if matches!(self.kind.as_str(), "identity" | "reset") {
            expected.after_sequence != 0
        } else {
            self.sequence <= expected.after_sequence
        } {
            return Err(mismatch());
        }
        if expected.require_input_ready && !self.input_ready {
            return Err(error("bridge.input_not_ready"));
        }
        Ok(())
    }
}

/// Verify the exact persisted bytes against independent expectations and the
/// original immutable submitted code. No filesystem or caller-owned mutation.
pub fn verify_report(
    receipt_bytes: &[u8],
    body: &[u8],
    code: &[u8],
    expected: &SignalExpectation,
    control: &OperationControl,
) -> Result<VerifiedReport> {
    control.check()?;
    if expected.kind != "reported"
        || [
            &expected.release,
            &expected.session_nonce,
            &expected.request_id,
            &expected.character,
            &expected.realm,
            &expected.product,
            &expected.build,
        ]
        .iter()
        .any(|value| value.is_empty())
    {
        return Err(error("bridge.incomplete_report_expectation"));
    }
    let receipt = fill_signal_identity(
        parse_signal(receipt_bytes, control)?,
        &SignalIdentity {
            release: expected.release.clone(),
            character: expected.character.clone(),
            realm: expected.realm.clone(),
            product: expected.product.clone(),
            build: expected.build.clone(),
            session_nonce: expected.session_nonce.clone(),
            ..SignalIdentity::default()
        },
    );
    receipt.matches(expected)?;
    if code.len() > CODE_LIMIT || body.is_empty() || body.len() > REPORT_LIMIT {
        return Err(error("bridge.report_payload_limit"));
    }
    if code.len() != receipt.code_bytes as usize
        || (!code.is_empty() && byte_checksum(code, control)? != receipt.code_adler32)
        || (code.is_empty() && !receipt.code_adler32.is_empty())
    {
        return Err(error("bridge.report_code_mismatch"));
    }
    if body.len() != receipt.report_bytes as usize
        || byte_checksum(body, control)? != receipt.report_adler32
    {
        return Err(error("bridge.report_body_mismatch"));
    }
    control.check()?;
    if std::str::from_utf8(body).is_err() || !lycheedev_core::raw_json::is_valid(body) {
        return Err(error("bridge.report_invalid_json"));
    }
    control.check()?;
    json::validate(body, 32, 32768, control)?;
    let mut hasher = Sha256::new();
    for chunk in body.chunks(65536) {
        control.check()?;
        hasher.update(chunk);
    }
    let sha256 = hex::encode(hasher.finalize());
    let result = VerifiedReport {
        receipt,
        receipt_bytes: receipt_bytes.to_vec(),
        body: body.to_vec(),
        sha256,
    };
    control.check()?;
    Ok(result)
}

fn byte_checksum(bytes: &[u8], control: &OperationControl) -> Result<String> {
    // Adler-32 is small enough to implement without another dependency. 5552
    // is the zlib/Go safe block length before reducing 32-bit accumulators.
    let (mut a, mut b) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        control.check()?;
        for byte in chunk {
            a += u32::from(*byte);
            b += a;
        }
        a %= 65521;
        b %= 65521;
    }
    Ok(format!("{:08x}", b << 16 | a))
}
