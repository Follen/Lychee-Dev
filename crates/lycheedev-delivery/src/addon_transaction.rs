//! One durable intent covers the main addon and the complete 500-slot generation.
//! A main replacement can be durable while slot migration remains pending; resume never picks a new release.
use crate::{deploy, installation, release, slots};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, coordination::InstallationGuard, storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub struct Request<'a> {
    pub release: Option<&'a Path>,
    pub archive: Option<&'a Path>,
    pub version: &'a str,
    pub resume: bool,
    pub control: &'a OperationControl,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Transaction {
    schema: String,
    target: PathBuf,
    archive: Option<PathBuf>,
    wanted: installation::Receipt,
    previous: Value,
    phase: String,
    result: Value,
}
fn base(target: &Path) -> Result<PathBuf> {
    Ok(target
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| ToolkitError::invalid("addon target has no discovery grandparent"))?
        .join(".lycheedev-addon-transactions"))
}
fn identity(target: &Path) -> String {
    target
        .to_string_lossy()
        .trim_start_matches("\\\\?\\")
        .to_ascii_lowercase()
}
fn journal(
    target: &Path,
    archive: Option<&Path>,
    wanted: &installation::Receipt,
) -> Result<PathBuf> {
    let digest = storage::sha256(&storage::identity_json(
        &json!({"target":identity(target),"archive":archive.map(identity),"wanted":wanted}),
    )?);
    Ok(base(target)?.join(format!("{digest}.json")))
}
fn validate(
    transaction: &Transaction,
    target: &Path,
    archive: Option<&Path>,
    version: &str,
) -> Result<()> {
    if transaction.schema != "lycheedev.addon-transaction.v1"
        || identity(&transaction.target) != identity(target)
        || transaction.archive.as_deref().map(identity) != archive.map(identity)
        || transaction.wanted.component != "addon"
        || transaction.wanted.version != version
        || !["prepared", "main", "slots", "complete"].contains(&transaction.phase.as_str())
    {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "combined addon intent identity differs",
            4,
        ));
    }
    Ok(())
}
fn load(path: &Path) -> Result<Transaction> {
    let parent = path.parent().unwrap();
    storage::guard_path(parent, path)?;
    if fs::metadata(path)?.len() > 1 << 20 {
        return Err(ToolkitError::invalid("addon transaction exceeds 1 MiB"));
    }
    storage::read_json(path)
}
fn other_pending(target: &Path, selected: Option<&Path>) -> Result<()> {
    let root = base(target)?;
    if !root.exists() {
        return Ok(());
    }
    storage::guard_path(root.parent().unwrap(), &root)?;
    let mut count = 0;
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        count += 1;
        if count > 1000 {
            return Err(ToolkitError::new(
                "delivery.transaction_limit",
                "addon intent scan exceeds 1000 records",
                3,
            ));
        }
        if !entry.file_type()?.is_file() || entry.path().extension().is_none_or(|ext| ext != "json")
        {
            return Err(ToolkitError::invalid("unknown addon transaction entry"));
        }
        let path = entry.path();
        if selected.is_some_and(|selected| selected == path) {
            continue;
        }
        let record = load(&path)?;
        if record.schema != "lycheedev.addon-transaction.v1" {
            return Err(ToolkitError::invalid("unknown addon transaction schema"));
        }
        if identity(&record.target) == identity(target) && record.phase != "complete" {
            return Err(ToolkitError::new("delivery.addon_transaction_pending","resume the original combined addon and slot intent before selecting another release",6).with_details(json!({"journal":path})));
        }
    }
    Ok(())
}
pub fn apply(target: &Path, request: Request<'_>, guard: &InstallationGuard) -> Result<Value> {
    guard.require_idle()?;
    request.control.check()?;
    let target = deploy::destination(target)?;
    if identity(&guard.target) != identity(&target) {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "combined transaction maintenance guard is for another target",
            4,
        ));
    }
    let archive = request.archive.map(deploy::destination).transpose()?;
    if let Some(archive) = &archive {
        deploy::outside(target.parent().unwrap(), archive)?;
    }
    let candidate = request
        .release
        .map(|root| release::inspect(root, request.version))
        .transpose()?;
    let (path, mut record) = if let Some(candidate) = &candidate {
        let wanted = release::receipt(candidate, "addon");
        let path = journal(&target, archive.as_deref(), &wanted)?;
        other_pending(&target, Some(&path))?;
        if path.exists() {
            let record = load(&path)?;
            if record.wanted != wanted {
                return Err(ToolkitError::invalid("frozen addon release changed"));
            }
            (path, record)
        } else {
            if request.resume {
                return Err(ToolkitError::new(
                    "delivery.invalid_installation",
                    "no exact combined addon intent exists to resume",
                    4,
                ));
            }
            let root = request.release.unwrap();
            let toc = fs::read_to_string(root.join("payload/addon/Lychee Dev.toc"))?;
            if !toc
                .lines()
                .any(|line| line.trim().replace('\\', "/") == "Bridge/SlotRuntime.lua")
            {
                return Err(ToolkitError::new(
                    "delivery.protocol_mismatch",
                    "mailbox protocol v1 addon install needs matching SlotRuntime closure",
                    3,
                ));
            }
            slots::preflight(target.parent().unwrap(), request.version)?;
            let previous = installation::inspect(&target, "addon")?;
            if !["managed", "absent"].contains(&previous["state"].as_str().unwrap_or(""))
                || previous["state"] == "managed"
                    && previous["receipt"] != serde_json::to_value(&wanted)?
                    && archive.is_none()
            {
                return Err(ToolkitError::new(
                    "delivery.installation_conflict",
                    "unchanged managed upgrade requires an explicit archive; unknown bytes are never adopted",
                    4,
                ));
            }
            let record = Transaction {
                schema: "lycheedev.addon-transaction.v1".into(),
                target: target.clone(),
                archive: archive.clone(),
                wanted,
                previous,
                phase: "prepared".into(),
                result: Value::Null,
            };
            fs::create_dir_all(path.parent().unwrap())?;
            storage::guard_path(path.parent().unwrap(), &path)?;
            storage::atomic_json(&path, &record)?;
            (path, record)
        }
    } else {
        if !request.resume {
            return Err(ToolkitError::arguments(
                "addon install requires --release or exact --resume",
            ));
        }
        let root = base(&target)?;
        if !root.exists() {
            return Err(ToolkitError::new(
                "delivery.invalid_installation",
                "addon resume intent missing",
                4,
            ));
        }
        let mut found = None;
        for entry in fs::read_dir(&root)? {
            let path = entry?.path();
            let record = load(&path)?;
            if identity(&record.target) == identity(&target)
                && record.archive.as_deref().map(identity) == archive.as_deref().map(identity)
                && record.wanted.version == request.version
            {
                if found.is_some() {
                    return Err(ToolkitError::new(
                        "delivery.installation_conflict",
                        "ambiguous combined addon resume",
                        4,
                    ));
                }
                found = Some((path, record));
            }
        }
        let (path, record) = found.ok_or_else(|| {
            ToolkitError::new(
                "delivery.invalid_installation",
                "exact addon resume intent missing",
                4,
            )
        })?;
        other_pending(&target, Some(&path))?;
        (path, record)
    };
    validate(&record, &target, archive.as_deref(), request.version)?;
    if path != journal(&target, archive.as_deref(), &record.wanted)? {
        return Err(ToolkitError::invalid(
            "addon intent filename digest differs",
        ));
    }
    let wanted = serde_json::to_value(&record.wanted)?;
    let advance = (|| -> Result<()> {
        if record.phase == "complete" {
            installation::verify_clean(&target, request.version, slots::TRANSPORT, slots::COUNT)?;
            return Ok(());
        }
        if record.phase == "prepared" {
            request.control.check()?;
            let current = installation::inspect(&target, "addon")?;
            record.result = if let Some(archive) = &archive {
                let resume = archive.join("replacement.json").exists();
                deploy::upgrade(
                    request.release,
                    &target,
                    archive,
                    "addon",
                    request.version,
                    resume,
                )?
            } else if current["state"] == "managed" && current["receipt"] == wanted {
                wanted.clone()
            } else {
                if current != record.previous {
                    return Err(ToolkitError::new(
                        "delivery.installation_conflict",
                        "main addon changed after combined preparation",
                        4,
                    ));
                }
                serde_json::to_value(deploy::fresh(
                    request.release.ok_or_else(|| {
                        ToolkitError::arguments("unfinished fresh addon needs original --release")
                    })?,
                    &target,
                    "addon",
                    request.version,
                )?)?
            };
            let installed = installation::inspect(&target, "addon")?;
            if installed["state"] != "managed" || installed["receipt"] != wanted {
                return Err(ToolkitError::invalid(
                    "main addon differs from frozen combined receipt",
                ));
            }
            record.phase = "main".into();
            storage::atomic_json(&path, &record)?;
        }
        if record.phase == "main" {
            request.control.check()?;
            let installed = installation::inspect(&target, "addon")?;
            if installed["state"] != "managed" || installed["receipt"] != wanted {
                return Err(ToolkitError::new(
                    "delivery.installation_conflict",
                    "main addon was edited while slots were pending",
                    4,
                ));
            }
            slots::install(target.parent().unwrap(), request.version)?;
            record.phase = "slots".into();
            storage::atomic_json(&path, &record)?;
        }
        request.control.check()?;
        installation::verify_clean(&target, request.version, slots::TRANSPORT, slots::COUNT)?;
        record.phase = "complete".into();
        storage::atomic_json(&path, &record)?;
        Ok(())
    })();
    if let Err(error) = advance {
        return Err(ToolkitError::new(error.code,error.message,error.exit_code).with_details(json!({"result":{"complete":false,"phase":record.phase,"target":target,"journal":path,"main":record.result},"cause":error.details})));
    }
    Ok(record.result)
}
