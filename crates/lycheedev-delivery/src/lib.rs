#![forbid(unsafe_code)]
//! Managed installation and release verification. Live receives an adapter at the entry point.
use lycheedev_core::{Outcome, Result, ToolkitError, request::RequestContext};
#[cfg(test)]
mod acquisition_control_tests;
#[cfg(test)]
mod status_control_tests;
pub mod addon_transaction;
pub mod deploy;
pub mod formal_repair;
pub mod installation;
pub mod npm_update;
pub mod release;
pub mod removal;
pub mod slot_publication;
pub mod slot_reset;
#[cfg(test)]
mod slot_test_diagnostics;
pub mod slots;
pub mod update;
pub use installation::VerifiedInstallation;
#[derive(Clone, Debug)]
pub enum AddonSelector {
    Installation(std::path::PathBuf),
    Path(std::path::PathBuf),
}
#[derive(Clone, Debug)]
pub enum InstallMode {
    Release { root: std::path::PathBuf, archive: Option<std::path::PathBuf> },
    Resume { archive: std::path::PathBuf },
}
impl InstallMode {
    fn parts(&self) -> (Option<&std::path::Path>, Option<&std::path::Path>, bool) {
        match self {
            Self::Release { root, archive } => (Some(root), archive.as_deref(), false),
            Self::Resume { archive } => (None, Some(archive), true),
        }
    }
}
#[derive(Clone, Debug)]
pub enum Request {
    AddonStatus(AddonSelector),
    SkillStatus { path: std::path::PathBuf },
    AddonInstall { installation: std::path::PathBuf, mode: InstallMode },
    AddonRemove { installation: std::path::PathBuf, archive: std::path::PathBuf },
    SkillInstall { path: std::path::PathBuf, mode: InstallMode },
    SkillRemove { path: std::path::PathBuf, archive: std::path::PathBuf },
    Update(update::Request),
}
pub fn dispatch(ctx: &RequestContext, request: &Request) -> Result<Outcome> {
    ctx.control.check()?;
    match request {
        Request::Update(request) => update::dispatch(ctx, request),
        Request::AddonRemove { installation, archive } => {
            let client = selected_client(ctx, installation)?;
            let target = lycheedev_core::client::addon_destination(&client)?;
            Ok(Outcome::new(removal::addon_controlled(&target, archive, &ctx.control)?))
        }
        Request::AddonInstall { installation, mode } => {
            let client = selected_client(ctx, installation)?;
            let target = lycheedev_core::client::addon_destination(&client)?;
            #[cfg(test)]
            acquisition_control_tests::before_acquire(acquisition_control_tests::AcquirePoint::Install, &ctx.control);
            let guard = lycheedev_core::coordination::InstallationMaintenanceLease::acquire_controlled(&target, &ctx.control)?;
            let (release, archive, resume) = mode.parts();
            let result = addon_transaction::apply(&target, addon_transaction::Request {
                release, archive, version: lycheedev_core::VERSION, resume, control: &ctx.control,
            }, &guard).map(Outcome::new);
            deploy::finish_close(result, guard.close())
        }
        Request::SkillInstall { path, mode } => {
            skill_directory(path)?;
            let (release, archive, resume) = mode.parts();
            if let Some(archive) = archive {
                Ok(Outcome::new(deploy::upgrade_controlled(release, path, archive, "skill", lycheedev_core::VERSION, resume, &ctx.control)?))
            } else {
                // The admitted Release variant owns a release when no archive
                // is present; Resume always owns its original archive.
                Ok(Outcome::new(serde_json::to_value(deploy::fresh_controlled(
                    release.expect("admitted fresh skill release"), path, "skill", lycheedev_core::VERSION, &ctx.control,
                )?)?))
            }
        }
        Request::SkillRemove { path, archive } => {
            skill_directory(path)?;
            Ok(Outcome::new(deploy::remove_controlled(path, archive, "skill", &ctx.control)?))
        }
        Request::AddonStatus(AddonSelector::Installation(installation)) => {
            let client = selected_client(ctx, installation)?;
            let target = lycheedev_core::client::addon_destination(&client)?;
            #[cfg(test)]
            status_control_tests::before_inspect(status_control_tests::ReadPoint::Main, &ctx.control);
            let installation = installation::inspect_main_controlled(&target, "addon", &ctx.control)?;
            let mut result = serde_json::json!({"client":lycheedev_core::installation_selection::Client::from(&client),"installation":installation});
            let slot_parent = target.parent().unwrap();
            let has_slots = match std::fs::symlink_metadata(slot_parent.join(slots::MARKER)) {
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(error.into()),
            };
            if has_slots {
                let version = installation["receipt"]["version"].as_str().unwrap_or(lycheedev_core::VERSION);
                #[cfg(test)]
                status_control_tests::before_inspect(status_control_tests::ReadPoint::Slots, &ctx.control);
                result["slots"] = match slots::inspect_controlled(slot_parent, version, &ctx.control) {
                    Ok(pool) => serde_json::json!({"state":"managed","count":pool["files"].as_array().map(Vec::len).unwrap_or(0),"pending":pool["files"].as_array().map(|files|files.iter().filter(|m|m["pendingHash"].as_str().is_some_and(|hash| !hash.is_empty())).count()).unwrap_or(0)}),
                    Err(error) if error.code == "delivery.invalid_installation" && error.details["schema"] != lycheedev_core::storage::FILE_CLEANUP_SCHEMA => serde_json::json!({"state":"incomplete","count":0,"pending":0,"detail":error.message}),
                    Err(error) => return Err(error),
                };
            }
            ctx.control.check()?;
            Ok(Outcome::new(result))
        }
        Request::AddonStatus(AddonSelector::Path(path)) => {
            #[cfg(test)]
            status_control_tests::before_inspect(status_control_tests::ReadPoint::Main, &ctx.control);
            Ok(Outcome::new(installation::inspect_main_controlled(path, "addon", &ctx.control)?))
        }
        Request::SkillStatus { path } => {
            #[cfg(test)]
            status_control_tests::before_inspect(status_control_tests::ReadPoint::Main, &ctx.control);
            Ok(Outcome::new(installation::inspect_main_controlled(path, "skill", &ctx.control)?))
        }
    }
}
fn skill_directory(path: &std::path::Path) -> Result<()> {
    if path.file_name().is_none_or(|name| name != "lycheedev") {
        return Err(ToolkitError::new("delivery.invalid_installation", "skill directory must be named lycheedev", 4));
    }
    Ok(())
}
fn selected_client(ctx: &RequestContext, installation: &std::path::Path) -> Result<lycheedev_core::client::ClientInstallation> {
    lycheedev_core::installation_selection::select_with(installation, "", &mut lycheedev_core::installation_selection::InvocationMetadata(&ctx.control))
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    use std::{
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
        fn addon_status(&self, ctx: &RequestContext, installation_path: &std::path::Path, selector: &str) -> serde_json::Value {
            // Keep the public dispatch assertion, but first identify which
            // read-only boundary fails if CI reports an uncontextualized OS error.
            let client = selected_client(ctx, installation_path).unwrap_or_else(|error| {
                panic!("{selector}: select client under {:?}: {error:?}", self.0)
            });
            let target =
                lycheedev_core::client::addon_destination(&client).unwrap_or_else(|error| {
                    panic!(
                        "{selector}: resolve addon target for {}: {error:?}",
                        client.directory
                    )
                });
            let installation = installation::inspect_main_controlled(&target, "addon", &Default::default()).unwrap_or_else(|error| {
                panic!("{selector}: inspect addon at {target:?}: {error:?}")
            });
            let result = dispatch(ctx, &Request::AddonStatus(AddonSelector::Installation(installation_path.to_owned())))
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
        fn context(&self) -> RequestContext {
            RequestContext { home: self.0.join("absent-home"), cwd: self.0.clone(), control: Default::default() }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn addon_status_selects_root_or_client_with_only_public_client_fields() {
        let fixture = Fixture::new();
        let ctx = fixture.context();
        let root = fixture.addon_status(&ctx, &fixture.0, "game root");
        let exact = fixture.addon_status(&ctx, &fixture.0.join("_classic_titan_"), "exact client");
        assert_eq!(root, exact);
        assert_eq!(root["client"]["product"], "titan");
        // The shared client selection omits catalogProduct for ordinary slots.
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
        assert!(!ctx.home.exists());
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
        let ctx = second.context();
        let result = second.addon_status(&ctx, &second.0, "surviving same-timestamp fixture");
        assert_eq!(result["client"]["product"], "titan");
        assert_eq!(result["installation"]["state"], "absent");
        assert!(!ctx.home.exists());
    }
    #[test]
    fn all_three_delivery_consumers_obey_invocation_cancellation_before_writes() {
        let fixture = Fixture::new();
        for request in [
            Request::AddonStatus(AddonSelector::Installation(fixture.0.clone())),
            Request::AddonInstall { installation: fixture.0.clone(), mode: InstallMode::Release { root: "unopened-release".into(), archive: None } },
            Request::AddonRemove { installation: fixture.0.clone(), archive: "unopened-archive".into() },
        ] {
            let ctx = fixture.context();
            ctx.control.tracked_cancel_handle().unwrap().cancel();
            assert_eq!(dispatch(&ctx, &request).unwrap_err().code, "command.cancelled", "{request:?}");
        }
        assert!(
            !fixture
                .0
                .join("_classic_titan_/Interface/AddOns/Lychee Dev")
                .exists()
        );
    }
}
