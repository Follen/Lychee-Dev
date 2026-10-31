//! Managed 500-slot installation, publication inventory and version upgrades.
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
pub const INTERFACES: &str = "120105, 120100, 50504, 38002, 16001, 11509, 20506";
pub const SCHEMA: &str = "lycheedev.slots.v3";
pub const MARKER: &str = ".lycheedev-slots.json";
const MIGRATION: &str = "lycheedev.slots.migration.v4";
const MANIFEST_LIMIT: usize = 256 << 10;
const STATIC_LIMIT: usize = 16 << 10;
const PAYLOAD_LIMIT: usize = 2 << 20;
const INERT: &[u8] = b"LycheeDevSlotEnvelope = nil\n";

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
        self.files.len() == COUNT as usize
            && match self.schema.as_str() {
                SCHEMA => {
                    matches!(self.state.as_str(), "ready" | "installing")
                        && self.source_schema.is_empty()
                }
                MIGRATION => {
                    self.state == "upgrading"
                        && !self.pending_version.is_empty()
                        && self.source_schema == SCHEMA
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
pub(crate) fn static_files(index: u16, version: &str) -> [(String, Vec<u8>); 2] {
    let [toc, loader, _] = files(index, version);
    [toc, loader]
}
/// Bounded immutable read with the caller's original control and checked close.
pub(crate) fn read_bounded_controlled(
    path: &Path,
    limit: usize,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    use std::io::{Seek, SeekFrom};
    control.check()?;
    let metadata = fs::symlink_metadata(path).map_err(member_io)?;
    if !metadata.file_type().is_file() || metadata.len() > limit as u64 {
        return Err(invalid("slot member is not a bounded regular file"));
    }
    storage::guard_path(
        path.parent()
            .ok_or_else(|| invalid("slot member has no parent"))?,
        path,
    )?;
    let (mut file, digest) = storage::locked_sha256(path, limit as u64, control)?;
    let result = (|| {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::with_capacity(metadata.len() as usize);
        let mut block = [0u8; 65536];
        loop {
            control.check()?;
            let count = file.read(&mut block)?;
            if count == 0 {
                break;
            }
            if count > limit.saturating_sub(bytes.len()) {
                return Err(invalid("slot member exceeds byte limit"));
            }
            bytes.extend_from_slice(&block[..count]);
        }
        if storage::sha256(&bytes) != digest {
            return Err(invalid("slot member changed during read"));
        }
        Ok(bytes)
    })();
    storage::finish_file(file, result, control)
}

fn absent(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(error.into()),
    }
}
// Missing managed members are an incomplete inventory. Access, sharing and
// other I/O failures do not establish corruption or authorize slot repair.
fn member_io(error: std::io::Error) -> ToolkitError {
    if error.kind() == std::io::ErrorKind::NotFound {
        invalid("managed slot member is missing")
    } else {
        error.into()
    }
}
fn plain_directory(parent: &Path, path: &Path) -> Result<()> {
    storage::guard_path(parent, path)?;
    if !fs::symlink_metadata(path)
        .map_err(member_io)?
        .file_type()
        .is_dir()
    {
        return Err(invalid("redirected or non-directory slot"));
    }
    Ok(())
}
fn exact_members(path: &Path) -> Result<()> {
    // Stop at the fourth entry instead of allowing an unbounded foreign tree.
    let entries = fs::read_dir(path)
        .map_err(member_io)?
        .take(4)
        .collect::<std::io::Result<Vec<_>>>()?;
    if entries.len() != 3 {
        return Err(invalid("unexpected slot files"));
    }
    Ok(())
}
#[cfg(test)]
pub(crate) fn read_pool(parent: &Path) -> Result<SlotPool> {
    read_pool_controlled(parent, &OperationControl::default())
}
pub(crate) fn read_pool_controlled(parent: &Path, control: &OperationControl) -> Result<SlotPool> {
    control.check()?;
    let marker = parent.join(MARKER);
    storage::guard_path(parent, &marker)?;
    let pool: SlotPool =
        serde_json::from_slice(&read_bounded_controlled(&marker, MANIFEST_LIMIT, control)?)
            .map_err(|error| invalid(error.to_string()))?;
    if !pool.valid_shape() {
        return Err(invalid("slot inventory shape differs"));
    }
    control.check()?;
    Ok(pool)
}

/// Read only reservation metadata after the caller proved its process/runtime
/// ended. Missing or undecodable metadata stays untouched. Filesystem failures
/// and a readable inventory for another generation remain real failures.
pub(crate) fn retirement_pool(
    parent: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<Option<SlotPool>> {
    control.check()?;
    match fs::symlink_metadata(parent) {
        Ok(metadata) => {
            storage::guard_path(parent, parent)?;
            if !metadata.is_dir() {
                return Err(invalid("slot parent is not a plain directory"));
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let marker = parent.join(MARKER);
    storage::guard_path(parent, &marker)?;
    let metadata = match fs::symlink_metadata(&marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(invalid("slot manifest is not a plain file"));
    }
    if metadata.len() > MANIFEST_LIMIT as u64 {
        return Ok(None);
    }
    let raw = read_bounded_controlled(&marker, MANIFEST_LIMIT, control)?;
    let Ok(pool) = serde_json::from_slice::<SlotPool>(&raw) else {
        return Ok(None);
    };
    // Recognize installation metadata at this retirement boundary only. Older
    // inventories and migration records are readable evidence of a different
    // generation, not lost metadata and never business-operation authority.
    let recognized = !pool.version.is_empty()
        && match pool.schema.as_str() {
            SCHEMA => matches!(pool.files.len(), 200 | 500) && pool.source_schema.is_empty(),
            "lycheedev.slots.v1" => {
                pool.files.len() == 64 && pool.state == "ready" && pool.source_schema.is_empty()
            }
            "lycheedev.slots.v2" => {
                pool.files.len() == 200 && pool.state == "ready" && pool.source_schema.is_empty()
            }
            MIGRATION | "lycheedev.slots.migration.v3" => {
                pool.state == "upgrading"
                    && !pool.pending_version.is_empty()
                    && match pool.source_schema.as_str() {
                        "lycheedev.slots.v1" => pool.files.len() == 64,
                        "lycheedev.slots.v2" => pool.files.len() == 200,
                        SCHEMA => matches!(pool.files.len(), 200 | 500),
                        _ => false,
                    }
            }
            _ => false,
        };
    if !recognized {
        return Ok(None);
    }
    if pool.schema != SCHEMA
        || pool.files.len() != COUNT as usize
        || pool.state != "ready"
        || pool.version != version
    {
        return Err(invalid(
            "slot inventory is not ready for the original version",
        ));
    }
    control.check()?;
    Ok(Some(pool))
}

pub(crate) fn save_pool(parent: &Path, pool: &SlotPool, control: &OperationControl) -> Result<()> {
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
        for (name, want) in static_files((index + 1) as u16, version) {
            if read_bounded_controlled(&target.join(&name), STATIC_LIMIT, control)? != want {
                return Err(invalid("slot static file changed"));
            }
        }
        let digest = storage::sha256(&read_bounded_controlled(
            &target.join("Payload.lua"),
            PAYLOAD_LIMIT,
            control,
        )?);
        if digest != entry.payload_hash && digest != entry.pending_hash {
            return Err(invalid("slot payload drift"));
        }
    }
    Ok(())
}
fn no_extension(parent: &Path, count: usize, control: &OperationControl) -> Result<()> {
    for index in count + 1..=COUNT as usize {
        control.check()?;
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
    writes: Vec<Replacement>,
}
fn upgrade_plan(
    parent: &Path,
    pool: &SlotPool,
    version: &str,
    control: &OperationControl,
) -> Result<UpgradePlan> {
    if pool.schema != MIGRATION
        || pool.state != "upgrading"
        || pool.pending_version != version
        || pool.source_schema != SCHEMA
        || pool.files.len() != COUNT as usize
    {
        return Err(conflict(
            "slot upgrade intent targets another inventory or version",
        ));
    }
    check_idle(pool)?;
    let mut plan = UpgradePlan { writes: vec![] };
    for index in 1..=COUNT {
        control.check()?;
        let target = directory(parent, index);
        plain_directory(parent, &target)?;
        exact_members(&target)?;
        let previous = static_files(index, &pool.version);
        for (name, want) in files(index, version) {
            let path = target.join(&name);
            let payload = name == "Payload.lua";
            let actual = read_bounded_controlled(
                &path,
                if payload { PAYLOAD_LIMIT } else { STATIC_LIMIT },
                control,
            )?;
            if actual == want {
                continue;
            }
            let owned = if payload {
                storage::sha256(&actual) == pool.files[usize::from(index) - 1].payload_hash
            } else {
                actual == previous.iter().find(|(key, _)| key == &name).unwrap().1
            };
            if !owned {
                return Err(invalid("upgrade found unowned or changed slot bytes"));
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
        return no_extension(parent, 0, control);
    }
    let mut pool = read_pool_controlled(parent, control)?;
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
            for (name, expected) in static_files(index, version) {
                control.check()?;
                if read_bounded_controlled(
                    &directory(parent, index).join(name),
                    STATIC_LIMIT,
                    control,
                )? != expected
                {
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
        no_extension(parent, pool.files.len(), control)?;
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
        no_extension(parent, 0, control)?;
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
        read_pool_controlled(parent, control)?
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
        for (name, want) in static_files(index as u16, &pool.version) {
            let path = target.join(&name);
            if absent(&path)? && pool.state == "installing" {
                storage::slot_atomic_write_controlled(&path, &want, control)?;
            } else {
                let actual = read_bounded_controlled(&path, STATIC_LIMIT, control)?;
                if actual != want {
                    return Err(invalid("modified slot static file"));
                }
            }
        }
        let payload = target.join("Payload.lua");
        let actual = if absent(&payload)? && pool.state == "installing" {
            storage::slot_atomic_write_controlled(&payload, INERT, control)?;
            INERT.to_vec()
        } else {
            read_bounded_controlled(&payload, PAYLOAD_LIMIT, control)?
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
        no_extension(parent, pool.files.len(), control)?;
        pool.source_schema = pool.schema;
        pool.schema = MIGRATION.into();
        pool.state = "upgrading".into();
        pool.pending_version = version.into();
        save_pool(parent, &pool, control)?;
    }
    if pool.state != "upgrading" || pool.pending_version != version {
        return Err(conflict("slot migration intent targets another version"));
    }
    let plan = upgrade_plan(parent, &pool, version, control)?;
    // Every byte is admitted before the first replacement, including resumed
    // mixed-static and partially created extension directories.
    for write in plan.writes {
        control.check()?;
        storage::slot_atomic_write_controlled(&write.path, &write.bytes, control)?;
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
    Ok(serde_json::to_value(inspect_typed(
        parent, version, control,
    )?)?)
}
pub(crate) fn inspect_typed(
    parent: &Path,
    version: &str,
    control: &OperationControl,
) -> Result<SlotPool> {
    control.check()?;
    let pool = read_pool_controlled(parent, control)?;
    if pool.schema != SCHEMA || pool.files.len() != COUNT as usize {
        return Err(invalid("business requires the current 500-slot inventory"));
    }
    inspect_pool(parent, &pool, version, control)?;
    Ok(pool)
}
