use super::*;
use serde::Deserialize;
use std::fmt;
pub(super) fn error(message: impl fmt::Display) -> ToolkitError {
    ToolkitError::new("vault.cache_protection", format!("vault.cache_protection: {message}"), 4)
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Owner { kind: String, owner: String, retained_at: String }
impl Owner {
    pub(super) fn validate(&self) -> Result<()> {
        if !["pinned", "evidence", "operation"].contains(&self.kind.as_str()) { return Err(error(format!("unknown kind {:?}", self.kind))); }
        if self.owner.is_empty() || self.owner.len() > 512 || storage::parse_timestamp(&self.retained_at).is_none() { return Err(error("invalid owner")); }
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Retain { pub sha256: String, pub owners: Vec<Owner> }
