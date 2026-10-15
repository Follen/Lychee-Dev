//! Fixed Go 3.2 doctor installation adapter, without the retired claim registry.
//!
//! The Live bridge must hold formal installation admission (no CON/OP/BTP
//! claims) throughout this call. Delivery additionally holds the shared Rust
//! maintenance and installation locks. This module never reads a business
//! journal, retires a claim, reloads a client, or replays unknown input.
//! Oracle: 3057a9666ab078270b63c7f06de748069b0d1f4b,
//! delivery/{addon_install,install,upgrade,stage,slots}.go.
#![forbid(unsafe_code)]
use crate::{
    deploy,
    installation::{self, Receipt},
    release, slots,
};
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    installation_selection::{self, Client, InvocationMetadata},
    storage::{self, ExclusiveLock},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

mod cleanup;
mod publication;
#[cfg(test)]
mod slot_reads_tests;
mod snapshot;
#[cfg(test)]
mod tests;

/// The host composes its authenticated, compile-only addon validator. It must
/// verify the fixed Go TOC/header/XML/Lua closure, using this SAME control.
/// The snapshot contains the complete manifest and payload, including luac.
/// A missing validator is a failure, never permission to install unchecked Lua.
pub trait AddonValidator {
    fn validate(
        &mut self,
        prepared_release: &Path,
        version: &str,
        control: &OperationControl,
    ) -> Result<()>;
}
fn conflict(detail: &str) -> ToolkitError {
    ToolkitError::new("delivery.installation_conflict", detail, 4)
}
fn invalid(detail: &str) -> ToolkitError {
    ToolkitError::new("delivery.invalid_installation", detail, 4)
}

