//! Managed resource update decisions and recovery live here, not in the npm launcher.
use crate::{deploy, installation, release, slots};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError, VERSION,
    control::OperationControl,
    coordination::InstallationGuard,
    storage::{self, ExclusiveLock},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTarget {
    pub component: String,
    pub path: PathBuf,
}
fn state_path(home: &Path) -> PathBuf {
    let mut value = home.as_os_str().to_os_string();
    value.push(".delivery");
    PathBuf::from(value)
}
fn read_targets(state: &Path, control: &OperationControl) -> Result<Vec<UpdateTarget>> {
    let path = state.join("targets.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    storage::guard_path(state, &path)?;
    if fs::metadata(&path)?.len() > 64 << 10 {
        return Err(ToolkitError::invalid(
            "update target inventory exceeds 64 KiB",
        ));
    }
    let values: Vec<UpdateTarget> =
        serde_json::from_slice(&slots::read_bounded_controlled(&path, 64 << 10, control)?)?;
    if values.len() > 64 {
        return Err(ToolkitError::invalid("update supports at most 64 targets"));
    }
    Ok(values)
}
fn paths_are_implicit(inv: &Invocation) -> bool {
    inv.flag("--file").is_none()
        && inv.flag_values("--path").is_empty()
        && inv.flag_values("--installation").is_empty()
}
fn path_identity(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .replace('\\', "/")
        .to_ascii_lowercase()
}
/// Remembered paths are discovery evidence, never a request to reinstall a
/// deliberately removed tree. A moved-old transaction still owns its target.
fn discover_targets(
    targets: Vec<UpdateTarget>,
    control: &OperationControl,
) -> Result<Vec<UpdateTarget>> {
    let mut selected = Vec::new();
    for target in targets {
        control.check()?;
        let directory = if target.component == "addon" {
            target.path.join("Interface/AddOns/Lychee Dev")
        } else {
            target.path.clone()
        };
        let current =
            installation::inspect_main_controlled(&directory, &target.component, control)?;
        if current["state"] != "absent"
            || !pending_recoveries(&directory, &target.component, control)?.is_empty()
        {
            selected.push(target);
        }
    }
    Ok(selected)
}
/// Scan the target's whole recovery namespace, including earlier release hashes
/// and older selector hashes. Cleanup markers win when the archive is gone.
fn pending_recoveries(
    target: &Path,
    component: &str,
    control: &OperationControl,
) -> Result<Vec<(PathBuf, Value)>> {
    control.check()?;
    let root = target
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| ToolkitError::invalid("update target needs discovery grandparent"))?
        .join(".lycheedev-updates");
    match fs::symlink_metadata(&root) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
        Ok(info) if !info.is_dir() || info.file_type().is_symlink() => {
            return Err(ToolkitError::invalid(
                "redirected update recovery namespace",
            ));
        }
        Ok(_) => (),
    }
    storage::guard_path(root.parent().unwrap(), &root)?;
    let mut candidates = vec![];
    for (count, entry) in fs::read_dir(&root)?.enumerate() {
        control.check()?;
        if count >= 1000 {
            return Err(ToolkitError::invalid(
                "update recovery scan exceeds 1000 entries",
            ));
        }
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("replace-") {
            continue;
        }
        let path = entry.path();
        storage::guard_path(&root, &path)?;
        if name.ends_with(".cleanup.json") {
            candidates.push((
                0,
                path.clone(),
                root.join(name.strip_suffix(".cleanup.json").unwrap()),
            ));
        } else if entry.file_type()?.is_dir() {
            let record = path.join("replacement.json");
            match fs::symlink_metadata(&record) {
                Ok(_) => candidates.push((1, record, path)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.into()),
            }
        }
    }
    candidates.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    let mut seen = BTreeSet::new();
    let mut found = vec![];
    for (_, path, archive) in candidates {
        control.check()?;
        storage::guard_path(&root, &path)?;
        let raw = slots::read_bounded_controlled(&path, 1 << 20, control)?;
        let Ok(record) = serde_json::from_slice::<Value>(&raw) else {
            continue;
        };
        if record["schema"] != "lycheedev.replacement.v1"
            || record["target"]
                .as_str()
                .is_none_or(|path| path_identity(Path::new(path)) != path_identity(target))
            || record["next"]["component"] != component
        {
            continue;
        }
        if seen.insert(path_identity(&archive)) {
            found.push((archive, record));
        }
    }
    Ok(found)
}
fn select_recovery(
    target: &Path,
    component: &str,
    default: &Path,
    wanted: &Value,
    control: &OperationControl,
) -> Result<PathBuf> {
    let recoveries = pending_recoveries(target, component, control)?;
    if recoveries.len() > 1 {
        return Err(ToolkitError::new("delivery.installation_conflict", "multiple update recoveries for the selected target", 4)
            .with_details(json!({"path":target,"recoveries":recoveries.iter().map(|(path, _)|path).collect::<Vec<_>>()})));
    }
    if let Some((archive, record)) = recoveries.into_iter().next() {
        if !installation::same_receipt_value(&record["next"], wanted)? {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "resume the original update before selecting another release",
                4,
            )
            .with_details(json!({"path":target,"recovery":archive,"receipt":record["next"]})));
        }
        Ok(archive)
    } else {
        Ok(default.to_path_buf())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UpdateStep {
    BeforeTarget(usize),
    LockedTarget(usize),
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    let state = state_path(&inv.home);
    let paths = inv.flag_values("--path");
    let installations = inv.flag_values("--installation");
    let mut targets =
        if let Some(file) = inv.flag("--file") {
            if !paths.is_empty() || !installations.is_empty() {
                return Err(ToolkitError::arguments(
                    "update --file cannot be combined with target selectors",
                ));
            }
            if fs::metadata(file)?.len() > 64 << 10 {
                return Err(ToolkitError::arguments("update target file exceeds 64 KiB"));
            }
            let values: Vec<UpdateTarget> = serde_json::from_slice(
                &slots::read_bounded_controlled(Path::new(file), 64 << 10, &inv.control)?,
            )?;
            values
        } else {
            vec![]
        };
    targets.extend(paths.into_iter().map(|path| UpdateTarget {
        component: "skill".into(),
        path: path.into(),
    }));
    targets.extend(installations.into_iter().map(|path| UpdateTarget {
        component: "addon".into(),
        path: path.into(),
    }));
    if targets.is_empty() && inv.flag("--file").is_none() {
        targets = read_targets(&state, &inv.control)?;
        let user = std::env::var_os("USERPROFILE").map(PathBuf::from);
        if let Some(user) = user {
            let codex = std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| user.join(".codex"));
            for path in [
                user.join(".agents/skills/lycheedev"),
                codex.join("skills/lycheedev"),
            ] {
                if path.exists() {
                    targets.push(UpdateTarget {
                        component: "skill".into(),
                        path,
                    });
                }
            }
        }
        if inv.home.join("state/toolkit.sqlite").exists() {
            let configs: Vec<Value> = lycheedev_core::metadata::Metadata::open(&inv.home, false)?
                .list("target/", 10001)?;
            for config in configs {
                if config["source"] == "installation"
                    && let Some(path) = config["installation"].as_str()
                {
                    targets.push(UpdateTarget {
                        component: "addon".into(),
                        path: path.into(),
                    });
                }
            }
        }
    }
    if paths_are_implicit(inv) {
        targets = discover_targets(targets, &inv.control)?;
    }
    let root = inv
        .flag("--release")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent()?.parent()?.parent().map(Path::to_path_buf))
        })
        .ok_or_else(|| {
            ToolkitError::new(
                "delivery.invalid_release",
                "unable to locate installed release",
                3,
            )
        })?;
    if inv.flag("--release").is_none() && !inv.flag_bool("--plan") {
        if let (Some(node), Some(nonce)) = (
            std::env::var_os("LYCHEEDEV_NODE_EXE"),
            std::env::var_os("LYCHEEDEV_NPM_NONCE"),
        ) {
            let plan = managed_with_control(&root, VERSION, &state, &targets, true, &inv.control)?;
            let frozen: Vec<UpdateTarget> = plan.data["targets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|target| {
                    serde_json::from_value(
                        json!({"component":target["component"],"path":target["path"]}),
                    )
                })
                .collect::<std::result::Result<_, _>>()?;
            return crate::npm_update::prepare(
                inv,
                &root,
                &PathBuf::from(node),
                &nonce.to_string_lossy(),
                frozen,
            );
        }
        return Err(ToolkitError::new(
            "update.self_update_handoff_required",
            "native archives require --release; npm process self-update handoff is not yet connected",
            3,
        ));
    }
    managed_with_control(
        &root,
        VERSION,
        &state,
        &targets,
        inv.flag_bool("--plan"),
        &inv.control,
    )
}
pub fn managed(
    root: &Path,
    version: &str,
    state: &Path,
    targets: &[UpdateTarget],
    plan: bool,
) -> Result<Outcome> {
    managed_with_control(root, version, state, targets, plan, &Default::default())
}
pub fn managed_with_control(
    root: &Path,
    version: &str,
    state: &Path,
    targets: &[UpdateTarget],
    plan: bool,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Outcome> {
    managed_observed(root, version, state, targets, plan, control, &mut |_| {})
}
fn managed_observed(
    root: &Path,
    version: &str,
    state: &Path,
    targets: &[UpdateTarget],
    plan: bool,
    control: &OperationControl,
    observe_target: &mut impl FnMut(UpdateStep),
) -> Result<Outcome> {
    managed_with_close(
        root,
        version,
        state,
        targets,
        plan,
        control,
        observe_target,
        ExclusiveLock::close,
    )
}
#[allow(clippy::too_many_arguments)]
fn managed_with_close(
    root: &Path,
    version: &str,
    state: &Path,
    targets: &[UpdateTarget],
    plan: bool,
    control: &OperationControl,
    observe_target: &mut impl FnMut(UpdateStep),
    close_state: impl FnOnce(ExclusiveLock) -> Result<()>,
) -> Result<Outcome> {
    control.check()?;
    let release = release::inspect_with_control(root, version, control)?;
    if targets.len() > 64 {
        return Err(ToolkitError::invalid("update supports at most 64 targets"));
    }
    let mut seen = BTreeSet::new();
    let mut items = vec![];
    for target in targets {
        control.check()?;
        if !["addon", "skill"].contains(&target.component.as_str())
            || target.path.as_os_str().is_empty()
        {
            return Err(ToolkitError::invalid(
                "update target component/path invalid",
            ));
        }
        let (client, path) = if target.component == "addon" {
            let client = lycheedev_core::client::inspect(&target.path)?;
            let path = lycheedev_core::client::addon_destination(&client)?;
            let guard = InstallationGuard::acquire(&path)?;
            let checked = guard.require_idle().and_then(|()| {
                slots::preflight_controlled(path.parent().unwrap(), version, control)
            });
            deploy::finish_close(checked, guard.close())?;
            (Some(client), path)
        } else {
            if target
                .path
                .file_name()
                .is_none_or(|name| name != "lycheedev")
            {
                return Err(ToolkitError::invalid(
                    "skill directory must be named lycheedev",
                ));
            }
            (None, deploy::destination(&target.path)?)
        };
        let logical = client
            .map(|client| PathBuf::from(client.directory))
            .unwrap_or_else(|| path.clone());
        let key = format!(
            "{}:{}",
            target.component,
            logical.to_string_lossy().to_ascii_lowercase()
        );
        if !seen.insert(key.clone()) {
            continue;
        }
        let current = installation::inspect_main_controlled(&path, &target.component, control)?;
        if !["managed", "absent"].contains(&current["state"].as_str().unwrap_or("")) {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "one selected target is unmanaged or modified; no targets changed",
                4,
            )
            .with_details(json!({"path":path,"assessment":current})));
        }
        let mut recovery = path
            .parent()
            .unwrap()
            .parent()
            .ok_or_else(|| ToolkitError::invalid("update target needs discovery grandparent"))?
            .join(".lycheedev-updates")
            .join(format!(
                "replace-{}",
                &storage::sha256(
                    format!("{key}:{}:{}", release.version, release.commit).as_bytes()
                )[..32]
            ));
        recovery = select_recovery(
            &path,
            &target.component,
            &recovery,
            &serde_json::to_value(release::receipt(&release, &target.component))?,
            control,
        )?;
        let mut item = json!({"component":target.component,"path":logical,"state":current["state"],"recovery":recovery});
        if let Some(previous) = current["receipt"]["version"].as_str() {
            item["previousVersion"] = json!(previous);
        }
        items.push((target.component.clone(), path, recovery, item));
    }
    if items
        .iter()
        .any(|(component, _, _, _)| component == "addon")
    {
        let toc = String::from_utf8(slots::read_bounded_controlled(
            &root.join("payload/addon/Lychee Dev.toc"),
            256 << 20,
            control,
        )?)
        .map_err(|e| ToolkitError::invalid(e.to_string()))?;
        if !toc
            .lines()
            .any(|line| line.trim().replace('\\', "/") == "Bridge/SlotRuntime.lua")
        {
            return Err(ToolkitError::new(
                "delivery.protocol_mismatch",
                "mailbox protocol v1 addon update needs matching SlotRuntime closure",
                3,
            ));
        }
    }
    let mut result = json!({"version":release.version,"commit":release.commit,"complete":!plan&&items.is_empty(),"plan":plan,"targets":items.iter().map(|(_,_,_,item)|item.clone()).collect::<Vec<_>>()});
    control.check()?;
    if plan || items.is_empty() {
        return Ok(Outcome::new(result));
    }
    fs::create_dir_all(state)?;
    storage::guard_path(state, state)?;
    let lock = ExclusiveLock::resource_controlled(state, "managed-update", control)?;
    let updated = (|| {
        let mut remembered = read_targets(state, control)?;
        for (_, _, _, item) in &items {
            let target: UpdateTarget =
                serde_json::from_value(json!({"component":item["component"],"path":item["path"]}))?;
            if !remembered.iter().any(|value| {
                value.component == target.component
                    && value
                        .path
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&target.path.to_string_lossy())
            }) {
                remembered.push(target);
            }
        }
        if remembered.len() > 64 {
            return Err(ToolkitError::invalid("remembered update targets exceed 64"));
        }
        storage::atomic_write_controlled(
            &state.join("targets.json"),
            &serde_json::to_vec_pretty(&remembered)?,
            control,
        )?;
        for (index, (component, path, planned_recovery, item)) in items.iter().enumerate() {
            let mut recovery = planned_recovery.clone();
            let mut installed = false;
            let mut completed = false;
            let mutation = (|| -> Result<()> {
                observe_target(UpdateStep::BeforeTarget(index));
                control.check()?;
                // Always maintenance before the shared installation lock. Direct
                // addon install uses the same order; skills only need the latter.
                let maintenance = if component == "addon" {
                    Some(InstallationGuard::acquire(path)?)
                } else {
                    None
                };
                let changed = (|| {
                    if let Some(guard) = &maintenance {
                        guard.require_idle()?;
                    }
                    let lease = deploy::InstallationLease::acquire(path, control)?;
                    let applied = (|| -> Result<()> {
                        let wanted = serde_json::to_value(release::receipt(&release, component))?;
                        // Preflight is informational. Re-scan under the exact lease
                        // held through fresh/upgrade/cleanup, so an older intent
                        // appearing after preflight cannot be bypassed by fresh.
                        recovery = select_recovery(path, component, &recovery, &wanted, control)?;
                        observe_target(UpdateStep::LockedTarget(index));
                        let current =
                            installation::inspect_main_controlled(path, component, control)?;
                        installed = current["state"] == "managed"
                            && installation::same_receipt_value(&current["receipt"], &wanted)?;
                        if component == "addon" {
                            if cleanup_marker(&recovery).exists() {
                                cleanup_replacement(&lease, &recovery, &wanted, control)?;
                            }
                            let archive = if recovery.exists()
                                || current["state"] == "managed" && !installed
                            {
                                Some(recovery.as_path())
                            } else {
                                None
                            };
                            if let Some(archive) = archive {
                                control.check()?;
                                fs::create_dir_all(archive.parent().unwrap())?;
                            }
                            crate::addon_transaction::apply_locked(
                                path,
                                crate::addon_transaction::Request {
                                    release: Some(root),
                                    archive,
                                    version,
                                    resume: false,
                                    control,
                                },
                                maintenance.as_ref().expect("addon maintenance guard"),
                                &lease,
                            )?;
                        } else if cleanup_marker(&recovery).exists() {
                            cleanup_replacement(&lease, &recovery, &wanted, control)?;
                        } else if recovery.exists() {
                            deploy::upgrade_locked(
                                None, &lease, &recovery, component, version, true, control,
                            )?;
                        } else if current["state"] == "absent" {
                            deploy::fresh_locked(root, &lease, component, version, control)?;
                        } else if !installed {
                            control.check()?;
                            fs::create_dir_all(recovery.parent().unwrap())?;
                            deploy::upgrade_locked(
                                Some(root),
                                &lease,
                                &recovery,
                                component,
                                version,
                                false,
                                control,
                            )?;
                        }
                        installed = true;
                        if recovery.exists() || cleanup_marker(&recovery).exists() {
                            cleanup_replacement(&lease, &recovery, &wanted, control)?;
                        }
                        completed = true;
                        Ok(())
                    })();
                    lease.finish(applied)
                })();
                match maintenance {
                    Some(guard) => deploy::finish_close(changed, guard.close()),
                    None => changed,
                }
            })();
            result["targets"][index] = item.clone();
            result["targets"][index]["recovery"] = json!(recovery);
            if installed {
                result["targets"][index]["activation"] = json!(if component == "addon" {
                    "reload_required"
                } else {
                    "new_agent_context_required"
                });
            }
            if let Err(error) = mutation {
                if let Some(original) = error.details["recovery"].as_str() {
                    result["targets"][index]["recovery"] = json!(original);
                }
                result["targets"][index]["state"] = json!(if completed {
                    "current"
                } else if installed {
                    "cleanup_pending"
                } else {
                    "pending"
                });
                return Err(
                    ToolkitError::new(error.code, error.message, error.exit_code)
                        .with_details(json!({"result":result,"cause":error.details})),
                );
            }
            result["targets"][index]["state"] = json!("current");
            if recovery.exists() {
                result["targets"][index]["recoveryRetained"] = json!(true);
            } else {
                result["targets"][index]
                    .as_object_mut()
                    .unwrap()
                    .remove("recovery");
            }
        }
        result["complete"] = json!(true);
        Ok(Outcome::new(result))
    })();
    deploy::finish_close(updated, close_state(lock))
}
fn cleanup_marker(archive: &Path) -> PathBuf {
    let mut name = archive.as_os_str().to_os_string();
    name.push(".cleanup.json");
    PathBuf::from(name)
}
fn cleanup_replacement(
    lease: &deploy::InstallationLease,
    archive: &Path,
    wanted: &Value,
    control: &OperationControl,
) -> Result<()> {
    let target = lease.target();
    deploy::outside(
        target
            .parent()
            .ok_or_else(|| ToolkitError::invalid("update target has no parent"))?,
        archive,
    )?;
    if archive
        .parent()
        .and_then(Path::file_name)
        .is_none_or(|name| name != ".lycheedev-updates")
        || archive
            .file_name()
            .is_none_or(|name| !name.to_string_lossy().starts_with("replace-"))
    {
        return Err(ToolkitError::invalid(
            "automatic cleanup archive is outside the owned update namespace",
        ));
    }
    let assessment = installation::inspect_main_controlled(
        target,
        wanted["component"].as_str().unwrap_or(""),
        control,
    )?;
    if assessment["state"] != "managed"
        || !installation::same_receipt_value(&assessment["receipt"], wanted)?
    {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "installed bytes changed before old-tree cleanup",
            4,
        ));
    }
    let marker = cleanup_marker(archive);
    let record: Value = if marker.exists() {
        serde_json::from_slice(&slots::read_bounded_controlled(&marker, 1 << 20, control)?)?
    } else {
        let record: Value = serde_json::from_slice(&slots::read_bounded_controlled(
            &archive.join("replacement.json"),
            1 << 20,
            control,
        )?)?;
        if record["schema"] != "lycheedev.replacement.v1"
            || record["target"] != serde_json::to_value(target)?
            || !installation::same_receipt_value(&record["next"], wanted)?
        {
            return Err(ToolkitError::invalid(
                "cleanup replacement identity differs",
            ));
        }
        let previous = installation::inspect_main_controlled(
            &archive.join("previous"),
            wanted["component"].as_str().unwrap_or(""),
            control,
        )?;
        if previous["state"] != "managed"
            || !installation::same_receipt_value(&previous["receipt"], &record["previous"])?
        {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "old owned bytes changed before cleanup intent",
                4,
            ));
        }
        storage::atomic_write_controlled(&marker, &serde_json::to_vec_pretty(&record)?, control)?;
        record
    };
    if record["schema"] != "lycheedev.replacement.v1"
        || record["target"] != serde_json::to_value(target)?
        || !installation::same_receipt_value(&record["next"], wanted)?
    {
        return Err(ToolkitError::invalid("cleanup journal identity differs"));
    }
    if archive.exists() {
        storage::guard_path(archive.parent().unwrap(), archive)?;
        let mut expected = BTreeSet::new();
        expected.insert(PathBuf::from("replacement.json"));
        expected.insert(PathBuf::from(format!("previous/{}", installation::MARKER)));
        for resource in record["previous"]["resources"]
            .as_array()
            .ok_or_else(|| ToolkitError::invalid("cleanup owned inventory missing"))?
        {
            control.check()?;
            let relative = resource["path"]
                .as_str()
                .and_then(|name| {
                    name.strip_prefix(&format!("{}/", wanted["component"].as_str().unwrap_or("")))
                })
                .ok_or_else(|| ToolkitError::invalid("cleanup resource component differs"))?;
            let safe = storage::safe_relative(Path::new("previous"), relative)?;
            let existing = archive.join(&safe);
            if existing.exists() {
                storage::guard_path(archive, &existing)?;
                let bytes = slots::read_bounded_controlled(&existing, 256 << 20, control)?;
                if resource["bytes"].as_u64() != Some(bytes.len() as u64)
                    || resource["sha256"] != storage::sha256(&bytes)
                {
                    return Err(ToolkitError::new(
                        "delivery.installation_conflict",
                        "remaining owned archive bytes changed after cleanup checkpoint",
                        4,
                    ));
                }
            }
            expected.insert(safe);
        }
        let mut stack = vec![archive.to_path_buf()];
        let mut count = 0usize;
        while let Some(directory) = stack.pop() {
            for entry in fs::read_dir(&directory)? {
                control.check()?;
                let entry = entry?;
                storage::guard_path(archive, &entry.path())?;
                count += 1;
                if count > 32768 {
                    return Err(ToolkitError::invalid("cleanup inventory exceeds budget"));
                }
                let path = entry.path();
                let relative = path
                    .strip_prefix(archive)
                    .map_err(|_| ToolkitError::invalid("cleanup path escaped archive"))?;
                if entry.file_type()?.is_dir() {
                    stack.push(path);
                } else if !expected.contains(relative) && !relative.starts_with(".lycheedev-locks")
                {
                    return Err(ToolkitError::new(
                        "delivery.installation_conflict",
                        "unowned file appeared in automatic recovery archive",
                        4,
                    ));
                }
            }
        }
        // The canonical archive was checked inside its exact owned namespace above.
        control.check()?;
        fs::remove_dir_all(archive)?;
    }
    control.check()?;
    fs::remove_file(marker)?;
    Ok(())
}

#[cfg(test)]
mod tests;
