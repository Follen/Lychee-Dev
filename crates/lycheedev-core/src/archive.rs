//! Explicit whole-directory legacy isolation. Marker names are observed; legacy file contents are never read.
use crate::{
    Result, ToolkitError,
    control::OperationControl,
    selection, storage,
    workspace::{self, InitControl, InitPhase},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::Read,
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
fn state(root: &Path, control: InitControl<'_>) -> Result<&'static str> {
    let result = (|| {
        control.check()?;
        if !root.exists() {
            return Ok("absent");
        }
        storage::guard_path_with_check(root, root, || control.check())?;
        control.check()?;
        if !root.is_dir() {
            return Err(ToolkitError::new(
                "vault.unsupported_workspace",
                "workspace root is not an ordinary directory",
                4,
            ));
        }
        control.check()?;
        if root.join("workspace.json").exists() {
            control.open(root)?;
            return Ok("workspace");
        }
        control.check()?;
        let mut entries = fs::read_dir(root)?;
        control.check()?;
        let occupied = entries.next().transpose()?.is_some();
        Ok(if occupied { "legacy" } else { "empty" })
    })();
    control.finish(result)
}
fn read_plan(root: &Path, control: InitControl<'_>) -> Result<Value> {
    control.check()?;
    let path = journal_path(root)?;
    storage::guard_path_with_check(path.parent().unwrap(), &path, || control.check())?;
    control.check()?;
    let mut file = fs::File::open(&path)?;
    let result = (|| {
        control.check()?;
        if file.metadata()?.len() > 65536 {
            return Err(ToolkitError::new(
                "vault.archive_state",
                "archive journal exceeds 64 KiB",
                4,
            ));
        }
        let mut bytes = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            control.check()?;
            let capacity = buffer.len().min(65537 - bytes.len());
            let n = file.read(&mut buffer[..capacity])?;
            bytes.extend_from_slice(&buffer[..n]);
            control.check()?;
            if bytes.len() > 65536 {
                return Err(ToolkitError::new(
                    "vault.archive_state",
                    "archive journal exceeds 64 KiB",
                    4,
                ));
            }
            if n == 0 {
                break;
            }
        }
        let plan: Value = serde_json::from_slice(&bytes)?;
        control.check()?;
        Ok(plan)
    })();
    let plan = match control.0 {
        Some(original) => storage::finish_file(file, result, original),
        None => {
            selection::selection_finish(result, storage::close_file(file).map_err(ToolkitError::io))
        }
    }?;
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
    control.finish(Ok(plan))
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchivePhase {
    InspectJournal,
    InspectRoot,
    SelectArchive,
    PlanReady,
    ParentPrepared,
    LeaseAcquired,
    JournalWriting,
    JournalRecorded,
    Move,
    Moved,
    MovedJournalWriting,
    MovedJournalRecorded,
    Workspace(InitPhase),
    WorkspaceReady,
    CompleteJournalWriting,
    CompleteJournalRecorded,
    LeaseClosed,
}
fn checkpoint(
    phase: ArchivePhase,
    path: &Path,
    control: InitControl<'_>,
    observe: &mut impl FnMut(ArchivePhase, &Path) -> Result<()>,
) -> Result<()> {
    control.finish(observe(phase, path))
}
pub fn plan_controlled(root: &Path, control: &OperationControl) -> Result<Value> {
    plan_observed(root, InitControl(Some(control)), &mut |_, _| Ok(()))
}
fn plan_observed(
    root: &Path,
    control: InitControl<'_>,
    observe: &mut impl FnMut(ArchivePhase, &Path) -> Result<()>,
) -> Result<Value> {
    let result = (|| {
        control.check()?;
        let root = absolute(root)?;
        let journal = journal_path(&root)?;
        checkpoint(ArchivePhase::InspectJournal, &journal, control, observe)?;
        if journal.exists() {
            let plan = read_plan(&root, control)?;
            checkpoint(ArchivePhase::PlanReady, &root, control, observe)?;
            return Ok(plan);
        }
        checkpoint(ArchivePhase::InspectRoot, &root, control, observe)?;
        let current = state(&root, control)?;
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
                checkpoint(ArchivePhase::SelectArchive, &root, control, observe)?;
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
            let mut markers = Vec::new();
            for name in [
                "config.json",
                "state",
                "cache",
                "profiles",
                "storage",
                "task-registry.json",
            ] {
                control.check()?;
                if root.join(name).exists() {
                    markers.push(name);
                }
            }
            result["markers"] = json!(markers);
        }
        checkpoint(ArchivePhase::PlanReady, &root, control, observe)?;
        Ok(result)
    })();
    control.finish(result)
}
pub fn fresh_controlled(root: &Path, control: &OperationControl) -> Result<Value> {
    archive_observed(root, false, InitControl(Some(control)), &mut |_, _| Ok(()))
}
/// Resume only the recorded switch. Missing intent must never create either
/// its workspace, parent, or lease directory.
pub fn resume_controlled(root: &Path, control: &OperationControl) -> Result<Value> {
    archive_observed(root, true, InitControl(Some(control)), &mut |_, _| Ok(()))
}
fn read_resume_plan(root: &Path, control: InitControl<'_>) -> Result<Value> {
    control.check()?;
    let journal = journal_path(root)?;
    match fs::symlink_metadata(&journal) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(ToolkitError::new(
                "vault.archive_state",
                format!(
                    "vault.archive_state: no archive journal for {}",
                    root.display()
                ),
                3,
            ));
        }
        Err(error) => return Err(error.into()),
    }
    read_plan(root, control)
}
fn archive_observed(
    root: &Path,
    resume_only: bool,
    control: InitControl<'_>,
    observe: &mut impl FnMut(ArchivePhase, &Path) -> Result<()>,
) -> Result<Value> {
    control.check()?;
    let root = absolute(root)?;
    if resume_only {
        read_resume_plan(&root, control)?;
    }
    control.check()?;
    control.finish(fs::create_dir_all(root.parent().unwrap()).map_err(ToolkitError::from))?;
    checkpoint(ArchivePhase::ParentPrepared, &root, control, observe)?;
    let guard = control.lock(
        &root.parent().unwrap().join(".lycheedev-locks"),
        &format!("initialize:{}", root.file_name().unwrap().to_string_lossy()),
    )?;
    let mut reported_plan = None;
    let mut summary = None;
    let mut journal_state = None;
    let mut archive_moved = false;
    let result = (|| {
        checkpoint(ArchivePhase::LeaseAcquired, &root, control, observe)?;
        let journal = journal_path(&root)?;
        // Fresh planning stays inside the root lease. Resume rereads mandatory
        // intent here, never substituting a newly generated plan.
        let mut plan = if resume_only {
            read_resume_plan(&root, control)?
        } else {
            plan_observed(&root, control, observe)?
        };
        reported_plan = Some(plan.clone());
        control.check()?;
        if journal.exists() {
            journal_state = plan["state"].as_str().map(str::to_owned);
        }
        archive_moved = plan.get("archivePath").is_some()
            && matches!(plan["state"].as_str(), Some("moved" | "complete"));
        if plan["state"] == "complete" {
            let identity = control.open(&root)?;
            summary = Some(json!({"root":root,"identity":identity}));
            checkpoint(ArchivePhase::WorkspaceReady, &root, control, observe)?;
            return Ok(json!({"plan":plan,"summary":summary}));
        }
        if let Some(archive) = plan["archivePath"].as_str().map(PathBuf::from) {
            control.check()?;
            if !journal.exists() {
                checkpoint(ArchivePhase::JournalWriting, &journal, control, observe)?;
                control.write(&journal, &plan)?;
                journal_state = Some("planned".to_owned());
                checkpoint(ArchivePhase::JournalRecorded, &journal, control, observe)?;
            }
            if plan["state"] == "planned" {
                match state(&root, control)? {
                    "legacy" | "empty" => {
                        control.check()?;
                        if archive.exists() {
                            return Err(ToolkitError::new(
                                "vault.archive_state",
                                "archive destination appeared before move",
                                4,
                            ));
                        }
                        checkpoint(ArchivePhase::Move, &root, control, observe)?;
                        fs::rename(&root, &archive)?;
                        archive_moved = true;
                        checkpoint(ArchivePhase::Moved, &archive, control, observe)?;
                    }
                    "absent" => {
                        control.check()?;
                        if !archive.is_dir() {
                            return Err(ToolkitError::new(
                                "vault.archive_state",
                                "selected root and expected archive are both missing",
                                4,
                            ));
                        }
                        archive_moved = true;
                        control.check()?;
                    }
                    _ => {
                        return Err(ToolkitError::new(
                            "vault.archive_state",
                            "selected root changed to another workspace",
                            4,
                        ));
                    }
                }
                let mut moved = plan.clone();
                moved["state"] = json!("moved");
                checkpoint(
                    ArchivePhase::MovedJournalWriting,
                    &journal,
                    control,
                    observe,
                )?;
                control.write(&journal, &moved)?;
                plan = moved;
                reported_plan = Some(plan.clone());
                journal_state = Some("moved".to_owned());
                checkpoint(
                    ArchivePhase::MovedJournalRecorded,
                    &journal,
                    control,
                    observe,
                )?;
            }
        }
        let initialized = workspace::initialize_unlocked_observed(
            &root,
            control,
            &mut |phase, path| observe(ArchivePhase::Workspace(phase), path),
            |stage| fs::remove_dir_all(stage).map_err(ToolkitError::from),
        );
        summary = workspace::initialization_result(&root, &initialized);
        let identity = initialized?;
        summary = Some(json!({"root":root,"identity":identity}));
        checkpoint(ArchivePhase::WorkspaceReady, &root, control, observe)?;
        let mut complete = plan;
        complete["state"] = json!("complete");
        complete["completedAt"] =
            json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true));
        if complete.get("archivePath").is_some() {
            checkpoint(
                ArchivePhase::CompleteJournalWriting,
                &journal,
                control,
                observe,
            )?;
            control.write(&journal, &complete)?;
            journal_state = Some("complete".to_owned());
        }
        reported_plan = Some(complete.clone());
        checkpoint(
            ArchivePhase::CompleteJournalRecorded,
            &journal,
            control,
            observe,
        )?;
        Ok(json!({"plan":complete,"summary":summary}))
    })();
    let result = selection::selection_finish(result, guard.close());
    let result = selection::selection_finish(
        result,
        checkpoint(ArchivePhase::LeaseClosed, &root, control, observe),
    );
    result.map_err(|error| {
        let mut combined = error.clone();
        combined.details = json!({
            "schema":"lycheedev.archive-operation-error.v1",
            "primary":error,
            "result":{
                "plan":reported_plan,"summary":summary,"complete":false,
                "archiveMoved":archive_moved,"journalState":journal_state,
            },
        });
        combined
    })
}
#[cfg(test)]
#[path = "archive_control_tests.rs"]
mod control_tests;
