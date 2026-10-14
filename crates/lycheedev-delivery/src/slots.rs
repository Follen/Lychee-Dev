//! Managed slot installation metadata follows the fixed Go 3.2.0 contract.
//! Old metadata is readable only for maintenance; it never authorizes business input.
use lycheedev_core::{Result, ToolkitError, control::OperationControl, storage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
pub const COUNT: u16 = 500;
pub const TRANSPORT: &str = "Lycheedev mailbox protocol v1";
pub const INTERFACES: &str = "120100, 50504, 38002, 16001, 11509, 20506";
pub const SCHEMA: &str = "lycheedev.slots.v3";
pub const MARKER: &str = ".lycheedev-slots.json";
const PREVIOUS: &str = "lycheedev.slots.v2";
const LEGACY: &str = "lycheedev.slots.v1";
const MIGRATION: &str = "lycheedev.slots.migration.v4";
const PREVIOUS_MIGRATION: &str = "lycheedev.slots.migration.v3";
const MANIFEST_LIMIT: usize = 256 << 10;
const STATIC_LIMIT: usize = 16 << 10;
const PAYLOAD_LIMIT: usize = 2 << 20;
const INERT: &[u8] = b"LycheeDevSlotEnvelope = nil\n";
const PREVIOUS_INTERFACES: &str = "120100, 50504, 38002, 16001";
const PREVIOUS_MAILBOX_INTERFACES: &str = "120100, 50504, 38002, 16001, 11509";

fn is_false(value: &bool) -> bool {
    !value
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotFile {
    pub payload_hash: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pending_hash: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub nonce: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub runtime: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub consumer: String,
    pub consumed: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub retired_runtime: String,
    #[serde(skip_serializing_if = "is_false")]
    pub retired_process: bool,
}
impl SlotFile {
    pub(crate) fn unresolved(&self) -> bool {
        !self.pending_hash.is_empty()
            || !self.nonce.is_empty()
                && !self.consumed
                && self.retired_runtime.is_empty()
                && !self.retired_process
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotPool {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub source_schema: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pending_version: String,
    pub schema: String,
    pub version: String,
    pub state: String,
    pub files: Vec<SlotFile>,
}
// Go's encoding/json applies fields in input order, case-insensitively. Null
// scalars leave their existing value unchanged. Derive/map-first decoding can
// silently erase a reservation ("Nonce":"...","nonce":null), so retain the
// sequence and seed existing slice elements exactly as Go does.
pub(crate) fn field_name(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '\u{017f}' => 's', // Unicode simple-fold equivalence with ASCII S.
            '\u{212a}' => 'k', // Unicode simple-fold equivalence with ASCII K.
            ch => ch.to_ascii_lowercase(),
        })
        .collect()
}
pub(crate) fn scalar<'de, A, T>(map: &mut A, value: &mut T) -> std::result::Result<(), A::Error>
where
    A: serde::de::MapAccess<'de>,
    T: Deserialize<'de>,
{
    if let Some(next) = map.next_value::<Option<T>>()? {
        *value = next;
    }
    Ok(())
}
struct FileSeed<'a>(&'a mut SlotFile);
impl<'de> serde::de::DeserializeSeed<'de> for FileSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for FileSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a slot file object or null")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match field_name(&key).as_str() {
                "payloadhash" => scalar(&mut map, &mut self.0.payload_hash)?,
                "pendinghash" => scalar(&mut map, &mut self.0.pending_hash)?,
                "nonce" => scalar(&mut map, &mut self.0.nonce)?,
                "runtime" => scalar(&mut map, &mut self.0.runtime)?,
                "consumer" => scalar(&mut map, &mut self.0.consumer)?,
                "consumed" => scalar(&mut map, &mut self.0.consumed)?,
                "retiredruntime" => scalar(&mut map, &mut self.0.retired_runtime)?,
                "retiredprocess" => scalar(&mut map, &mut self.0.retired_process)?,
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for SlotFile {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        serde::de::DeserializeSeed::deserialize(FileSeed(&mut value), deserializer)?;
        Ok(value)
    }
}
struct FilesSeed<'a>(&'a mut Vec<SlotFile>, &'a mut usize);
impl<'de> serde::de::DeserializeSeed<'de> for FilesSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for FilesSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a slot file array or null")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        self.0.clear();
        *self.1 = 0;
        Ok(())
    }
    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut sequence: A,
    ) -> std::result::Result<(), A::Error> {
        let mut index = 0;
        loop {
            if index < self.0.len() {
                if sequence
                    .next_element_seed(FileSeed(&mut self.0[index]))?
                    .is_none()
                {
                    break;
                }
            } else {
                let mut value = SlotFile::default();
                if sequence.next_element_seed(FileSeed(&mut value))?.is_none() {
                    break;
                }
                self.0.push(value);
            }
            index += 1;
        }
        // A nonempty Go slice reuses its backing elements even after a
        // preceding duplicate array shortened len. Keep them until the whole
        // object is decoded; null and [] sever that reuse.
        if index == 0 {
            self.0.clear();
        }
        *self.1 = index;
        Ok(())
    }
}
pub(crate) struct PoolSeed<'a>(pub &'a mut SlotPool, pub &'a mut usize);
impl<'de> serde::de::DeserializeSeed<'de> for PoolSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for PoolSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a slot pool object or null")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match field_name(&key).as_str() {
                "sourceschema" => scalar(&mut map, &mut self.0.source_schema)?,
                "pendingversion" => scalar(&mut map, &mut self.0.pending_version)?,
                "schema" => scalar(&mut map, &mut self.0.schema)?,
                "version" => scalar(&mut map, &mut self.0.version)?,
                "state" => scalar(&mut map, &mut self.0.state)?,
                "files" => map.next_value_seed(FilesSeed(&mut self.0.files, self.1))?,
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
impl<'de> Deserialize<'de> for SlotPool {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let mut value = Self::default();
        let mut file_length = 0;
        deserializer.deserialize_any(PoolSeed(&mut value, &mut file_length))?;
        value.files.truncate(file_length);
        Ok(value)
    }
}

