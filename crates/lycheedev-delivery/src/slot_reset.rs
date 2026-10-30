//! Explicit archive-and-rebuild of the fixed slot paths, without interpreting
//! old business state. Live owns shared connection admission before this call.
use crate::{
    deploy,
    installation::{self, Receipt},
    slots,
};
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    coordination::InstallationMaintenanceLease,
    installation_selection::{self, InvocationMetadata},
    storage,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const SCHEMA: &str = "lycheedev.slot-reset.v1";
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Clone, Debug, Serialize)]
pub struct SlotReset {
    pub archive: PathBuf,
    pub complete: bool,
}
#[derive(Debug, Serialize, Deserialize)]
struct Member {
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    digest: String,
}
#[derive(Debug, Serialize, Deserialize)]
struct Intent {
    schema: String,
    parent: PathBuf,
    archive: PathBuf,
    version: String,
    phase: String,
    receipt: Receipt,
    members: Vec<Member>,
}
#[derive(Clone, Copy, Debug)]
enum Step {
    Intent,
    Archived,
    Installing,
    Complete,
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("delivery.invalid_installation", message, 4)
}
fn conflict(message: &str) -> ToolkitError {
    ToolkitError::new("delivery.installation_conflict", message, 4)
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn plain_directory(path: &Path, control: &OperationControl) -> Result<()> {
    storage::guard_path_with_check(path, path, || control.check())?;
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(invalid("slot reset requires a plain directory"));
    }
    Ok(())
}
fn target(client_directory: &Path, control: &OperationControl) -> Result<PathBuf> {
    let client = installation_selection::select_with(
        client_directory,
        "",
        &mut InvocationMetadata(control),
    )?;
    let root = Path::new(&client.directory);
    for path in [
        root.to_path_buf(),
        root.join("Interface"),
        root.join("Interface/AddOns"),
    ] {
        plain_directory(&path, control)?;
    }
    deploy::destination(&lycheedev_core::client::addon_destination(&client)?)
}
fn names() -> Vec<String> {
    (1..=slots::COUNT)
        .map(|index| format!("Lychee Dev Slot {index:02}"))
        .chain(std::iter::once(slots::MARKER.into()))
        .collect()
}
fn companion(archive: &Path) -> PathBuf {
    let mut path = archive.as_os_str().to_os_string();
    path.push(".slot-reset.json");
    PathBuf::from(path)
}

