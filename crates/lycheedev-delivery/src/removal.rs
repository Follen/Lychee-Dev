//! One durable intent covers the main addon and its owned slot pool.
//! Every moved and remaining member is revalidated before a resumed move.
use crate::{deploy, installation, slots};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, coordination::InstallationMaintenanceLease,
    storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Default, Serialize)]
struct Removal {
    schema: String,
    parent: PathBuf,
    archive: PathBuf,
    pool: slots::SlotPool,
    receipt: Option<IntentReceipt>,
}
// Signed bytes are specific to the Go recovery intent. Actual installation
// markers retain their existing strict unsigned representation and file checks.
// Retain a final negative value until comparison (a conflict), while any i64
// overflow during decoding rejects the whole intent even if later overwritten.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IntentResource {
    path: String,
    bytes: i64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
struct IntentReceipt {
    schema: String,
    component: String,
    version: String,
    commit: String,
    resources: Vec<IntentResource>,
}
// Recovery intents use Go's permissive, ordered encoding/json contract. This
// decoder is deliberately private: actual installed receipts retain their
// strict independent parser, ownership/hash validation and inventory checks.
fn empty_resource() -> IntentResource {
    IntentResource {
        path: String::new(),
        sha256: String::new(),
        bytes: 0,
    }
}
fn empty_receipt() -> IntentReceipt {
    IntentReceipt {
        schema: String::new(),
        component: String::new(),
        version: String::new(),
        commit: String::new(),
        resources: vec![],
    }
}
struct ResourceSeed<'a>(&'a mut IntentResource);
impl<'de> serde::de::DeserializeSeed<'de> for ResourceSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for ResourceSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a removal resource object or null")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        Ok(())
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            match slots::field_name(&key).as_str() {
                "path" => slots::scalar(&mut map, &mut self.0.path)?,
                "sha256" => slots::scalar(&mut map, &mut self.0.sha256)?,
                "bytes" => slots::scalar(&mut map, &mut self.0.bytes)?,
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
struct ResourcesSeed<'a>(&'a mut Vec<IntentResource>, &'a mut usize);
impl<'de> serde::de::DeserializeSeed<'de> for ResourcesSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for ResourcesSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a removal resource array or null")
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
                    .next_element_seed(ResourceSeed(&mut self.0[index]))?
                    .is_none()
                {
                    break;
                }
            } else {
                let mut value = empty_resource();
                if sequence
                    .next_element_seed(ResourceSeed(&mut value))?
                    .is_none()
                {
                    break;
                }
                self.0.push(value);
            }
            index += 1;
        }
        if index == 0 {
            self.0.clear();
        }
        *self.1 = index;
        Ok(())
    }
}
struct ReceiptSeed<'a>(&'a mut Option<IntentReceipt>, &'a mut usize);
impl<'de> serde::de::DeserializeSeed<'de> for ReceiptSeed<'_> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> std::result::Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}
impl<'de> serde::de::Visitor<'de> for ReceiptSeed<'_> {
    type Value = ();
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a removal receipt object or null")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<(), E> {
        // Unlike an embedded struct, a Go pointer is cleared by null. Any later
        // object starts from zero, without inheriting old slice backing state.
        *self.0 = None;
        *self.1 = 0;
        Ok(())
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<(), A::Error> {
        let value = self.0.get_or_insert_with(empty_receipt);
        while let Some(key) = map.next_key::<String>()? {
            match slots::field_name(&key).as_str() {
                "schema" => slots::scalar(&mut map, &mut value.schema)?,
                "component" => slots::scalar(&mut map, &mut value.component)?,
                "version" => slots::scalar(&mut map, &mut value.version)?,
                "commit" => slots::scalar(&mut map, &mut value.commit)?,
                "resources" => map.next_value_seed(ResourcesSeed(&mut value.resources, self.1))?,
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        Ok(())
    }
}
struct RemovalVisitor;
impl<'de> serde::de::Visitor<'de> for RemovalVisitor {
    type Value = Removal;
    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("a removal intent object or null")
    }
    fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Removal, E> {
        Ok(Removal::default())
    }
    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut map: A,
    ) -> std::result::Result<Removal, A::Error> {
        let mut value = Removal::default();
        let mut file_length = 0;
        let mut resource_length = 0;
        while let Some(key) = map.next_key::<String>()? {
            match slots::field_name(&key).as_str() {
                "schema" => slots::scalar(&mut map, &mut value.schema)?,
                "parent" => slots::scalar(&mut map, &mut value.parent)?,
                "archive" => slots::scalar(&mut map, &mut value.archive)?,
                "pool" => {
                    map.next_value_seed(slots::PoolSeed(&mut value.pool, &mut file_length))?
                }
                "receipt" => {
                    map.next_value_seed(ReceiptSeed(&mut value.receipt, &mut resource_length))?
                }
                _ => {
                    map.next_value::<serde::de::IgnoredAny>()?;
                }
            }
        }
        // Keep backing elements across all duplicate nested objects. Truncating
        // after each object would lose Go's later-array reuse semantics.
        value.pool.files.truncate(file_length);
        if let Some(receipt) = &mut value.receipt {
            receipt.resources.truncate(resource_length);
        }
        Ok(value)
    }
}
impl<'de> Deserialize<'de> for Removal {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        deserializer.deserialize_any(RemovalVisitor)
    }
}

