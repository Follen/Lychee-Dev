#![forbid(unsafe_code)]
//! Owned boundary for the pinned Emmy analysis backend.
//!
//! Source owns authentication, load/graph policy, original coordinates and
//! public reports. This crate owns engine sessions and projects small owned
//! results without exposing Emmy identities or executing studied code.
//! The initial slice covers authenticated API declaration type queries only.
mod declaration;
mod types;

pub use declaration::DeclarationModel;
pub use types::{
    ByteRange, CallableInput, CoverageRow, DeclarationFailure, OriginInput, ParseIssue,
    ParseIssues, TypeProjection,
};
