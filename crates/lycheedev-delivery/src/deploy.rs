use crate::{
    installation::{self, Receipt},
    release,
};
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
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
    installation_lock_controlled(target, &OperationControl::default())
}
pub(crate) fn installation_lock_controlled(
    target: &Path,
    control: &OperationControl,
) -> Result<ExclusiveLock> {
    control.check()?;
    let target = destination(target)?;
    let parent = target.parent().unwrap();
    ExclusiveLock::resource_controlled(
        &parent.join(".lycheedev-locks"),
        &format!(
            "installation:{}",
            target
                .to_string_lossy()
                .trim_start_matches("\\\\?\\")
                .to_ascii_lowercase()
        ),
        control,
    )
}
/// The exact installation lock shared by update discovery and every move.
/// Borrowing this owner avoids nested acquisition of the same OS lock.
pub(crate) struct InstallationLease {
    target: PathBuf,
    lock: ExclusiveLock,
}
impl InstallationLease {
    pub(crate) fn acquire(target: &Path, control: &OperationControl) -> Result<Self> {
        control.check()?;
        let target = destination(target)?;
        let lock = installation_lock_controlled(&target, control)?;
        Ok(Self { target, lock })
    }
    pub(crate) fn target(&self) -> &Path {
        &self.target
    }
    pub(crate) fn finish<T: serde::Serialize>(self, result: Result<T>) -> Result<T> {
        self.finish_with(result, ExclusiveLock::close)
    }
    fn finish_with<T: serde::Serialize>(
        self,
        result: Result<T>,
        close: impl FnOnce(ExclusiveLock) -> Result<()>,
    ) -> Result<T> {
        finish_close(result, close(self.lock))
    }
}
/// Close exactly once even after a business failure. A completed mutation stays
/// represented if close fails; cleanup never applies a new cancellation check.
pub(crate) fn finish_close<T: serde::Serialize>(result: Result<T>, close: Result<()>) -> Result<T> {
    match (result, close) {
        (result, Ok(())) => result,
        (Err(mut primary), Err(cleanup)) => {
            let original = primary.clone();
            primary.details = json!({"schema":"lycheedev.delivery-cleanup.v1","primary":original,"cleanup":cleanup});
            Err(primary)
        }
        (Ok(result), Err(mut cleanup)) => {
            let original = cleanup.clone();
            let (value, encoding) = match serde_json::to_value(&result) {
                Ok(value) => (value, Value::Null),
                Err(error) => (Value::Null, json!(ToolkitError::from(error))),
            };
            cleanup.details = json!({"schema":"lycheedev.delivery-cleanup.v1","operationCompleted":true,"result":value,"resultEncodingFailure":encoding,"cleanup":original});
            Err(cleanup)
        }
    }
}
pub fn outside(parent: &Path, archive: &Path) -> Result<()> {
    let parent_key = parent
        .to_string_lossy()
        .replace('\\', "/")
        .trim_end_matches('/')
        .to_ascii_lowercase();
    let archive_key = archive
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if archive_key == parent_key || archive_key.starts_with(&format!("{parent_key}/")) {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "archive must be outside discovery parent",
            4,
        ));
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Staged,
    Prepared,
    OldMoved,
    Published,
    RemoveReady,
    Removed,
}