fn conflict<T>(message: &str) -> Result<T> {
    Err(ToolkitError::new(
        "delivery.installation_conflict",
        message,
        3,
    ))
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("delivery.invalid_installation", message, 4)
}
fn companion(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}
fn location(source: &Path, destination: &Path) -> Result<(PathBuf, bool)> {
    match (exists(source)?, exists(destination)?) {
        (true, false) => Ok((source.into(), false)),
        (false, true) => Ok((destination.into(), true)),
        _ => conflict("removal member is missing or exists in both locations"),
    }
}
// Compare only after resolving and checking both destinations. Windows
// canonicalize returns verbatim paths while Go v1 intents use normal drive/UNC
// spelling; that representation difference is not a different installation.
fn path_identity(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("removal intent path must be absolute"));
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = path.components();
        let Some(Component::Prefix(prefix)) = components.next() else {
            return Err(invalid("removal path has no disk or UNC prefix"));
        };
        let mut normalized = match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
                PathBuf::from(format!("{}:\\", char::from(drive)))
            }
            Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => {
                let mut value = std::ffi::OsString::from("\\\\");
                value.push(server);
                value.push("\\");
                value.push(share);
                PathBuf::from(value)
            }
            _ => return Err(invalid("removal path is not a disk or UNC destination")),
        };
        for component in components {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => normalized.push(name),
                _ => return Err(invalid("removal path contains unresolved traversal")),
            }
        }
        Ok(normalized)
    }
    #[cfg(not(windows))]
    {
        Ok(path.to_path_buf())
    }
}
fn same_destination(left: &Path, right: &Path) -> Result<bool> {
    // Validate the original namespace before canonicalization can obscure it.
    path_identity(left)?;
    path_identity(right)?;
    Ok(path_identity(&deploy::destination(left)?)? == path_identity(&deploy::destination(right)?)?)
}

