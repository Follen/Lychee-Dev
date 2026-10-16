#![forbid(unsafe_code)]
//! Owned boundary for the pinned Emmy analysis backend.
//!
//! Source owns authentication, load/graph policy, original coordinates and
//! public reports. This crate owns engine sessions and projects small owned
//! results without exposing Emmy identities or executing studied code.
//! The initial slice covers authenticated API declaration type queries only.
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
