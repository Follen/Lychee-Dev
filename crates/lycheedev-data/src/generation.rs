//! Demand-driven typed table generations. Existing DB2/JSONL consumers keep
//! their public execution and error boundaries until explicitly migrated.
mod application;
mod consumer;
mod identity;
mod import;
mod reading;

pub(crate) use application::dispatch;
pub(crate) use consumer::{Generation, Reuse, open_existing};
pub(crate) use identity::{StaticIdentity, StaticSource};
pub(crate) use import::{ImportLimits, TableImporter};
#[cfg(feature = "datafusion-qualification")]
pub(crate) use import::{ImportedBatch, ImportedBatchAllocation, ImportedSchema};
pub(crate) use reading::ipc::OwnedBatch as GenerationBatch;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use consumer::observations;
