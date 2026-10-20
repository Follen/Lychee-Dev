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
#[cfg(any(test, feature = "migration-oracle"))]
pub mod db2;
#[cfg(not(any(test, feature = "migration-oracle")))]
pub(crate) mod db2;
pub mod decoded_cache;
pub mod definitions;
pub mod engine;
pub(crate) mod generation;
pub mod hotfix;
pub mod installed;
pub mod keys;
#[cfg(any(test, feature = "migration-oracle"))]
pub mod local_query;
#[cfg(not(any(test, feature = "migration-oracle")))]
pub(crate) mod local_query;
pub mod metadata_format;
#[cfg(any(test, feature = "migration-oracle"))]
pub mod migration_oracle;
#[cfg(any(test, feature = "migration-oracle"))]
pub mod navigate;
#[cfg(not(any(test, feature = "migration-oracle")))]
pub(crate) mod navigate;
pub(crate) mod phase_profile;
#[cfg(any(test, feature = "migration-oracle"))]
pub mod relational;
#[cfg(not(any(test, feature = "migration-oracle")))]
pub(crate) mod relational;
pub mod remote;
pub mod resource;
pub mod schema;
#[cfg(any(test, feature = "migration-oracle"))]
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
        "data agent schema",
        "data agent serve",
        "data cache prepare",
        "data hotfix",
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
    if inv.command == "data agent schema" {
        inv.control.check()?;
        return Ok(Outcome::new(agent_session::schema()));
    }
    if inv.command == "data agent serve" {
        return Err(fault(
            "data.session.supervisor_required",
            3,
            "Data Agent serve requires the CLI-owned supervised process entry",
        ));
    }
    if inv.command == "data cache prepare" {
        return generation::dispatch(inv);
    }
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
    if inv.command == "asset inspect" {
        return local_query::asset(inv);
    }
    if inv.command == "data hotfix" {
        return match inv.required("--source")? {
            "dbcache" => hotfix::dispatch(inv),
            "wago" | "raidbots" => hotfix::remote::dispatch(inv),
            _ => Err(ToolkitError::arguments(
                "--source must be wago, dbcache, or raidbots",
            )),
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
        assert_eq!(
            additions,
            BTreeSet::from([
                "data agent schema",
                "data agent serve",
                "data cache prepare"
            ])
        );
        assert!(
            expected.difference(&registered).copied().collect::<BTreeSet<_>>()
                == BTreeSet::from(["data sql", "data db2", "data db2 schema", "data db2 search", "data db2 foreign-key", "data db2 stream", "data spell info", "data spell auras", "data spell summons", "data item get", "data item models", "data item geosets", "data item textures", "data decor list", "data decor get", "data creature display", "data creature model", "data encounter get"]),
            "retired commands are available only through explicit migration oracle; production uses typed tasks"
        );
    }

    #[test]
    fn agent_discovery_is_available_without_opening_a_session() {
        let mut inv = Invocation {
            command: "data agent schema".into(),
            positional: None,
            flags: BTreeMap::new(),
            home: std::path::PathBuf::new(),
            cwd: std::path::PathBuf::new(),
            control: Default::default(),
        };
        let outcome = dispatch(&inv).unwrap();
        assert!(outcome.complete);
        assert_eq!(outcome.data, agent_session::schema());
        inv.command = "data agent serve".into();
        assert_eq!(
            dispatch(&inv).unwrap_err().code,
            "data.session.supervisor_required"
        );
    }

    #[test]
    fn registered_aura_and_summon_paths_reach_the_existing_handler() {
        for command in ["data spell auras", "data spell summons"] {
            assert!(!commands().contains(&command));
            let inv = Invocation {
                command: command.into(),
                positional: None,
                flags: BTreeMap::new(),
                home: std::path::PathBuf::new(),
                cwd: std::path::PathBuf::new(),
                control: Default::default(),
            };
            let error = migration_oracle::dispatch(&inv).unwrap_err();
            assert_eq!(error.code, "command.invalid_arguments");
            assert_eq!(error.message, format!("{command} requires --spell-id"));
            assert_eq!(error.exit_code, 2);
        }
    }
}