fn validate_slot(
    path: &Path,
    index: u16,
    pool: &slots::SlotPool,
    control: &OperationControl,
) -> Result<()> {
    control.check()?;
    let entry = &pool.files[usize::from(index) - 1];
    if entry.unresolved() {
        return conflict("slot reservation is unresolved");
    }
    storage::guard_path(path, path)?;
    if !fs::symlink_metadata(path)?.file_type().is_dir()
        || fs::read_dir(path)?
            .take(4)
            .collect::<std::io::Result<Vec<_>>>()?
            .len()
            != 3
    {
        return Err(invalid("slot has redirected or unlisted files"));
    }
    for (name, want) in slots::static_files(index, &pool.version) {
        control.check()?;
        let actual = slots::read_bounded_controlled(&path.join(&name), 16 << 10, control)?;
        if actual != want {
            return conflict("slot static file differs from removal intent");
        }
    }
    control.check()?;
    let payload = slots::read_bounded_controlled(&path.join("Payload.lua"), 2 << 20, control)?;
    if storage::sha256(&payload) != entry.payload_hash {
        return conflict("slot payload differs from removal intent");
    }
    Ok(())
}
pub fn addon(target: &Path, archive: &Path) -> Result<Value> {
    addon_controlled(target, archive, &OperationControl::default())
}
pub fn addon_controlled(
    target: &Path,
    archive: &Path,
    control: &OperationControl,
) -> Result<Value> {
    addon_with_move(target, archive, control, |source, destination| {
        storage::durable_move_noreplace(source, destination).map_err(ToolkitError::io)
    })
}
fn addon_with_move(
    target: &Path,
    archive: &Path,
    control: &OperationControl,
    move_member: impl FnMut(&Path, &Path) -> Result<()>,
) -> Result<Value> {
    addon_with_cleanup(
        target,
        archive,
        control,
        move_member,
        |_| {},
        storage::ExclusiveLock::close,
        InstallationMaintenanceLease::close,
    )
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RemovalStep {
    Locked,
    Validated,
}
fn addon_with_cleanup(
    target: &Path,
    archive: &Path,
    control: &OperationControl,
    move_member: impl FnMut(&Path, &Path) -> Result<()>,
    mut observe: impl FnMut(RemovalStep),
    close_installation: impl FnOnce(storage::ExclusiveLock) -> Result<()>,
    close_maintenance: impl FnOnce(InstallationMaintenanceLease) -> Result<()>,
) -> Result<Value> {
    control.check()?;
    let target = deploy::destination(target)?;
    let parent = target
        .parent()
        .ok_or_else(|| invalid("addon has no parent"))?;
    if target.file_name().is_none_or(|name| name != "Lychee Dev") {
        return Err(invalid("addon removal target must be Lychee Dev"));
    }
    let archive = deploy::destination(archive)?;
    deploy::outside(parent, &archive)?;
    let guard = InstallationMaintenanceLease::acquire_controlled(&target, control)?;
    let result = (|| {
        control.check()?;
        let lock = deploy::installation_lock_controlled(&target, control)?;
        let result = (|| {
            observe(RemovalStep::Locked);
            addon_locked(&target, &archive, control, move_member, &mut observe)
        })();
        deploy::finish_close(result, close_installation(lock))
    })();
    deploy::finish_close(result, close_maintenance(guard))
}
// Both owners stay held across inventory validation and every durable move.
// Explicit finish never rechecks cancellation after a committed final move.
fn addon_locked(
    target: &Path,
    archive: &Path,
    control: &OperationControl,
    mut move_member: impl FnMut(&Path, &Path) -> Result<()>,
    observe: &mut impl FnMut(RemovalStep),
) -> Result<Value> {
    control.check()?;
    let parent = target
        .parent()
        .ok_or_else(|| invalid("addon has no parent"))?;
    let journal = companion(&archive, ".removal.json");
    let slot_archive = companion(&archive, ".slots");
    let marker = parent.join(slots::MARKER);
    if !exists(&journal)? && !exists(&marker)? {
        let assessment = installation::inspect_main_controlled(target, "addon", control)?;
        control.check()?;
        if assessment["state"] == "absent" {
            return Ok(json!({"state":"absent"}));
        }
        if assessment["state"] != "managed" {
            return conflict("only unchanged managed addon can be archived");
        }
        if exists(&archive)? {
            return conflict("explicit recovery destination already exists");
        }
        observe(RemovalStep::Validated);
        control.check()?;
        move_member(target, archive)?;
        return Ok(json!({"state":"archived","archive":archive,"receipt":assessment["receipt"]}));
    }
    let intent: Removal = if exists(&journal)? {
        serde_json::from_slice(&slots::read_bounded_controlled(&journal, 1 << 20, control)?)
            .map_err(|error| invalid(&error.to_string()))?
    } else {
        let pool = slots::read_pool_controlled(parent, control)?;
        // A new removal is business maintenance for the current 500-slot pool.
        // Historical schemas are accepted only by an already durable v1 intent.
        slots::inspect_controlled(parent, &pool.version, control)?;
        if pool.files.iter().any(slots::SlotFile::unresolved) {
            return conflict("slot reservation is unresolved");
        }
        let assessment = installation::inspect_main_controlled(target, "addon", control)?;
        if assessment["state"] != "managed" {
            return conflict("only unchanged managed addon can be archived");
        }
        if exists(&archive)? || exists(&slot_archive)? {
            return conflict("explicit recovery destination already exists");
        }
        let receipt = serde_json::from_value(assessment["receipt"].clone())
            .map_err(|error| invalid(&error.to_string()))?;
        let intent = Removal {
            schema: "lycheedev.addon-removal.v1".into(),
            parent: parent.into(),
            archive: archive.into(),
            pool,
            receipt: Some(receipt),
        };
        let bytes = serde_json::to_vec_pretty(&intent)?;
        storage::atomic_write_controlled(&journal, &bytes, control)?;
        intent
    };
    if intent.schema != "lycheedev.addon-removal.v1"
        || !same_destination(&intent.parent, parent)?
        || !same_destination(&intent.archive, &archive)?
        || intent.receipt.is_none()
        || !intent.pool.valid_shape()
        || intent.pool.state != "ready"
    {
        return Err(invalid("removal intent does not match explicit selection"));
    }
    if !exists(&slot_archive)? {
        control.check()?;
        fs::create_dir(&slot_archive)?;
    }
    storage::guard_path(
        slot_archive
            .parent()
            .ok_or_else(|| invalid("slot archive has no parent"))?,
        &slot_archive,
    )?;
    if !fs::symlink_metadata(&slot_archive)?.file_type().is_dir() {
        return Err(invalid("slot archive is not a plain directory"));
    }
    let pool = &intent.pool;
    let mut moves = Vec::with_capacity(pool.files.len() + 2);
    for index in 1..=pool.files.len() {
        control.check()?;
        let source = slots::directory(parent, index as u16);
        let destination = slots::directory(&slot_archive, index as u16);
        let (path, moved) = location(&source, &destination)?;
        validate_slot(&path, index as u16, pool, control)?;
        moves.push((source, destination, moved));
    }
    control.check()?;
    let destination_marker = slot_archive.join(slots::MARKER);
    let (path, moved) = location(&marker, &destination_marker)?;
    if slots::read_pool_controlled(
        path.parent()
            .ok_or_else(|| invalid("slot marker has no parent"))?,
        control,
    )? != *pool
    {
        return conflict("slot manifest differs from removal intent");
    }
    moves.push((marker, destination_marker, moved));
    control.check()?;
    let (path, moved) = location(&target, &archive)?;
    let assessment = installation::inspect_main_controlled(&path, "addon", control)?;
    if assessment["state"] != "managed"
        || assessment["receipt"] != serde_json::to_value(&intent.receipt)?
    {
        return conflict("main addon differs from removal intent");
    }
    moves.push((target.into(), archive.into(), moved));
    observe(RemovalStep::Validated);
    for (source, destination, moved) in moves {
        control.check()?;
        if !moved {
            if exists(&destination)? {
                return conflict("removal destination appeared during transaction");
            }
            control.check()?;
            move_member(&source, &destination)?;
        }
    }
    Ok(
        json!({"state":"archived","archive":archive,"slotArchive":slot_archive,"receipt":intent.receipt}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lycheedev-removal-control-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(fs::canonicalize(path).unwrap())
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture(root: &Path) -> (PathBuf, PathBuf) {
        let parent = root.join("AddOns");
        fs::create_dir(&parent).unwrap();
        let target = parent.join("Lychee Dev");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("owned.lua"), b"-- fixture").unwrap();
        let receipt = installation::Receipt {
            schema: "lycheedev.installation.v1".into(),
            component: "addon".into(),
            version: "3.2.0".into(),
            commit: "a".repeat(40),
            resources: vec![installation::Resource {
                path: "addon/owned.lua".into(),
                bytes: 10,
                sha256: storage::sha256(b"-- fixture"),
            }],
        };
        storage::atomic_json(&target.join(installation::MARKER), &receipt).unwrap();
        let control = OperationControl::default();
        let phase = crate::slot_test_diagnostics::SlotPhase::begin(
            "removal.fixture_install",
            Some(&control),
        );
        let result = slots::install_controlled(&parent, "3.2.0", &control);
        phase.finish(result.as_ref().err());
        result.unwrap();
        (target, root.join("archive"))
    }
    #[test]
    fn cancelled_removal_creates_no_intent_or_archive() {
        let temp = Temp::new();
        let (target, archive) = fixture(&temp.0);
        let control = OperationControl::default();
        control.tracked_cancel_handle().unwrap().cancel();
        let error = addon_controlled(&target, &archive, &control).unwrap_err();
        assert_eq!(error.exit_code, 7);
        assert!(target.exists());
        assert!(!archive.exists());
        assert!(!companion(&archive, ".removal.json").exists());
        assert!(!companion(&archive, ".slots").exists());
    }
    #[test]
    fn cancellation_after_first_move_keeps_exact_intent_for_resume() {
        let temp = Temp::new();
        let (target, archive) = fixture(&temp.0);
        let control = OperationControl::default();
        let mut moved = 0;
        let error = addon_with_move(&target, &archive, &control, |source, destination| {
            storage::durable_move_noreplace(source, destination)?;
            moved += 1;
            if moved == 1 {
                control.tracked_cancel_handle().unwrap().cancel();
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.exit_code, 7);
        assert_eq!(moved, 1);
        assert!(target.exists());
        assert!(!archive.exists());
        let journal = companion(&archive, ".removal.json");
        let before = fs::read(&journal).unwrap();
        let slot_archive = companion(&archive, ".slots");
        assert!(slots::directory(&slot_archive, 1).exists());
        assert!(slots::directory(target.parent().unwrap(), 2).exists());
        let result = addon(&target, &archive).unwrap();
        assert_eq!(result["state"], "archived");
        assert_eq!(fs::read(journal).unwrap(), before);
        assert!(slots::directory(&slot_archive, 500).exists());
        assert!(archive.exists());
    }
    #[test]
    fn racing_slot_marker_destination_is_not_overwritten_or_main_moved() {
        let temp = Temp::new();
        let (target, archive) = fixture(&temp.0);
        let slot_archive = companion(&archive, ".slots");
        let destination_marker = slot_archive.join(slots::MARKER);
        let error = addon_with_move(
            &target,
            &archive,
            &OperationControl::default(),
            |source, destination| {
                if destination == destination_marker {
                    fs::write(destination, b"competitor marker")?;
                }
                storage::durable_move_noreplace(source, destination).map_err(ToolkitError::io)
            },
        )
        .unwrap_err();
        assert_eq!(error.exit_code, 5);
        assert_eq!(fs::read(&destination_marker).unwrap(), b"competitor marker");
        assert!(target.exists());
        assert!(target.parent().unwrap().join(slots::MARKER).exists());
        assert!(!archive.exists());
        assert!(companion(&archive, ".removal.json").exists());
    }
    #[cfg(windows)]
    #[test]
    fn normal_drive_removal_intent_resumes_under_verbatim_paths() {
        let temp = Temp::new();
        let (target, archive) = fixture(&temp.0);
        let parent = target.parent().unwrap();
        let pool = slots::read_pool(parent).unwrap();
        let receipt = serde_json::from_value(
            installation::inspect(&target, "addon").unwrap()["receipt"].clone(),
        )
        .unwrap();
        let intent = Removal {
            schema: "lycheedev.addon-removal.v1".into(),
            parent: path_identity(parent).unwrap(),
            archive: path_identity(&archive).unwrap(),
            pool,
            receipt: Some(receipt),
        };
        assert!(!intent.parent.to_string_lossy().starts_with("\\\\?\\"));
        storage::atomic_json(&companion(&archive, ".removal.json"), &intent).unwrap();
        let slot_archive = companion(&archive, ".slots");
        fs::create_dir(&slot_archive).unwrap();
        storage::durable_move_noreplace(
            &slots::directory(parent, 1),
            &slots::directory(&slot_archive, 1),
        )
        .unwrap();
        assert_eq!(addon(&target, &archive).unwrap()["state"], "archived");
        assert!(archive.exists());
    }
    #[cfg(windows)]
    #[test]
    fn unc_identity_preserves_server_share_and_rejects_device_namespaces() {
        let normal = Path::new(r"\\server\share\recovery\addon");
        let verbatim = Path::new(r"\\?\UNC\server\share\recovery\addon");
        assert_eq!(
            path_identity(normal).unwrap(),
            path_identity(verbatim).unwrap()
        );
        for foreign in [
            r"\\server\other\recovery\addon",
            r"\\other\share\recovery\addon",
        ] {
            assert_ne!(
                path_identity(normal).unwrap(),
                path_identity(Path::new(foreign)).unwrap()
            );
        }
        for invalid in [
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\archive",
            r"\\.\C:\archive",
            r"C:relative",
        ] {
            assert!(path_identity(Path::new(invalid)).is_err());
        }
    }
    #[test]
    fn removal_decoder_merges_nested_objects_and_pointer_null_exactly() {
        let intent:Removal=serde_json::from_str(r#"{
            "Schema":"first","schema":null,"SCHEMA":"last","Unknown":{"ignored":true},
            "Pool":{"FILES":[{"Nonce":"first"},{"Nonce":"second"}],"Version":"v"},
            "pool":{"Files":[{}]},"POOL":null,"pool":{"files":[null,{}]},
            "Receipt":{"Version":"v","RESOURCES":[{"PATH":"a","Bytes":7},{"Path":"b","SHA256":"saved"}]},
            "receipt":{"Version":null,"resources":[{}],"ignored":true},
            "RECEIPT":{"Resources":[null,{"bytes":9,"unknown":null}]}
        }"#).unwrap();
        assert_eq!(intent.schema, "last");
        assert_eq!(intent.pool.version, "v");
        assert_eq!(intent.pool.files.len(), 2);
        assert_eq!(intent.pool.files[0].nonce, "first");
        assert_eq!(intent.pool.files[1].nonce, "second");
        let receipt = intent.receipt.unwrap();
        assert_eq!(receipt.version, "v");
        assert_eq!(receipt.resources.len(), 2);
        assert_eq!(receipt.resources[0].path, "a");
        assert_eq!(receipt.resources[0].bytes, 7);
        assert_eq!(receipt.resources[1].path, "b");
        assert_eq!(receipt.resources[1].sha256, "saved");
        assert_eq!(receipt.resources[1].bytes, 9);
        for reset in ["null", "[]"] {
            let text = format!(
                r#"{{"receipt":{{"resources":[{{"path":"old"}}]}},"Receipt":{{"resources":{reset}}},"receipt":{{"resources":[null]}}}}"#
            );
            let intent: Removal = serde_json::from_str(&text).unwrap();
            assert_eq!(intent.receipt.unwrap().resources[0], empty_resource());
        }
        let cleared: Removal = serde_json::from_str(
            r#"{"receipt":{"version":"old","resources":[{"path":"old"}]},"Receipt":null}"#,
        )
        .unwrap();
        assert!(cleared.receipt.is_none());
        let reset:Removal=serde_json::from_str(r#"{"receipt":{"version":"old","resources":[{"path":"old"}]},"Receipt":null,"receipt":{"Component":"addon","resources":[{}]}}"#).unwrap();
        let receipt = reset.receipt.unwrap();
        assert!(receipt.version.is_empty());
        assert_eq!(receipt.component, "addon");
        assert_eq!(receipt.resources[0], empty_resource());
    }
    #[test]
    fn cased_repeated_recovery_intent_resumes_without_relaxing_installed_receipt() {
        let temp = Temp::new();
        let (target, archive) = fixture(&temp.0);
        let parent = target.parent().unwrap();
        let pool = slots::read_pool(parent).unwrap();
        let mut receipt = installation::inspect(&target, "addon").unwrap()["receipt"].clone();
        let object = receipt.as_object_mut().unwrap();
        let mut cased = serde_json::Map::new();
        for (key, value) in std::mem::take(object) {
            cased.insert(key.to_ascii_uppercase(), value);
        }
        cased.insert("unknownReceipt".into(), json!({"ignored":true}));
        for resource in cased.get_mut("RESOURCES").unwrap().as_array_mut().unwrap() {
            let mut fields = serde_json::Map::new();
            for (key, value) in std::mem::take(resource.as_object_mut().unwrap()) {
                fields.insert(key.to_ascii_uppercase(), value);
            }
            fields.insert("unknownResource".into(), json!(true));
            *resource = Value::Object(fields);
        }
        let zero_rows = vec![Value::Null; 500];
        let text = format!(
            r#"{{"Schema":"lycheedev.addon-removal.v1","Parent":{},"Archive":{},"Pool":{},"pool":{{"Files":[null]}},"POOL":{{"FILES":{}}},"Receipt":{},"receipt":{{"Version":null,"Resources":[{{"unknown":true}}]}},"unknown":null}}"#,
            serde_json::to_string(&path_identity(parent).unwrap()).unwrap(),
            serde_json::to_string(&path_identity(&archive).unwrap()).unwrap(),
            serde_json::to_string(&pool).unwrap(),
            serde_json::to_string(&zero_rows).unwrap(),
            serde_json::to_string(&cased).unwrap()
        );
        let journal = companion(&archive, ".removal.json");
        fs::write(&journal, &text).unwrap();
        let marker = target.join(installation::MARKER);
        let original = fs::read(&marker).unwrap();
        let mut changed: Value = serde_json::from_slice(&original).unwrap();
        changed["unknownReceipt"] = json!(true);
        fs::write(&marker, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(installation::inspect(&target, "addon").is_err());
        fs::write(&marker, &original).unwrap();
        assert_eq!(addon(&target, &archive).unwrap()["state"], "archived");
        assert_eq!(fs::read(&journal).unwrap(), text.as_bytes());
        assert!(slots::directory(&companion(&archive, ".slots"), 500).exists());
    }
    #[test]
    fn recovery_resource_bytes_follow_signed_int64_sequence_and_conflict_order() {
        let valid: Removal =
            serde_json::from_str(r#"{"receipt":{"resources":[{"bytes":-1,"Bytes":10}]}}"#).unwrap();
        assert_eq!(valid.receipt.unwrap().resources[0].bytes, 10);
        for bytes in [
            "18446744073709551615",
            "9223372036854775808",
            "-9223372036854775809",
        ] {
            let raw = format!(r#"{{"receipt":{{"resources":[{{"bytes":{bytes},"Bytes":10}}]}}}}"#);
            assert!(
                serde_json::from_str::<Removal>(&raw).is_err(),
                "overflow later overwritten: {bytes}"
            );
        }
        let negative: Removal =
            serde_json::from_str(r#"{"receipt":{"resources":[{"bytes":-1}]}}"#).unwrap();
        assert_eq!(negative.receipt.unwrap().resources[0].bytes, -1);
        let temp = Temp::new();
        let (target, archive) = fixture(&temp.0);
        let parent = target.parent().unwrap();
        let pool = slots::read_pool(parent).unwrap();
        let receipt = installation::inspect(&target, "addon").unwrap()["receipt"].clone();
        let mut intent = json!({"schema":"lycheedev.addon-removal.v1","parent":parent,"archive":archive,"pool":pool,"receipt":receipt});
        intent["receipt"]["resources"][0]["bytes"] = json!(-1);
        let journal = companion(&archive, ".removal.json");
        let raw = serde_json::to_vec(&intent).unwrap();
        fs::write(&journal, &raw).unwrap();
        let error = addon(&target, &archive).unwrap_err();
        assert_eq!(error.code, "delivery.installation_conflict");
        assert_eq!(error.exit_code, 3);
        assert!(target.exists());
        assert!(!archive.exists());
        assert_eq!(fs::read(&journal).unwrap(), raw);
        // A negative intermediate value that Go later replaces is a valid
        // intent and must reach the same strict installed receipt comparison.
        let raw = String::from_utf8(raw)
            .unwrap()
            .replace("\"bytes\":-1", "\"bytes\":-1,\"Bytes\":10");
        fs::write(&journal, &raw).unwrap();
        assert_eq!(addon(&target, &archive).unwrap()["state"], "archived");
        assert_eq!(fs::read(journal).unwrap(), raw.as_bytes());
    }
}

#[cfg(test)]
mod cleanup_tests;
