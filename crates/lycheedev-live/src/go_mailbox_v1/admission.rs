//! Formal CON project admission, distinct from the legacy OP SQLite Book.
//!
//! Oracle: Go 3.2.0 `3057a9666ab078270b63c7f06de748069b0d1f4b`,
//! channel/{project_windows,client_identity}.go and
//! journal/{window_owner,installation_admission,window_run}.go.
//! Only filesystem coordination lives here. The caller still resolves and
//! authenticates the selected client, deployment and runtime before creating a
//! connection, and checks native identity/readiness before every input effect.
//! Activation records (which legitimately precede a channel log) are separate.
//!
//! A fresh connection persists target -> first log -> shared claim. A crash
//! before claim publication leaves evidence, not permission to reconstruct a
//! claim. A crash after publication allows only original-CON journal recovery.
//! Claim removal never deletes logs/results or rewrites unknown business state.
use super::driver::{Clock, TokenSource, budget};
use super::durable_journal::{
    self, ChannelPaths, DurableChannelJournal, Lease, SeedPoint, WindowOwner, close_file, error,
    io_error, json, ordinary_path, read_file, seed_connection, storage_error,
};
use super::observations::{ChannelError, ChannelResult, check_control};
use super::state::{CHANNEL_SCHEMA, DEFAULT_CONNECT_MILLIS, State};
use super::{Identity, parse_token};
use lycheedev_core::{control::OperationControl, storage};
use serde::{
    Serialize,
    de::{DeserializeSeed, MapAccess, Visitor},
};
use std::{
    fmt, fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const OWNER_SCHEMA: &str = "lycheedev.window-owner.v1";
const TARGET_SCHEMA: &str = "lycheedev.channel-target.v1";

/// Go's WindowOccupied payload remains available to a future host. In
/// particular foreign ownership is not collapsed into an ordinary active lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmissionError {
    Channel(ChannelError),
    Occupied { owner: WindowOwner, foreign: bool },
    Joined(Vec<AdmissionError>),
}
pub type AdmissionResult<T> = Result<T, AdmissionError>;
impl From<ChannelError> for AdmissionError {
    fn from(e: ChannelError) -> Self {
        Self::Channel(e)
    }
}
impl From<super::ContractError> for AdmissionError {
    fn from(e: super::ContractError) -> Self {
        Self::Channel(e.into())
    }
}
impl fmt::Display for AdmissionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Channel(e) => e.fmt(f),
            Self::Occupied {
                owner,
                foreign: true,
            } => write!(
                f,
                "journal.foreign_owner: workspace={} operation={}",
                owner.workspace_id, owner.operation_id
            ),
            Self::Occupied {
                owner,
                foreign: false,
            } => write!(f, "journal.window_owned: {}", owner.operation_id),
            Self::Joined(errors) => {
                for (i, e) in errors.iter().enumerate() {
                    if i != 0 {
                        writeln!(f)?;
                    }
                    e.fmt(f)?;
                }
                Ok(())
            }
        }
    }
}
impl std::error::Error for AdmissionError {}
impl AdmissionError {
    pub fn is_busy(&self) -> bool {
        match self {
            Self::Occupied { .. } => true,
            Self::Channel(e) => e.any_cause(
                &|e| matches!(e, ChannelError::Contract(c) if c.0 == "journal.resource_busy"),
            ),
            Self::Joined(es) => es.iter().any(Self::is_busy),
        }
    }
}
fn occupied(owner: WindowOwner, workspace: &str) -> AdmissionError {
    let foreign = owner.workspace_id != workspace;
    AdmissionError::Occupied { owner, foreign }
}
fn finish<T>(result: AdmissionResult<T>, close: ChannelResult<()>) -> AdmissionResult<T> {
    match (result, close) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(e), Ok(())) => Err(e),
        (Ok(_), Err(e)) => Err(e.into()),
        (Err(e), Err(close)) => Err(AdmissionError::Joined(vec![e, close.into()])),
    }
}

