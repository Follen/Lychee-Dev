//! Fixed Go 3.2.0 Mailbox v1 data contract, not an input driver.
//!
//! Oracle: `3057a9666ab078270b63c7f06de748069b0d1f4b`.
//! These decoders do not establish publication stability, process identity,
//! readiness or permission to send input. Native integration remains separate.
pub mod journal;
pub mod observations;
pub mod state;

mod input;
mod protocol;
mod wire;

pub use input::*;
pub use protocol::*;
pub use wire::*;

pub const SCHEMA: &str = "Lycheedev mailbox protocol v1";
pub const PREVIOUS_SCHEMA: &str = "lycheedev.mailbox.v1";
pub const SLOT_COUNT: i32 = 500;
pub const LEGACY_IDENTITY_SCHEMA: &str = "lycheedev.slot.identity.v2";
pub const LEGACY_INPUT_SCHEMA: &str = "lycheedev.input.v3";
pub const LEGACY_HYBRID_SCHEMA: &str = "lycheedev.input.hybrid.v2";
pub const MAX_EXACT_INTEGER: u64 = 9_007_199_254_740_991;

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

#[cfg(test)]
mod tests;
