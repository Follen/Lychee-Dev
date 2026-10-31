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
}
/// Fixed Go sameReceipt ignores inventory order but never duplicate paths.
pub(crate) fn same_receipt(left: &Receipt, right: &Receipt) -> Result<bool> {
    fn resources(receipt: &Receipt) -> Result<std::collections::BTreeMap<&str, &Resource>> {
        let mut paths = std::collections::BTreeMap::new();
        let mut folded = std::collections::BTreeSet::new();
        for resource in &receipt.resources {
            if !folded.insert(resource.path.to_lowercase())
                || paths.insert(resource.path.as_str(), resource).is_some()
            {
                return Err(ToolkitError::new(
                    "delivery.invalid_installation",
                    "duplicate resource path in receipt",
                    4,
                ));
            }
        }
        Ok(paths)
    }
    let left_resources = resources(left)?;
    let right_resources = resources(right)?;
    Ok(left.schema == right.schema
        && left.component == right.component
        && left.version == right.version
        && left.commit == right.commit
        && left_resources == right_resources)
}
pub(crate) fn same_receipt_value(left: &Value, right: &Value) -> Result<bool> {
    same_receipt(
        &serde_json::from_value(left.clone())?,
        &serde_json::from_value(right.clone())?,
    )
}
pub fn verify_clean_controlled(
    path: &Path,
    release: &str,
    protocol: &str,
    slots: u16,
    control: &lycheedev_core::control::OperationControl,
) -> Result<VerifiedInstallation> {
    control.check()?;
    let result = inspect_main_controlled(path, "addon", control)?;
    control.check()?;
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
    crate::slots::inspect_controlled(
        path.parent()
            .ok_or_else(|| ToolkitError::invalid("addon has no parent"))?,
        release,
        control,
    )?;
    control.check()?;
    Ok(VerifiedInstallation {
        canonical_path: fs::canonicalize(path)?,
        manifest_sha256: storage::sha256(&crate::slots::read_bounded_controlled(
            &path.join(MARKER),
            1 << 20,
            control,
        )?),
    })
}

/// Verify only the selected main addon under the caller's original control.
/// Shared slot files are intentionally excluded: their publication owner may be
/// replacing them, and the publication lease validates them separately.
/// This is the same inventory algorithm used at publication's main-addon gate.
/// The receipt handle acquired here is explicitly closed before returning.
pub fn verify_main_addon_controlled(
    path: &Path,
    release: &str,
    control: &lycheedev_core::control::OperationControl,
) -> Result<VerifiedInstallation> {
    use std::io::{Read, Seek, SeekFrom};
    const LIMIT: usize = 1 << 20;
    control.check()?;
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Err(ToolkitError::new(
                "delivery.installation_not_clean",
                "addon is absent",
                3,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    control.check()?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ToolkitError::new(
            "delivery.installation_not_clean",
            "addon must be an ordinary directory",
            3,
        ));
    }
    let canonical_path = fs::canonicalize(path)?;
    let marker = canonical_path.join(MARKER);
    let marker_metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            control.check()?;
            return Err(ToolkitError::new(
                "delivery.installation_not_clean",
                "addon is unmanaged",
                3,
            ));
        }
        Err(error) => return Err(error.into()),
    };
    control.check()?;
    if !marker_metadata.is_file()
        || marker_metadata.file_type().is_symlink()
        || marker_metadata.len() > LIMIT as u64
    {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "receipt file",
            4,
        ));
    }
    storage::guard_path(&canonical_path, &marker)?;
    let (mut file, digest) =
        storage::locked_sha256(&canonical_path.join(MARKER), LIMIT as u64, control)?;
    let verified = (|| -> Result<()> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        let mut block = [0u8; 65536];
        loop {
            control.check()?;
            let count = file.read(&mut block)?;
            if count == 0 {
                break;
            }
            if count > LIMIT.saturating_sub(bytes.len()) {
                return Err(ToolkitError::new(
                    "delivery.invalid_installation",
                    "installation receipt exceeds byte limit",
                    4,
                ));
            }
            bytes.extend_from_slice(&block[..count]);
        }
        let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|e| {
            ToolkitError::new("delivery.invalid_installation", format!("receipt: {e}"), 4)
        })?;
        if storage::sha256(&bytes) != digest
            || receipt.schema != "lycheedev.installation.v1"
            || receipt.component != "addon"
            || receipt.version.is_empty()
            || !valid_digest(&receipt.commit, 40)
        {
            return Err(ToolkitError::new(
                "delivery.invalid_installation",
                "receipt identity",
                4,
            ));
        }
        if receipt.version != release || release.is_empty() {
            return Err(ToolkitError::new(
                "delivery.installation_not_clean",
                "installation receipt does not match selected release",
                3,
            ));
        }
        crate::release::verify_inventory_with_control(
            &canonical_path,
            &receipt.resources,
            Some("addon"),
            control,
        )?;
        control.check()
    })();
    storage::finish_file(file, verified, control)?;
    Ok(VerifiedInstallation {
        canonical_path,
        manifest_sha256: digest,
    })
}

mod inspect_controlled;

pub use inspect_controlled::inspect_main_controlled;