// Observers are private fault seams. The public path always uses a no-op;
// cancellation is checked after observations and immediately before each move.
fn publish(source: &Path, target: &Path, control: &OperationControl) -> Result<()> {
    control.check()?;
    storage::durable_move_noreplace(source, target).map_err(ToolkitError::io)
}
fn private_directory(
    parent: &Path,
    prefix: &str,
    target: &Path,
    control: &OperationControl,
) -> Result<PathBuf> {
    let key = storage::sha256(target.to_string_lossy().to_ascii_lowercase().as_bytes());
    for slot in 0..4 {
        control.check()?;
        let path = parent.join(format!("{prefix}{key}-{slot}"));
        storage::guard_path(parent, &path)?;
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
    }
    Err(ToolkitError::new(
        "delivery.preparation_capacity",
        "retained preparations require review before another attempt",
        6,
    )
    .with_details(json!({"target":target,"parent":parent,"maximumRetainedRoots":4})))
}
fn finish_preparation<T>(stage: &Path, result: Result<T>) -> Result<T> {
    if !stage.exists() {
        return result;
    }
    // An atomic-write/checked-close error can retain an uncertain temporary.
    // Never erase that evidence or mask its original structured error.
    if let Err(mut error) = result {
        if error.details.is_null() {
            if let Err(cleanup) = fs::remove_dir_all(stage) {
                let primary = error.clone();
                error.details = json!({"retainedPreparation":stage,"cause":primary,"cleanup":ToolkitError::io(cleanup)});
            }
        } else {
            let primary = error.clone();
            error.details = json!({"retainedPreparation":stage,"cause":primary});
        }
        Err(error)
    } else {
        result
    }
}
pub fn fresh(root: &Path, target: &Path, component: &str, version: &str) -> Result<Receipt> {
    fresh_controlled(
        root,
        target,
        component,
        version,
        &OperationControl::default(),
    )
}
pub fn fresh_controlled(
    root: &Path,
    target: &Path,
    component: &str,
    version: &str,
    control: &OperationControl,
) -> Result<Receipt> {
    fresh_observed(root, target, component, version, control, &mut |_, _| {})
}
fn fresh_observed(
    root: &Path,
    target: &Path,
    component: &str,
    version: &str,
    control: &OperationControl,
    observe: &mut impl FnMut(Step, &Path),
) -> Result<Receipt> {
    let lease = InstallationLease::acquire(target, control)?;
    let result = fresh_locked_observed(root, &lease, component, version, control, observe);
    lease.finish(result)
}
pub(crate) fn fresh_locked(
    root: &Path,
    lease: &InstallationLease,
    component: &str,
    version: &str,
    control: &OperationControl,
) -> Result<Receipt> {
    fresh_locked_observed(root, lease, component, version, control, &mut |_, _| {})
}
fn fresh_locked_observed(
    root: &Path,
    lease: &InstallationLease,
    component: &str,
    version: &str,
    control: &OperationControl,
    observe: &mut impl FnMut(Step, &Path),
) -> Result<Receipt> {
    control.check()?;
    let target = lease.target();
    let release = release::inspect_with_control(root, version, control)?;
    let wanted = release::receipt(&release, component);
    let current = installation::inspect_main_controlled(&target, component, control)?;
    if current["state"] == "managed"
        && installation::same_receipt_value(&current["receipt"], &serde_json::to_value(&wanted)?)?
    {
        return Ok(wanted);
    }
    if current["state"] != "absent" {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "existing target requires explicit managed upgrade",
            4,
        ));
    }
    let stage = private_directory(
        target.parent().unwrap(),
        ".lycheedev-stage-",
        &target,
        control,
    )?;
    let result = (|| {
        for resource in &wanted.resources {
            control.check()?;
            let relative = resource
                .path
                .strip_prefix(&format!("{component}/"))
                .unwrap();
            let source = storage::safe_relative(&root.join("payload"), &resource.path)?;
            storage::guard_path(&root.join("payload"), &source)?;
            let bytes =
                crate::slots::read_bounded_controlled(&source, resource.bytes as usize, control)?;
            if bytes.len() as u64 != resource.bytes || storage::sha256(&bytes) != resource.sha256 {
                return Err(ToolkitError::new(
                    "delivery.invalid_payload",
                    "release changed during staging",
                    4,
                ));
            }
            storage::atomic_write_controlled(
                &storage::safe_relative(&stage, relative)?,
                &bytes,
                control,
            )?;
        }
        storage::atomic_write_controlled(
            &stage.join(installation::MARKER),
            &serde_json::to_vec_pretty(&wanted)?,
            control,
        )?;
        release::verify_inventory_with_control(
            &stage,
            &wanted.resources,
            Some(component),
            control,
        )?;
        observe(Step::Staged, &target);
        if target.exists() {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "target appeared during staging",
                4,
            ));
        }
        publish(&stage, &target, control)?;
        observe(Step::Published, &target);
        // A completed move is a fact; a later stop must not undo or relabel it.
        Ok(wanted)
    })();
    finish_preparation(&stage, result)
}
pub fn upgrade(
    root: Option<&Path>,
    target: &Path,
    archive: &Path,
    component: &str,
    version: &str,
    resume: bool,
) -> Result<Value> {
    upgrade_controlled(
        root,
        target,
        archive,
        component,
        version,
        resume,
        &OperationControl::default(),
    )
}
pub fn upgrade_controlled(
    root: Option<&Path>,
    target: &Path,
    archive: &Path,
    component: &str,
    version: &str,
    resume: bool,
    control: &OperationControl,
) -> Result<Value> {
    upgrade_observed(
        root,
        target,
        archive,
        component,
        version,
        resume,
        control,
        &mut |_, _| {},
    )
}
#[allow(clippy::too_many_arguments)]
fn upgrade_observed(
    root: Option<&Path>,
    target: &Path,
    archive: &Path,
    component: &str,
    version: &str,
    resume: bool,
    control: &OperationControl,
    observe: &mut impl FnMut(Step, &Path),
) -> Result<Value> {
    let lease = InstallationLease::acquire(target, control)?;
    let result = upgrade_locked_observed(
        root, &lease, archive, component, version, resume, control, observe,
    );
    lease.finish(result)
}
pub(crate) fn upgrade_locked(
    root: Option<&Path>,
    lease: &InstallationLease,
    archive: &Path,
    component: &str,
    version: &str,
    resume: bool,
    control: &OperationControl,
) -> Result<Value> {
    upgrade_locked_observed(
        root,
        lease,
        archive,
        component,
        version,
        resume,
        control,
        &mut |_, _| {},
    )
}
#[allow(clippy::too_many_arguments)]
fn upgrade_locked_observed(
    root: Option<&Path>,
    lease: &InstallationLease,
    archive: &Path,
    component: &str,
    version: &str,
    resume: bool,
    control: &OperationControl,
    observe: &mut impl FnMut(Step, &Path),
) -> Result<Value> {
    control.check()?;
    let target = lease.target();
    let archive = destination(archive)?;
    outside(target.parent().unwrap(), &archive)?;
    let record_path = archive.join("replacement.json");
    if !resume {
        let current = installation::inspect_main_controlled(&target, component, control)?;
        if current["state"] != "managed" {
            return Err(ToolkitError::new(
                "delivery.installation_conflict",
                "upgrade requires unchanged managed installation",
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
        let root = root.ok_or_else(|| ToolkitError::arguments("upgrade requires --release"))?;
        // Publish the recovery path only after its next tree and intent exist.
        // Failed preparation leaves both original arguments retryable.
        let stage = private_directory(
            archive.parent().unwrap(),
            ".lycheedev-prepare-",
            &archive,
            control,
        )?;
        let prepared = (|| {
            let next = fresh_observed(
                root,
                &stage.join("next"),
                component,
                version,
                control,
                observe,
            )?;
            storage::atomic_write_controlled(
                &stage.join("replacement.json"),
                &serde_json::to_vec_pretty(
                    &json!({"schema":"lycheedev.replacement.v1","target":target,"previous":current["receipt"],"next":next}),
                )?,
                control,
            )?;
            observe(Step::Prepared, &archive);
            publish(&stage, &archive, control)
        })();
        finish_preparation(&stage, prepared)?;
    }
    storage::guard_path(&archive, &record_path)?;
    let record: Value = serde_json::from_slice(&crate::slots::read_bounded_controlled(
        &record_path,
        1 << 20,
        control,
    )?)?;
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
    let mut current = installation::inspect_main_controlled(&target, component, control)?;
    let previous =
        installation::inspect_main_controlled(&archive.join("previous"), component, control)?;
    let next = installation::inspect_main_controlled(&archive.join("next"), component, control)?;
    let mut old_valid = previous["state"] == "managed"
        && installation::same_receipt_value(&previous["receipt"], &record["previous"])?;
    if old_valid
        && current["state"] == "managed"
        && installation::same_receipt_value(&current["receipt"], &record["next"])?
        && next["state"] == "absent"
    {
        return Ok(json!({"target":target,"archive":archive,"receipt":record["next"]}));
    }
    if next["state"] != "managed"
        || !installation::same_receipt_value(&next["receipt"], &record["next"])?
    {
        return Err(ToolkitError::new(
            "delivery.installation_conflict",
            "prepared replacement changed",
            4,
        ));
    }
    if previous["state"] == "absent"
        && current["state"] == "managed"
        && installation::same_receipt_value(&current["receipt"], &record["previous"])?
    {
        publish(&target, &archive.join("previous"), control)?;
        observe(Step::OldMoved, &target);
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
    publish(&archive.join("next"), &target, control)?;
    observe(Step::Published, &target);
    Ok(json!({"target":target,"archive":archive,"receipt":record["next"]}))
}
pub fn remove(target: &Path, archive: &Path, component: &str) -> Result<Value> {
    remove_controlled(target, archive, component, &OperationControl::default())
}
pub fn remove_controlled(
    target: &Path,
    archive: &Path,
    component: &str,
    control: &OperationControl,
) -> Result<Value> {
    remove_observed(target, archive, component, control, &mut |_, _| {})
}
fn remove_observed(
    target: &Path,
    archive: &Path,
    component: &str,
    control: &OperationControl,
    observe: &mut impl FnMut(Step, &Path),
) -> Result<Value> {
    let lease = InstallationLease::acquire(target, control)?;
    let result = (|| {
        control.check()?;
        let target = lease.target();
        let archive = destination(archive)?;
        outside(target.parent().unwrap(), &archive)?;
        let assessment = installation::inspect_main_controlled(&target, component, control)?;
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
        observe(Step::RemoveReady, &target);
        publish(&target, &archive, control)?;
        observe(Step::Removed, &target);
        Ok(json!({"state":"archived","archive":archive,"receipt":assessment["receipt"]}))
    })();
    lease.finish(result)
}

#[cfg(test)]
mod tests;
