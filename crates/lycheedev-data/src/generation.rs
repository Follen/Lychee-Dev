//! Demand-driven typed table generations and their authenticated read owners.
mod application;
mod consumer;
pub(crate) mod engine_adapter;
mod identity;
mod import;
mod reading;
pub(crate) mod retirement;
pub(crate) use retirement::prune;

pub(crate) use application::dispatch;
pub(crate) use consumer::{Generation, Reuse, open_existing};
pub(crate) use identity::{StaticIdentity, StaticSource};
pub(crate) use import::{ImportLimits, TableImporter};
pub(crate) use import::{ImportedBatchAllocation, ImportedSchema};
pub(crate) use reading::ipc::OwnedBatch as GenerationBatch;

#[cfg(test)]
mod tests;

