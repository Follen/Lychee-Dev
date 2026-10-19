//! Demand-driven typed table generations. Existing DB2/JSONL consumers keep
//! their public execution and error boundaries until explicitly migrated.
mod application;
mod identity;
mod import;

pub(crate) use application::dispatch;
pub(crate) use identity::StaticIdentity;
pub(crate) use import::{ImportLimits, TableImporter};

#[cfg(test)]
mod tests;
