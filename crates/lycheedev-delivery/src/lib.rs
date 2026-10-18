#![forbid(unsafe_code)]
//! Managed installation and release verification. Live receives an adapter at the entry point.
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};
pub mod addon_transaction;
pub mod deploy;
pub mod formal_repair;
pub mod installation;
pub mod npm_update;
pub mod release;
pub mod removal;
pub mod slot_publication;
#[cfg(test)]
mod slot_test_diagnostics;
pub mod slots;
pub mod update;
pub use installation::{VerifiedInstallation, verify_clean};
pub fn commands() -> &'static [&'static str] {
    &[
        "addon status",
        "skill status",
        "skill install",
        "skill remove",
        "addon install",
        "addon remove",
        "update",
    ]
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    match inv.command.as_str() {
        "update" => update::dispatch(inv),
        "addon remove" => {
            let client = selected_client(inv)?;
            let target = lycheedev_core::client::addon_destination(&client)?;
            Ok(Outcome::new(removal::addon_controlled(
                &target,
                std::path::Path::new(inv.required("--output")?),
                &inv.control,
            )?))
        }
        "addon install" => {
            let client = selected_client(inv)?;
            let target = lycheedev_core::client::addon_destination(&client)?;
            let guard = lycheedev_core::coordination::InstallationGuard::acquire(&target)?;
            let result = (|| {
                guard.require_idle()?;
                if inv.flag_bool("--resume") && inv.flag("--output").is_none() {
                    return Err(ToolkitError::arguments("--resume requires --output"));
                }
                let result = addon_transaction::apply(
                    &target,
                    addon_transaction::Request {
                        release: inv.flag("--release").map(std::path::Path::new),
                        archive: inv.flag("--output").map(std::path::Path::new),
                        version: lycheedev_core::VERSION,
                        resume: inv.flag_bool("--resume"),
                        control: &inv.control,
                    },
                    &guard,
                )?;
                Ok(Outcome::new(result))
            })();
            deploy::finish_close(result, guard.close())
        }
        "skill install" => {
            let target = std::path::Path::new(inv.required("--path")?);
            if target.file_name().is_none_or(|name| name != "lycheedev") {
                return Err(ToolkitError::new(
                    "delivery.invalid_installation",
                    "skill directory must be named lycheedev",
                    4,
                ));
            }
            if let Some(archive) = inv.flag("--output") {
                Ok(Outcome::new(deploy::upgrade_controlled(
                    inv.flag("--release").map(std::path::Path::new),
                    target,
                    std::path::Path::new(archive),
                    "skill",
                    lycheedev_core::VERSION,
                    inv.flag_bool("--resume"),
                    &inv.control,
                )?))
            } else {
                if inv.flag_bool("--resume") {
                    return Err(ToolkitError::arguments("--resume requires --output"));
                }
                Ok(Outcome::new(serde_json::to_value(
                    deploy::fresh_controlled(
                        std::path::Path::new(inv.required("--release")?),
                        target,
                        "skill",
                        lycheedev_core::VERSION,
                        &inv.control,
                    )?,
                )?))
            }
        }
        "skill remove" => {
            let target = std::path::Path::new(inv.required("--path")?);
            if target.file_name().is_none_or(|name| name != "lycheedev") {
                return Err(ToolkitError::new(
                    "delivery.invalid_installation",
                    "skill directory must be named lycheedev",
                    4,
                ));
            }
            Ok(Outcome::new(deploy::remove_controlled(
                target,
                std::path::Path::new(inv.required("--output")?),
                "skill",
                &inv.control,
            )?))
        }
        "addon status" | "skill status" => {
            if inv.command == "addon status" && inv.flag("--installation").is_some() {
                if inv.flag("--path").is_some() {
                    return Err(ToolkitError::arguments(
                        "addon status requires exactly one selector",
                    ));
                }
                let client = selected_client(inv)?;
                let target = lycheedev_core::client::addon_destination(&client)?;
                let installation = installation::inspect(&target, "addon")?;
                let mut result = serde_json::json!({"client":lycheedev_core::installation_selection::Client::from(&client),"installation":installation});
                if target
                    .parent()
                    .unwrap()
                    .join(".lycheedev-slots.json")
                    .exists()
                {
                    let version = installation["receipt"]["version"]
                        .as_str()
                        .unwrap_or(lycheedev_core::VERSION);
                    result["slots"] = match slots::inspect(target.parent().unwrap(), version) {
                        Ok(pool) => {
                            serde_json::json!({"state":"managed","count":pool["files"].as_array().map(Vec::len).unwrap_or(0),"pending":pool["files"].as_array().map(|files|files.iter().filter(|m|m["pendingHash"].as_str().is_some_and(|hash| !hash.is_empty())).count()).unwrap_or(0)})
                        }
                        Err(error) => {
                            serde_json::json!({"state":"incomplete","count":0,"pending":0,"detail":error.message})
                        }
                    };
                }
                return Ok(Outcome::new(result));
            }
            let component = if inv.command.starts_with("addon") {
                "addon"
            } else {
                "skill"
            };
            let target = inv
                .flag("--path")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    inv.flag("--installation")
                        .map(|p| std::path::Path::new(p).join("Interface/AddOns/Lychee Dev"))
                })
                .ok_or_else(|| {
                    ToolkitError::arguments("status requires --path or --installation")
                })?;
            Ok(Outcome::new(installation::inspect(&target, component)?))
        }
        _ => Err(ToolkitError::new(
            "delivery.migration_incomplete",
            "requested delivery handler has not yet been migrated",
            3,
        )),
    }
}