/// Install or managed-upgrade the exact selected client. The explicit archive
/// must be absent and its parent must exist; only the host creates a generated
/// default parent. Existing archives are never silently resumed. Go replacement
/// and slot migration records survive interruption for explicit recovery.
pub fn apply(
    selected: &Client,
    release_root: &Path,
    archive: Option<&Path>,
    version: &str,
    validator: &mut impl AddonValidator,
    control: &OperationControl,
) -> Result<Receipt> {
    control.check()?;
    let target = revalidate(selected, control)?;
    let parent = target
        .parent()
        .ok_or_else(|| invalid("addon parent is missing"))?;
    let original = release::inspect_with_control(release_root, version, control)?;
    let mut snapshot = snapshot::Snapshot::prepare(release_root, parent, &original, control)?;
    let mut phase = "prepared";
    let result = (|| {
        validator.validate(snapshot.root(), version, control)?;
        snapshot.verify(&original, control)?;
        if revalidate(selected, control)? != target {
            return Err(conflict("client identity changed during preparation"));
        }
        let wanted = release::receipt(&original, "addon");
        // The formal protocol requires its actual release closure, not a
        // version-only manifest that cannot load the slot runtime.
        if !wanted
            .resources
            .iter()
            .any(|r| r.path == "addon/Bridge/SlotRuntime.lua")
        {
            return Err(ToolkitError::new(
                "delivery.protocol_mismatch",
                "mailbox protocol v1 release has no SlotRuntime closure",
                3,
            ));
        }
        let archive = archive.map(deploy::destination).transpose()?;
        if let Some(archive) = &archive {
            outside(parent, archive)?;
        }
        let canonical = target
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .to_ascii_lowercase();
        let scope = parent.join(".lycheedev-locks");
        storage::guard_path(parent, &scope)?;
        let maintenance = lock(
            &scope,
            &format!("installation-maintenance:{canonical}"),
            control,
        )?;
        let work = (|| {
            let installation = lock(&scope, &format!("installation:{canonical}"), control)?;
            let work = (|| {
                if revalidate(selected, control)? != target {
                    return Err(conflict(
                        "client identity changed after maintenance admission",
                    ));
                }
                // Full preflight precedes either main directory move. A foreign
                // slot, pending reservation or edited main tree is preserved.
                slots::preflight_controlled(parent, version, control)?;
                let current = installation::inspect_main_controlled(&target, "addon", control)?;
                let prepared = snapshot.prepare_addon(&wanted, control)?;
                phase = "publishing";
                if publication::publish(
                    &target,
                    &prepared,
                    archive.as_deref(),
                    &wanted,
                    &current,
                    control,
                )? {
                    snapshot.moved_addon();
                }
                phase = "main";
                // On failure the main replacement and slots' own installing /
                // upgrading journal remain durable. Never undo a partially
                // published slot generation or erase the archived old tree.
                slots::install_controlled(parent, version, control)?;
                phase = "slots";
                let installed = installation::inspect_main_controlled(&target, "addon", control)?;
                require_receipt(
                    &installed,
                    &wanted,
                    "published addon differs from prepared receipt",
                )?;
                slots::inspect_controlled(parent, version, control)?;
                control.check()?;
                phase = "complete";
                Ok(wanted)
            })();
            finish(work, [installation.close()], control)
        })();
        finish(work, [maintenance.close()], control)
    })();
    match result {
        Ok(receipt) => finish(Ok(receipt), [snapshot.cleanup(control)], control),
        Err(mut error) => {
            // Never recursively delete a changed/unverified staging tree on a
            // failed or cancelled operation. It is owned evidence, not a new
            // release selection or authority for implicit recovery.
            error.details = json!({"schema":"lycheedev.formal-repair.v1", "phase":phase, "target":target, "archive":archive, "retainedPreparation":snapshot.root(), "cause":error.clone()});
            Err(error)
        }
    }
}
fn revalidate(selected: &Client, control: &OperationControl) -> Result<PathBuf> {
    control.check()?;
    if selected.directory.is_empty() {
        return Err(invalid("selected client is absent"));
    }
    let observed = installation_selection::select_with(
        Path::new(&selected.directory),
        "",
        &mut InvocationMetadata(control),
    )?;
    let public = Client::from(&observed);
    if serde_json::to_value(&public)? != serde_json::to_value(selected)? {
        return Err(conflict("selected client identity changed"));
    }
    let target = lycheedev_core::client::addon_destination(&observed)?;
    let root = Path::new(&observed.directory);
    for directory in [root.join("Interface"), root.join("Interface/AddOns")] {
        storage::guard_path(root, &directory)?;
        if !fs::symlink_metadata(&directory)?.is_dir() {
            return Err(invalid("redirected addon parent"));
        }
    }
    deploy::destination(&target)
}
fn outside(parent: &Path, archive: &Path) -> Result<()> {
    let parent = parent
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    let archive_parts = archive
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
        .collect::<Vec<_>>();
    if archive_parts.starts_with(&parent) {
        return Err(invalid("archive must be outside discovery parent"));
    }
    // Directory replacement must remain on the same volume. No copy/delete
    // fallback is allowed when Windows reports ERROR_NOT_SAME_DEVICE.
    Ok(())
}
fn require_receipt(value: &Value, receipt: &Receipt, detail: &str) -> Result<()> {
    if value["state"] != "managed" {
        return Err(conflict(detail));
    }
    let observed: Receipt = serde_json::from_value(value["receipt"].clone())?;
    if !same_receipt(&observed, receipt)? {
        return Err(conflict(detail));
    }
    Ok(())
}
/// Fixed Go sameReceipt treats resource ordering as immaterial. Never collapse
/// duplicate paths into a map first: both inputs must independently be unique.
fn same_receipt(left: &Receipt, right: &Receipt) -> Result<bool> {
    fn resources(
        receipt: &Receipt,
    ) -> Result<std::collections::BTreeMap<&str, &installation::Resource>> {
        let mut paths = std::collections::BTreeMap::new();
        let mut folded = std::collections::BTreeSet::new();
        for resource in &receipt.resources {
            if !folded.insert(resource.path.to_lowercase())
                || paths.insert(resource.path.as_str(), resource).is_some()
            {
                return Err(invalid("duplicate resource path in receipt"));
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
fn lock(scope: &Path, resource: &str, control: &OperationControl) -> Result<ExclusiveLock> {
    let path = scope.join(format!("{}.lock", storage::sha256(resource.as_bytes())));
    loop {
        control.check()?;
        storage::guard_path(
            scope.parent().ok_or_else(|| invalid("lock scope parent"))?,
            &path,
        )?;
        match ExclusiveLock::resource_controlled(scope, resource, control) {
            Ok(lock) => return Ok(lock),
            Err(error) if error.code == "workspace.busy" && error.details.is_null() => {
                control.check()?;
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
}
/// Preserve every typed cause. A lone cause is unchanged, and checked close is
/// always consumed before this final control observation, even after failure.
fn finish<T>(
    result: Result<T>,
    cleanups: impl IntoIterator<Item = Result<()>>,
    control: &OperationControl,
) -> Result<T> {
    let (value, mut errors) = match result {
        Ok(value) => (Some(value), vec![]),
        Err(e) => (None, vec![e]),
    };
    for error in cleanups.into_iter().filter_map(Result::err) {
        let duplicate_control = error.details.is_null()
            && matches!(
                error.code.as_str(),
                "command.cancelled" | "command.deadline"
            )
            && errors.iter().any(|e| {
                e.code == error.code && e.message == error.message && e.details == error.details
            });
        if !duplicate_control {
            errors.push(error);
        }
    }
    if let Err(error) = control.check() {
        if !errors.iter().any(|e| {
            e.code == error.code && e.message == error.message && e.details == error.details
        }) {
            errors.push(error);
        }
    }
    if errors.is_empty() {
        return Ok(value.expect("successful repair retained value"));
    }
    let mut first = errors[0].clone();
    if errors.len() > 1 {
        first.details = json!({"schema":"lycheedev.repair-cleanup.v1","causes":errors});
    }
    Err(first)
}
