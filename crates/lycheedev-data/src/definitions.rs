//! Exact-revision definition resources. Cached bytes are rehashed and parsed;
//! corruption never causes a silent replacement download.
use crate::{fault, schema};
use lycheedev_core::{Result, control::OperationControl, evidence, metadata::Metadata, storage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub(crate) mod semantics;

use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableIdentity {
    pub name: String,
    pub hash: u32,
    #[serde(rename = "db2FileDataId")]
    pub db2_file_data_id: u32,
    #[serde(rename = "dbcFileDataId")]
    pub dbc_file_data_id: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    #[serde(rename = "tableName")]
    name: String,
    #[serde(rename = "tableHash")]
    hash: String,
    #[serde(rename = "db2FileDataID", default)]
    db2: Option<u32>,
    #[serde(rename = "dbcFileDataID", default)]
    dbc: Option<u32>,
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name.bytes().enumerate().all(|(i, b)| {
            b.is_ascii_alphabetic() || b == b'_' || (i > 0 && (b.is_ascii_digit() || b == b'-'))
        })
}
fn invalid() -> lycheedev_core::ToolkitError {
    fault("schema.invalid_definition", 4, "schema.invalid_definition")
}
pub struct Manifest {
    tables: BTreeMap<String, TableIdentity>,
}
impl Manifest {
    pub fn parse(raw: &[u8]) -> Result<Self> {
        if raw.len() > 4 << 20 {
            return Err(fault(
                "schema.limit_exceeded",
                3,
                "Manifest byte budget exceeded",
            ));
        }
        // Struct deserialization rejects duplicate fields; optional file IDs
        // still require explicit non-null integers when the field is present.
        let entries: Vec<Entry> = serde_json::from_slice(raw).map_err(|_| invalid())?;
        if entries.is_empty() || entries.len() > 16384 {
            return Err(invalid());
        }
        // Preserve field presence independently of Option's null semantics.
        let presence: Vec<Value> = serde_json::from_slice(raw).map_err(|_| invalid())?;
        let mut tables = BTreeMap::new();
        let mut modern = BTreeSet::new();
        let mut legacy = BTreeSet::new();
        for (entry, object) in entries.into_iter().zip(presence) {
            if !valid_name(&entry.name)
                || entry.hash.len() != 8
                || !entry.hash.bytes().all(|b| b.is_ascii_hexdigit())
                || ["db2FileDataID", "dbcFileDataID"]
                    .iter()
                    .any(|k| object.get(*k).is_some_and(Value::is_null))
            {
                return Err(invalid());
            }
            if entry.db2.is_some_and(|id| id == 0 || !modern.insert(id))
                || entry.dbc.is_some_and(|id| id == 0 || !legacy.insert(id))
            {
                return Err(invalid());
            }
            let key = entry.name.to_ascii_lowercase();
            let value = TableIdentity {
                name: entry.name,
                hash: u32::from_str_radix(&entry.hash, 16).map_err(|_| invalid())?,
                db2_file_data_id: entry.db2.unwrap_or(0),
                dbc_file_data_id: entry.dbc.unwrap_or(0),
            };
            if tables.insert(key, value).is_some() {
                return Err(invalid());
            }
        }
        Ok(Self { tables })
    }
    pub fn lookup(&self, name: &str) -> Result<TableIdentity> {
        if !valid_name(name) {
            return Err(invalid());
        }
        self.tables
            .get(&name.to_ascii_lowercase())
            .cloned()
            .ok_or_else(|| {
                fault(
                    "schema.definition_not_found",
                    3,
                    "schema.definition_not_found",
                )
            })
    }
    pub fn resolve(&self, name: &str, id: u32, hash: u32) -> Result<TableIdentity> {
        let value = self.lookup(name)?;
        if id == 0 || value.db2_file_data_id != id || value.hash != hash {
            return Err(invalid());
        }
        Ok(value)
    }
}
#[derive(Debug, Serialize)]
pub struct Bundle {
    pub commit: String,
    pub identity: TableIdentity,
    pub manifest: Value,
    pub definition: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Object {
    url: String,
    blob: Value,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    reference: String,
    commit: String,
}
fn reference(value: &str) -> Option<&str> {
    if value.is_empty() {
        return Some("refs/heads/master");
    }
    let suffix = value
        .strip_prefix("refs/heads/")
        .or_else(|| value.strip_prefix("refs/tags/"))?;
    if value.len() > 1024
        || suffix.is_empty()
        || suffix.contains("//")
        || suffix.contains("..")
        || suffix.contains("@{")
        || suffix.split('/').any(|part| {
            part.is_empty()
                || part.starts_with('.')
                || part.ends_with('.')
                || part.ends_with(".lock")
        })
        || value
            .chars()
            .any(|c| c.is_control() || c <= ' ' || "~^:?*[\\".contains(c))
    {
        return None;
    }
    Some(value)
}
/// Online symbolic resolutions refresh each call. Offline aliases are the last
/// validated observation for that exact full ref, never an invented latest pin.
pub fn resolve_revision(home: &Path, value: &str, offline: bool) -> Result<String> {
    if lycheedev_core::selection::valid_digest(value, 40) {
        return Ok(value.to_owned());
    }
    let value = reference(value).ok_or_else(|| {
        fault(
            "records.definition_identity",
            4,
            "Definition reference requires exact commit or valid full ref",
        )
    })?;
    let key = format!("definition-ref/{}", storage::sha256(value.as_bytes()));
    let mut metadata = Metadata::open(home, true)?;
    let cached = match metadata.read::<Revision>(&key) {
        Ok(document) => {
            if document.reference != value
                || !lycheedev_core::selection::valid_digest(&document.commit, 40)
            {
                return Err(fault(
                    "records.definition_identity",
                    4,
                    "Cached revision identity differs",
                ));
            }
            Some(document)
        }
        Err(error) if error.code == "vault.record_missing" => None,
        Err(error) => return Err(error),
    };
    if offline {
        return cached.map(|document| document.commit).ok_or_else(|| {
            fault(
                "records.definition_unavailable_offline",
                3,
                "No validated observation for definition reference",
            )
        });
    }
    let generation = metadata.generation(&key)?;
    let mut escaped = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            escaped.push(b as char);
        } else {
            escaped.push_str(&format!("%{b:02X}"));
        }
    }
    let agent = http_agent(Duration::from_secs(30));
    let mut response = agent
        .get(format!(
            "https://api.github.com/repos/wowdev/WoWDBDefs/commits/{escaped}"
        ))
        .header("Accept", "application/vnd.github.sha")
        .call()
        .map_err(|error| fault("records.definition_http", 5, error.to_string()))?;
    if response.status().as_u16() != 200 {
        return Err(fault(
            "records.definition_http",
            5,
            format!("Definition HTTP status {}", response.status()),
        ));
    }
    let mut raw = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(129)
        .read_to_end(&mut raw)?;
    if raw.len() > 128 {
        return Err(fault(
            "records.metadata_limit",
            3,
            "Revision response byte budget exceeded",
        ));
    }
    let commit = std::str::from_utf8(&raw)
        .map_err(|_| {
            fault(
                "records.definition_identity",
                4,
                "Revision response must be exact commit",
            )
        })?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    if !lycheedev_core::selection::valid_digest(&commit, 40) {
        return Err(fault(
            "records.definition_identity",
            4,
            "Revision response must be exact commit",
        ));
    }
    let revision = Revision {
        reference: value.to_owned(),
        commit: commit.clone(),
    };
    if let Err(error) = metadata.put(&key, &revision, generation) {
        if error.code != "vault.generation_conflict" {
            return Err(error);
        }
        let other: Revision = metadata.read(&key)?;
        if other.reference != value || !lycheedev_core::selection::valid_digest(&other.commit, 40) {
            return Err(fault(
                "records.definition_identity",
                4,
                "Concurrent revision cache is invalid",
            ));
        }
    }
    Ok(commit)
}
pub(crate) fn http_agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .max_redirects(0)
        .timeout_global(Some(timeout))
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .new_agent()
}
fn load(
    home: &Path,
    url: &str,
    offline: bool,
    deadline: Instant,
    budget: Option<&crate::resource::Budget>,
    control: Option<&OperationControl>,
    mut validate: impl FnMut(&[u8]) -> Result<()>,
) -> Result<(Value, Vec<u8>)> {
    let check = || -> Result<()> {
        if let Some(control) = control {
            control.check()?;
        }
        if let Some(budget) = budget {
            budget.check()?;
        }
        Ok(())
    };
    check()?;
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| fault("records.resource_limit", 3, "Definition deadline exceeded"))?;
    let key = format!("definition/{}", storage::sha256(url.as_bytes()));
    let mut metadata = Metadata::open(home, true)?;
    match metadata.read::<Object>(&key) {
        Ok(object) => {
            if object.url != url {
                return Err(fault(
                    "records.definition_identity",
                    4,
                    "Cached definition URL differs",
                ));
            }
            if let Some(budget) = budget {
                let bytes = object.blob["bytes"]
                    .as_u64()
                    .filter(|v| *v <= 4 << 20)
                    .ok_or_else(|| {
                        fault(
                            "records.definition_identity",
                            4,
                            "Invalid cached definition size",
                        )
                    })?;
                budget.charge(crate::resource::Cost::metadata(bytes as usize * 8))?;
            }
            let raw = match control {
                Some(control) => evidence::read_blob_bounded(home, &object.blob, 4 << 20, control)?,
                None => evidence::read_blob(home, &object.blob, 4 << 20)?,
            };
            check()?;
            validate(&raw)?;
            check()?;
            return Ok((object.blob, raw));
        }
        Err(error) if error.code == "vault.record_missing" => {}
        Err(error) => return Err(error),
    }
    if offline {
        return Err(fault(
            "records.definition_unavailable_offline",
            3,
            "Exact definition resource has not been cached",
        ));
    }
    let agent = http_agent(remaining.min(Duration::from_secs(30)));
    if let Some(budget) = budget {
        budget.charge(crate::resource::Cost {
            network_requests: 1,
            ..Default::default()
        })?;
    }
    check()?;
    let response = agent.get(url).header("Accept-Encoding", "identity").call();
    check()?;
    let mut response = response.map_err(|e| fault("records.definition_http", 5, e.to_string()))?;
    if response.status().as_u16() != 200 {
        return Err(fault(
            "records.definition_http",
            5,
            format!("Definition HTTP status {}", response.status()),
        ));
    }
    let mut raw = Vec::new();
    let mut reader = response.body_mut().as_reader().take((4 << 20) + 1);
    let mut chunk = [0; 65536];
    loop {
        check()?;
        let read = reader.read(&mut chunk);
        check()?;
        let n = match read {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if n == 0 {
            break;
        }
        if let Some(budget) = budget {
            budget.charge(crate::resource::Cost {
                retained_bytes: n as u64 * 8,
                metadata_bytes: n as u64 * 8,
                network_bytes: n as u64,
                ..Default::default()
            })?;
        }
        raw.extend_from_slice(&chunk[..n]);
    }
    if raw.len() > 4 << 20 {
        return Err(fault(
            "records.metadata_limit",
            3,
            "Definition byte budget exceeded",
        ));
    }
    check()?;
    validate(&raw)?;
    check()?;
    let blob = match control {
        Some(control) => evidence::publish_blob_bounded(home, &raw, 4 << 20, control)?,
        None => evidence::publish_blob(home, &raw, 4 << 20)?,
    };
    check()?;
    let object = Object {
        url: url.to_owned(),
        blob: blob.clone(),
    };
    let published = match control {
        Some(control) => metadata.put_controlled(&key, &object, None, control),
        None => metadata.put(&key, &object, None),
    };
    match published {
        Ok(()) => {}
        Err(error) if error.code == "vault.generation_conflict" => {
            let other: Object = metadata.read(&key)?;
            if other.url != url || other.blob != blob {
                return Err(fault(
                    "records.definition_identity",
                    4,
                    "Concurrent definition content differs",
                ));
            }
        }
        Err(error) => return Err(error),
    }
    check()?;
    Ok((blob, raw))
}
pub fn prepare(home: &Path, commit: &str, name: &str, offline: bool) -> Result<(Bundle, Vec<u8>)> {
    prepare_until(
        home,
        commit,
        name,
        offline,
        Instant::now() + Duration::from_secs(60),
    )
}
pub fn prepare_until(
    home: &Path,
    commit: &str,
    name: &str,
    offline: bool,
    deadline: Instant,
) -> Result<(Bundle, Vec<u8>)> {
    prepare_budgeted(home, commit, name, offline, deadline, None)
}
pub fn prepare_budgeted(
    home: &Path,
    commit: &str,
    name: &str,
    offline: bool,
    deadline: Instant,
    budget: Option<&crate::resource::Budget>,
) -> Result<(Bundle, Vec<u8>)> {
    Ok(prepare_many_budgeted(home, commit, &[name], offline, deadline, budget)?.remove(0))
}