fn selected_client(inv: &Invocation) -> Result<lycheedev_core::client::ClientInstallation> {
    lycheedev_core::installation_selection::select_with(
        std::path::Path::new(inv.required("--installation")?),
        "",
        &mut lycheedev_core::installation_selection::InvocationMetadata(&inv.control),
    )
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use std::{
        collections::BTreeMap,
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self::with_timestamp(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
            )
        }
        fn with_timestamp(timestamp: u128) -> Self {
            let root = std::env::temp_dir().join(format!(
                "lychee-delivery-selection-{}-{timestamp}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            // The clock is not a uniqueness guarantee between parallel tests.
            // Own the root exclusively before any later Drop can remove it.
            fs::create_dir(&root).unwrap();
            let fixture = Self(root);
            let client = fixture.0.join("_classic_titan_");
            fs::create_dir_all(client.join("Interface/AddOns")).unwrap();
            fs::write(client.join(".flavor.info"), "wow_classic_titan").unwrap();
            fs::write(client.join("version.txt"), "3.80.2.69874").unwrap();
            fixture
        }
        fn addon_status(&self, inv: &Invocation, selector: &str) -> serde_json::Value {
            // Keep the public dispatch assertion, but first identify which
            // read-only boundary fails if CI reports an uncontextualized OS error.
            let client = selected_client(inv).unwrap_or_else(|error| {
                panic!("{selector}: select client under {:?}: {error:?}", self.0)
            });
            let target =
                lycheedev_core::client::addon_destination(&client).unwrap_or_else(|error| {
                    panic!(
                        "{selector}: resolve addon target for {}: {error:?}",
                        client.directory
                    )
                });
            let installation = installation::inspect(&target, "addon").unwrap_or_else(|error| {
                panic!("{selector}: inspect addon at {target:?}: {error:?}")
            });
            let result = dispatch(inv)
                .unwrap_or_else(|error| {
                    panic!(
                        "{selector}: dispatch addon status under {:?}: {error:?}",
                        self.0
                    )
                })
                .data;
            assert_eq!(
                result["installation"], installation,
                "{selector}: dispatch installation"
            );
            result
        }
        fn invocation(&self, command: &str) -> Invocation {
            Invocation {
                command: command.into(),
                positional: None,
                flags: BTreeMap::from([(
                    "--installation".into(),
                    self.0.to_string_lossy().into_owned(),
                )]),
                home: self.0.join("absent-home"),
                cwd: self.0.clone(),
                control: Default::default(),
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn addon_status_selects_root_or_client_with_only_go_public_client_fields() {
        let fixture = Fixture::new();
        let mut inv = fixture.invocation("addon status");
        let root = fixture.addon_status(&inv, "game root");
        inv.flags.insert(
            "--installation".into(),
            fixture
                .0
                .join("_classic_titan_")
                .to_string_lossy()
                .into_owned(),
        );
        let exact = fixture.addon_status(&inv, "exact client");
        assert_eq!(root, exact);
        assert_eq!(root["client"]["product"], "titan");
        // Go selection.ClientInstallation omits catalogProduct for ordinary slots.
        let keys = root["client"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            [
                "directory",
                "product",
                "productCode",
                "fullBuild",
                "interface",
                "toc",
                "identitySource"
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(root["installation"]["state"], "absent");
        assert!(!inv.home.exists());
    }
    #[test]
    fn selection_fixtures_with_identical_timestamps_own_independent_roots() {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // Force the clock collision rather than hoping concurrent threads hit it.
        let first = Fixture::with_timestamp(timestamp);
        let second = Fixture::with_timestamp(timestamp);
        assert_ne!(first.0, second.0);
        let first_path = first.0.clone();
        drop(first);
        assert!(!first_path.exists());
        assert!(second.0.is_dir());
        let inv = second.invocation("addon status");
        let result = second.addon_status(&inv, "surviving same-timestamp fixture");
        assert_eq!(result["client"]["product"], "titan");
        assert_eq!(result["installation"]["state"], "absent");
        assert!(!inv.home.exists());
    }
    #[test]
    fn all_three_delivery_consumers_obey_invocation_cancellation_before_writes() {
        let fixture = Fixture::new();
        for command in ["addon status", "addon install", "addon remove"] {
            let inv = fixture.invocation(command);
            inv.control.cancel_handle().store(true, Ordering::Release);
            assert_eq!(
                dispatch(&inv).unwrap_err().code,
                "command.cancelled",
                "{command}"
            );
        }
        assert!(
            !fixture
                .0
                .join("_classic_titan_/Interface/AddOns/Lychee Dev")
                .exists()
        );
    }
}
