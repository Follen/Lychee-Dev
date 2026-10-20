#![forbid(unsafe_code)]
//! Authenticated game data formats and bounded queries.
#[cfg(test)]
extern crate self as lycheedev_data;
pub mod assets;
pub mod availability;
pub mod blte;
mod capture;
pub mod casc;
pub mod db2;
pub mod decoded_cache;
pub mod definitions;
pub(crate) mod generation;
pub mod hotfix;
pub mod installed;
pub mod keys;
pub mod local_query;
pub mod metadata_format;
pub mod navigate;
pub(crate) mod phase_profile;
pub mod relational;
pub mod remote;
pub mod resource;
pub mod schema;
pub mod stream;
pub mod table;
#[cfg(test)]
mod unicode_checks;
#[cfg(test)]
#[path = "../tests/support/data_cache.rs"]
mod cache_fixture;
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
        "data cache prepare",
        "data hotfix",
        "data sql",
        "data db2",
        "data db2 schema",
        "data db2 search",
        "data db2 foreign-key",
        "data db2 stream",
        "data spell info",
        "data spell auras",
        "data spell summons",
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

/// Explicit Data scope of the existing cache prune command. CLI composes this
/// domain handler without introducing a Core-to-Data dependency.
pub fn prune_derived_cache(inv: &Invocation) -> Result<Outcome> {
    generation::prune(inv)
}

pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    if inv.command == "data cache prepare" { return generation::dispatch(inv); }
    if inv.command == "data hotfix"
        && inv.flag_bool("--scan")
        && inv.flag("--source") != Some("dbcache")
    {
        return Err(ToolkitError::arguments("--scan requires --source dbcache"));
    }
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
    if inv.command == "data hotfix" {
        return match inv.required("--source")? {
            "dbcache" => hotfix::dispatch(inv),
            "wago" | "raidbots" => hotfix::remote::dispatch(inv),
            _ => Err(ToolkitError::arguments("--source must be wago, dbcache, or raidbots")),
        };
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

#[cfg(test)]
mod command_surface_tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn registered_data_asset_paths_match_the_fixed_go_catalog() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/rust-parity/go-3.2.0.manifest.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["oracle"]["commit"],
            "3057a9666ab078270b63c7f06de748069b0d1f4b"
        );
        let expected: BTreeSet<_> = oracle["commands"]
            .as_array()
            .unwrap()
            .iter()
            .map(|command| command["path"].as_str().unwrap())
            .filter(|path| path.starts_with("data ") || path.starts_with("asset "))
            .collect();
        let registered: BTreeSet<_> = commands().iter().copied().collect();
        assert_eq!(
            registered.len(),
            commands().len(),
            "no duplicate registration"
        );
        let additions: BTreeSet<_> = registered.difference(&expected).copied().collect();
        assert_eq!(additions, BTreeSet::from(["data cache prepare"]));
        assert!(expected.is_subset(&registered), "all fixed Go commands remain registered");
    }

    #[test]
    fn registered_aura_and_summon_paths_reach_the_existing_handler() {
        for command in ["data spell auras", "data spell summons"] {
            assert!(commands().contains(&command));
            let inv = Invocation {
                command: command.into(),
                positional: None,
                flags: BTreeMap::new(),
                home: std::path::PathBuf::new(),
                cwd: std::path::PathBuf::new(),
                control: Default::default(),
            };
            let error = dispatch(&inv).unwrap_err();
            assert_eq!(error.code, "command.invalid_arguments");
            assert_eq!(error.message, format!("{command} requires --spell-id"));
            assert_eq!(error.exit_code, 2);
        }
    }
}
#[cfg(all(test, feature = "datafusion-qualification"))]
mod engine;
