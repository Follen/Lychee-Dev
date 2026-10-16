#![forbid(unsafe_code)]
//! Owned boundary for the pinned Emmy analysis backend.
//!
//! Source owns authentication, load/graph policy, original coordinates and
//! public reports. This crate owns engine sessions and projects small owned
//! results without executing studied code. Engine-host configuration and ingestion
//! may use pinned public engine types; business contracts and worker wire records
//! do not borrow engine AST/model lifetimes.
mod config;
pub use config::lua51_config;
mod declaration;
mod ingestion;
pub use ingestion::{
    IngestedBatch, IngestionFailure, IngestionTextCost, ingest_public_batch,
    snapshot_contribution_order,
};
mod projection;
mod types;

pub use declaration::DeclarationModel;
pub use types::{
    ByteRange, CallableInput, CoverageRow, DeclarationFailure, DeclarationLookup,
    DeclarationProvenance, OriginInput, ParseIssue, ParseIssues, TypeProjection,
};

/// Selected evidence and ordered coverage ownership; production migration pending.
pub mod session;
