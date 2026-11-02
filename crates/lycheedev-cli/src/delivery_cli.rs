//! Delivery command admission. Maintenance leases and recovery stay in Delivery.
use crate::Invocation;
use lycheedev_core::{Outcome, Result, ToolkitError};
use lycheedev_delivery::{
    AddonSelector, InstallMode, Request,
    update::{self, Action, Launcher, TargetSelection, UpdateTarget},
};
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Environment {
    user_profile: Option<PathBuf>,
    codex_home: Option<PathBuf>,
    installed_release: Option<PathBuf>,
    node: Option<PathBuf>,
    nonce: Option<String>,
    npm_search_paths: Vec<PathBuf>,
}
impl Environment {
    fn current() -> Self {
        Self {
            user_profile: std::env::var_os("USERPROFILE").map(PathBuf::from),
            codex_home: std::env::var_os("CODEX_HOME").map(PathBuf::from),
            installed_release: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent()?.parent()?.parent().map(Path::to_path_buf)),
            node: std::env::var_os("LYCHEEDEV_NODE_EXE").map(PathBuf::from),
            nonce: std::env::var_os("LYCHEEDEV_NPM_NONCE")
                .map(|value| value.to_string_lossy().into_owned()),
            npm_search_paths: std::env::var_os("PATH")
                .map(|value| std::env::split_paths(&value).collect())
                .unwrap_or_default(),
        }
    }
}
fn absolute(inv: &Invocation, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        inv.cwd.join(path)
    }
}
fn input(inv: &Invocation, flag: &str) -> Result<PathBuf> {
    Ok(absolute(inv, Path::new(inv.required(flag)?)))
}
fn optional(inv: &Invocation, flag: &str) -> Result<Option<PathBuf>> {
    inv.flag(flag)
        .map(|value| {
            if value.is_empty() {
                return Err(ToolkitError::arguments(format!("{flag} requires a path")));
            }
            Ok(absolute(inv, Path::new(value)))
        })
        .transpose()
}
fn install_mode(inv: &Invocation, component: &str, selector: &str) -> Result<InstallMode> {
    let present = |name| inv.flag(name).is_some_and(|value| !value.is_empty());
    if !present(selector)
        || (!inv.flag_bool("--resume") && !present("--release"))
        || (inv.flag_bool("--resume") && (!present("--output") || present("--release")))
    {
        return Err(ToolkitError::arguments(format!(
            "{component} install requires {selector} and --release, or --resume with --output and no --release"
        )));
    }
    if inv.flag_bool("--resume") {
        Ok(InstallMode::Resume {
            archive: input(inv, "--output")?,
        })
    } else {
        Ok(InstallMode::Release {
            root: input(inv, "--release")?,
            archive: optional(inv, "--output")?,
        })
    }
}
fn removal(inv: &Invocation, component: &str, selector: &str) -> Result<(PathBuf, PathBuf)> {
    if ![selector, "--output"]
        .into_iter()
        .all(|name| inv.flag(name).is_some_and(|value| !value.is_empty()))
    {
        return Err(ToolkitError::arguments(format!(
            "{component} remove requires {selector} and --output"
        )));
    }
    Ok((input(inv, selector)?, input(inv, "--output")?))
}
fn update_request(inv: &Invocation, env: &Environment) -> Result<update::Request> {
    let paths = inv.flag_values("--path");
    let installations = inv.flag_values("--installation");
    let files = inv.flag_values("--file");
    if files.len() > 1 {
        return Err(ToolkitError::arguments("update accepts exactly one --file"));
    }
    let selection = if let Some(file) = files.first() {
        if !paths.is_empty() || !installations.is_empty() {
            return Err(ToolkitError::arguments(
                "update --file cannot be combined with target selectors",
            ));
        }
        if file.is_empty() {
            return Err(ToolkitError::arguments("--file requires a path"));
        }
        let file = absolute(inv, Path::new(file));
        let mut targets = update::read_target_file(&file, &inv.control)?;
        for target in &mut targets {
            if !target.path.as_os_str().is_empty() {
                target.path = absolute(inv, &target.path);
            }
        }
        TargetSelection::Explicit(targets)
    } else if paths.is_empty() && installations.is_empty() {
        let mut skill_candidates = Vec::new();
        if let Some(user) = &env.user_profile {
            let codex = env
                .codex_home
                .clone()
                .unwrap_or_else(|| user.join(".codex"));
            skill_candidates.push(absolute(inv, &user.join(".agents/skills/lycheedev")));
            skill_candidates.push(absolute(inv, &codex.join("skills/lycheedev")));
        }
        TargetSelection::Discover { skill_candidates }
    } else {
        let mut targets = Vec::new();
        for (component, paths) in [("skill", paths), ("addon", installations)] {
            for path in paths {
                if path.is_empty() {
                    return Err(ToolkitError::arguments(
                        "update target path must not be empty",
                    ));
                }
                targets.push(UpdateTarget {
                    component: component.into(),
                    path: absolute(inv, Path::new(&path)),
                });
            }
        }
        TargetSelection::Explicit(targets)
    };
    let release = optional(inv, "--release")?;
    let installed = || {
        env.installed_release.clone().ok_or_else(|| {
            ToolkitError::new(
                "delivery.invalid_release",
                "unable to locate installed release",
                3,
            )
        })
    };
    let action = match release {
        Some(release) => Action::Managed {
            release,
            plan: inv.flag_bool("--plan"),
        },
        None if inv.flag_bool("--plan") => Action::Managed {
            release: installed()?,
            plan: true,
        },
        None => Action::SelfUpdate {
            installed: installed()?,
            launcher: env
                .node
                .as_ref()
                .zip(env.nonce.as_ref())
                .map(|(node, nonce)| Launcher {
                    node: node.clone(),
                    nonce: nonce.clone(),
                    npm_search_paths: env.npm_search_paths.clone(),
                }),
            presentation: inv.flag("--format").unwrap_or("text").into(),
        },
    };
    Ok(update::Request { selection, action })
}
fn admit_with_environment(inv: &Invocation, env: &Environment) -> Result<Request> {
    inv.control.check()?;
    match inv.command.as_str() {
        "addon install" => {
            let mode = install_mode(inv, "addon", "--installation")?;
            Ok(Request::AddonInstall {
                installation: input(inv, "--installation")?,
                mode,
            })
        }
        "skill install" => {
            let mode = install_mode(inv, "skill", "--path")?;
            Ok(Request::SkillInstall {
                path: input(inv, "--path")?,
                mode,
            })
        }
        "addon remove" => {
            let (installation, archive) = removal(inv, "addon", "--installation")?;
            Ok(Request::AddonRemove {
                installation,
                archive,
            })
        }
        "skill remove" => {
            let (path, archive) = removal(inv, "skill", "--path")?;
            Ok(Request::SkillRemove { path, archive })
        }
        "addon status" => {
            if inv
                .flag("--installation")
                .is_some_and(|path| !path.is_empty())
            {
                if inv.flag("--path").is_some_and(|path| !path.is_empty()) {
                    return Err(ToolkitError::arguments(
                        "addon status accepts --installation or --path, not both",
                    ));
                }
                Ok(Request::AddonStatus(AddonSelector::Installation(input(
                    inv,
                    "--installation",
                )?)))
            } else {
                if inv.flag("--path").is_none_or(|path| path.is_empty()) {
                    return Err(ToolkitError::arguments(
                        "installation status requires --path",
                    ));
                }
                Ok(Request::AddonStatus(AddonSelector::Path(input(
                    inv, "--path",
                )?)))
            }
        }
        "skill status" => {
            if inv.flag("--path").is_none_or(|path| path.is_empty()) {
                return Err(ToolkitError::arguments(
                    "installation status requires --path",
                ));
            }
            Ok(Request::SkillStatus {
                path: input(inv, "--path")?,
            })
        }
        "update" => Ok(Request::Update(update_request(inv, env)?)),
        _ => Err(ToolkitError::arguments("unknown delivery command")),
    }
}
pub(crate) fn admit(inv: &Invocation) -> Result<Request> {
    admit_with_environment(inv, &Environment::current())
}
pub(crate) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let request = admit(inv)?;
    lycheedev_delivery::dispatch(&crate::core_cli::context(inv), &request)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    fn invocation(command: &str, flags: &[(&str, &str)]) -> Invocation {
        Invocation {
            command: command.into(),
            positional: None,
            flags: flags
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect::<BTreeMap<_, _>>(),
            home: "absent-delivery-home".into(),
            cwd: PathBuf::from(r"D:\fixture-delivery"),
            control: Default::default(),
        }
    }
    #[test]
    fn install_and_remove_admission_keep_release_resume_archive_contract() {
        for (command, selector) in [
            ("addon install", "--installation"),
            ("skill install", "--path"),
        ] {
            for flags in [
                vec![],
                vec![(selector, "target")],
                vec![(selector, "target"), ("--resume", "true")],
                vec![
                    (selector, "target"),
                    ("--resume", "true"),
                    ("--output", "archive"),
                    ("--release", "release"),
                ],
            ] {
                assert_eq!(
                    admit_with_environment(&invocation(command, &flags), &Environment::default())
                        .unwrap_err()
                        .exit_code,
                    2
                );
            }
            let request = admit_with_environment(
                &invocation(
                    command,
                    &[
                        (selector, "target"),
                        ("--resume", "true"),
                        ("--output", "archive"),
                    ],
                ),
                &Environment::default(),
            )
            .unwrap();
            assert!(matches!(
                request,
                Request::AddonInstall {
                    mode: InstallMode::Resume { .. },
                    ..
                } | Request::SkillInstall {
                    mode: InstallMode::Resume { .. },
                    ..
                }
            ));
        }
        for (command, selector) in [
            ("addon remove", "--installation"),
            ("skill remove", "--path"),
        ] {
            assert_eq!(
                admit_with_environment(
                    &invocation(command, &[(selector, "target")]),
                    &Environment::default()
                )
                .unwrap_err()
                .exit_code,
                2
            );
        }
    }
    #[test]
    fn status_admits_exactly_the_public_selector_and_keeps_relative_paths_in_context() {
        let inv = invocation(
            "addon status",
            &[("--installation", "game"), ("--path", "addon")],
        );
        assert_eq!(
            admit_with_environment(&inv, &Environment::default())
                .unwrap_err()
                .exit_code,
            2
        );
        let inv = invocation("skill status", &[("--installation", "game")]);
        assert_eq!(
            admit_with_environment(&inv, &Environment::default())
                .unwrap_err()
                .exit_code,
            2
        );
        let inv = invocation("addon status", &[("--installation", "game")]);
        let Request::AddonStatus(AddonSelector::Installation(path)) =
            admit_with_environment(&inv, &Environment::default()).unwrap()
        else {
            panic!("selector")
        };
        assert_eq!(path, inv.cwd.join("game"));
    }
    #[test]
    fn repeated_update_selectors_are_preserved_and_file_selector_conflicts_precede_io() {
        let mut inv = invocation("update", &[("--release", "release")]);
        inv.flags.insert(
            "__repeat:--path".into(),
            serde_json::to_string(&["one/lycheedev", "two/lycheedev"]).unwrap(),
        );
        inv.flags.insert(
            "__repeat:--installation".into(),
            serde_json::to_string(&["client1", "client2"]).unwrap(),
        );
        let Request::Update(update::Request {
            selection: TargetSelection::Explicit(targets),
            ..
        }) = admit_with_environment(&inv, &Environment::default()).unwrap()
        else {
            panic!("targets")
        };
        assert_eq!(targets.len(), 4);
        assert_eq!(targets[0].component, "skill");
        assert_eq!(targets[2].component, "addon");
        assert_eq!(targets[1].path, inv.cwd.join("two/lycheedev"));
        inv.flags.insert("--file".into(), "unopened.json".into());
        assert_eq!(
            admit_with_environment(&inv, &Environment::default())
                .unwrap_err()
                .message,
            "update --file cannot be combined with target selectors"
        );
        inv.flags.insert(
            "__repeat:--file".into(),
            serde_json::to_string(&["a.json", "b.json"]).unwrap(),
        );
        assert_eq!(
            admit_with_environment(&inv, &Environment::default())
                .unwrap_err()
                .exit_code,
            2
        );
    }
    #[test]
    fn update_launcher_and_discovery_inputs_are_frozen_in_typed_request() {
        let env = Environment {
            user_profile: Some(r"D:\user".into()),
            codex_home: Some(r"D:\codex".into()),
            installed_release: Some(r"D:\prefix\node_modules\lycheedev".into()),
            node: Some(r"D:\node.exe".into()),
            nonce: Some("a".repeat(64)),
            npm_search_paths: vec![r"D:\npm-path".into()],
        };
        let inv = invocation("update", &[("--format", "jsonl")]);
        let Request::Update(update::Request {
            selection: TargetSelection::Discover { skill_candidates },
            action:
                Action::SelfUpdate {
                    installed,
                    launcher: Some(launcher),
                    presentation,
                },
        }) = admit_with_environment(&inv, &env).unwrap()
        else {
            panic!("self update")
        };
        assert_eq!(
            skill_candidates,
            [
                PathBuf::from(r"D:\user\.agents\skills\lycheedev"),
                PathBuf::from(r"D:\codex\skills\lycheedev")
            ]
        );
        assert_eq!(Some(installed), env.installed_release);
        assert_eq!(Some(launcher.node), env.node);
        assert_eq!(Some(launcher.nonce), env.nonce);
        assert_eq!(launcher.npm_search_paths, env.npm_search_paths);
        assert_eq!(presentation, "jsonl");
        let Request::Update(update::Request {
            action: Action::Managed { plan, .. },
            ..
        }) = admit_with_environment(&invocation("update", &[("--plan", "true")]), &env).unwrap()
        else {
            panic!("plan")
        };
        assert!(plan);
    }
    #[test]
    fn original_cancel_precedes_admission_io() {
        let inv = invocation("update", &[("--file", "unopened.json")]);
        inv.control.tracked_cancel_handle().unwrap().cancel();
        let expected = inv.control.check().unwrap_err();
        let actual = admit_with_environment(&inv, &Environment::default()).unwrap_err();
        assert_eq!(actual.code, expected.code);
        assert_eq!(actual.exit_code, expected.exit_code);
        assert_eq!(actual.details, expected.details);
    }
}
