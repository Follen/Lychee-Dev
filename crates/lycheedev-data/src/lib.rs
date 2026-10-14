#![forbid(unsafe_code)]
//! Authenticated game data formats and bounded queries.
pub mod assets;
pub mod availability;
pub mod blte;
mod capture;
pub mod casc;
pub mod db2;
pub mod decoded_cache;
pub mod definitions;
pub mod hotfix;
pub mod installed;
pub mod keys;
pub mod local_query;
pub mod metadata_format;
pub mod navigate;
pub mod relational;
pub mod resource;
pub mod schema;
pub mod stream;
pub mod table;
#[cfg(test)]
mod unicode_checks;
mod unicode_simple;

use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};

pub(crate) fn fault(code: &str, exit: i32, message: impl Into<String>) -> ToolkitError {
    ToolkitError {
        code: code.into(),
        message: message.into(),
        exit_code: exit,
        details: serde_json::Value::Null,
    }
}

pub fn commands() -> &'static [&'static str] {
    &[
        "data hotfix",
        "data sql",
        "data db2",
        "data db2 schema",
        "data db2 search",
        "data db2 foreign-key",
        "data db2 stream",
        "data spell info",
        "data item get",
        "data item models",
        "data item geosets",
        "data item textures",
        "data decor list",
        "data decor get",
        "data creature display",
        "data creature model",
        "data encounter get",
        "asset inspect",
        "asset search",
        "asset export",
        "asset demux",
    ]
}

pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    if matches!(
        inv.command.as_str(),
        "asset search" | "asset export" | "asset demux"
    ) {
        return assets::dispatch(inv);
    }
    if matches!(
        inv.command.as_str(),
        "data spell info"
            | "data spell auras"
            | "data spell summons"
            | "data item get"
            | "data item models"
            | "data item geosets"
            | "data item textures"
            | "data decor list"
            | "data decor get"
            | "data creature display"
            | "data creature model"
            | "data encounter get"
    ) {
        return navigate::dispatch(inv);
    }
    if matches!(
        inv.command.as_str(),
        "data db2" | "data db2 schema" | "data db2 search" | "data db2 foreign-key"
    ) {
        return db2::dispatch(inv);
    }
    if inv.command == "data sql" {
        return local_query::sql(inv);
    }
    if inv.command == "asset inspect" {
        return local_query::asset(inv);
    }
    if inv.command == "data hotfix" && inv.required("--source")? == "dbcache" {
        return hotfix::dispatch(inv);
    }
    Err(fault(
        "command.unsupported",
        3,
        format!(
            "Rust data command has not completed parity: {}",
            inv.command
        ),
    ))
}
