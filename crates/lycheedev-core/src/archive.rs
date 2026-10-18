//! Explicit whole-directory legacy isolation. Marker names are observed; legacy file contents are never read.
use crate::{
    Result, ToolkitError,
    storage::{self, ExclusiveLock},
    workspace,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
mod inspection;
pub(crate) use inspection::journal_controlled;
fn absolute(path: &Path) -> Result<PathBuf> {
    let raw = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut clean = PathBuf::new();
    for part in raw.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !clean.pop() {
                    return Err(ToolkitError::invalid("archive root escapes filesystem"));
                }
            }
            other => clean.push(other.as_os_str()),
        }
    }
    if clean.parent().is_none() {
        return Err(ToolkitError::invalid(
            "workspace cannot be a filesystem root",
        ));
    }
    Ok(clean)
}
pub fn journal_path(root: &Path) -> Result<PathBuf> {
    let root = absolute(root)?;
    let mut name = root
        .file_name()
        .ok_or_else(|| ToolkitError::invalid("workspace root has no name"))?
        .to_os_string();
    name.push(".archive-state.json");
    Ok(root.parent().unwrap().join(name))
}
fn state(root: &Path) -> Result<&'static str> {
    if !root.exists() {
        return Ok("absent");
    }
    storage::guard_path(root, root)?;
    if !root.is_dir() {
        return Err(ToolkitError::new(
            "vault.unsupported_workspace",
            "workspace root is not an ordinary directory",
            4,
        ));
    }
    if root.join("workspace.json").exists() {
        workspace::open(root)?;
        return Ok("workspace");
    }
    if fs::read_dir(root)?.next().is_none() {
        Ok("empty")
    } else {
        Ok("legacy")
    }
}
fn read_plan(root: &Path) -> Result<Value> {
    let path = journal_path(root)?;
    storage::guard_path(path.parent().unwrap(), &path)?;
    if fs::metadata(&path)?.len() > 65536 {
        return Err(ToolkitError::new(
            "vault.archive_state",
            "archive journal exceeds 64 KiB",
            4,
        ));
    }
    let plan: Value = storage::read_json(&path)?;
    let root = absolute(root)?;
    if plan["schema"] != "lycheedev.archive.v1"
        || plan["root"] != serde_json::to_value(&root)?
        || plan["journalPath"] != serde_json::to_value(&path)?
        || !["planned", "moved", "complete"].contains(&plan["state"].as_str().unwrap_or(""))
    {
        return Err(ToolkitError::new(
            "vault.archive_state",
            "archive journal identity or state is invalid",
            4,
        ));
    }
    if let Some(archive) = plan["archivePath"].as_str() {
        let archive = Path::new(archive);
        let prefix = format!("{}-legacy-", root.file_name().unwrap().to_string_lossy());
        if archive.parent() != root.parent()
            || archive
                .file_name()
                .is_none_or(|name| !name.to_string_lossy().starts_with(&prefix))
            || archive == root
        {
            return Err(ToolkitError::new(
                "vault.archive_state",
                "journal archive is outside the selected sibling scope",
                4,
            ));
        }
    }
    Ok(plan)
}
pub fn plan(root: &Path) -> Result<Value> {
    let root = absolute(root)?;
    let journal = journal_path(&root)?;
    if journal.exists() {
        return read_plan(&root);
    }
    let current = state(&root)?;
    if current == "workspace" {
        return Err(ToolkitError::new(
            "vault.workspace_exists",
            "fresh init never archives an existing new-format workspace",
            4,
        ));
    }
    let now = chrono::Utc::now();
    let mut result = json!({"schema":"lycheedev.archive.v1","root":root,"journalPath":journal,"steps":[{"id":"create","detail":format!("create new-format workspace at {}",root.display())}],"state":"planned","createdAt":now.to_rfc3339_opts(chrono::SecondsFormat::Nanos,true)});
    if current == "legacy" {
        let parent = root.parent().unwrap();
        let base = root.file_name().unwrap().to_string_lossy();
        let stamp = now.format("%Y%m%dT%H%M%SZ");
        let mut selected = None;
        for attempt in 1..=100 {
            let name = if attempt == 1 {
                format!("{base}-legacy-{stamp}")
            } else {
                format!("{base}-legacy-{stamp}-{attempt}")
            };
            let path = parent.join(name);
            if !path.exists() {
                selected = Some(path);
                break;
            }
        }
        let archive = selected.ok_or_else(|| {
            ToolkitError::new(
                "vault.archive_state",
                "archive collision budget exhausted",
                4,
            )
        })?;
        result["archivePath"] = json!(archive);
        result["steps"] = json!([{"id":"journal","detail":"record fixed sibling archive intent"},{"id":"move","detail":"isolate legacy root without reading or importing contents"},{"id":"create","detail":"create new-format workspace"}]);
        let markers: [&str; 6] = [
            "config.json",
            "state",
            "cache",
            "profiles",
            "storage",
            "task-registry.json",
        ];
        result["markers"] = json!(
            markers
                .into_iter()
                .filter(|name| root.join(name).exists())
                .collect::<Vec<_>>()
        );
    }
    Ok(result)
}
pub fn fresh(root: &Path) -> Result<Value> {
    let root = absolute(root)?;
    run_archive(&root, false)
}
/// Resume only the recorded switch. Missing intent must never become a fresh
/// initialization, including creation of its parent or lock directory.
pub fn resume(root: &Path) -> Result<Value> {
    let root = absolute(root)?;
    read_resume_plan(&root)?;
    run_archive(&root, true)
}
fn read_resume_plan(root: &Path) -> Result<Value> {
    let journal = journal_path(&root)?;
    match fs::symlink_metadata(&journal) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ToolkitError::new(
                "vault.archive_state",
                format!("vault.archive_state: no archive journal for {}", root.display()),
                3,
            ));
        }
        Err(error) => return Err(error.into()),
    }
    read_plan(root)
}
fn run_archive(root: &Path, resume_only: bool) -> Result<Value> {
    fs::create_dir_all(root.parent().unwrap())?;
    let _guard = ExclusiveLock::resource(
        &root.parent().unwrap().join(".lycheedev-locks"),
        &format!("initialize:{}", root.file_name().unwrap().to_string_lossy()),
    )?;
    let journal = journal_path(&root)?;
    // Resolve fresh intent under the root lease. Resume rereads its mandatory
    // journal here and cannot substitute new intent if that journal disappeared.
    let mut plan = if resume_only { read_resume_plan(root)? } else { plan(root)? };
    if plan["state"] == "complete" {
        let identity = workspace::open(&root)?;
        return Ok(json!({"plan":plan,"summary":{"root":root,"identity":identity}}));
    }
    if let Some(archive) = plan["archivePath"].as_str().map(PathBuf::from) {
        if !journal.exists() {
            storage::atomic_json(&journal, &plan)?;
        }
        if plan["state"] == "planned" {
            match state(&root)? {
                "legacy" | "empty" => {
                    if archive.exists() {
                        return Err(ToolkitError::new(
                            "vault.archive_state",
                            "archive destination appeared before move",
                            4,
                        ));
                    }
                    fs::rename(&root, &archive)?;
                }
                "absent" => {
                    if !archive.is_dir() {
                        return Err(ToolkitError::new(
                            "vault.archive_state",
                            "selected root and expected archive are both missing",
                            4,
                        ));
                    }
                }
                _ => {
                    return Err(ToolkitError::new(
                        "vault.archive_state",
                        "selected root changed to another workspace",
                        4,
                    ));
                }
            };
            plan["state"] = json!("moved");
            storage::atomic_json(&journal, &plan)?;
        }
    }
    let identity = workspace::initialize_unlocked(&root)?;
    plan["state"] = json!("complete");
    plan["completedAt"] =
        json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
    if plan.get("archivePath").is_some() {
        storage::atomic_json(&journal, &plan)?;
    }
    Ok(json!({"plan":plan,"summary":{"root":root,"identity":identity}}))
}
