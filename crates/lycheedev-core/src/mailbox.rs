//! Shared fixed-Go Mailbox v1 slot publication contract.
//!
//! Oracle: `3057a9666ab078270b63c7f06de748069b0d1f4b`, bridge/slot.go.
//! Slot DTOs and payload encoding for both Delivery and Live. Validation is
//! static admission only; native reads, routing and input authority stay in Live.
use serde::{Deserialize, Serialize};
use std::fmt::Write;

mod code_bytes;
pub use code_bytes::CodeBytes;

pub const SCHEMA: &str = "Lycheedev mailbox protocol v1";
pub const SLOT_COUNT: i32 = 500;
pub const MAX_EXACT_INTEGER: u64 = 9_007_199_254_740_991;
pub type Token = [u8; 16];
pub const ZERO_TOKEN: Token = [0; 16];

/// Stable public error code. Never includes addresses, payloads or tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractError(pub &'static str);
impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ContractError {}
pub type Result<T> = std::result::Result<T, ContractError>;

pub fn parse_token(text: &str) -> Result<Token> {
    if text.len() != 32
        || !text
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err(ContractError("live.channel_token_invalid"));
    }
    let mut token = ZERO_TOKEN;
    hex::decode_to_slice(text, &mut token)
        .map_err(|_| ContractError("live.channel_token_invalid"))?;
    Ok(token)
}

pub fn checksum(bytes: &[u8]) -> u32 {
    let (mut lo, mut hi) = (1u32, 0u32);
    for chunk in bytes.chunks(5552) {
        for &byte in chunk {
            lo += u32::from(byte);
            hi += lo;
        }
        lo %= 65521;
        hi %= 65521;
    }
    (hi << 16) | lo
}
fn zero_i32(n: &i32) -> bool {
    *n == 0
}
fn zero_u32(n: &u32) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SlotEnvelope {
    pub schema: String,
    pub index: i32,
    #[serde(skip_serializing_if = "zero_i32")]
    pub start_slot: i32,
    pub runtime: String,
    pub owner: String,
    pub fence: u64,
    pub nonce: String,
    pub ticket: String,
    pub action: String,
    pub guid: String,
    pub build: String,
    #[serde(skip_serializing_if = "CodeBytes::is_empty")]
    pub code: CodeBytes,
    #[serde(skip_serializing_if = "zero_i32")]
    pub code_bytes: i32,
    #[serde(skip_serializing_if = "zero_u32")]
    pub code_checksum: u32,
    #[serde(skip_serializing_if = "zero_i32")]
    pub budget: i32,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub challenge: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prepared_nonce: String,
    #[serde(skip_serializing_if = "zero_u32")]
    pub report_bytes: u32,
    #[serde(skip_serializing_if = "zero_u32")]
    pub report_checksum: u32,
}
impl SlotEnvelope {
    /// Go SlotPayload's static admission, not action or input authorization.
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            || !(1..=SLOT_COUNT).contains(&self.index)
            || self.start_slot < 0
            || self.start_slot > self.index
            || self.code.len() > 262144
            || self.fence == 0
            || self.fence > MAX_EXACT_INTEGER
        {
            return Err(ContractError("bridge.invalid_slot_envelope"));
        }
        for value in [&self.runtime, &self.owner, &self.nonce, &self.ticket] {
            parse_token(value).map_err(|_| ContractError("bridge.invalid_slot_token"))?;
        }
        Ok(())
    }
    pub fn expected(&self) -> ExpectedEnvelope {
        ExpectedEnvelope {
            schema: self.schema.clone(),
            index: self.index,
            start_slot: self.start_slot,
            runtime: self.runtime.clone(),
            owner: self.owner.clone(),
            fence: self.fence,
            nonce: self.nonce.clone(),
            ticket: self.ticket.clone(),
            action: self.action.clone(),
            guid: self.guid.clone(),
            build: self.build.clone(),
        }
    }
    pub fn slot_start(&self) -> i32 {
        if self.start_slot > 0 {
            self.start_slot
        } else {
            self.index
        }
    }
}

/// Exact envelope fields consumed by receipt and INPUT correlation.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct ExpectedEnvelope {
    pub schema: String,
    pub index: i32,
    pub start_slot: i32,
    pub runtime: String,
    pub owner: String,
    pub fence: u64,
    pub nonce: String,
    pub ticket: String,
    pub action: String,
    pub guid: String,
    pub build: String,
}
impl ExpectedEnvelope {
    pub fn slot_start(&self) -> i32 {
        if self.start_slot > 0 {
            self.start_slot
        } else {
            self.index
        }
    }
}
fn lua_string(out: &mut String, value: &[u8]) {
    out.push('"');
    for &byte in value {
        match byte {
            b'"' | b'\\' => {
                out.push('\\');
                out.push(byte as char);
            }
            32..=126 => out.push(byte as char),
            _ => {
                // Writing to a String cannot fail; fixed-width decimal Lua 5.1
                // byte escapes avoid both Unicode loss and delimiter injection.
                let _ = write!(out, "\\{byte:03}");
            }
        }
    }
    out.push('"');
}

/// Exact bridge.SlotPayload bytes. This emits data, never top-level user code.
/// Static payload validation does not prove action/readiness/input authority.
pub fn slot_payload(envelope: &SlotEnvelope) -> Result<Vec<u8>> {
    envelope.validate()?;
    let mut out = String::with_capacity(envelope.code.len() + 512);
    out.push_str("LycheeDevSlotEnvelope = {\n");
    if envelope.start_slot != 0 {
        let _ = writeln!(out, "  startSlot = {},", envelope.start_slot);
    }
    for (name, value) in [
        ("schema", envelope.schema.as_bytes()),
        ("runtime", envelope.runtime.as_bytes()),
        ("owner", envelope.owner.as_bytes()),
        ("nonce", envelope.nonce.as_bytes()),
        ("ticket", envelope.ticket.as_bytes()),
        ("action", envelope.action.as_bytes()),
        ("guid", envelope.guid.as_bytes()),
        ("build", envelope.build.as_bytes()),
        ("code", envelope.code.as_bytes()),
        ("challenge", envelope.challenge.as_bytes()),
        ("preparedNonce", envelope.prepared_nonce.as_bytes()),
    ] {
        let _ = write!(out, "  {name} = ");
        lua_string(&mut out, value);
        out.push_str(",\n");
    }
    let (code_bytes, code_checksum) = if envelope.code.is_empty() {
        (envelope.code_bytes, envelope.code_checksum)
    } else {
        (
            envelope.code.len() as i32,
            checksum(envelope.code.as_bytes()),
        )
    };
    let _ = writeln!(
        out,
        "  index = {}, fence = {}, codeBytes = {}, codeChecksum = {}, budget = {}, reportBytes = {}, reportChecksum = {},\n}}",
        envelope.index,
        envelope.fence,
        code_bytes,
        code_checksum,
        envelope.budget,
        envelope.report_bytes,
        envelope.report_checksum
    );
    Ok(out.into_bytes())
}

#[cfg(test)]
mod code_bytes_tests;