/// Archive the original manifest and all fixed 500 member paths, then install
/// an initial pool beneath the same clean managed addon. The archive's parent
/// must already exist, be on the installation volume, and lie outside AddOns.
/// The same archive resumes its durable intent. A completed retry verifies the
/// archive and current pool without clearing reservations created afterwards.
pub fn reset(
    client_directory: &Path,
    archive: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<SlotReset> {
    reset_with_observer(client_directory, archive, version, control, |_| Ok(()))
}
fn reset_with_observer(
    client_directory: &Path,
    archive: &Path,
    version: &str,
    control: &OperationControl,
    mut observe: impl FnMut(Step) -> Result<()>,
) -> Result<SlotReset> {
    control.check()?;
    if archive.as_os_str().is_empty() || version.is_empty() {
        return Err(invalid("slot reset requires archive and version"));
    }
    let target = target(client_directory, control)?;
    let parent = target
        .parent()
        .ok_or_else(|| invalid("addon has no parent"))?
        .to_path_buf();
    let absolute_archive = std::path::absolute(archive)?;
    plain_directory(
        absolute_archive
            .parent()
            .ok_or_else(|| invalid("archive has no parent"))?,
        control,
    )?;
    let archive = deploy::destination(&absolute_archive)?;
    deploy::outside(&parent, &archive)?;
    #[cfg(windows)]
    if parent.components().next() != archive.components().next() {
        return Err(invalid("slot archive must be on the installation volume"));
    }
    let maintenance = InstallationMaintenanceLease::acquire_controlled(&target, control)?;
    let result = (|| {
        let installation = deploy::installation_lock_controlled(&target, control)?;
        let result = (|| {
            if self::target(client_directory, control)? != target || maintenance.target != target {
                return Err(conflict("client identity changed after reset admission"));
            }
            plain_directory(&target, control)?;
            let main = installation::inspect_main_controlled(&target, "addon", control)?;
            if main["state"] != "managed" {
                return Err(conflict("slot reset requires a clean managed addon"));
            }
            let receipt: Receipt = serde_json::from_value(main["receipt"].clone())?;
            if receipt.version != version
                || !receipt
                    .resources
                    .iter()
                    .any(|resource| resource.path == "addon/Bridge/SlotRuntime.lua")
            {
                return Err(conflict(
                    "slot reset requires the managed slot runtime at the selected version",
                ));
            }
            reset_locked(&parent, &archive, version, receipt, control, &mut observe)
        })();
        deploy::finish_close(result, installation.close())
    })();
    deploy::finish_close(result, maintenance.close())
}
fn reset_locked(
    parent: &Path,
    archive: &Path,
    version: &str,
    receipt: Receipt,
    control: &OperationControl,
    observe: &mut impl FnMut(Step) -> Result<()>,
) -> Result<SlotReset> {
    let intent_path = companion(archive);
    let mut intent = if exists(&intent_path)? {
        let raw = slots::read_bounded_controlled(&intent_path, 1 << 20, control)?;
        serde_json::from_slice::<Intent>(&raw).map_err(|_| invalid("invalid slot reset intent"))?
    } else {
        if exists(archive)? {
            return Err(conflict("slot archive exists without its reset intent"));
        }
        let mut members = vec![];
        for name in names() {
            control.check()?;
            let source = parent.join(&name);
            let digest = if exists(&source)? {
                digest(&source, control)?
            } else {
                String::new()
            };
            members.push(Member { name, digest });
        }
        let intent = Intent {
            schema: SCHEMA.into(),
            parent: parent.into(),
            archive: archive.into(),
            version: version.into(),
            phase: "archiving".into(),
            receipt: receipt.clone(),
            members,
        };
        save(&intent_path, &intent, true, control)?;
        observe(Step::Intent)?;
        control.check()?;
        intent
    };
    if intent.schema != SCHEMA
        || intent.parent != parent
        || intent.archive != archive
        || intent.version != version
        || !installation::same_receipt(&intent.receipt, &receipt)?
        || !matches!(
            intent.phase.as_str(),
            "archiving" | "installing" | "complete"
        )
        || intent.members.len() != slots::COUNT as usize + 1
        || intent.members.iter().zip(names()).any(|(member, name)| {
            member.name != name
                || !member.digest.is_empty()
                    && !lycheedev_core::selection::valid_digest(&member.digest, 64)
        })
    {
        return Err(conflict("slot reset intent differs from this installation"));
    }
    match fs::create_dir(archive) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    plain_directory(archive, control)?;
    for entry in fs::read_dir(archive)? {
        control.check()?;
        let entry = entry?;
        if !intent.members.iter().any(|member| {
            !member.digest.is_empty() && entry.file_name() == std::ffi::OsStr::new(&member.name)
        }) {
            return Err(conflict("unexpected slot archive member"));
        }
    }
    // Verify every original before the first move. During archiving each
    // existing member must reside on exactly one side of the durable intent.
    for member in &intent.members {
        control.check()?;
        let source = parent.join(&member.name);
        let destination = archive.join(&member.name);
        let location = if intent.phase == "archiving" {
            match (
                exists(&source)?,
                exists(&destination)?,
                member.digest.is_empty(),
            ) {
                (false, false, true) => continue,
                (true, false, false) => source,
                (false, true, false) => destination,
                _ => {
                    return Err(conflict(
                        "original reset member appeared, disappeared or exists on both sides",
                    ));
                }
            }
        } else {
            destination
        };
        if member.digest.is_empty() && !exists(&location)? {
            continue;
        }
        if digest(&location, control)? != member.digest {
            return Err(conflict("original slot reset member changed"));
        }
    }
    if intent.phase == "archiving" {
        for member in &intent.members {
            control.check()?;
            if !member.digest.is_empty() {
                let source = parent.join(&member.name);
                let destination = archive.join(&member.name);
                match (exists(&source)?, exists(&destination)?) {
                    (true, false) => storage::durable_move_noreplace(&source, &destination)?,
                    (false, true) => {}
                    _ => return Err(conflict("reset member disappeared or exists on both sides")),
                }
                observe(Step::Archived)?;
                control.check()?;
            }
        }
        intent.phase = "installing".into();
        save(&intent_path, &intent, false, control)?;
        observe(Step::Installing)?;
        control.check()?;
    }
    if intent.phase == "installing" {
        slots::install_controlled(parent, version, control)?;
        intent.phase = "complete".into();
        save(&intent_path, &intent, false, control)?;
        observe(Step::Complete)?;
    }
    slots::inspect_controlled(parent, version, control)?;
    Ok(SlotReset {
        archive: archive.into(),
        complete: true,
    })
}

// Match the v1 intent's tree digest framing, including Go's ModeDir bit and
// relative native path spelling. Payload bytes are streamed, never decoded.
fn digest(path: &Path, control: &OperationControl) -> Result<String> {
    fn walk(root: &Path, path: &Path, hash: &mut Sha256, control: &OperationControl) -> Result<()> {
        control.check()?;
        storage::guard_path_with_check(root, path, || control.check())?;
        let metadata = fs::symlink_metadata(path)?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| invalid("reset member escaped root"))?;
        let relative = if relative.as_os_str().is_empty() {
            ".".into()
        } else {
            relative.to_string_lossy().into_owned()
        };
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(invalid("redirected or special reset member"));
        }
        hash.update(format!(
            "{}:{}:{}:",
            relative.len(),
            relative,
            if metadata.is_dir() { 1u64 << 31 } else { 0 }
        ));
        if metadata.is_dir() {
            let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
            entries.sort_by_cached_key(|entry| entry.file_name().to_string_lossy().into_owned());
            for entry in entries {
                walk(root, &entry.path(), hash, control)?;
            }
        } else {
            hash.update(format!("{}:", metadata.len()));
            let (mut file, expected) = storage::locked_sha256(path, u64::MAX, control)?;
            let result = (|| {
                file.seek(SeekFrom::Start(0))?;
                let mut observed = Sha256::new();
                let mut block = [0u8; 65536];
                loop {
                    control.check()?;
                    let count = file.read(&mut block)?;
                    if count == 0 {
                        break;
                    }
                    hash.update(&block[..count]);
                    observed.update(&block[..count]);
                }
                if format!("{:x}", observed.finalize()) != expected {
                    return Err(conflict("reset member changed during read"));
                }
                Ok(())
            })();
            storage::finish_file(file, result, control)?;
        }
        Ok(())
    }
    let mut hash = Sha256::new();
    walk(path, path, &mut hash, control)?;
    Ok(format!("{:x}", hash.finalize()))
}
fn save(path: &Path, intent: &Intent, create: bool, control: &OperationControl) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(intent)?;
    if !create {
        return storage::atomic_write_controlled(path, &bytes, control);
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("intent has no parent"))?;
    let temporary = loop {
        control.check()?;
        let temporary = parent.join(format!(
            ".lycheedev-slot-reset-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(mut file) => {
                let result = file
                    .write_all(&bytes)
                    .and_then(|_| file.sync_all())
                    .map_err(ToolkitError::from);
                if let Err(error) = storage::finish_file(file, result, control) {
                    return deploy::finish_close(
                        Err(error),
                        fs::remove_file(&temporary).map_err(Into::into),
                    );
                }
                break temporary;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    };
    let result = (|| {
        control.check()?;
        storage::durable_move_noreplace(&temporary, path)?;
        Ok(())
    })();
    let cleanup = match fs::remove_file(&temporary) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    };
    deploy::finish_close(result, cleanup)
}

#[cfg(test)]
#[path = "slot_reset_tests.rs"]
mod tests;
