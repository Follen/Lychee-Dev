//! Workspace and selection command admission; Core executes typed operations.
use crate::Invocation;
use lycheedev_core::{
    Outcome, Result, ToolkitError,
    doctor::{self, LegacyRoot},
    selection::{
        self, ProjectRequest,
        target::{TargetConfig, TargetRequest},
    },
    workspace::{self, ConfigUpdate, InitMode},
};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Debug)]
pub enum WorkspaceRequest {
    Init(InitMode),
    ConfigShow,
    ConfigSet(ConfigUpdate),
}
pub fn admit_workspace(inv: &Invocation) -> Result<WorkspaceRequest> {
    match inv.command.as_str() {
        "init" => Ok(WorkspaceRequest::Init(if inv.flag_bool("--plan") {
            InitMode::Plan
        } else if inv.flag_bool("--fresh") {
            InitMode::Fresh
        } else if inv.flag_bool("--resume") {
            InitMode::Resume
        } else {
            InitMode::Open
        })),
        "config show" => Ok(WorkspaceRequest::ConfigShow),
        "config set" => Ok(WorkspaceRequest::ConfigSet(ConfigUpdate {
            cache_max_bytes: inv
                .flag("--cache-max-bytes")
                .map(|_| inv.usize_flag("--cache-max-bytes", 0, 0, 1usize << 45))
                .transpose()?,
            download_workers: inv
                .flag("--download-workers")
                .map(|_| inv.usize_flag("--download-workers", 0, 0, 256))
                .transpose()?,
        })),
        _ => Err(ToolkitError::new(
            "core.command_unrouted",
            "no current handler for the requested workspace command",
            3,
        )),
    }
}
pub fn dispatch_workspace(inv: &Invocation) -> Result<Outcome> {
    let request = admit_workspace(inv)?;
    let ctx = crate::core_cli::context(inv);
    match request {
        WorkspaceRequest::Init(mode) => workspace::initialize_request(&ctx, mode),
        WorkspaceRequest::ConfigShow => workspace::config(&ctx.home).map(Outcome::new),
        WorkspaceRequest::ConfigSet(update) => workspace::set_config(&ctx, &update),
    }
}
pub fn admit_target(inv: &Invocation) -> Result<TargetRequest> {
    match inv.command.as_str() {
        "target list" => Ok(TargetRequest::List),
        "target add" => {
            let name = inv
                .positional
                .as_deref()
                .filter(|name| !name.is_empty())
                .ok_or_else(|| ToolkitError::arguments("target add requires a name argument"))?;
            let (product, region, locale) = (
                inv.flag("--product").unwrap_or(""),
                inv.flag("--region").unwrap_or(""),
                inv.flag("--locale").unwrap_or(""),
            );
            if product.is_empty() || region.is_empty() || locale.is_empty() {
                return Err(ToolkitError::arguments(
                    "target add requires --product, --region, and --locale",
                ));
            }
            if lycheedev_core::client::product(product).is_none() {
                return Err(ToolkitError::arguments(
                    "target add requires a supported --product",
                ));
            }
            if selection::target::validate_locale(region, locale).is_err() {
                return Err(ToolkitError::arguments(
                    "target add requires a supported --region and --locale pair",
                ));
            }
            let installation = inv.flag("--installation").unwrap_or("");
            Ok(TargetRequest::Add {
                config: TargetConfig {
                    schema: "lycheedev.target.v1".into(),
                    name: name.into(),
                    product: product.into(),
                    region: region.into(),
                    locale: locale.into(),
                    full_build: inv.flag("--build").unwrap_or("").into(),
                    source: if inv.flag_bool("--remote") {
                        "remote"
                    } else {
                        "installation"
                    }
                    .into(),
                    installation: installation.into(),
                    definitions: inv.flag("--definitions").unwrap_or("").into(),
                    repository: String::new(),
                },
                replace: inv.flag_bool("--replace"),
            })
        }
        "target show" => {
            let key = inv.argument()?;
            if key.is_empty() {
                return Err(ToolkitError::arguments(
                    "missing selection file or record ID; use describe",
                ));
            }
            Ok(TargetRequest::Show { key: key.into() })
        }
        "target remove" => {
            let key = inv.argument()?;
            if key.is_empty() {
                return Err(ToolkitError::arguments(
                    "target remove requires a name argument",
                ));
            }
            Ok(TargetRequest::Remove { key: key.into() })
        }
        _ => Err(ToolkitError::arguments("unknown target command")),
    }
}
pub fn dispatch_target(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    let request = admit_target(inv)?;
    selection::target::execute(&crate::core_cli::context(inv), &request)
}
pub fn admit_target_file(inv: &Invocation) -> Result<PathBuf> {
    let file = inv
        .flag("--file")
        .ok_or_else(|| ToolkitError::arguments("target resolve requires --file"))?;
    Ok(file.into())
}
pub fn dispatch_target_file(inv: &Invocation) -> Result<Outcome> {
    let path = admit_target_file(inv)?;
    workspace::resolve_file(&crate::core_cli::context(inv), &path)
}
pub fn admit_project(inv: &Invocation) -> Result<ProjectRequest> {
    match inv.command.as_str() {
        "project init" => Ok(ProjectRequest::Init {
            product: inv
                .flag("--product")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| ToolkitError::arguments("project init requires --product"))?
                .into(),
            path: inv.flag("--path").unwrap_or(".").into(),
        }),
        "project status" => Ok(ProjectRequest::Status {
            path: inv
                .flag("--path")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        }),
        "project lock" => Ok(ProjectRequest::Lock {
            snapshot: inv
                .flag("--snapshot")
                .filter(|value| !value.is_empty())
                .ok_or_else(|| ToolkitError::arguments("project lock requires --snapshot"))?
                .into(),
            path: inv
                .flag("--path")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        }),
        _ => Err(ToolkitError::arguments("unknown project verb")),
    }
}
pub fn dispatch_project(inv: &Invocation) -> Result<Outcome> {
    let request = admit_project(inv)?;
    selection::execute_project(&crate::core_cli::context(inv), &request)
}
/// CLI supplies its existing cross-domain provider observations lazily, after
/// Core's own checks; no Source/Live/Delivery dependency enters Core.
pub fn inspect_doctor(
    inv: &Invocation,
    legacy: &[LegacyRoot],
    provider: impl FnOnce() -> Result<Vec<Value>>,
) -> Result<Outcome> {
    doctor::inspect_with_provider(&crate::core_cli::context(inv), legacy, provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn invocation(command: &str, positional: Option<&str>, flags: &[(&str, &str)]) -> Invocation {
        Invocation {
            command: command.into(),
            positional: positional.map(str::to_owned),
            flags: flags
                .iter()
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect(),
            home: "absent-home".into(),
            cwd: "absent-project".into(),
            control: Default::default(),
        }
    }
    #[test]
    fn config_update_parses_only_selected_fields_and_keeps_existing_ranges() {
        for (flag, value) in [
            ("--cache-max-bytes", "-1"),
            ("--cache-max-bytes", "35184372088833"),
            ("--download-workers", "no"),
            ("--download-workers", "257"),
        ] {
            let error =
                admit_workspace(&invocation("config set", None, &[(flag, value)])).unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("command.invalid_arguments", 2)
            );
            assert!(error.message.starts_with(flag));
        }
        let WorkspaceRequest::ConfigSet(update) = admit_workspace(&invocation(
            "config set",
            None,
            &[("--download-workers", "0")],
        ))
        .unwrap() else {
            panic!()
        };
        assert_eq!(update.cache_max_bytes, None);
        assert_eq!(update.download_workers, Some(0));
    }
    #[test]
    fn required_project_arguments_are_checked_before_any_filesystem_work() {
        for (command, flag, message) in [
            (
                "project init",
                "--product",
                "project init requires --product",
            ),
            (
                "project lock",
                "--snapshot",
                "project lock requires --snapshot",
            ),
        ] {
            let error = admit_project(&invocation(command, None, &[(flag, "")])).unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("command.invalid_arguments", 2)
            );
            assert_eq!(error.message, message);
        }
        assert!(
            matches!(admit_project(&invocation("project lock", None, &[("--snapshot", "PIN-fixed"), ("--path", "")])).unwrap(), ProjectRequest::Lock { path: None, snapshot } if snapshot == "PIN-fixed")
        );
    }
    #[test]
    fn target_add_argument_errors_keep_their_original_priority() {
        let valid = [
            ("--product", "retail"),
            ("--region", "us"),
            ("--locale", "enUS"),
            ("--remote", "true"),
        ];
        for (positional, flags, message) in [
            (None, vec![], "target add requires a name argument"),
            (
                Some("n"),
                vec![],
                "target add requires --product, --region, and --locale",
            ),
            (
                Some("n"),
                vec![
                    ("--product", "invalid"),
                    ("--region", "us"),
                    ("--locale", "enUS"),
                ],
                "target add requires a supported --product",
            ),
            (
                Some("n"),
                vec![
                    ("--product", "retail"),
                    ("--region", "us"),
                    ("--locale", "invalid"),
                ],
                "target add requires a supported --region and --locale pair",
            ),
        ] {
            let error = admit_target(&invocation("target add", positional, &flags)).unwrap_err();
            assert_eq!(
                (error.code.as_str(), error.exit_code),
                ("command.invalid_arguments", 2)
            );
            assert_eq!(error.message, message);
        }
        let TargetRequest::Add { config, replace } =
            admit_target(&invocation("target add", Some("n"), &valid)).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            (config.name.as_str(), config.source.as_str()),
            ("n", "remote")
        );
        assert!(!replace);
    }
}
