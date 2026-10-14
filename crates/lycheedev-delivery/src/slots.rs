use lycheedev_core::{Result, ToolkitError, storage};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub const COUNT: u16 = 500;
pub const TRANSPORT: &str = "Lycheedev mailbox protocol v1";
pub const INTERFACES: &str = "120100, 50504, 38002, 16001, 11509, 20506";
pub fn directory(parent: &Path, index: u16) -> PathBuf {
    parent.join(format!("Lychee Dev Slot {index:02}"))
}
pub fn files(index: u16, version: &str) -> [(String, Vec<u8>); 3] {
    let name = format!("Lychee Dev Slot {index:02}");
    [
        (
            format!("{name}.toc"),
            format!("## Interface: {INTERFACES}\n## Title: Lychee Dev input slot {index:02}\n## Version: {version}\n## LoadOnDemand: 1\n## X-Lychee-Slot: {index}\n## X-Lychee-Transport: {TRANSPORT}\nPayload.lua\nLoader.lua\n").into_bytes(),
        ),
        (
            "Loader.lua".into(),
            format!("local envelope = LycheeDevSlotEnvelope\nLycheeDevSlotEnvelope = nil\nlocal ns = LycheeDevInternal\nif ns and ns.SlotRuntime then ns.SlotRuntime.Receive({index}, envelope) end\n").into_bytes(),
        ),
        ("Payload.lua".into(), b"LycheeDevSlotEnvelope = nil\n".to_vec()),
    ]
}

