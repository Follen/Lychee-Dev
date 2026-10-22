//! Current Mailbox protocol, durable connection lifecycle and native backend.
//! The named-field Lua read path is [`lookup`]; public Mailbox supplies
//! observations, never authority.
pub mod admission;
pub mod connection;
pub mod driver;
pub mod durable_journal;
pub mod journal;
pub mod lookup;
pub mod native_backend;
pub mod native_reader;
pub mod observations;
pub mod recovery;
pub mod reload;
pub mod runtime_retirement;
pub mod slot_allocation;
pub mod state;
pub mod transactions;

mod input;
mod protocol;
mod wire;

pub use input::*;
pub use lycheedev_core::mailbox::{ContractError, MAX_EXACT_INTEGER, Result, SCHEMA, SLOT_COUNT};
pub use protocol::*;
pub use wire::*;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod shared_contract_identity {
    use super::*;
    use lycheedev_core::mailbox as shared;
    use std::any::TypeId;

    #[test]
    fn mailbox_live_paths_reexport_the_shared_types_and_payload() {
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

pub mod activation;

pub mod doctor;

#[cfg(test)]
mod slot_reload_tests;
