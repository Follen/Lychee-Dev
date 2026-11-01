#![forbid(unsafe_code)]
//! Author publishing contracts. Remote author records are separate from local
//! managed Delivery installations and game-process Live operations.
pub mod blackbox;
pub mod contract;
pub mod curseforge;
pub mod dd;
mod desktop_runtime;
mod input;
mod local_backup;
pub mod modus;
mod mutation;
pub mod newbee;
pub mod newbee_official;
mod newbee_session;
mod output;
pub mod provider;
#[cfg(test)]
mod test_support;
mod transport;
mod zip_contract;
