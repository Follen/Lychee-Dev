use crate::{
    installation::{self, Receipt},
    release,
};
use lycheedev_core::{
    Result, ToolkitError,
    storage::{self, ExclusiveLock},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub fn destination(path: &Path) -> Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| ToolkitError::invalid("installation requires parent"))?;
    let parent = fs::canonicalize(parent)?;
    let name = path
        .file_name()
        .ok_or_else(|| ToolkitError::invalid("installation requires basename"))?;
    let target = parent.join(name);
    storage::guard_path(&parent, &target)?;
    Ok(if target.exists() {
        fs::canonicalize(target)?
    } else {
        target
    })
}
pub fn installation_lock(target: &Path) -> Result<ExclusiveLock> {
    let target = destination(target)?;
    let parent = target.parent().unwrap();
    ExclusiveLock::resource(
        &parent.join(".lycheedev-locks"),
        &format!(
            "installation:{}",
            target
                .to_string_lossy()
                .trim_start_matches("\\\\?\\")
                .to_ascii_lowercase()
        ),
    )
}
pub fn outside(parent: &Path, archive: &Path) -> Result<()> {
    if archive
        .to_string_lossy()
        .to_ascii_lowercase()
        .starts_with(&format!(
            "{}\\",
            parent.to_string_lossy().to_ascii_lowercase()
        ))
        || archive == parent
    {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "archive must be outside discovery parent",
            4,
        ));
    }
    Ok(())
}
pub fn fresh(root: &Path, target: &Path, component: &str, version: &str) -> Result<Receipt> {
    let release = release::inspect(root, version)?;
    let wanted = release::receipt(&release, component);
    let target = destination(target)?;
    let _lock = installation_lock(&target)?;
    let current = installation::inspect(&target, component)?;
    if current["state"] == "managed" && current["receipt"] == serde_json::to_value(&wanted)? {
        return Ok(wanted);
    }
    if current["state"] != "absent" {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "existing target requires explicit managed upgrade",
            4,
        ));
    }
    let parent = target.parent().unwrap();
    let stage = parent.join(format!(
        ".lycheedev-stage-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    fs::create_dir(&stage)?;
    let result = (|| {
        for resource in &wanted.resources {
            let relative = resource
                .path
                .strip_prefix(&format!("{component}/"))
                .unwrap();
            let source = storage::safe_relative(&root.join("payload"), &resource.path)?;
            storage::guard_path(&root.join("payload"), &source)?;
            let bytes = fs::read(source)?;
            if bytes.len() as u64 != resource.bytes || storage::sha256(&bytes) != resource.sha256 {
                return Err(ToolkitError::new(
                    "delivery.invalid_payload",
                    "release changed during staging",
                    4,
                ));
            }
            storage::atomic_write(&storage::safe_relative(&stage, relative)?, &bytes)?;
        }
        storage::atomic_json(&stage.join(installation::MARKER), &wanted)?;
        release::verify_inventory(&stage, &wanted.resources, Some(component))?;
        if target.exists() {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "target appeared during staging",
                4,
            ));
        }
        fs::rename(&stage, &target)?;
        Ok(wanted)
    })();
    if stage.exists() {
        let _ = fs::remove_dir_all(&stage);
    }
    result
}
pub fn upgrade(
    root: Option<&Path>,
    target: &Path,
    archive: &Path,
    component: &str,
    version: &str,
    resume: bool,
) -> Result<Value> {
    let target = destination(target)?;
    let archive = destination(archive)?;
    outside(target.parent().unwrap(), &archive)?;
    let _lock = installation_lock(&target)?;
    let record_path = archive.join("replacement.json");
    if !resume {
        let current = installation::inspect(&target, component)?;
        if current["state"] != "managed" {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "upgrade requires unchanged managed installation",
                4,
            ));
        }
        fs::create_dir(&archive)?;
        let next = fresh(
            root.ok_or_else(|| ToolkitError::arguments("upgrade requires --release"))?,
            &archive.join("next"),
            component,
            version,
        )?;
        storage::atomic_json(
            &record_path,
            &json!({"schema":"lycheedev.replacement.v1","target":target,"previous":current["receipt"],"next":next}),
        )?;
    }
    storage::guard_path(&archive, &record_path)?;
    let record: Value = storage::read_json(&record_path)?;
    if record["schema"] != "lycheedev.replacement.v1"
        || record["target"] != serde_json::to_value(&target)?
        || record["previous"]["component"] != component
        || record["next"]["component"] != component
    {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "replacement identity differs from explicit target",
            4,
        ));
    }
    let mut current = installation::inspect(&target, component)?;
    let previous = installation::inspect(&archive.join("previous"), component)?;
    let next = installation::inspect(&archive.join("next"), component)?;
    if previous["state"] == "managed"
        && previous["receipt"] == record["previous"]
        && current["state"] == "managed"
        && current["receipt"] == record["next"]
        && next["state"] == "absent"
    {
        return Ok(json!({"target":target,"archive":archive,"receipt":record["next"]}));
    }
    if next["state"] != "managed" || next["receipt"] != record["next"] {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "prepared replacement changed",
            4,
        ));
    }
    let mut old_valid = previous["state"] == "managed" && previous["receipt"] == record["previous"];
    if previous["state"] == "absent"
        && current["state"] == "managed"
        && current["receipt"] == record["previous"]
    {
        fs::rename(&target, archive.join("previous"))?;
        old_valid = true;
        current["state"] = json!("absent");
    }
    if !old_valid || current["state"] != "absent" {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "replacement state is ambiguous",
            4,
        ));
    }
    fs::rename(archive.join("next"), &target)?;
    Ok(json!({"target":target,"archive":archive,"receipt":record["next"]}))
}
pub fn remove(target: &Path, archive: &Path, component: &str) -> Result<Value> {
    let target = destination(target)?;
    let archive = destination(archive)?;
    outside(target.parent().unwrap(), &archive)?;
    let _lock = installation_lock(&target)?;
    let assessment = installation::inspect(&target, component)?;
    if assessment["state"] == "absent" {
        return Ok(json!({"state":"absent"}));
    }
    if assessment["state"] != "managed" {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "only unchanged owned installation can be archived",
            4,
        ));
    }
    if archive.exists() {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "recovery destination exists",
            4,
        ));
    }
    fs::rename(target, &archive)?;
    Ok(json!({"state":"archived","archive":archive,"receipt":assessment["receipt"]}))
}