/// One fixed manifest read and one definition read per distinct table. A domain
/// request shares its caller's deadline and admission ledger across the batch.
pub(crate) fn prepare_many_budgeted(
    home: &Path,
    commit: &str,
    names: &[&str],
    offline: bool,
    deadline: Instant,
    budget: Option<&crate::resource::Budget>,
) -> Result<Vec<(Bundle, Vec<u8>)>> {
    prepare_many_controlled(home, commit, names, offline, deadline, budget, None)
}

fn prepare_many_controlled(
    home: &Path,
    commit: &str,
    names: &[&str],
    offline: bool,
    deadline: Instant,
    budget: Option<&crate::resource::Budget>,
    control: Option<&OperationControl>,
) -> Result<Vec<(Bundle, Vec<u8>)>> {
    if let Some(control) = control {
        control.check()?;
    }
    if let Some(budget) = budget {
        budget.check()?;
    }
    if !lycheedev_core::selection::valid_digest(commit, 40)
        || names.is_empty()
        || names.len() > 32
        || names.iter().any(|name| !valid_name(name))
    {
        return Err(fault(
            "records.definition_identity",
            4,
            "Definitions require an exact lowercase commit and valid table name",
        ));
    }
    let base = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{commit}/");
    let (manifest, raw) = load(
        home,
        &format!("{base}manifest.json"),
        offline,
        deadline,
        budget,
        control,
        |raw| Manifest::parse(raw).map(|_| ()),
    )?;
    let catalog = Manifest::parse(&raw)?;
    let mut result = Vec::with_capacity(names.len());
    let mut seen = BTreeSet::new();
    for name in names {
        if let Some(budget) = budget {
            budget.check()?;
        }
        let identity = catalog.lookup(name)?;
        if !seen.insert(identity.name.clone()) {
            continue;
        }
        let (definition, raw) = load(
            home,
            &format!("{base}definitions/{}.dbd", identity.name),
            offline,
            deadline,
            budget,
            control,
            |raw| schema::Document::parse(raw).map(|_| ()),
        )?;
        result.push((
            Bundle {
                commit: commit.to_owned(),
                identity,
                manifest: manifest.clone(),
                definition,
            },
            raw,
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use crate::resource::{Budget, Cost};
    use lycheedev_core::{control::OperationControl, workspace};
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    };

    #[test]
    fn one_manifest_and_distinct_definitions_share_admission_and_caller_control() {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-definition-batch-fixture")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                storage::utc_now().replace([':', '.'], "-"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        workspace::initialize(&home).unwrap();
        let commit = "cccccccccccccccccccccccccccccccccccccccc";
        let base = format!("https://raw.githubusercontent.com/wowdev/WoWDBDefs/{commit}/");
        let manifest = serde_json::to_vec(&json!([
            {"tableName":"Item","tableHash":"00000001","db2FileDataID":1},
            {"tableName":"ItemSparse","tableHash":"00000002","db2FileDataID":2}
        ]))
        .unwrap();
        let definition =
            b"COLUMNS\nint ID\n\nBUILD 12.1.0.69933\nLAYOUT 00000001\n$id$ID<u32>\n".to_vec();
        let expected_metadata = (manifest.len() + definition.len() * 2) as u64 * 8;
        let mut metadata = Metadata::open(&home, true).unwrap();
        for (suffix, raw) in [
            ("manifest.json", manifest),
            ("definitions/Item.dbd", definition.clone()),
            ("definitions/ItemSparse.dbd", definition),
        ] {
            let url = format!("{base}{suffix}");
            let blob = evidence::publish_blob(&home, &raw, 4 << 20).unwrap();
            metadata
                .put(
                    &format!("definition/{}", storage::sha256(url.as_bytes())),
                    &json!({"url":url,"blob":blob}),
                    None,
                )
                .unwrap();
        }
        drop(metadata);
        let deadline = Instant::now() + Duration::from_secs(30);
        let cancel = Arc::new(AtomicBool::new(false));
        let budget = Budget::controlled(
            Cost::defaults(),
            deadline,
            OperationControl::new(deadline, cancel.clone()),
        );
        let prepared = prepare_many_budgeted(
            &home,
            commit,
            &["Item", "ItemSparse", "item"],
            true,
            deadline,
            Some(&budget),
        )
        .unwrap();
        assert_eq!(prepared.len(), 2);
        assert_eq!(prepared[0].0.identity.name, "Item");
        assert_eq!(prepared[1].0.identity.name, "ItemSparse");
        assert_eq!(budget.snapshot().unwrap().metadata_bytes, expected_metadata);
        assert_eq!(prepared[0].0.manifest, prepared[1].0.manifest);
        let tight = Budget::new(
            Cost {
                metadata_bytes: expected_metadata - 1,
                ..Cost::defaults()
            },
            deadline,
            None,
        );
        assert_eq!(
            prepare_many_budgeted(
                &home,
                commit,
                &["Item", "ItemSparse"],
                true,
                deadline,
                Some(&tight)
            )
            .unwrap_err()
            .code,
            "records.query_resource_budget"
        );
        cancel.store(true, Ordering::Release);
        assert_eq!(
            prepare_many_budgeted(
                &home,
                commit,
                &["Item", "ItemSparse"],
                true,
                deadline,
                Some(&budget)
            )
            .unwrap_err()
            .exit_code,
            7
        );
        assert_eq!(budget.snapshot().unwrap().metadata_bytes, expected_metadata);
    }
}
