//! One durable intent covers the main addon and its owned slot pool. Interrupted moves resume exactly.
use crate::{deploy, installation, slots};
use lycheedev_core::{Result, ToolkitError, coordination::InstallationGuard, storage};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
fn conflict<T>(message: &str) -> Result<T> {
    Err(ToolkitError::new(
        "delivery.installation_conflict",
        message,
        4,
    ))
}
fn companion(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}
fn location(source: &Path, destination: &Path) -> Result<(PathBuf, bool)> {
    match (source.exists(), destination.exists()) {
        (true, false) => Ok((source.into(), false)),
        (false, true) => Ok((destination.into(), true)),
        _ => conflict("removal member is missing or exists in both locations"),
    }
}
fn validate_slot(path: &Path, index: u16, pool: &Value) -> Result<()> {
    let schema = pool["schema"].as_str().unwrap_or("");
    let version = pool["version"]
        .as_str()
        .ok_or_else(|| ToolkitError::invalid("slot pool version missing"))?;
    let entry = if schema == "lycheedev.slots.v4" {
        &pool["modules"][index as usize - 1]
    } else {
        &pool["files"][index as usize - 1]
    };
    if entry["pendingHash"].as_str().is_some_and(|v| !v.is_empty())
        || entry["nonce"].as_str().is_some_and(|v| !v.is_empty())
            && entry["consumed"] != true
            && entry["retiredRuntime"].as_str().unwrap_or("").is_empty()
            && entry["retiredProcess"] != true
    {
        return conflict("slot reservation is unresolved");
    }
    storage::guard_path(path, path)?;
    if fs::read_dir(path)?.count() != 3 {
        return conflict("slot has unlisted files");
    }
    let mut expected = slots::files(index, version);
    if schema != "lycheedev.slots.v4" {
        let transport = match schema {
            "lycheedev.slots.v1" => "memory-slot-v1",
            "lycheedev.slots.v2" => "memory-slot-v2",
            "lycheedev.slots.v3" => "memory-slot-v3",
            _ => return conflict("unknown slot schema"),
        };
        expected[0].1 = String::from_utf8(expected[0].1.clone())
            .map_err(|e| ToolkitError::invalid(e.to_string()))?
            .replace(slots::TRANSPORT, transport)
            .into_bytes();
        expected[1].1 = String::from_utf8(expected[1].1.clone())
            .map_err(|e| ToolkitError::invalid(e.to_string()))?
            .replace("ns.NativeRuntime", "ns.SlotRuntime")
            .into_bytes();
    }
    for (name, bytes) in expected {
        let file = path.join(&name);
        storage::guard_path(path, &file)?;
        let actual = fs::read(&file)?;
        if name == "Payload.lua" {
            if actual.len() > 2 << 20
                || storage::sha256(&actual) != entry["payloadHash"].as_str().unwrap_or("")
            {
                return conflict("slot payload differs from removal intent");
            }
        } else if actual != bytes {
            return conflict("slot static file differs from removal intent");
        }
    }
    Ok(())
}
fn members(pool: &Value) -> Result<usize> {
    let schema = pool["schema"].as_str().unwrap_or("");
    let array = if schema == "lycheedev.slots.v4" {
        pool["modules"].as_array()
    } else {
        pool["files"].as_array()
    }
    .ok_or_else(|| ToolkitError::invalid("removal slot inventory missing"))?;
    if ![
        "lycheedev.slots.v1",
        "lycheedev.slots.v2",
        "lycheedev.slots.v3",
        "lycheedev.slots.v4",
    ]
    .contains(&schema)
        || pool["state"] != "ready"
        || array.is_empty()
        || array.len() > 500
    {
        return conflict("removal slot inventory shape is invalid");
    }
    Ok(array.len())
}
pub fn addon(target: &Path, archive: &Path) -> Result<Value> {
    let target = deploy::destination(target)?;
    let parent = target.parent().unwrap();
    let archive = deploy::destination(archive)?;
    deploy::outside(parent, &archive)?;
    let guard = InstallationGuard::acquire(&target)?;
    guard.require_idle()?;
    let _lock = deploy::installation_lock(&target)?;
    let journal = companion(&archive, ".removal.json");
    let slot_archive = companion(&archive, ".slots");
    let marker = parent.join(".lycheedev-slots.json");
    if !journal.exists() && !marker.exists() {
        drop(_lock);
        return deploy::remove(&target, &archive, "addon");
    }
    let intent: Value = if journal.exists() {
        let root = journal.parent().unwrap();
        storage::guard_path(root, &journal)?;
        storage::read_json(&journal)?
    } else {
        let assessment = installation::inspect(&target, "addon")?;
        if assessment["state"] != "managed" {
            return conflict("only unchanged managed addon can be archived");
        }
        if archive.exists() || slot_archive.exists() {
            return conflict("explicit recovery destination already exists");
        }
        let pool: Value = storage::read_json(&marker)?;
        let count = members(&pool)?;
        for index in 1..=count {
            validate_slot(&slots::directory(parent, index as u16), index as u16, &pool)?;
        }
        let mut controls = vec![];
        let migration = parent.join(".lycheedev-slots.migration-v4.json");
        if migration.exists() {
            storage::guard_path(parent, &migration)?;
            let value: Value = storage::read_json(&migration)?;
            if value["schema"] != "lycheedev.slots.migration.v4"
                || value["parent"] != serde_json::to_value(parent)?
            {
                return conflict("migration control identity differs");
            }
            controls.push(json!({"name":".lycheedev-slots.migration-v4.json","sha256":storage::sha256(&fs::read(&migration)?)}));
        }
        let intent = json!({"schema":"lycheedev.addon-removal.v2","parent":parent,"target":target,"archive":archive,"slotArchive":slot_archive,"pool":pool,"receipt":assessment["receipt"],"controls":controls});
        storage::atomic_json(&journal, &intent)?;
        intent
    };
    if intent["schema"] != "lycheedev.addon-removal.v2"
        || intent["parent"] != serde_json::to_value(parent)?
        || intent["target"] != serde_json::to_value(&target)?
        || intent["archive"] != serde_json::to_value(&archive)?
        || intent["slotArchive"] != serde_json::to_value(&slot_archive)?
    {
        return conflict("removal intent does not match explicit selection");
    }
    let pool = &intent["pool"];
    let count = members(pool)?;
    if !slot_archive.exists() {
        fs::create_dir(&slot_archive)?;
    }
    storage::guard_path(slot_archive.parent().unwrap(), &slot_archive)?;
    let mut moves = Vec::with_capacity(count + 2);
    for index in 1..=count {
        let source = slots::directory(parent, index as u16);
        let destination = slots::directory(&slot_archive, index as u16);
        let (path, moved) = location(&source, &destination)?;
        validate_slot(&path, index as u16, pool)?;
        moves.push((source, destination, moved));
    }
    let destination_marker = slot_archive.join(".lycheedev-slots.json");
    let (path, moved) = location(&marker, &destination_marker)?;
    if storage::read_json::<Value>(&path)? != *pool {
        return conflict("slot manifest differs from removal intent");
    }
    moves.push((marker, destination_marker, moved));
    for control in intent
        .get("controls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let name = control["name"].as_str().unwrap_or("");
        if name != ".lycheedev-slots.migration-v4.json" {
            return conflict("unknown owned control member");
        }
        let source = parent.join(name);
        let destination = slot_archive.join(name);
        let (path, moved) = location(&source, &destination)?;
        storage::guard_path(path.parent().unwrap(), &path)?;
        if storage::sha256(&fs::read(&path)?) != control["sha256"].as_str().unwrap_or("") {
            return conflict("owned control changed during removal");
        }
        moves.push((source, destination, moved));
    }
    let (path, moved) = location(&target, &archive)?;
    let assessment = installation::inspect(&path, "addon")?;
    if assessment["state"] != "managed" || assessment["receipt"] != intent["receipt"] {
        return conflict("main addon differs from removal intent");
    }
    moves.push((target, archive.clone(), moved));
    for (source, destination, moved) in moves {
        if !moved {
            if destination.exists() {
                return conflict("removal destination appeared during transaction");
            }
            fs::rename(source, destination)?;
        }
    }
    Ok(
        json!({"state":"archived","archive":archive,"slotArchive":slot_archive,"receipt":intent["receipt"]}),
    )
}
