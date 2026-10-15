//! Immutable remote data identities and strictly bounded CDN transport.
//! Catalog observations are not content authentication: decoded bytes must
//! still pass their EKey/chunk/CKey checks in the consuming reader.
pub(crate) mod admission;
pub(crate) mod provider;
pub(crate) mod session;
pub(crate) mod cache;
pub mod target;
pub(crate) mod fragments;
pub(crate) mod files;
pub(crate) mod index;
pub mod catalog;
mod proxy;
pub mod transport;

use crate::fault;
use lycheedev_core::ToolkitError;

fn invalid() -> ToolkitError {
    fault("records.invalid_metadata", 4, "records.invalid_metadata")
}
fn limit() -> ToolkitError {
    fault("records.metadata_limit", 3, "records.metadata_limit")
}
fn unavailable() -> ToolkitError {
    fault("records.build_unavailable", 3, "records.build_unavailable")
}
fn ambiguous() -> ToolkitError {
    fault("records.build_ambiguous", 4, "records.build_ambiguous")
}

/// Only availability may try another authenticated source. Corrupt metadata,
/// range errors and cancellation/budget errors stop immediately.
pub(crate) fn availability_failure(error: &ToolkitError) -> bool {
    matches!(
        error.code.as_str(),
        "records.remote_http" | "records.remote_object_missing"
    )
}