pub fn install(parent: &Path, version: &str) -> Result<Value> {
    let marker = parent.join(".lycheedev-slots.json");
    let migration = parent.join(".lycheedev-slots.migration-v4.json");
    storage::guard_path(parent, &marker)?;
    storage::guard_path(parent, &migration)?;
    if migration.exists() {
        if completed_previous_migration(parent, version)? {
            let raw = fs::read(&migration)?;
            let digest = storage::sha256(&raw);
            let history = parent.join(".lycheedev-slots.history");
            storage::guard_path(parent, &history)?;
            fs::create_dir_all(&history)?;
            let archive = history.join(format!("{digest}.json"));
            storage::guard_path(parent, &archive)?;
            if archive.exists() {
                if fs::read(&archive)? != raw {
                    return Err(ToolkitError::invalid("previous migration history differs"));
                }
                fs::remove_file(&migration)?;
            } else {
                fs::rename(&migration, &archive)?;
            }
        } else {
            return migrate(parent, version);
        }
    }
    let mut pool = if marker.exists() {
        let value: Value = storage::read_json(&marker)?;
        if value["schema"] != "lycheedev.slots.v4" || value["version"] != version {
            return migrate(parent, version);
        }
        value
    } else {
        for index in 1..=COUNT {
            if directory(parent, index).exists() {
                return Err(ToolkitError::new(
                    "delivery.slot_conflict",
                    "unowned slot directory exists",
                    4,
                ));
            }
        }
        let value = json!({"schema":"lycheedev.slots.v4","version":version,"state":"installing","transport":TRANSPORT,"slotCount":COUNT,"generation":1,"modules":expected_modules(version)});
        storage::atomic_json(&marker, &value)?;
        value
    };
    if pool["state"] == "ready" {
        inspect(parent, version)?;
        return Ok(pool);
    }
    let mut modules = vec![];
    for index in 1..=COUNT {
        let target = directory(parent, index);
        let expected = files(index, version);
        if target.exists() {
            storage::guard_path(parent, &target)?;
            for (name, want) in &expected {
                if target.join(name).exists() {
                    if fs::read(target.join(name))? != *want {
                        return Err(ToolkitError::new(
                            "delivery.slot_conflict",
                            format!("slot {index} contains unowned or modified bytes"),
                            4,
                        ));
                    }
                } else {
                    storage::atomic_write(&target.join(name), want)?;
                }
            }
            if fs::read_dir(&target)?.count() != 3 {
                return Err(ToolkitError::new(
                    "delivery.slot_conflict",
                    "unlisted slot files",
                    4,
                ));
            }
        } else {
            fs::create_dir(&target)?;
            for (name, bytes) in &expected {
                storage::atomic_write(&target.join(name), bytes)?;
            }
        }
        modules.push(json!({"slot":index,"files":expected.iter().map(|(name,bytes)|json!({"path":name,"bytes":bytes.len(),"sha256":storage::sha256(bytes)})).collect::<Vec<_>>(),"payloadHash":storage::sha256(b"LycheeDevSlotEnvelope = nil\n")}));
    }
    pool["modules"] = json!(modules);
    pool["state"] = json!("ready");
    storage::atomic_json(&marker, &pool)?;
    inspect(parent, version)?;
    Ok(pool)
}
fn expected_modules(version: &str) -> Vec<Value> {
    (1..=COUNT).map(|slot|json!({"slot":slot,"files":files(slot,version).iter().map(|(name,bytes)|json!({"path":name,"bytes":bytes.len(),"sha256":storage::sha256(bytes)})).collect::<Vec<_>>(),"payloadHash":storage::sha256(b"LycheeDevSlotEnvelope = nil\n")})).collect()
}
fn old_static(index: u16, version: &str, transport: &str) -> [(String, Vec<u8>); 2] {
    let name = format!("Lychee Dev Slot {index:02}");
    [(format!("{name}.toc"),format!("## Interface: 120100, 50504, 38002, 16001\n## Title: Lychee Dev input slot {index:02}\n## Version: {version}\n## LoadOnDemand: 1\n## X-Lychee-Slot: {index}\n## X-Lychee-Transport: {transport}\nPayload.lua\nLoader.lua\n").into_bytes()),("Loader.lua".into(),format!("local envelope = LycheeDevSlotEnvelope\nLycheeDevSlotEnvelope = nil\nlocal ns = LycheeDevInternal\nif ns and ns.SlotRuntime then ns.SlotRuntime.Receive({index}, envelope) end\n").into_bytes())]
}
fn completed_previous_migration(parent: &Path, version: &str) -> Result<bool> {
    let journal = parent.join(".lycheedev-slots.migration-v4.json");
    storage::guard_path(parent, &journal)?;
    if fs::metadata(&journal)?.len() > 4 << 20 {
        return Err(ToolkitError::invalid("migration journal exceeds 4 MiB"));
    }
    let intent: Value = storage::read_json(&journal)?;
    if intent["schema"] != "lycheedev.slots.migration.v4"
        || intent["parent"] != serde_json::to_value(fs::canonicalize(parent)?)?
    {
        return Err(ToolkitError::invalid("previous migration identity differs"));
    }
    let previous = intent["version"]
        .as_str()
        .ok_or_else(|| ToolkitError::invalid("previous migration version missing"))?;
    if previous == version {
        return Ok(false);
    }
    let pool = inspect(parent, previous)?;
    if pool["generation"].as_u64()
        != intent["source"]["generation"]
            .as_u64()
            .unwrap_or(0)
            .checked_add(1)
    {
        return Err(ToolkitError::invalid(
            "previous migration completion generation differs",
        ));
    }
    if pool["modules"].as_array().unwrap().iter().any(|module| {
        module["pendingHash"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
            || module["nonce"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
                && module["retiredRuntime"].as_str().unwrap_or("").is_empty()
                && module["retiredProcess"] != true
    }) {
        return Err(ToolkitError::new(
            "delivery.slot_reserved",
            "previous completed migration still has unresolved runtime ownership",
            6,
        ));
    }
    Ok(true)
}
pub fn preflight(parent: &Path, version: &str) -> Result<()> {
    let marker = parent.join(".lycheedev-slots.json");
    storage::guard_path(parent, &marker)?;
    if !marker.exists() {
        for index in 1..=COUNT {
            if directory(parent, index).exists() {
                return Err(ToolkitError::new(
                    "delivery.slot_conflict",
                    "unowned slot directory exists",
                    4,
                ));
            }
        }
        return Ok(());
    }
    if parent.join(".lycheedev-slots.migration-v4.json").exists() {
        completed_previous_migration(parent, version)?;
        return Ok(());
    }
    let pool: Value = storage::read_json(&marker)?;
    if pool["schema"] == "lycheedev.slots.v4" && pool["version"] != version {
        let old_version = pool["version"]
            .as_str()
            .ok_or_else(|| ToolkitError::invalid("old v4 slot version missing"))?;
        inspect(parent, old_version)?;
        for module in pool["modules"].as_array().unwrap() {
            if module["pendingHash"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
                || module["nonce"]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
                    && module["retiredRuntime"].as_str().unwrap_or("").is_empty()
                    && module["retiredProcess"] != true
            {
                return Err(ToolkitError::new(
                    "delivery.slot_reserved",
                    "previous v4 publication/runtime must be explicitly retired before upgrade",
                    6,
                ));
            }
        }
        return Ok(());
    }
    if pool["schema"] == "lycheedev.slots.v4" && pool["version"] == version {
        if pool["state"] == "ready" {
            inspect(parent, version)?;
        }
        return Ok(());
    }
    let schema = pool["schema"].as_str().unwrap_or("");
    if ![
        "lycheedev.slots.v1",
        "lycheedev.slots.v2",
        "lycheedev.slots.v3",
    ]
    .contains(&schema)
        || pool["state"] != "ready"
    {
        return Err(ToolkitError::new(
            "delivery.slot_migration_required",
            "unsupported or unfinished old slot inventory",
            6,
        ));
    }
    let entries = pool["files"]
        .as_array()
        .ok_or_else(|| ToolkitError::invalid("old slot inventory missing"))?;
    if entries.is_empty() || entries.len() > COUNT as usize {
        return Err(ToolkitError::invalid("old slot count exceeds bounds"));
    }
    let transport = if schema.ends_with("v1") {
        "memory-slot-v1"
    } else if schema.ends_with("v2") {
        "memory-slot-v2"
    } else {
        "memory-slot-v3"
    };
    let old_version = pool["version"]
        .as_str()
        .ok_or_else(|| ToolkitError::invalid("old version missing"))?;
    for (index, entry) in entries.iter().enumerate() {
        let slot = (index + 1) as u16;
        let target = directory(parent, slot);
        storage::guard_path(parent, &target)?;
        if entry["pendingHash"].as_str().is_some_and(|v| !v.is_empty())
            || entry["nonce"].as_str().is_some_and(|v| !v.is_empty())
                && entry["retiredRuntime"].as_str().unwrap_or("").is_empty()
                && entry["retiredProcess"] != true
        {
            return Err(ToolkitError::new(
                "delivery.slot_reserved",
                "old runtime must be explicitly retired",
                6,
            ));
        }
        for (name, bytes) in old_static(slot, old_version, transport) {
            let path = target.join(name);
            storage::guard_path(parent, &path)?;
            if fs::read(path)? != bytes {
                return Err(ToolkitError::new(
                    "delivery.slot_conflict",
                    "old static slot bytes differ",
                    4,
                ));
            }
        }
        let payload = target.join("Payload.lua");
        storage::guard_path(parent, &payload)?;
        if storage::sha256(&fs::read(payload)?) != entry["payloadHash"].as_str().unwrap_or("")
            || fs::read_dir(&target)?.count() != 3
        {
            return Err(ToolkitError::new(
                "delivery.slot_conflict",
                "old payload or inventory differs",
                4,
            ));
        }
    }
    for slot in entries.len() as u16 + 1..=COUNT {
        if directory(parent, slot).exists() {
            return Err(ToolkitError::new(
                "delivery.slot_conflict",
                "new slot destination is occupied",
                4,
            ));
        }
    }
    Ok(())
}
fn migrate(parent: &Path, version: &str) -> Result<Value> {
    let marker = parent.join(".lycheedev-slots.json");
    let journal = parent.join(".lycheedev-slots.migration-v4.json");
    storage::guard_path(parent, &marker)?;
    storage::guard_path(parent, &journal)?;
    let intent: Value = if journal.exists() {
        storage::guard_path(parent, &journal)?;
        storage::read_json(&journal)?
    } else {
        preflight(parent, version)?;
        let source: Value = storage::read_json(&marker)?;
        let old_schema = source["schema"].as_str().unwrap_or("");
        if ![
            "lycheedev.slots.v1",
            "lycheedev.slots.v2",
            "lycheedev.slots.v3",
            "lycheedev.slots.v4",
        ]
        .contains(&old_schema)
            || source["state"] != "ready"
        {
            return Err(ToolkitError::new(
                "delivery.slot_migration_required",
                "unsupported or unfinished source slot inventory",
                6,
            ));
        }
        let old_count = if old_schema == "lycheedev.slots.v4" {
            source["modules"].as_array().map(Vec::len)
        } else {
            source["files"].as_array().map(Vec::len)
        }
        .ok_or_else(|| ToolkitError::invalid("source slot inventory missing"))?;
        if old_count == 0 || old_count > COUNT as usize {
            return Err(ToolkitError::invalid("source slot count out of range"));
        }
        let mut originals = vec![];
        for index in 1..=COUNT {
            let target = directory(parent, index);
            if index as usize <= old_count {
                storage::guard_path(parent, &target)?;
                let entry = if old_schema == "lycheedev.slots.v4" {
                    &source["modules"][index as usize - 1]
                } else {
                    &source["files"][index as usize - 1]
                };
                if entry["pendingHash"].as_str().is_some_and(|v| !v.is_empty())
                    || entry["nonce"].as_str().is_some_and(|v| !v.is_empty())
                        && entry["retiredRuntime"].as_str().unwrap_or("").is_empty()
                        && entry["retiredProcess"] != true
                {
                    return Err(ToolkitError::new(
                        "delivery.slot_reserved",
                        "old runtime ownership must be explicitly retired before migration",
                        6,
                    ));
                }
                let name = format!("Lychee Dev Slot {index:02}.toc");
                let names = [name.as_str(), "Loader.lua", "Payload.lua"];
                let mut records = vec![];
                for name in names {
                    let path = target.join(name);
                    storage::guard_path(parent, &path)?;
                    let bytes = fs::read(path)?;
                    if name == "Payload.lua"
                        && storage::sha256(&bytes) != entry["payloadHash"].as_str().unwrap_or("")
                    {
                        return Err(ToolkitError::new(
                            "delivery.slot_conflict",
                            "source payload differs",
                            4,
                        ));
                    }
                    if name.ends_with(".toc") {
                        let toc = String::from_utf8(bytes.clone())
                            .map_err(|e| ToolkitError::invalid(e.to_string()))?;
                        let transport = if old_schema.ends_with("v1") {
                            "memory-slot-v1"
                        } else if old_schema.ends_with("v2") {
                            "memory-slot-v2"
                        } else if old_schema.ends_with("v3") {
                            "memory-slot-v3"
                        } else {
                            TRANSPORT
                        };
                        if !toc.contains(&format!("## X-Lychee-Slot: {index}\n"))
                            || !toc.contains(&format!("## X-Lychee-Transport: {transport}\n"))
                        {
                            return Err(ToolkitError::new(
                                "delivery.slot_conflict",
                                "source static TOC identity differs",
                                4,
                            ));
                        }
                    }
                    records.push(json!({"path":name,"sha256":storage::sha256(&bytes)}));
                }
                if fs::read_dir(&target)?.count() != 3 {
                    return Err(ToolkitError::new(
                        "delivery.slot_conflict",
                        "unlisted source slot files",
                        4,
                    ));
                }
                originals.push(json!({"slot":index,"files":records}));
            } else if target.exists() {
                return Err(ToolkitError::new(
                    "delivery.slot_conflict",
                    "new slot target is already occupied",
                    4,
                ));
            }
        }
        let intent = json!({"schema":"lycheedev.slots.migration.v4","parent":fs::canonicalize(parent)?,"version":version,"source":source,"sourceHash":storage::sha256(&fs::read(&marker)?),"originals":originals,"state":"prepared"});
        storage::atomic_json(&journal, &intent)?;
        intent
    };
    if intent["schema"] != "lycheedev.slots.migration.v4"
        || intent["version"] != version
        || intent["parent"] != serde_json::to_value(fs::canonicalize(parent)?)?
    {
        return Err(ToolkitError::invalid(
            "migration intent differs from explicit destination",
        ));
    }
    let current: Value = storage::read_json(&marker)?;
    if current["schema"] == "lycheedev.slots.v4"
        && current["version"] == version
        && current["state"] == "ready"
    {
        return inspect(parent, version);
    }
    if storage::sha256(&fs::read(&marker)?) != intent["sourceHash"].as_str().unwrap_or("") {
        return Err(ToolkitError::new(
            "delivery.slot_conflict",
            "source slot manifest changed",
            4,
        ));
    }
    for index in 1..=COUNT {
        let target = directory(parent, index);
        if !target.exists() {
            fs::create_dir(&target)?;
        }
        storage::guard_path(parent, &target)?;
        for (name, want) in files(index, version) {
            let path = target.join(&name);
            if path.exists() {
                storage::guard_path(parent, &path)?;
                let bytes = fs::read(&path)?;
                let old = intent["originals"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| v["slot"] == index)
                    .and_then(|v| v["files"].as_array())
                    .and_then(|records| records.iter().find(|v| v["path"] == name))
                    .and_then(|v| v["sha256"].as_str());
                if bytes != want && old != Some(storage::sha256(&bytes).as_str()) {
                    return Err(ToolkitError::new(
                        "delivery.slot_conflict",
                        "partially migrated slot has modified bytes",
                        4,
                    ));
                }
            }
            storage::atomic_write(&path, &want)?;
        }
        if fs::read_dir(&target)?.count() != 3 {
            return Err(ToolkitError::invalid("unlisted migration files"));
        }
    }
    let generation = intent["source"]["generation"].as_u64().unwrap_or(0) + 1;
    let pool = json!({"schema":"lycheedev.slots.v4","version":version,"state":"ready","transport":TRANSPORT,"slotCount":COUNT,"generation":generation,"modules":expected_modules(version)});
    storage::atomic_json(&marker, &pool)?;
    inspect(parent, version)?;
    Ok(pool)
}
pub fn inspect(parent: &Path, version: &str) -> Result<Value> {
    let marker = parent.join(".lycheedev-slots.json");
    storage::guard_path(parent, &marker)?;
    let pool: Value = storage::read_json(&marker)?;
    if pool["schema"] != "lycheedev.slots.v4"
        || pool["version"] != version
        || pool["state"] != "ready"
        || pool["transport"] != TRANSPORT
        || pool["slotCount"] != COUNT
        || pool["modules"]
            .as_array()
            .is_none_or(|m| m.len() != COUNT as usize)
    {
        return Err(ToolkitError::new(
            "delivery.slot_inventory_mismatch",
            "slot inventory identity differs",
            4,
        ));
    }
    for (index, module) in pool["modules"].as_array().unwrap().iter().enumerate() {
        let slot = (index + 1) as u16;
        if module["slot"] != slot {
            return Err(ToolkitError::invalid("slot index differs"));
        }
        let target = directory(parent, slot);
        storage::guard_path(parent, &target)?;
        for (name, want) in files(slot, version) {
            let path = target.join(&name);
            storage::guard_path(parent, &path)?;
            if name == "Payload.lua" {
                let actual = storage::sha256(&fs::read(path)?);
                if actual != module["payloadHash"].as_str().unwrap_or("")
                    && module["pendingHash"].as_str() != Some(actual.as_str())
                {
                    return Err(ToolkitError::new(
                        "delivery.slot_payload_changed",
                        "published payload differs from manifest",
                        4,
                    ));
                }
            } else if fs::read(path)? != want {
                return Err(ToolkitError::new(
                    "delivery.slot_inventory_mismatch",
                    "slot static file changed",
                    4,
                ));
            }
        }
        if fs::read_dir(target)?.count() != 3 {
            return Err(ToolkitError::invalid("unlisted slot files"));
        }
    }
    Ok(pool)
}
pub fn publish(
    identity: &crate::installation::VerifiedInstallation,
    index: u16,
    expected_sha256: Option<&str>,
    bytes: &[u8],
) -> Result<()> {
    if !(1..=COUNT).contains(&index) || bytes.len() > 2 << 20 {
        return Err(ToolkitError::invalid("slot or payload exceeds bounds"));
    }
    let parent = identity.canonical_path.parent().unwrap();
    let _guard = crate::deploy::installation_lock(&identity.canonical_path)?;
    if storage::sha256(&fs::read(
        identity.canonical_path.join(crate::installation::MARKER),
    )?) != identity.manifest_sha256
    {
        return Err(ToolkitError::new(
            "delivery.installation_changed",
            "managed receipt changed",
            4,
        ));
    }
    let mut pool = inspect(parent, lycheedev_core::VERSION)?;
    if pool["generation"] != identity.generation {
        return Err(ToolkitError::new(
            "delivery.slot_generation_changed",
            "slot pool changed",
            4,
        ));
    }
    let module = &mut pool["modules"][index as usize - 1];
    let current = module["payloadHash"].as_str().unwrap_or("").to_owned();
    let digest = storage::sha256(bytes);
    if let Some(pending) = module["pendingHash"].as_str()
        && pending != digest
    {
        return Err(ToolkitError::new(
            "delivery.slot_publication_pending",
            "different immutable publication still owns this slot",
            6,
        )
        .with_details(json!({"slot":index,"pendingHash":pending})));
    }
    if expected_sha256.is_some_and(|digest| digest != current) {
        if current == digest && module.get("pendingHash").is_none() {
            return Ok(());
        }
        return Err(ToolkitError::new(
            "delivery.slot_payload_changed",
            "slot expected digest differs",
            4,
        ));
    }
    module["pendingHash"] = json!(digest);
    storage::atomic_json(&parent.join(".lycheedev-slots.json"), &pool)?;
    let target = directory(parent, index).join("Payload.lua");
    let actual = storage::sha256(&fs::read(&target)?);
    if actual != digest {
        if actual != current {
            return Err(ToolkitError::new(
                "delivery.slot_payload_changed",
                "slot changed outside its exact publication intent",
                4,
            ));
        }
        storage::atomic_write(&target, bytes)?;
    }
    pool["modules"][index as usize - 1]["payloadHash"] = json!(digest);
    pool["modules"][index as usize - 1]
        .as_object_mut()
        .unwrap()
        .remove("pendingHash");
    storage::atomic_json(&parent.join(".lycheedev-slots.json"), &pool)
}