/// Persistence-only projection of fixed Go selection.ClientInstallation. It
/// deliberately excludes newer Rust presentation fields. Identity selection
/// remains in core; this record performs no filesystem discovery or inference.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientRecord {
    pub directory: String,
    pub product: String,
    pub product_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub catalog_product: String,
    pub full_build: String,
    pub interface: i64,
    pub toc: String,
    pub identity_source: String,
}
impl From<&lycheedev_core::client::ClientInstallation> for ClientRecord {
    fn from(client: &lycheedev_core::client::ClientInstallation) -> Self {
        Self {
            directory: client.directory.clone(),
            product: client.product.clone(),
            product_code: client.product_code.clone(),
            catalog_product: client.catalog_product.clone(),
            full_build: client.full_build.clone(),
            interface: i64::from(client.interface),
            toc: client.toc.clone(),
            identity_source: client.identity_source.clone(),
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowRecord {
    #[serde(serialize_with = "decimal_u64")]
    pub handle: u64,
    pub process_id: u32,
    #[serde(serialize_with = "decimal_u64")]
    pub process_started_at: u64,
    pub executable: String,
    pub class: String,
    pub title: String,
}
fn decimal_u64<S: serde::Serializer>(v: &u64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&v.to_string())
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConnectionTarget {
    pub client: ClientRecord,
    pub window: WindowRecord,
}
impl ConnectionTarget {
    pub fn resource(&self) -> String {
        format!(
            "window/{}/{}/{}",
            self.window.process_id, self.window.process_started_at, self.window.handle
        )
    }
    pub fn addon_parent(&self) -> PathBuf {
        Path::new(&self.client.directory)
            .join("Interface")
            .join("AddOns")
    }
    /// Go normalizes only the historical original Forever DataSlot, on a copy.
    /// The CN reusable slot never becomes evidence for the Asia slot, and all
    /// other fields, including title/class/executable, remain exact.
    pub fn same_connection_target(&self, observed: &Self) -> bool {
        if self == observed {
            return true;
        }
        if !self.client.catalog_product.is_empty() || observed.client.catalog_product.is_empty() {
            return false;
        }
        if self.client.product == "forever"
            && self.client.product_code == "wow_forever"
            && self.client.full_build.starts_with("1.60.1.")
            && observed.client.catalog_product == "wow_classic_beta"
        {
            let mut saved = self.clone();
            saved.client.catalog_product = "wow_classic_beta".into();
            return saved == *observed;
        }
        false
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ConnectionMetadata {
    pub schema: String,
    pub target: ConnectionTarget,
    pub owner: WindowOwner,
}
impl Default for ConnectionMetadata {
    fn default() -> Self {
        Self {
            schema: String::new(),
            target: ConnectionTarget::default(),
            owner: WindowOwner {
                schema: String::new(),
                workspace_id: String::new(),
                resource: String::new(),
                operation_id: String::new(),
                intent_sha256: String::new(),
            },
        }
    }
}

// encoding/json-compatible object decoding: ignore unknown fields; known names
// are case-insensitive; repeated scalars update in encounter order; null is a
// no-op for non-pointer fields; repeated nested objects merge into the same
// record. Unlike claim records, project metadata need not be canonical JSON.
pub(crate) trait WireRecord {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error>;
}
pub(crate) struct RecordSeed<'a, T>(pub(crate) &'a mut T);
impl<'de, T: WireRecord> DeserializeSeed<'de> for RecordSeed<'_, T> {
    type Value = ();
    fn deserialize<D: serde::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
        struct RecordVisitor<'a, T>(&'a mut T);
        impl<'de, T: WireRecord> Visitor<'de> for RecordVisitor<'_, T> {
            type Value = ();
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object or null")
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
                Ok(())
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
                while let Some(key) = map.next_key::<String>()? {
                    let folded: String = key
                        .chars()
                        .map(|c| match c {
                            'ſ' => 's',
                            'K' => 'k',
                            _ => c.to_ascii_lowercase(),
                        })
                        .collect();
                    self.0.field(&folded, &mut map)?;
                }
                Ok(())
            }
        }
        d.deserialize_any(RecordVisitor(self.0))
    }
}
/// Shared typed Go object decoder. Callers own their input bound and original
/// control budget; the complete JSON value must be consumed.
pub(crate) fn decode_wire_record<T: WireRecord + Default>(
    raw: &[u8],
) -> Result<T, serde_json::Error> {
    let mut value = T::default();
    let mut decoder = serde_json::Deserializer::from_slice(raw);
    RecordSeed(&mut value).deserialize(&mut decoder)?;
    decoder.end()?;
    Ok(value)
}
pub(crate) fn scalar<'de, A: MapAccess<'de>, T: serde::Deserialize<'de>>(
    field: &mut T,
    map: &mut A,
) -> Result<(), A::Error> {
    if let Some(value) = map.next_value::<Option<T>>()? {
        *field = value;
    }
    Ok(())
}
fn number_string<'de, A: MapAccess<'de>>(field: &mut u64, map: &mut A) -> Result<(), A::Error> {
    if let Some(value) = map.next_value::<Option<String>>()? {
        // Go's quoted-number field accepts the literal string "null" as no-op.
        if value != "null" {
            // Go's uint,string decoder rejects an explicit plus/minus sign;
            // Rust parse::<u64>() alone would incorrectly accept "+19".
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(serde::de::Error::custom("expected quoted unsigned decimal"));
            }
            *field = value.parse().map_err(serde::de::Error::custom)?;
        }
    }
    Ok(())
}
// Parse the original JSON integer lexeme. serde_json's typed i64 decoder
// routes -0 through f64, but fixed Go accepts -0 for the signed Interface
// field. Accepting an arbitrary integral float would also admit 0.0/1e0,
// which Go rejects, so retain and check the actual syntax instead.
pub(crate) fn signed_integer<'de, A: MapAccess<'de>>(
    field: &mut i64,
    map: &mut A,
) -> Result<(), A::Error> {
    if let Some(value) = map.next_value::<Option<Box<serde_json::value::RawValue>>>()? {
        let text = value.get();
        let digits = text.strip_prefix('-').unwrap_or(text);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(serde::de::Error::custom("expected signed decimal integer"));
        }
        *field = text.parse().map_err(serde::de::Error::custom)?;
    }
    Ok(())
}
pub(crate) fn ignored<'de, A: MapAccess<'de>>(map: &mut A) -> Result<(), A::Error> {
    map.next_value::<serde::de::IgnoredAny>().map(|_| ())
}
impl WireRecord for ClientRecord {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "directory" => scalar(&mut self.directory, map),
            "product" => scalar(&mut self.product, map),
            "productcode" => scalar(&mut self.product_code, map),
            "catalogproduct" => scalar(&mut self.catalog_product, map),
            "fullbuild" => scalar(&mut self.full_build, map),
            "interface" => signed_integer(&mut self.interface, map),
            "toc" => scalar(&mut self.toc, map),
            "identitysource" => scalar(&mut self.identity_source, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for WindowRecord {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "handle" => number_string(&mut self.handle, map),
            "processid" => scalar(&mut self.process_id, map),
            "processstartedat" => number_string(&mut self.process_started_at, map),
            "executable" => scalar(&mut self.executable, map),
            "class" => scalar(&mut self.class, map),
            "title" => scalar(&mut self.title, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for ConnectionTarget {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "client" => map.next_value_seed(RecordSeed(&mut self.client)),
            "window" => map.next_value_seed(RecordSeed(&mut self.window)),
            _ => ignored(map),
        }
    }
}
impl WireRecord for WindowOwner {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "workspaceid" => scalar(&mut self.workspace_id, map),
            "resource" => scalar(&mut self.resource, map),
            "operationid" => scalar(&mut self.operation_id, map),
            "intentsha256" => scalar(&mut self.intent_sha256, map),
            _ => ignored(map),
        }
    }
}
impl WireRecord for ConnectionMetadata {
    fn field<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<(), A::Error> {
        match key {
            "schema" => scalar(&mut self.schema, map),
            "target" => map.next_value_seed(RecordSeed(&mut self.target)),
            "owner" => map.next_value_seed(RecordSeed(&mut self.owner)),
            _ => ignored(map),
        }
    }
}
fn decode_metadata(bytes: &[u8]) -> ChannelResult<ConnectionMetadata> {
    let mut value = ConnectionMetadata::default();
    let mut d = serde_json::Deserializer::from_slice(bytes);
    RecordSeed(&mut value)
        .deserialize(&mut d)
        .map_err(|_| error("live.channel_record_invalid"))?;
    d.end().map_err(|_| error("live.channel_record_invalid"))?;
    Ok(value)
}

