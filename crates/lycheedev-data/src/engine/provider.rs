//! DataFusion operator pool boundary used by the current session Engine.
//!
//! The original bounded pool policy is owned by `runtime`; the previous
//! fixed-snapshot qualification provider no longer has a production consumer
//! and was retired. Current query execution goes through
//! `session_engine`/`source_demand`/`pull_provider`.
pub(super) mod runtime;

#[cfg(test)]
mod tests;
