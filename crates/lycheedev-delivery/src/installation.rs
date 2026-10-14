use lycheedev_core::{Result, ToolkitError, selection::valid_digest, storage};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub const MARKER: &str = ".lycheedev-install.json";
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub component: String,
    pub version: String,
    pub commit: String,
    pub resources: Vec<Resource>,
}
#[derive(Debug, Clone)]
pub struct VerifiedInstallation {
    pub canonical_path: PathBuf,
    pub manifest_sha256: String,
    pub generation: u64,
}
pub fn inspect(path: &Path, component: &str) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({"state":"absent"}));
    }
    if fs::symlink_metadata(path)?.file_type().is_symlink() || !path.is_dir() {
        return Ok(json!({"state":"unmanaged","detail":"target is not an ordinary directory"}));
    }
    let marker = path.join(MARKER);
    if !marker.exists() {
        return Ok(json!({"state":"unmanaged"}));
    }
    let raw = fs::read(&marker)?;
    if raw.len() > 1_048_576 {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "receipt exceeds 1 MiB",
            4,
        ));
    }
    let receipt: Receipt = serde_json::from_slice(&raw)?;
    if receipt.schema != "lycheedev.installation.v1"
        || receipt.component != component
        || receipt.version.is_empty()
        || !valid_digest(&receipt.commit, 40)
    {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "receipt identity is invalid",
            4,
        ));
    }
    let prefix = format!("{component}/");
    let mut names = std::collections::BTreeSet::new();
    for resource in &receipt.resources {
        let relative = resource
            .path
            .strip_prefix(&prefix)
            .ok_or_else(|| ToolkitError::invalid("resource belongs to another component"))?;
        if !names.insert(relative.to_ascii_lowercase()) || !valid_digest(&resource.sha256, 64) {
            return Err(ToolkitError::invalid("duplicate or invalid resource"));
        }
        let file = storage::safe_relative(path, relative)?;
        if !file.exists() {
            return Ok(
                json!({"state":"modified","receipt":receipt,"detail":format!("missing file {relative}")}),
            );
        }
        storage::guard_path(path, &file)?;
        let bytes = fs::read(file)?;
        if bytes.len() as u64 != resource.bytes || storage::sha256(&bytes) != resource.sha256 {
            return Ok(
                json!({"state":"modified","receipt":receipt,"detail":format!("modified file {relative}")}),
            );
        }
    }
    if let Err(error) = crate::release::verify_inventory(path, &receipt.resources, Some(component))
    {
        return Ok(json!({"state":"modified","receipt":receipt,"detail":error.message}));
    }
    Ok(json!({"state":"managed","receipt":receipt}))
}
pub fn verify_clean(
    path: &Path,
    release: &str,
    protocol: &str,
    slots: u16,
) -> Result<VerifiedInstallation> {
    let result = inspect(path, "addon")?;
    if result["state"] != "managed" || result["receipt"]["version"] != release {
        return Err(ToolkitError::new(
            "delivery.installation_not_clean",
            "managed files/version must match",
            3,
        ));
    }
    if protocol != crate::slots::TRANSPORT || slots != crate::slots::COUNT {
        return Err(ToolkitError::new(
            "delivery.protocol_mismatch",
            "Live requires Lycheedev mailbox protocol v1 with 500 slots",
            3,
        ));
    }
    let manifest = path
        .parent()
        .ok_or_else(|| ToolkitError::invalid("addon has no parent"))?
        .join(".lycheedev-slots.json");
    let pool: Value = storage::read_json(&manifest)?;
    crate::slots::inspect(path.parent().unwrap(), release)?;
    if pool["schema"] != "lycheedev.slots.v4"
        || pool["transport"] != protocol
        || pool["slotCount"] != slots
    {
        return Err(ToolkitError::new(
            "delivery.slot_inventory_mismatch",
            "slot inventory requires explicit managed upgrade",
            3,
        ));
    }
    Ok(VerifiedInstallation {
        canonical_path: fs::canonicalize(path)?,
        manifest_sha256: storage::sha256(&fs::read(path.join(MARKER))?),
        generation: pool["generation"].as_u64().unwrap_or(1),
    })
}