static NEXT_TARGET_STAGE: AtomicU64 = AtomicU64::new(0);

/// Publish one newly allocated CON target without replacing another creator.
/// Installation admission alone is insufficient: different installations can
/// share one project and, on a token collision, the same target/log pathname.
/// Stage fully, sync, checked-close, then reuse the platform's atomic
/// no-replacement move. No project lock or extra lock ordering is introduced.
fn write_target_once(
    path: &Path,
    bytes: &[u8],
    control: &OperationControl,
    mut staged: impl FnMut() -> ChannelResult<()>,
) -> ChannelResult<()> {
    check_control(control)?;
    let parent = path
        .parent()
        .ok_or_else(|| error("live.channel_project_invalid"))?;
    fs::create_dir_all(parent).map_err(io_error)?;
    let mut selected = None;
    for _ in 0..64 {
        check_control(control)?;
        let temporary = parent.join(format!(
            ".channel-target-{}-{}-{}.part",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            NEXT_TARGET_STAGE.fetch_add(1, Ordering::Relaxed),
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => {
                selected = Some((temporary, file));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(io_error(e)),
        }
    }
    let (temporary, mut file) = selected.ok_or_else(|| error("vault.temporary_collision"))?;
    let write = (|| {
        for chunk in bytes.chunks(65536) {
            check_control(control)?;
            file.write_all(chunk).map_err(io_error)?;
        }
        Ok(())
    })();
    // Always attempt sync and checked close, retaining independent failures.
    // No target is visible before all three stages succeed.
    let sync = file.sync_all().map_err(io_error);
    let close = close_file(file);
    let result = (|| {
        durable_journal::joined([write, sync, close])?;
        staged()?;
        check_control(control)?;
        storage::durable_move_noreplace(&temporary, path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists || fs::symlink_metadata(path).is_ok() {
                error("live.channel_connection_exists")
            } else {
                io_error(e)
            }
        })
    })();
    if result.is_err() {
        // Only our exclusively-created staging file is eligible for cleanup.
        // A winning target/log/claim is never removed, even after uncertainty.
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn marker(scope: &Path, resource: &str) -> PathBuf {
    scope.join(format!("{}.json", storage::sha256(resource.as_bytes())))
}
fn hex_of_length(value: &str, len: usize) -> bool {
    value.len() == len && value.bytes().all(|c| c.is_ascii_hexdigit())
}
fn read_marker(
    path: &Path,
    resource: Option<&str>,
    control: &OperationControl,
) -> ChannelResult<Option<WindowOwner>> {
    check_control(control)?;
    ordinary_path(path, false, true)?;
    let Some(bytes) = read_file(path, 4096, "journal.invalid_window_owner", control)? else {
        return Ok(None);
    };
    let owner: WindowOwner =
        serde_json::from_slice(&bytes).map_err(|_| error("journal.invalid_window_owner"))?;
    let id = ["OP-", "BTP-", "CON-"]
        .iter()
        .find_map(|prefix| owner.operation_id.strip_prefix(*prefix));
    if json(&owner)? != bytes
        || owner.schema != OWNER_SCHEMA
        || resource.is_some_and(|r| r != owner.resource)
        || !hex_of_length(&owner.workspace_id, 32)
        || id.is_none_or(|id| !hex_of_length(id, 32))
        || (!owner.intent_sha256.is_empty() && !hex_of_length(&owner.intent_sha256, 64))
    {
        return Err(error("journal.invalid_window_owner"));
    }
    Ok(Some(owner))
}

/// Preserve Go's `(owner, occupied, error)` distinction: an unreadable existing
/// marker remains occupied. This read never creates a scope or acquires a lock.
#[derive(Debug)]
pub struct OwnerInspection {
    pub owner: Option<WindowOwner>,
    pub occupied: bool,
    pub error: Option<ChannelError>,
}
impl OwnerInspection {
    fn result(self) -> ChannelResult<Option<WindowOwner>> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(self.owner),
        }
    }
}
pub fn inspect_window_owner(
    parent: &Path,
    resource: &str,
    control: &OperationControl,
) -> OwnerInspection {
    let path = (|| {
        check_control(control)?;
        let absolute = std::path::absolute(parent).map_err(io_error)?;
        let parent = fs::canonicalize(&absolute).unwrap_or(absolute);
        Ok::<_, ChannelError>(marker(&parent.join(".lycheedev-window-owners"), resource))
    })();
    match path {
        Err(e) => OwnerInspection {
            owner: None,
            occupied: false,
            error: Some(e),
        },
        Ok(path) => match read_marker(&path, Some(resource), control) {
            Ok(owner) => OwnerInspection {
                occupied: owner.is_some(),
                owner,
                error: None,
            },
            Err(e) => OwnerInspection {
                owner: None,
                occupied: true,
                error: Some(e),
            },
        },
    }
}
struct AdmissionScope {
    path: PathBuf,
    gate: Lease,
}
impl AdmissionScope {
    fn acquire(parent: &Path, control: &OperationControl) -> ChannelResult<Self> {
        check_control(control)?;
        let path = fs::canonicalize(parent)
            .map_err(io_error)?
            .join(".lycheedev-window-owners");
        match fs::create_dir(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(io_error(e)),
        }
        ordinary_path(&path, true, false)?;
        let gate = Lease::acquire(&path, "window-admission", true, control)?;
        Ok(Self { path, gate })
    }
}
fn owner_limit(scope: &Path, control: &OperationControl) -> ChannelResult<()> {
    let mut count = 0;
    for entry in fs::read_dir(scope).map_err(io_error)? {
        check_control(control)?;
        let entry = entry.map_err(io_error)?;
        if entry.file_name().to_string_lossy().ends_with(".json") {
            count += 1;
        }
        if count >= 256 {
            return Err(error("journal.window_owner_limit"));
        }
    }
    Ok(())
}
fn publish_owner(
    scope: &Path,
    owner: &WindowOwner,
    control: &OperationControl,
) -> AdmissionResult<()> {
    let parts: Vec<_> = owner.resource.split('/').collect();
    if parts.len() == 4 {
        let mut entries = Vec::new();
        for entry in fs::read_dir(scope).map_err(io_error)? {
            check_control(control)?;
            entries.push(entry.map_err(io_error)?);
            if entries.len() > 1024 {
                return Err(error("journal.window_owner_limit").into());
            }
        }
        entries.sort_by_key(|e| e.file_name());
        let prefix = format!("{}/", parts[..3].join("/"));
        for entry in entries {
            if !entry.file_name().to_string_lossy().ends_with(".json") {
                continue;
            }
            let existing = read_marker(&entry.path(), None, control)?
                .ok_or_else(|| io_error(std::io::ErrorKind::NotFound.into()))?;
            if existing.resource.starts_with(&prefix) && existing.resource != owner.resource {
                return Err(occupied(existing, &owner.workspace_id));
            }
        }
    }
    storage::atomic_write_controlled(&marker(scope, &owner.resource), &json(owner)?, control)
        .map_err(storage_error)?;
    Ok(())
}

/// Same short installation admission lock used by CON, OP and BTP owners.
/// Any JSON claim refuses maintenance immediately, even if its contents are
/// unreadable. The caller must explicitly close and retain close failures.
pub struct InstallationMaintenance {
    gate: Option<Lease>,
}
impl InstallationMaintenance {
    pub fn close(&mut self) -> ChannelResult<()> {
        self.gate.take().map_or(Ok(()), Lease::close)
    }
}
pub fn acquire_installation_maintenance(
    parent: &Path,
    control: &OperationControl,
) -> ChannelResult<InstallationMaintenance> {
    let scope = AdmissionScope::acquire(parent, control)?;
    let inspect = (|| {
        for entry in fs::read_dir(&scope.path).map_err(io_error)? {
            check_control(control)?;
            if entry
                .map_err(io_error)?
                .file_name()
                .to_string_lossy()
                .ends_with(".json")
            {
                return Err(error("journal.resource_busy"));
            }
        }
        Ok(())
    })();
    if let Err(e) = inspect {
        return durable_journal::joined([Err(e), scope.gate.close()]).map(|_| unreachable!());
    }
    Ok(InstallationMaintenance {
        gate: Some(scope.gate),
    })
}

#[derive(Debug)]
pub struct ConnectionRecord {
    pub metadata: ConnectionMetadata,
    pub state: State,
    pub paths: ChannelPaths,
}
#[derive(Debug, Clone, Default)]
pub struct TargetFilter {
    pub build: String,
    pub product: String,
    pub character: String,
    pub realm: String,
}
/// The formal project-local CON book. It never imports the old Rust claims,
/// takes over another project, mutates the vault OP Book, or drives a game.
pub struct ConnectionBook {
    root: PathBuf,
    workspace: String,
    #[cfg(test)]
    fault: Option<tests::Fault>,
}
impl ConnectionBook {
    pub fn open(project: &Path) -> ChannelResult<Self> {
        // ChannelPaths is the single implementation of canonical project/workspace
        // spelling. This construction is read-only and creates no dummy record.
        let paths = ChannelPaths::new(project, &format!("CON-{}", "0".repeat(32)))?;
        Ok(Self {
            root: paths.project_root().into(),
            workspace: paths.workspace_id()?,
            #[cfg(test)]
            fault: None,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn workspace_id(&self) -> &str {
        &self.workspace
    }
    fn paths(&self, id: &str) -> ChannelResult<ChannelPaths> {
        ChannelPaths::new(&self.root, id)
    }
    fn target_path(paths: &ChannelPaths) -> PathBuf {
        paths
            .log_path()
            .with_file_name(format!("{}.target.json", paths.connection_id()))
    }
    pub fn metadata(
        &self,
        id: &str,
        control: &OperationControl,
    ) -> ChannelResult<ConnectionMetadata> {
        let paths = self.paths(id)?;
        let bytes = read_file(
            &Self::target_path(&paths),
            16384,
            "live.channel_record_invalid",
            control,
        )?
        .ok_or_else(|| io_error(std::io::ErrorKind::NotFound.into()))?;
        let meta = decode_metadata(&bytes)?;
        if meta.schema != TARGET_SCHEMA
            || meta.owner.operation_id != id
            || meta.owner.workspace_id != self.workspace
        {
            return Err(error("live.channel_project_owner_mismatch"));
        }
        Ok(meta)
    }
    /// Read-only status. A missing channel log is not an initial-state seed;
    /// activation status must be handled by the future activation adapter.
    pub fn status(&self, id: &str, control: &OperationControl) -> ChannelResult<ConnectionRecord> {
        let metadata = self.metadata(id, control)?;
        let paths = self.paths(id)?;
        let state = durable_journal::load(&paths, control)?;
        Ok(ConnectionRecord {
            metadata,
            state,
            paths,
        })
    }
    /// Fixed-Go Connect's existing-owner lookup, before runtime discovery. A
    /// matching result still requires acquire/reload before a driver effect.
    pub fn find_selected(
        &self,
        selected: &ConnectionTarget,
        filter: &TargetFilter,
        control: &OperationControl,
    ) -> AdmissionResult<Option<ConnectionRecord>> {
        if (!filter.build.is_empty() && filter.build != selected.client.full_build)
            || (!filter.product.is_empty() && filter.product != selected.client.product)
        {
            return Err(error("live.channel_target_mismatch").into());
        }
        let Some(owner) =
            inspect_window_owner(&selected.addon_parent(), &selected.resource(), control)
                .result()?
        else {
            return Ok(None);
        };
        if owner.workspace_id != self.workspace {
            return Err(occupied(owner, &self.workspace));
        }
        let meta = self.metadata(&owner.operation_id, control)?;
        if !meta.target.same_connection_target(selected) || meta.owner != owner {
            return Err(error("live.channel_ownership_changed").into());
        }
        let record = self.status(&owner.operation_id, control)?;
        if !record.state.identity.runtime.is_empty()
            && ((!filter.character.is_empty()
                && filter.character != record.state.identity.character)
                || (!filter.realm.is_empty() && filter.realm != record.state.identity.realm))
        {
            return Err(error("live.channel_target_mismatch").into());
        }
        Ok(Some(record))
    }
    /// Persist a fresh connection after the host has authenticated deployment,
    /// selected process/runtime and capability. This does not acquire an input
    /// lease or continue binding. Production TokenSource must be cryptographic.
    pub fn create(
        &mut self,
        target: ConnectionTarget,
        identity: Identity,
        clock: &impl Clock,
        tokens: &mut impl TokenSource,
        control: &OperationControl,
    ) -> AdmissionResult<ConnectionRecord> {
        check_control(control)?;
        identity.validate()?;
        let token = tokens.token()?;
        parse_token(&token)?;
        let state = State {
            schema: CHANNEL_SCHEMA.into(),
            id: format!("CON-{token}"),
            owner: token,
            identity,
            connect_budget: Some(budget(clock.unix_millis(), DEFAULT_CONNECT_MILLIS)?),
            progress_version: 1,
            ..State::default()
        };
        let paths = self.paths(&state.id)?;
        let resource = target.resource();
        let owner = WindowOwner {
            schema: OWNER_SCHEMA.into(),
            workspace_id: self.workspace.clone(),
            resource: resource.clone(),
            operation_id: state.id.clone(),
            intent_sha256: storage::sha256(format!("{resource}/{}", state.id).as_bytes()),
        };
        let meta = ConnectionMetadata {
            schema: TARGET_SCHEMA.into(),
            target,
            owner,
        };
        let scope = AdmissionScope::acquire(&meta.target.addon_parent(), control)?;
        let result = (|| {
            if let Some(owner) =
                read_marker(&marker(&scope.path, &resource), Some(&resource), control)?
            {
                return Err(occupied(owner, &self.workspace));
            }
            owner_limit(&scope.path, control)?;
            // A generated CON collision is not permission to rewrite evidence.
            // This path is never used for recovery of an existing connection.
            if fs::symlink_metadata(Self::target_path(&paths)).is_ok()
                || fs::symlink_metadata(paths.log_path()).is_ok()
            {
                return Err(error("live.channel_connection_exists").into());
            }
            write_target_once(&Self::target_path(&paths), &json(&meta)?, control, || {
                self.cut(FaultPoint::TargetStaged, control)
            })?;
            self.cut(FaultPoint::MetadataDurable, control)?;
            seed_connection(&paths, &state, control, |point, c| {
                self.cut(FaultPoint::Seed(point), c)
            })?;
            self.cut(FaultPoint::LogDurable, control)?;
            publish_owner(&scope.path, &meta.owner, control)?;
            self.cut(FaultPoint::ClaimDurable, control)?;
            Ok(ConnectionRecord {
                metadata: meta,
                state,
                paths,
            })
        })();
        finish(result, scope.gate.close())
    }
    /// Existing claim only. The durable adapter acquires admission then the
    /// try-only execution lock and reloads the nonempty chain under that lease.
    pub fn acquire(
        &self,
        id: &str,
        control: &OperationControl,
    ) -> AdmissionResult<(ConnectionRecord, DurableChannelJournal)> {
        let metadata = self.metadata(id, control)?;
        let paths = self.paths(id)?;
        let mut journal = DurableChannelJournal::acquire(
            paths.clone(),
            &metadata.target.addon_parent(),
            metadata.owner.clone(),
            control,
        )?;
        let state = match journal.load(control) {
            Ok(state) => state,
            Err(e) => return finish(Err(e.into()), journal.close()),
        };
        Ok((
            ConnectionRecord {
                metadata,
                state,
                paths,
            },
            journal,
        ))
    }
    /// Go Project.retire: a durable closed connection may retire its exact
    /// claim. A replacement owner is left untouched. Unknown operation/report
    /// fields survive unchanged; closing never invents business completion.
    pub fn retire_closed(
        &mut self,
        id: &str,
        control: &OperationControl,
    ) -> AdmissionResult<ConnectionRecord> {
        let record = self.status(id, control)?;
        if !record.state.closed {
            return Err(error("journal.invalid_transition").into());
        }
        let want = &record.metadata.owner;
        let parent = record.metadata.target.addon_parent();
        if inspect_window_owner(&parent, &want.resource, control)
            .result()?
            .is_some_and(|owner| owner != *want)
        {
            return Ok(record);
        }
        let scope = AdmissionScope::acquire(&parent, control)?;
        let result = (|| {
            let Some(owner) = read_marker(
                &marker(&scope.path, &want.resource),
                Some(&want.resource),
                control,
            )?
            else {
                return Ok(());
            };
            if owner != *want {
                return Err(occupied(owner, &want.workspace_id));
            }
            let execution = Lease::acquire(
                &scope.path,
                &format!("execution:{}", want.resource),
                false,
                control,
            )?;
            let remove = (|| {
                check_control(control)?;
                fs::remove_file(marker(&scope.path, &want.resource)).map_err(io_error)?;
                self.cut(FaultPoint::ClaimRetired, control)?;
                Ok(())
            })();
            finish(remove, execution.close())
        })();
        finish(result, scope.gate.close())?;
        Ok(record)
    }
    fn cut(&mut self, _point: FaultPoint, control: &OperationControl) -> ChannelResult<()> {
        #[cfg(test)]
        if self.fault.as_ref().is_some_and(|f| f.point == _point) {
            return self.fault.take().unwrap().trigger(control);
        }
        check_control(control)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FaultPoint {
    TargetStaged,
    MetadataDurable,
    Seed(SeedPoint),
    LogDurable,
    ClaimDurable,
    ClaimRetired,
}

#[cfg(test)]
#[path = "admission_tests.rs"]
mod tests;

#[path = "activation_store.rs"]
pub mod activation_store;

pub(crate) mod historical;
