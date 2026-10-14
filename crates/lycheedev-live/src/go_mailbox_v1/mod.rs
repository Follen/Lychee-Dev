//! Fixed Go 3.2.0 Mailbox v1 data contract, not an input driver.
//!
//! Oracle: `3057a9666ab078270b63c7f06de748069b0d1f4b`.
//! These decoders do not establish publication stability, process identity,
//! readiness or permission to send input. Native integration remains separate.
pub mod driver;
pub mod durable_journal;
pub mod slot_allocation;
pub mod transactions;
pub mod recovery;
pub mod reload;
pub mod runtime_retirement;
pub mod journal;
pub mod observations;
pub mod state;

mod input;
mod protocol;
mod wire;

pub use input::*;
pub use lycheedev_core::mailbox::{ContractError, MAX_EXACT_INTEGER, Result, SCHEMA, SLOT_COUNT};
pub use protocol::*;
pub use wire::*;

pub const PREVIOUS_SCHEMA: &str = "lycheedev.mailbox.v1";
pub const LEGACY_IDENTITY_SCHEMA: &str = "lycheedev.slot.identity.v2";
pub const LEGACY_INPUT_SCHEMA: &str = "lycheedev.input.v3";
pub const LEGACY_HYBRID_SCHEMA: &str = "lycheedev.input.hybrid.v2";

#[cfg(test)]
mod tests;

#[cfg(test)]
mod shared_contract_identity {
    use super::*;
    use lycheedev_core::mailbox as shared;
    use std::any::TypeId;

    #[test]
    fn go_mailbox_v1_live_paths_reexport_the_shared_types_and_payload() {
        assert_eq!(
            TypeId::of::<state::SlotEnvelope>(),
            TypeId::of::<shared::SlotEnvelope>()
        );
        assert_eq!(
            TypeId::of::<ExpectedEnvelope>(),
            TypeId::of::<shared::ExpectedEnvelope>()
        );
        assert_eq!(
            TypeId::of::<ContractError>(),
            TypeId::of::<shared::ContractError>()
        );
        // The signatures must agree without conversions or adapter wrappers.
        let _: fn(&shared::SlotEnvelope) -> shared::Result<Vec<u8>> = slot_allocation::slot_payload;
        let _: fn(&str) -> shared::Result<shared::Token> = parse_token;
        let value = shared::SlotEnvelope::default();
        let live: state::SlotEnvelope = value;
        let _: shared::ExpectedEnvelope = live.expected();
        assert_eq!(
            slot_allocation::slot_payload(&live),
            shared::slot_payload(&live)
        );
    }
}