impl SlotPool {
    pub(crate) fn valid_shape(&self) -> bool {
        if self.version.is_empty() {
            return false;
        }
        let count = self.files.len();
        let source_count = |schema: &str| match schema {
            SCHEMA => count == COUNT as usize || count == 200,
            PREVIOUS => count == 200,
            LEGACY => count == 64,
            _ => false,
        };
        match self.schema.as_str() {
            SCHEMA => source_count(SCHEMA) && self.source_schema.is_empty(),
            PREVIOUS | LEGACY => {
                source_count(&self.schema) && self.state == "ready" && self.source_schema.is_empty()
            }
            MIGRATION | PREVIOUS_MIGRATION => {
                self.state == "upgrading"
                    && !self.pending_version.is_empty()
                    && source_count(&self.source_schema)
            }
            _ => false,
        }
    }
}
fn invalid(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("delivery.invalid_installation", message, 4)
}
fn conflict(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("delivery.installation_conflict", message, 3)
}
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
pub(crate) fn static_files(index: u16, version: &str, schema: &str) -> [(String, Vec<u8>); 2] {
    let [mut toc, loader, _] = files(index, version);
    if schema == LEGACY || schema == PREVIOUS {
        let transport = if schema == LEGACY {
            "memory-slot-v1"
        } else {
            "memory-slot-v2"
        };
        toc.1 = String::from_utf8(toc.1)
            .expect("generated ASCII slot TOC")
            .replace(INTERFACES, PREVIOUS_INTERFACES)
            .replace(TRANSPORT, transport)
            .into_bytes();
    }
    [toc, loader]
}
pub(crate) fn static_matches(name: &str, actual: &[u8], want: &[u8]) -> bool {
    if actual == want {
        return true;
    }
    if !name.ends_with(".toc") {
        return false;
    }
    let Ok(want) = std::str::from_utf8(want) else {
        return false;
    };
    for interfaces in [INTERFACES, PREVIOUS_MAILBOX_INTERFACES, PREVIOUS_INTERFACES] {
        let toc = want.replacen(
            &format!("## Interface: {INTERFACES}\n"),
            &format!("## Interface: {interfaces}\n"),
            1,
        );
        if actual == toc.as_bytes() {
            return true;
        }
        for retired in ["memory-slot-v3", "lycheedev-mailbox-v1"] {
            let old = toc.replacen(
                &format!("## X-Lychee-Transport: {TRANSPORT}\n"),
                &format!("## X-Lychee-Transport: {retired}\n"),
                1,
            );
            if actual == old.as_bytes() {
                return true;
            }
        }
    }
    false
}
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() > limit as u64 {
        return Err(invalid("slot member is not a bounded regular file"));
    }
    storage::guard_path(
        path.parent()
            .ok_or_else(|| invalid("slot member has no parent"))?,
        path,
    )?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid("slot member exceeds byte limit"));
    }
    Ok(bytes)
}
fn absent(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}
fn plain_directory(parent: &Path, path: &Path) -> Result<()> {
    storage::guard_path(parent, path)?;
    if !fs::symlink_metadata(path)?.file_type().is_dir() {
        return Err(invalid("redirected or non-directory slot"));
    }
    Ok(())
}
fn exact_members(path: &Path) -> Result<()> {
    // Stop at the fourth entry instead of allowing an unbounded foreign tree.
    let entries = fs::read_dir(path)?
        .take(4)
        .collect::<std::io::Result<Vec<_>>>()?;
    if entries.len() != 3 {
        return Err(invalid("unexpected slot files"));
    }
    Ok(())
}
pub(crate) fn read_pool(parent: &Path) -> Result<SlotPool> {
    let marker = parent.join(MARKER);
    storage::guard_path(parent, &marker)?;
    let pool: SlotPool = serde_json::from_slice(&read_bounded(&marker, MANIFEST_LIMIT)?)
        .map_err(|error| invalid(error.to_string()))?;
    if !pool.valid_shape() {
        return Err(invalid("slot inventory shape differs"));
    }
    Ok(pool)
}
fn save_pool(parent: &Path, pool: &SlotPool, control: &OperationControl) -> Result<()> {
    control.check()?;
    let bytes = serde_json::to_vec_pretty(pool)?;
    if bytes.len() > MANIFEST_LIMIT {
        return Err(invalid("slot manifest exceeds 256 KiB"));
    }
    let marker = parent.join(MARKER);
    storage::guard_path(parent, &marker)?;
    storage::atomic_write_controlled(&marker, &bytes, control)
}
fn check_idle(pool: &SlotPool) -> Result<()> {
    if pool.files.iter().any(SlotFile::unresolved) {
        return Err(conflict("slot reservation is unresolved"));
    }
    Ok(())
}
pub(crate) fn inspect_pool(
    parent: &Path,
    pool: &SlotPool,
    version: &str,
    control: &OperationControl,
) -> Result<()> {
    if pool.state != "ready" || pool.version != version {
        return Err(invalid(
            "slot inventory is not ready for the selected version",
        ));
    }
    for (index, entry) in pool.files.iter().enumerate() {
        control.check()?;
        let target = directory(parent, (index + 1) as u16);
        plain_directory(parent, &target)?;
        exact_members(&target)?;
        for (name, want) in static_files((index + 1) as u16, version, &pool.schema) {
            if !static_matches(
                &name,
                &read_bounded(&target.join(&name), STATIC_LIMIT)?,
                &want,
            ) {
                return Err(invalid("slot static file changed"));
            }
        }
        let digest = storage::sha256(&read_bounded(&target.join("Payload.lua"), PAYLOAD_LIMIT)?);
        if digest != entry.payload_hash && digest != entry.pending_hash {
            return Err(invalid("slot payload drift"));
        }
    }
    Ok(())
}
fn no_extension(parent: &Path, count: usize) -> Result<()> {
    for index in count + 1..=COUNT as usize {
        if !absent(&directory(parent, index as u16))? {
            return Err(invalid(format!("unmanaged slot {index}")));
        }
    }
    Ok(())
}
struct Replacement {
    path: PathBuf,
    bytes: Vec<u8>,
}
struct UpgradePlan {
    directories: Vec<PathBuf>,
    writes: Vec<Replacement>,
}
fn upgrade_plan(
    parent: &Path,
    pool: &SlotPool,
    version: &str,
    control: &OperationControl,
) -> Result<UpgradePlan> {
    if pool.state != "upgrading" || pool.pending_version != version {
        return Err(conflict("slot migration intent targets another version"));
    }
    check_idle(pool)?;
    let mut plan = UpgradePlan {
        directories: vec![],
        writes: vec![],
    };
    for index in 1..=COUNT {
        control.check()?;
        let target = directory(parent, index);
        let existing = usize::from(index) <= pool.files.len();
        let missing = absent(&target)?;
        if missing && existing {
            return Err(invalid("existing migration slot is missing"));
        }
        if missing {
            plan.directories.push(target.clone());
        } else {
            plain_directory(parent, &target)?;
        }
        let desired = files(index, version);
        let previous = static_files(index, &pool.version, &pool.source_schema);
        let legacy_target = static_files(index, version, &pool.source_schema);
        if !missing {
            let entries = fs::read_dir(&target)?
                .take(4)
                .collect::<std::io::Result<Vec<_>>>()?;
            if entries.len() > 3
                || existing && entries.len() != 3
                || entries.iter().any(|entry| {
                    !desired
                        .iter()
                        .any(|(name, _)| entry.file_name() == name.as_str())
                })
            {
                return Err(invalid("migration slot contains unexpected files"));
            }
        }
        for (name, want) in desired {
            let path = target.join(&name);
            if absent(&path)? && !existing {
                plan.writes.push(Replacement { path, bytes: want });
                continue;
            }
            let payload = name == "Payload.lua";
            let actual = read_bounded(&path, if payload { PAYLOAD_LIMIT } else { STATIC_LIMIT })?;
            if actual == want {
                continue;
            }
            let allowed = if !existing {
                false
            } else if payload {
                storage::sha256(&actual) == pool.files[usize::from(index) - 1].payload_hash
            } else {
                let old = previous.iter().find(|(key, _)| key == &name).unwrap();
                let legacy = legacy_target.iter().find(|(key, _)| key == &name).unwrap();
                static_matches(&name, &actual, &old.1) || static_matches(&name, &actual, &legacy.1)
            };
            if !allowed {
                return Err(invalid("migration found unowned or changed slot bytes"));
            }
            plan.writes.push(Replacement { path, bytes: want });
        }
    }
    Ok(plan)
}
/// Read-only maintenance admission; no existing or extension bytes are adopted.
pub fn preflight(parent: &Path, version: &str) -> Result<()> {
    preflight_controlled(parent, version, &OperationControl::default())
}
pub fn preflight_controlled(
    parent: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    if absent(&parent.join(MARKER))? {
        return no_extension(parent, 0);
    }
    let mut pool = read_pool(parent)?;
    if pool.state == "upgrading" {
        return upgrade_plan(parent, &pool, version, control).map(|_| ());
    }
    if pool.state == "installing" && pool.schema == SCHEMA {
        return Ok(());
    }
    inspect_pool(parent, &pool, &pool.version, control)?;
    let mut needs_upgrade =
        pool.schema != SCHEMA || pool.version != version || pool.files.len() != COUNT as usize;
    if !needs_upgrade {
        for index in 1..=COUNT {
            for (name, expected) in static_files(index, version, SCHEMA) {
                control.check()?;
                if read_bounded(&directory(parent, index).join(name), STATIC_LIMIT)? != expected {
                    needs_upgrade = true;
                    break;
                }
            }
            if needs_upgrade {
                break;
            }
        }
    }
    if needs_upgrade {
        check_idle(&pool)?;
        no_extension(parent, pool.files.len())?;
        pool.source_schema = pool.schema;
        pool.schema = MIGRATION.into();
        pool.state = "upgrading".into();
        pool.pending_version = version.into();
        upgrade_plan(parent, &pool, version, control)?;
    }
    Ok(())
}
pub fn install(parent: &Path, version: &str) -> Result<Value> {
    install_controlled(parent, version, &OperationControl::default())
}
pub fn install_controlled(
    parent: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    if version.is_empty() {
        return Err(invalid("slot version is empty"));
    }
    let mut pool = if absent(&parent.join(MARKER))? {
        no_extension(parent, 0)?;
        let pool = SlotPool {
            schema: SCHEMA.into(),
            version: version.into(),
            state: "installing".into(),
            files: vec![SlotFile::default(); COUNT as usize],
            ..SlotPool::default()
        };
        save_pool(parent, &pool, control)?;
        pool
    } else {
        read_pool(parent)?
    };
    if pool.state == "upgrading"
        || pool.state == "ready"
            && (pool.version != version
                || pool.schema != SCHEMA
                || pool.files.len() != COUNT as usize)
    {
        return upgrade(parent, pool, version, control);
    }
    if pool.state != "ready" && pool.state != "installing" {
        return Err(invalid("unfinished slot inventory"));
    }
    for index in 1..=pool.files.len() {
        control.check()?;
        let target = directory(parent, index as u16);
        fs::create_dir_all(&target)?;
        plain_directory(parent, &target)?;
        for (name, want) in static_files(index as u16, &pool.version, &pool.schema) {
            let path = target.join(&name);
            if absent(&path)? && pool.state == "installing" {
                storage::atomic_write_controlled(&path, &want, control)?;
            } else {
                let actual = read_bounded(&path, STATIC_LIMIT)?;
                if actual != want {
                    if pool.state == "ready" && static_matches(&name, &actual, &want) {
                        return upgrade(parent, pool, version, control);
                    }
                    return Err(invalid("modified slot static file"));
                }
            }
        }
        let payload = target.join("Payload.lua");
        let actual = if absent(&payload)? && pool.state == "installing" {
            storage::atomic_write_controlled(&payload, INERT, control)?;
            INERT.to_vec()
        } else {
            read_bounded(&payload, PAYLOAD_LIMIT)?
        };
        let entry = &mut pool.files[index - 1];
        let expected = if entry.payload_hash.is_empty() {
            storage::sha256(INERT)
        } else {
            entry.payload_hash.clone()
        };
        if storage::sha256(&actual) != expected {
            return Err(invalid("slot payload drift"));
        }
        entry.payload_hash = expected;
        exact_members(&target)?;
    }
    pool.state = "ready".into();
    save_pool(parent, &pool, control)?;
    if pool.schema != SCHEMA || pool.version != version || pool.files.len() != COUNT as usize {
        return upgrade(parent, pool, version, control);
    }
    Ok(serde_json::to_value(pool)?)
}
fn upgrade(
    parent: &Path,
    mut pool: SlotPool,
    version: &str,
    control: &OperationControl,
) -> Result<Value> {
    if pool.state == "ready" {
        inspect_pool(parent, &pool, &pool.version, control)?;
        check_idle(&pool)?;
        no_extension(parent, pool.files.len())?;
        pool.source_schema = pool.schema;
        pool.schema = MIGRATION.into();
        pool.state = "upgrading".into();
        pool.pending_version = version.into();
        save_pool(parent, &pool, control)?;
    }
    if pool.state != "upgrading" || pool.pending_version != version {
        return Err(conflict("slot migration intent targets another version"));
    }
    if pool.schema == PREVIOUS_MIGRATION {
        pool.schema = MIGRATION.into();
        save_pool(parent, &pool, control)?;
    }
    let plan = upgrade_plan(parent, &pool, version, control)?;
    // Every byte is admitted before the first replacement, including resumed
    // mixed-static and partially created extension directories.
    for directory in plan.directories {
        control.check()?;
        fs::create_dir(&directory)?;
        plain_directory(parent, &directory)?;
    }
    for write in plan.writes {
        control.check()?;
        storage::atomic_write_controlled(&write.path, &write.bytes, control)?;
    }
    pool = SlotPool {
        schema: SCHEMA.into(),
        version: version.into(),
        state: "ready".into(),
        files: vec![
            SlotFile {
                payload_hash: storage::sha256(INERT),
                ..SlotFile::default()
            };
            COUNT as usize
        ],
        ..SlotPool::default()
    };
    save_pool(parent, &pool, control)?;
    Ok(serde_json::to_value(pool)?)
}
pub fn inspect(parent: &Path, version: &str) -> Result<Value> {
    inspect_controlled(parent, version, &OperationControl::default())
}
pub fn inspect_controlled(
    parent: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<Value> {
    control.check()?;
    let pool = read_pool(parent)?;
    if pool.schema != SCHEMA || pool.files.len() != COUNT as usize {
        return Err(invalid("business requires the current 500-slot inventory"));
    }
    inspect_pool(parent, &pool, version, control)?;
    Ok(serde_json::to_value(pool)?)
}
/// The historical byte-only API cannot establish the formal consumer/runtime/
/// nonce/envelope publication contract. Keep it fail-closed until that typed
/// business API and its caller are migrated; never feed arbitrary bytes to v1.
pub fn publish(
    _identity: &crate::installation::VerifiedInstallation,
    _index: u16,
    _expected_sha256: Option<&str>,
    _bytes: &[u8],
) -> Result<()> {
    Err(ToolkitError::new(
        "live.mailbox_v1_migration_incomplete",
        "typed mailbox protocol v1 publication is not connected",
        3,
    ))
}
