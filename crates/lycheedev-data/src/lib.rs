#![forbid(unsafe_code)]
//! Authenticated game data formats and bounded queries.
#[cfg(test)]
extern crate self as lycheedev_data;
pub mod agent_session;
pub mod assets;
pub mod availability;
pub mod blte;
#[cfg(test)]
#[path = "../tests/support/data_cache.rs"]
mod cache_fixture;
mod capture;
pub mod casc;
pub mod decoded_cache;
pub mod definitions;
pub(crate) mod domain;
pub mod engine;
pub(crate) mod generation;
pub mod hotfix;
pub mod installed;
pub mod keys;
pub mod metadata_format;
pub(crate) mod phase_profile;
pub mod query_source;
pub(crate) mod query_value;
pub mod remote;
pub mod request;
pub mod resource;
pub mod schema;
pub(crate) mod table_source;

pub mod table;
#[cfg(test)]
mod unicode_checks;
mod unicode_simple;

use lycheedev_core::{Outcome, Result, ToolkitError};

pub(crate) fn fault(code: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    ToolkitError {
        code: code.into(),
        message: message.into(),
        exit_code: exit,
        details: serde_json::Value::Null,
    }
}

/// Explicit Data scope of the existing cache prune command. CLI composes this
/// domain handler without introducing a Core-to-Data dependency.
pub fn prune_derived_cache(
    context: &request::DataContext,
    options: lycheedev_core::workspace::data_disk::DerivedRetirementOptions,
) -> Result<Outcome> {
    generation::prune(context, options)
}

/// Prepare one explicitly selected static table in the Data cache.
pub fn prepare_cache(
    context: &request::DataContext,
    request: &request::CachePrepareRequest,
) -> Result<Outcome> {
    generation::dispatch(context, request)
}
