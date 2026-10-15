//! Exact-revision definition resources. Cached bytes are rehashed and parsed;
//! corruption never causes a silent replacement download.
use crate::{fault, schema};
use lycheedev_core::{Result, control::OperationControl, evidence, metadata::Metadata, storage};
use serde::{Deserialize, Serialize};
use serde_json::Value;
mod loading;
pub(crate) mod semantics;
use loading::{check_definition, load};
mod remote_revision;
pub(crate) use remote_revision::normalized_reference;
pub use remote_revision::resolve_revision_controlled;

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
        Self::parse_with_check(raw, || Ok(()))
    }
    pub(crate) fn parse_with_check(
        raw: &[u8],
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Self> {
        check()?;
        if raw.len() > 4 << 20 {
            return Err(fault(
                "schema.limit_exceeded",
                3,
                "Manifest byte budget exceeded",
            ));
        }
        // Struct deserialization rejects duplicate fields; optional file IDs
        // still require explicit non-null integers when the field is present.
        let parsed = serde_json::from_slice(raw).map_err(|_| invalid());
        let entries: Vec<Entry> = parsed?;
        check()?;
        if entries.is_empty() || entries.len() > 16384 {
            return Err(invalid());
        }
        // Preserve field presence independently of Option's null semantics.
        let parsed = serde_json::from_slice(raw).map_err(|_| invalid());
        let presence: Vec<Value> = parsed?;
        check()?;
        let mut tables = BTreeMap::new();
        let mut modern = BTreeSet::new();
        let mut legacy = BTreeSet::new();
        for (entry, object) in entries.into_iter().zip(presence) {
            check()?;
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
        check()?;
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

/// Actual invocation scope is kept distinct from the Data budget's owner.
pub(crate) fn prepare_controlled(
    home: &Path,
    commit: &str,
    name: &str,
    offline: bool,
    deadline: Instant,
    budget: Option<&crate::resource::Budget>,
    control: &OperationControl,
) -> Result<(Bundle, Vec<u8>)> {
    Ok(prepare_many_controlled(
        home,
        commit,
        &[name],
        offline,
        deadline,
        budget,
        Some(control),
    )?
    .remove(0))
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

// Fixed Go definitions.go: charge actual bytes, not the HTTP buffer bound.
fn definition_cost(bytes: u64) -> crate::resource::Cost {
    crate::resource::Cost {
        retained_bytes: bytes * 4,
        metadata_bytes: bytes * 4,
        decode_work: bytes.div_ceil(65536),
        ..Default::default()
    }
}

pub(crate) fn prepare_many_controlled(
    home: &Path,
    commit: &str,
    names: &[&str],
    offline: bool,
    deadline: Instant,
    budget: Option<&crate::resource::Budget>,
    control: Option<&OperationControl>,
) -> Result<Vec<(Bundle, Vec<u8>)>> {
    let fallback = OperationControl::uncancelable(deadline);
    let control = control.unwrap_or(&fallback);
    let check = || check_definition(deadline, budget, control);
    check()?;
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
    let mut catalog = None;
    let (manifest, _) = load(
        home,
        &format!("{base}manifest.json"),
        offline,
        deadline,
        budget,
        Some(control),
        |raw| {
            catalog = Some(Manifest::parse_with_check(raw, check)?);
            Ok(())
        },
    )?;
    let catalog = catalog.expect("successful load validates the selected manifest");
    let mut result = Vec::with_capacity(names.len());
    let mut seen = BTreeSet::new();
    for name in names {
        check()?;
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
            Some(control),
            |raw| schema::Document::parse_with_check(raw, check).map(|_| ()),
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
    check()?;
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
        let expected_metadata = (manifest.len() + definition.len() * 2) as u64 * 4;
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
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            expected_metadata * 2
        );
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

#[cfg(test)]
mod publication_conflict_tests {
    use super::*;
    use crate::resource::{Budget, Cost};
    use lycheedev_core::workspace;
    use std::{
        io::Write,
        net::TcpListener,
        sync::atomic::{AtomicU64, Ordering},
        thread,
    };

    // The first validate callback publishes the winner before load's strict create.
    // This exercises a real generation conflict deterministically; no sleep is
    // used to race two writers and no production-only injection hook is needed.
    fn conflict(
        metadata_limit: u64,
        winner: &[u8],
        reject_second: bool,
        controlled: bool,
    ) -> (Result<(Value, Vec<u8>)>, Cost, Cost, usize) {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/rust-definition-conflict-fixture")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                storage::utc_now().replace([':', '.'], "-"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        workspace::initialize(&home).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/definition", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut connection = loop {
                match listener.accept() {
                    Ok((connection, _)) => break connection,
                    Err(error)
                        if error.kind() == std::io::ErrorKind::WouldBlock
                            && Instant::now() < deadline =>
                    {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("fixture HTTP accept: {error}"),
                }
            };
            // Windows accepted sockets can retain the listener's nonblocking
            // mode. The fixture uses bounded blocking reads after admission.
            connection.set_nonblocking(false).unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            connection
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len() < 8192);
                let mut byte = [0];
                connection.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            connection
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\n//x\n",
                )
                .unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        let budget = Budget::new(
            Cost {
                metadata_bytes: metadata_limit,
                ..Cost::defaults()
            },
            deadline,
            None,
        );
        let mut validations = 0;
        let control = OperationControl::uncancelable(deadline);
        let result = load(
            &home,
            &url,
            false,
            deadline,
            Some(&budget),
            controlled.then_some(&control),
            |raw| {
                validations += 1;
                // Equivalent to semantic comment-only text parsing: charge bytes,
                // and prove the repeated parse receives the winning content.
                budget.charge(Cost::metadata(raw.len()))?;
                if validations == 1 {
                    assert_eq!(raw, b"//x\n");
                    let blob = evidence::publish_blob(&home, winner, 4 << 20)?;
                    Metadata::open(&home, true)?.put(
                        &format!("definition/{}", storage::sha256(url.as_bytes())),
                        &Object {
                            url: url.clone(),
                            blob,
                        },
                        None,
                    )?;
                } else {
                    assert_eq!(raw, winner);
                    if reject_second {
                        return Err(fault(
                            "schema.invalid_definition",
                            4,
                            "second validation rejected",
                        ));
                    }
                }
                Ok(())
            },
        );
        server.join().unwrap();
        (
            result,
            budget.logical_snapshot().unwrap(),
            budget.snapshot().unwrap(),
            validations,
        )
    }
    #[test]
    fn identical_publication_winner_recharges_and_revalidates() {
        let (result, logical, safety, validations) = conflict(128 << 20, b"//x\n", false, true);
        assert_eq!(result.unwrap().1, b"//x\n");
        assert_eq!(validations, 2);
        assert_eq!(
            logical,
            Cost {
                retained_bytes: 40,
                metadata_bytes: 40,
                decode_work: 2,
                network_requests: 1,
                network_bytes: 4
            }
        );
        assert_eq!(safety.retained_bytes, 72);
        assert_eq!(safety.metadata_bytes, 40);
    }
    #[test]
    fn conflict_reread_can_exhaust_metadata_before_second_validation() {
        let (result, logical, safety, validations) = conflict(35, b"//x\n", false, true);
        assert_eq!(result.unwrap_err().code, "records.query_resource_budget");
        assert_eq!(validations, 1);
        assert_eq!(logical.metadata_bytes, 20);
        assert_eq!(logical.decode_work, 1);
        assert_eq!(safety.retained_bytes, 36);
    }
    #[test]
    fn conflict_second_validation_error_precedes_blob_comparison() {
        let (result, logical, _, validations) = conflict(128 << 20, b"//y\n", true, true);
        assert_eq!(result.unwrap_err().code, "schema.invalid_definition");
        assert_eq!(validations, 2);
        assert_eq!(logical.metadata_bytes, 40);
        assert_eq!(logical.decode_work, 2);
    }
    #[test]
    fn differing_valid_winner_is_revalidated_then_rejected() {
        let (result, logical, _, validations) = conflict(128 << 20, b"//y\n", false, true);
        assert_eq!(result.unwrap_err().code, "records.definition_identity");
        assert_eq!(validations, 2);
        assert_eq!(logical.metadata_bytes, 40);
    }
    #[test]
    fn ordinary_publication_conflict_uses_the_same_reread_contract() {
        let (result, logical, _, validations) = conflict(128 << 20, b"//x\n", false, false);
        assert!(result.is_ok());
        assert_eq!(validations, 2);
        assert_eq!(logical.metadata_bytes, 40);
        assert_eq!(logical.decode_work, 2);
    }
}
