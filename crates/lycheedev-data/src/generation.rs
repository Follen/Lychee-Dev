//! Demand-driven typed table generations. Existing DB2/JSONL consumers keep
//! their public execution and error boundaries until explicitly migrated.
mod application;
mod identity;
mod import;
mod reading;

pub(crate) use application::dispatch;
pub(crate) use identity::StaticIdentity;
pub(crate) use import::{ImportLimits, TableImporter};
#[cfg(feature = "datafusion-qualification")]
pub(crate) use import::{ImportedBatch, ImportedBatchAllocation, ImportedSchema};

#[cfg(test)]
mod tests;
