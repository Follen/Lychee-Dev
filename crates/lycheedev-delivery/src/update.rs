//! Managed resource update decisions and recovery live here, not in the npm launcher.
use crate::{deploy, installation, release, slots};
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError, VERSION,
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
fn read_targets(state: &Path) -> Result<Vec<UpdateTarget>> {
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
    let values: Vec<UpdateTarget> = storage::read_json(&path)?;
    if values.len() > 64 {
        return Err(ToolkitError::invalid("update supports at most 64 targets"));
    }
    Ok(values)
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let state = state_path(&inv.home);
    let paths = inv.flag_values("--path");
    let installations = inv.flag_values("--installation");
    let mut targets = if let Some(file) = inv.flag("--file") {
        if !paths.is_empty() || !installations.is_empty() {
            return Err(ToolkitError::arguments(
                "update --file cannot be combined with target selectors",
            ));
        }
        if fs::metadata(file)?.len() > 64 << 10 {
            return Err(ToolkitError::arguments("update target file exceeds 64 KiB"));
        }
        let values: Vec<UpdateTarget> = storage::read_json(Path::new(file))?;
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
        targets = read_targets(&state)?;
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
    control.check()?;
    let release = release::inspect(root, version)?;
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
            guard.require_idle()?;
            slots::preflight(path.parent().unwrap(), version)?;
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
        let current = installation::inspect(&path, &target.component)?;
        if !["managed", "absent"].contains(&current["state"].as_str().unwrap_or("")) {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "one selected target is unmanaged or modified; no targets changed",
                4,
            )
            .with_details(json!({"path":path,"assessment":current})));
        }
        let recovery = path
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
        let toc = fs::read_to_string(root.join("payload/addon/Lychee Dev.toc"))?;
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
    if plan {
        return Ok(Outcome::new(result));
    }
    fs::create_dir_all(state)?;
    storage::guard_path(state, state)?;
    let _lock = ExclusiveLock::resource(state, "managed-update")?;
    let mut remembered = read_targets(state)?;
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
    storage::atomic_json(&state.join("targets.json"), &remembered)?;
    for (index, (component, path, recovery, item)) in items.iter().enumerate() {
        control.check()?;
        let _maintenance = if component == "addon" {
            let guard = InstallationGuard::acquire(path)?;
            guard.require_idle()?;
            Some(guard)
        } else {
            None
        };
        let current = installation::inspect(path, component)?;
        let wanted = serde_json::to_value(release::receipt(&release, component))?;
        let mutation = (|| -> Result<()> {
            if component == "addon" {
                if cleanup_marker(recovery).exists() {
                    cleanup_replacement(path, recovery, &wanted)?;
                }
                let archive = if recovery.exists()
                    || current["state"] == "managed" && current["receipt"] != wanted
                {
                    Some(recovery.as_path())
                } else {
                    None
                };
                if let Some(archive) = archive {
                    fs::create_dir_all(archive.parent().unwrap())?;
                }
                crate::addon_transaction::apply(
                    path,
                    crate::addon_transaction::Request {
                        release: Some(root),
                        archive,
                        version,
                        resume: false,
                        control,
                    },
                    _maintenance.as_ref().expect("addon maintenance guard"),
                )?;
                if recovery.exists() || cleanup_marker(recovery).exists() {
                    cleanup_replacement(path, recovery, &wanted)?;
                }
                return Ok(());
            }
            if cleanup_marker(recovery).exists() {
                cleanup_replacement(path, recovery, &wanted)?;
            } else if recovery.exists() {
                deploy::upgrade(None, path, recovery, component, version, true)?;
            } else if current["state"] == "absent" {
                deploy::fresh(root, path, component, version)?;
            } else if current["state"] == "managed" && current["receipt"] == wanted {
            } else {
                fs::create_dir_all(recovery.parent().unwrap())?;
                deploy::upgrade(Some(root), path, recovery, component, version, false)?;
            }
            if recovery.exists() || cleanup_marker(recovery).exists() {
                cleanup_replacement(path, recovery, &wanted)?;
            }
            Ok(())
        })();
        if let Err(error) = mutation {
            result["targets"][index]["state"] = json!("pending");
            return Err(
                ToolkitError::new(error.code, error.message, error.exit_code)
                    .with_details(json!({"result":result})),
            );
        }
        result["targets"][index] = item.clone();
        result["targets"][index]["state"] = json!("current");
        if recovery.exists() {
            result["targets"][index]["recoveryRetained"] = json!(true);
        } else {
            result["targets"][index]
                .as_object_mut()
                .unwrap()
                .remove("recovery");
        }
        result["targets"][index]["activation"] = json!(if component == "addon" {
            "reload_required"
        } else {
            "new_agent_context_required"
        });
    }
    result["complete"] = json!(true);
    Ok(Outcome::new(result))
}
fn cleanup_marker(archive: &Path) -> PathBuf {
    let mut name = archive.as_os_str().to_os_string();
    name.push(".cleanup.json");
    PathBuf::from(name)
}
fn cleanup_replacement(target: &Path, archive: &Path, wanted: &Value) -> Result<()> {
    let _guard = deploy::installation_lock(target)?;
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
    let assessment = installation::inspect(target, wanted["component"].as_str().unwrap_or(""))?;
    if assessment["state"] != "managed" || assessment["receipt"] != *wanted {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "installed bytes changed before old-tree cleanup",
            4,
        ));
    }
    let marker = cleanup_marker(archive);
    let record: Value = if marker.exists() {
        storage::read_json(&marker)?
    } else {
        let record: Value = storage::read_json(&archive.join("replacement.json"))?;
        if record["schema"] != "lycheedev.replacement.v1"
            || record["target"] != serde_json::to_value(target)?
            || record["next"] != *wanted
        {
            return Err(ToolkitError::invalid(
                "cleanup replacement identity differs",
            ));
        }
        let previous = installation::inspect(
            &archive.join("previous"),
            wanted["component"].as_str().unwrap_or(""),
        )?;
        if previous["state"] != "managed" || previous["receipt"] != record["previous"] {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "old owned bytes changed before cleanup intent",
                4,
            ));
        }
        storage::atomic_json(&marker, &record)?;
        record
    };
    if record["schema"] != "lycheedev.replacement.v1"
        || record["target"] != serde_json::to_value(target)?
        || record["next"] != *wanted
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
                let bytes = fs::read(&existing)?;
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
        fs::remove_dir_all(archive)?;
    }
    fs::remove_file(marker)?;
    Ok(())
}
