use super::*;
pub use lycheedev_core::mailbox::ExpectedEnvelope;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Identity {
    pub schema: String,
    pub runtime: String,
    pub owner: String,
    pub fence: u64,
    pub next_slot: i32,
    pub slots: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inventory: Option<i32>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub input_state: String,
    pub character: String,
    pub realm: String,
    pub guid: String,
    pub build: String,
    pub product: String,
    pub release: String,
}

pub fn upgrade_input_capability(capability: &str) -> bool {
    matches!(
        capability,
        LEGACY_INPUT_SCHEMA | LEGACY_HYBRID_SCHEMA | PREVIOUS_SCHEMA
    )
}
impl Identity {
    pub fn validate(&self) -> Result<()> {
        if self.schema != SCHEMA
            && !(self.schema == LEGACY_IDENTITY_SCHEMA
                && upgrade_input_capability(&self.input_state))
            && !(self.schema == PREVIOUS_SCHEMA && self.input_state == PREVIOUS_SCHEMA)
        {
            return Err(ContractError("live.channel_identity_schema_invalid"));
        }
        self.validate_fields(SLOT_COUNT)
    }
    fn validate_fields(&self, slots: i32) -> Result<()> {
        if self.inventory.is_some_and(|n| n < 0 || n > self.slots) {
            return Err(ContractError("live.channel_inventory_invalid"));
        }
        parse_token(&self.runtime)?;
        if self.slots != slots
            || self.next_slot < 1
            || self.next_slot > self.slots + 1
            || [
                &self.guid,
                &self.character,
                &self.realm,
                &self.build,
                &self.product,
                &self.release,
            ]
            .iter()
            .any(|s| s.is_empty())
        {
            return Err(ContractError("live.channel_identity_invalid"));
        }
        Ok(())
    }
}

/// Legacy descriptors are returned separately so no caller can silently promote
/// a known 200-slot installation to a usable current connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityPublication {
    Current(Identity),
    /// Structurally valid 500-slot old capability; activation only, not business.
    UpgradeCapability(Identity),
    /// Go returns mailbox_incompatible + legacy_identity_requires_activation.
    ManualActivationRequired(Identity),
}
impl IdentityPublication {
    pub fn identity(&self) -> &Identity {
        match self {
            Self::Current(i) | Self::UpgradeCapability(i) | Self::ManualActivationRequired(i) => i,
        }
    }
    pub fn incompatibility(&self) -> Option<ContractError> {
        matches!(self, Self::ManualActivationRequired(_)).then_some(ContractError(
            "live.channel_legacy_identity_requires_activation",
        ))
    }
}
pub fn decode_identity(record: &Record<'_>, release: &str) -> Result<IdentityPublication> {
    let invalid = ContractError("live.channel_identity_record_invalid");
    let h = &record.header;
    if h.kind != Kind::Identity
        || h.state != 1
        || h.nonce != ZERO_TOKEN
        || h.ticket != ZERO_TOKEN
        || record.payload.len() > 16384
    {
        return Err(invalid);
    }
    let i: Identity = serde_json::from_slice(record.payload).map_err(|_| invalid)?;
    let runtime = parse_token(&i.runtime).map_err(|_| invalid)?;
    if i.release != release || runtime == ZERO_TOKEN || runtime != h.runtime {
        return Err(invalid);
    }
    if i.schema == LEGACY_IDENTITY_SCHEMA
        && matches!(
            i.input_state.as_str(),
            LEGACY_INPUT_SCHEMA | LEGACY_HYBRID_SCHEMA
        )
        && i.validate_fields(200).is_ok()
    {
        return Ok(IdentityPublication::ManualActivationRequired(i));
    }
    i.validate().map_err(|_| invalid)?;
    if i.schema != SCHEMA || upgrade_input_capability(&i.input_state) {
        Ok(IdentityPublication::UpgradeCapability(i))
    } else {
        Ok(IdentityPublication::Current(i))
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct Receipt {
    #[serde(flatten)]
    pub identity: Identity,
    pub nonce: String,
    pub ticket: String,
    pub action: String,
    pub state: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub reason: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub prepared_nonce: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub challenge: String,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub report_bytes: u32,
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub report_checksum: u32,
}
fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}
impl Receipt {
    pub fn validate(&self) -> Result<()> {
        if self.identity.schema != SCHEMA {
            return Err(ContractError("live.channel_receipt_schema_invalid"));
        }
        self.identity.validate_fields(SLOT_COUNT)
    }
}

pub fn decode_publication_receipt(record: &Record<'_>) -> Result<Receipt> {
    let invalid = ContractError("live.channel_receipt_record_invalid");
    let h = &record.header;
    if !matches!(h.kind, Kind::Receipt | Kind::Confirmation)
        || h.state != 1
        || h.sequence == 0
        || record.payload.len() > 16384
    {
        return Err(invalid);
    }
    let r: Receipt = serde_json::from_slice(record.payload).map_err(|_| invalid)?;
    r.validate()?;
    let nonce = parse_token(&r.nonce).map_err(|_| invalid)?;
    let runtime = parse_token(&r.identity.runtime).map_err(|_| invalid)?;
    let ticket = parse_token(&r.ticket).map_err(|_| invalid)?;
    if nonce == ZERO_TOKEN
        || runtime == ZERO_TOKEN
        || h.nonce != nonce
        || h.runtime != runtime
        || h.ticket != ticket
        || (h.kind == Kind::Confirmation && r.action != "confirm")
        || (h.kind == Kind::Receipt && r.action == "confirm" && r.state != "rejected")
    {
        return Err(invalid);
    }
    Ok(r)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptMatch {
    Accepted(Receipt),
    /// A checked receipt can report rejection. Do not equate it with execution.
    Rejected(Receipt),
}
pub fn decode_receipt(record: &Record<'_>, e: &ExpectedEnvelope) -> Result<ReceiptMatch> {
    let r = decode_publication_receipt(record)?;
    let i = &r.identity;
    if i.schema != e.schema
        || r.nonce != e.nonce
        || r.ticket != e.ticket
        || r.action != e.action
        || i.runtime != e.runtime
        || i.guid != e.guid
        || i.build != e.build
        || i.next_slot <= e.index
    {
        return Err(ContractError("live.channel_receipt_mismatch"));
    }
    if i.owner != e.owner || i.fence != e.fence {
        return Err(ContractError("live.channel_owner_mismatch"));
    }
    if r.state == "rejected" {
        Ok(ReceiptMatch::Rejected(r))
    } else {
        Ok(ReceiptMatch::Accepted(r))
    }
}

/// Logical public-map view for bounded named lookups. `schema` is metadata,
/// never a receipt/body key. This does not implement Lua hash-table traversal.
/// The native adapter must validate its Lua path and guard it before use.
#[derive(Debug, Clone)]
pub struct PublicMap<'a> {
    pub schema: &'a str,
    pub entries: BTreeMap<String, &'a [u8]>,
}
impl<'a> PublicMap<'a> {
    pub fn lookup(&self, key: &str) -> Result<Option<&'a [u8]>> {
        parse_token(key)?;
        // The Go reader selects the exact token, not schema or any neighboring
        // entry. Map metadata itself is not an additional authorization gate.
        Ok(self.entries.get(key).copied())
    }
}
