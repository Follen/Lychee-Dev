use crate::installation::{Receipt, Resource};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, selection::valid_digest, storage,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Release {
    pub schema: String,
    pub version: String,
    pub commit: String,
    pub binaries: BTreeMap<String, Resource>,
    pub resources: Vec<Resource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm_channel: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corresponding_source: Option<Value>,
}
pub fn inspect_with_control(
    root: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<Release> {
    let release = read_manifest_with_control(root, version, control)?;
    verify_inventory_with_control(&root.join("payload"), &release.resources, None, control)?;
    Ok(release)
}
/// Identity/status composition may read the sealed manifest without starting tools.
/// Execution/install callers must admit the complete inventory with inspect_with_control.
pub fn read_manifest_with_control(
    root: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<Release> {
    read_manifest_with_finish(root, version, control, |file, result| {
        storage::finish_file(file, result, control)
    })
}
fn read_manifest_with_finish(
    root: &Path,
    version: &str,
    control: &OperationControl,
    finish: impl FnOnce(fs::File, Result<Release>) -> Result<Release>,
) -> Result<Release> {
    control.check()?;
    storage::guard_path(root, &root.join("release.json"))?;
    let mut file = read_locked(&root.join("release.json"))?;
    // Keep manifest ownership throughout parsing and every identity check.
    let result = (|| {
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() > 1 << 20 {
            return Err(ToolkitError::new(
                "delivery.invalid_release",
                "release manifest exceeds 1 MiB",
                4,
            ));
        }
        let mut raw = Vec::new();
        (&mut file).take((1 << 20) + 1).read_to_end(&mut raw)?;
        if raw.len() as u64 != metadata.len() {
            return Err(ToolkitError::invalid(
                "release manifest changed during bounded read",
            ));
        }
        control.check()?;
        let release: Release = serde_json::from_slice(&raw)?;
        if release.runtime.is_some() != release.npm_channel.is_some()
            || release
                .runtime
                .as_deref()
                .is_some_and(|runtime| runtime != "rust")
            || release.npm_channel.as_ref().is_some_and(|channel| {
                !crate::npm_update::valid_channel_binding(channel, &release.version)
            })
        {
            return Err(ToolkitError::new(
                "delivery.invalid_release",
                "runtime/channel binding differs from the owner release contract",
                4,
            ));
        }
        if release.schema != "lycheedev.release.v1"
            || release.version != version
            || !valid_digest(&release.commit, 40)
        {
            return Err(ToolkitError::new(
                "delivery.invalid_release",
                "release identity or version mismatch",
                4,
            ));
        }
        if release.binaries.len() != 1 || !release.binaries.contains_key("windows-amd64") {
            return Err(ToolkitError::new(
                "delivery.invalid_release",
                "product requires exactly Windows amd64 binary",
                4,
            ));
        }
        let binary = &release.binaries["windows-amd64"];
        if binary.path != "native/windows-amd64/lycheedev.exe"
            || binary.bytes == 0
            || !valid_digest(&binary.sha256, 64)
        {
            return Err(ToolkitError::new(
                "delivery.invalid_release",
                "invalid binary manifest",
                4,
            ));
        }
        if let Some(source) = &release.corresponding_source
            && (source["commit"] != release.commit
                || !valid_digest(source["sha256"].as_str().unwrap_or(""), 64)
                || source["archive"]
                    .as_str()
                    .is_none_or(|s| s.contains(['/', '\\'])))
        {
            return Err(ToolkitError::new(
                "delivery.invalid_release",
                "corresponding source identity differs",
                4,
            ));
        }
        if !release.resources.iter().any(|r| r.path == "skill/SKILL.md")
            || !release
                .resources
                .iter()
                .any(|r| r.path == "addon/Lychee Dev.toc")
        {
            return Err(ToolkitError::new(
                "delivery.invalid_release",
                "missing mandatory addon/skill",
                4,
            ));
        }
        Ok(release)
    })();
    finish(file, result)
}
pub fn receipt(release: &Release, component: &str) -> Receipt {
    Receipt {
        schema: "lycheedev.installation.v1".into(),
        component: component.into(),
        version: release.version.clone(),
        commit: release.commit.clone(),
        resources: release
            .resources
            .iter()
            .filter(|r| r.path.starts_with(&format!("{component}/")))
            .cloned()
            .collect(),
    }
}
pub fn verify_inventory_with_control(
    root: &Path,
    resources: &[Resource],
    component: Option<&str>,
    control: &OperationControl,
) -> Result<()> {
    verify_inventory_with_finish(root, resources, component, control, |file, result| {
        let result = storage::finish_file(file, result, control);
        #[cfg(test)]
        let result = crate::status_control_tests::after_inventory_finish(result, control);
        result
    })
}
fn verify_inventory_with_finish(
    root: &Path,
    resources: &[Resource],
    component: Option<&str>,
    control: &OperationControl,
    mut finish: impl FnMut(fs::File, Result<()>) -> Result<()>,
) -> Result<()> {
    if resources.is_empty() || resources.len() > 32768 {
        return Err(ToolkitError::new(
            "delivery.invalid_payload",
            "inventory must contain 1..32768 files",
            4,
        ));
    }
    let mut names = BTreeSet::new();
    let mut total = 0u64;
    for resource in resources {
        control.check()?;
        total = total
            .checked_add(resource.bytes)
            .ok_or_else(|| ToolkitError::invalid("payload length overflow"))?;
        if total > 256 << 20 || !valid_digest(&resource.sha256, 64) {
            return Err(ToolkitError::new(
                "delivery.invalid_payload",
                "payload byte budget or digest invalid",
                4,
            ));
        }
        let relative = if let Some(component) = component {
            resource
                .path
                .strip_prefix(&format!("{component}/"))
                .ok_or_else(|| ToolkitError::invalid("resource belongs to another component"))?
        } else {
            let prefix = resource.path.split('/').next().unwrap_or("");
            if !["addon", "skill", "tool"].contains(&prefix) {
                return Err(ToolkitError::invalid("payload root is not whitelisted"));
            }
            resource.path.as_str()
        };
        if relative.contains('\\') || !names.insert(relative.to_ascii_lowercase()) {
            return Err(ToolkitError::invalid(
                "duplicate or noncanonical inventory path",
            ));
        }
        let path = storage::safe_relative(root, relative)?;
        storage::guard_path(root, &path)?;
        let (file, digest) = storage::locked_sha256(&path, resource.bytes, control)?;
        // Keep the same file locked through the final shape/digest decisions.
        // Every normal result, including rejection/cancellation, consumes it.
        let result = (|| {
            let info = file.metadata()?;
            if !info.is_file() || info.len() != resource.bytes {
                return Err(ToolkitError::new(
                    "delivery.invalid_payload",
                    format!("file shape differs: {relative}"),
                    4,
                ));
            }
            if digest != resource.sha256 {
                return Err(ToolkitError::new(
                    "delivery.invalid_payload",
                    format!("digest differs: {relative}"),
                    4,
                ));
            }
            Ok(())
        })();
        finish(file, result)?;
    }
    let mut stack = vec![root.to_path_buf()];
    let mut count = 0usize;
    let mut entries = 0usize;
    while let Some(directory) = stack.pop() {
        for entry in fs::read_dir(directory)? {
            control.check()?;
            entries += 1;
            if entries > 65536 {
                return Err(ToolkitError::invalid(
                    "payload directory scan exceeds bound",
                ));
            }
            let entry = entry?;
            storage::guard_path(root, &entry.path())?;
            if entry.file_type()?.is_dir() {
                stack.push(entry.path());
            } else {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| ToolkitError::invalid("payload escapes root"))?
                    .to_string_lossy()
                    .replace('\\', "/");
                if component.is_some() && relative == crate::installation::MARKER {
                    continue;
                }
                if !entry.file_type()?.is_file() || !names.contains(&relative.to_ascii_lowercase())
                {
                    return Err(ToolkitError::new(
                        "delivery.invalid_payload",
                        format!("unlisted file: {relative}"),
                        4,
                    ));
                }
                count += 1;
                if count > 32768 {
                    return Err(ToolkitError::invalid("file scan exceeded inventory budget"));
                }
            }
        }
    }
    Ok(())
}
fn read_locked(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1);
    }
    Ok(options.open(path)?)
}

#[cfg(test)]
#[path = "release/checked_close_tests.rs"]
mod checked_close_tests;
