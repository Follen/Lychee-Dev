//! Explicit local build identity and authenticated configurations. Reading a
//! client never selects a region, locale or definition revision implicitly.
use crate::{
    blte, casc, fault,
    metadata_format::{Encoding, EncodingRecord},
    resource::{Budget, Cost, RetainedReservation},
    schema,
};
use lycheedev_core::{Result, storage};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InstalledBuild {
    pub product: String,
    pub full_build: String,
    pub build_config: String,
    pub cdn_config: String,
    pub active: bool,
    /// Publishing metadata only; this is not the player's region.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub metadata_branch: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub channel: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigDocument {
    pub key: String,
    #[serde(rename = "sha256")]
    pub sha256: String,
    pub fields: BTreeMap<String, Vec<String>>,
    #[serde(skip)]
    pub raw: Vec<u8>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildMetadata {
    pub installed: InstalledBuild,
    #[serde(rename = "catalogSHA256")]
    pub catalog_sha256: String,
    pub root_content_key: String,
    pub encoding_content_key: String,
    pub encoding_key: String,
    pub content_bytes: usize,
    pub encoding_bytes: usize,
    pub build_document: ConfigDocument,
    pub cdn_document: ConfigDocument,
}
#[derive(Clone, Debug)]
pub struct LocalTargetRequest {
    pub installation: PathBuf,
    /// Toolkit product id or official display alias; never inferred from a build.
    pub product: Option<String>,
    pub full_build: Option<String>,
    pub region: String,
    pub language: String,
    /// Exact caller-resolved definition revision. Resolving latest/offline
    /// observations is a separate definition-provider operation.
    pub definition_commit: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedLocalTarget {
    pub data: lycheedev_core::selection::DataPin,
    pub metadata: BuildMetadata,
    pub installation: PathBuf,
    pub common_root: PathBuf,
    #[serde(rename = "flavorSHA256")]
    pub flavor_sha256: Option<String>,
}
pub fn resolve_target(request: &LocalTargetRequest) -> Result<ResolvedLocalTarget> {
    resolve_target_budgeted(request, None)
}
pub fn resolve_target_budgeted(
    request: &LocalTargetRequest,
    budget: Option<&Budget>,
) -> Result<ResolvedLocalTarget> {
    let requested_product = request
        .product
        .as_deref()
        .filter(|p| !p.is_empty())
        .map(|p| {
            lycheedev_core::client::canonical_product(p).ok_or_else(|| {
                fault(
                    "selection.unsupported_data_product",
                    2,
                    "Unknown toolkit product",
                )
            })
        })
        .transpose()?;
    if !["cn", "us", "eu", "kr", "tw"].contains(&request.region.as_str())
        || ![
            "enUS", "enGB", "deDE", "frFR", "esES", "esMX", "itIT", "ptBR", "ruRU", "koKR", "zhCN",
            "zhTW", "ptPT",
        ]
        .contains(&request.language.as_str())
        || request.definition_commit.len() != 40
        || !request
            .definition_commit
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(fault(
            "records.definition_identity",
            4,
            "Target needs explicit region, locale and exact lowercase definition commit",
        ));
    }
    let _paths = temporary(budget, request.installation.as_os_str().len() * 4 + 256)?;
    let (root, product_code, flavor_sha256) = if request.installation.join(".flavor.info").is_file()
    {
        let (root, flavor) = installation_root_budgeted(&request.installation, budget)?;
        let observed_track = slot_track(&flavor);
        if observed_track.is_none() {
            return Err(fault(
                "records.unsupported_profile",
                3,
                "Unknown client publishing slot",
            ));
        }
        if requested_product.is_some_and(|p| Some(p) != observed_track) {
            return Err(fault(
                "records.client_identity",
                4,
                "Explicit product differs from selected client flavor",
            ));
        }
        let bytes = read(
            &request.installation,
            &request.installation.join(".flavor.info"),
            65536,
            budget,
        )?;
        owned(budget, 64)?;
        work(budget, bytes.len())?;
        (root, flavor, Some(storage::sha256(&bytes)))
    } else {
        let product = requested_product.ok_or_else(|| {
            fault(
                "records.client_identity",
                2,
                "Common root selection requires explicit product",
            )
        })?;
        // A common root has no observed client flavor. Admit exactly one
        // active matching slot/build; a regional slot is never borrowed from
        // another slot selected by an explicit client flavor.
        let catalog = read(
            &request.installation,
            &request.installation.join(".build.info"),
            1 << 20,
            budget,
        )?;
        let mut matches = installed_rows_budgeted(&catalog, budget)?
            .into_iter()
            .filter(|b| {
                b.active
                    && slot_track(&b.product) == Some(product)
                    && request
                        .full_build
                        .as_ref()
                        .is_none_or(|build| build == &b.full_build)
            });
        let selected = matches.next();
        if selected.is_none() || matches.next().is_some() {
            return Err(fault(
                if selected.is_none() {
                    "records.build_unavailable"
                } else {
                    "records.build_ambiguous"
                },
                if selected.is_none() { 3 } else { 4 },
                "Common root needs one exact active toolkit track/slot/build",
            ));
        }
        owned(budget, request.installation.as_os_str().len())?;
        (
            request.installation.clone(),
            selected.expect("one matching build").product,
            None,
        )
    };
    let build = {
        let catalog = read(&root, &root.join(".build.info"), 1 << 20, budget)?;
        let mut matches = installed_rows_budgeted(&catalog, budget)?
            .into_iter()
            .filter(|b| b.active && b.product == product_code);
        let selected = matches.next();
        if selected.is_none() || matches.next().is_some() {
            return Err(fault(
                if selected.is_none() {
                    "records.build_unavailable"
                } else {
                    "records.build_ambiguous"
                },
                if selected.is_none() { 3 } else { 4 },
                "Target needs exactly one active product/build row",
            ));
        }
        let observed = selected.expect("one active build").full_build;
        if request
            .full_build
            .as_ref()
            .is_some_and(|build| build != &observed)
        {
            return Err(fault(
                "records.build_unavailable",
                3,
                "Requested build differs from the exact active product row",
            ));
        }
        observed
    };
    let track = slot_track(&product_code).ok_or_else(invalid)?;
    let series = lycheedev_core::client::product(track)
        .map(|profile| profile.series)
        .ok_or_else(invalid)?;
    version(&build, budget)?;
    let _series = temporary(budget, 16)?;
    if !build.starts_with(&format!("{series}.")) {
        return Err(fault(
            "records.client_identity",
            4,
            "Observed slot/build is outside the verified client series",
        ));
    }
    if request.installation.join("version.txt").is_file() {
        let raw = read(
            &request.installation,
            &request.installation.join("version.txt"),
            4096,
            budget,
        )?;
        let version = checked_text(&raw, budget)?
            .trim_start_matches('\u{feff}')
            .trim();
        if version != build {
            return Err(fault(
                "records.client_identity",
                4,
                "Client version.txt contradicts exact active catalog build",
            ));
        }
    }
    let metadata = resolve_local_budgeted(&root, &product_code, &build, budget)?;
    owned(
        budget,
        track.len()
            + request.region.len()
            + metadata.installed.build_config.len()
            + metadata.installed.cdn_config.len()
            + request.language.len()
            + request.definition_commit.len()
            + request.installation.as_os_str().len(),
    )?;
    let data = lycheedev_core::selection::DataPin {
        product: track.to_owned(),
        region: request.region.clone(),
        full_build: build,
        build_config: metadata.installed.build_config.clone(),
        cdn_config: metadata.installed.cdn_config.clone(),
        language: request.language.clone(),
        definition_commit: request.definition_commit.clone(),
    };
    Ok(ResolvedLocalTarget {
        data,
        metadata,
        installation: request.installation.clone(),
        common_root: root,
        flavor_sha256,
    })
}
fn slot_track(code: &str) -> Option<&'static str> {
    lycheedev_core::client::catalog_product(code).map(|profile| profile.id)
}
fn invalid() -> lycheedev_core::ToolkitError {
    fault(
        "records.invalid_metadata",
        4,
        "Malformed local CASC build metadata",
    )
}
fn exceeds() -> lycheedev_core::ToolkitError {
    fault(
        "records.metadata_limit",
        3,
        "Local CASC metadata exceeds resource budget",
    )
}
fn product(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn field(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn text(raw: &[u8]) -> Result<&str> {
    if raw.contains(&0) {
        return Err(invalid());
    }
    std::str::from_utf8(raw).map_err(|_| invalid())
}
fn work(budget: Option<&Budget>, bytes: usize) -> Result<()> {
    if let Some(budget) = budget {
        budget.charge(Cost::work(bytes.div_ceil(65536).max(1)))?;
    }
    Ok(())
}
fn checked_text<'a>(raw: &'a [u8], budget: Option<&Budget>) -> Result<&'a str> {
    work(budget, raw.len())?;
    text(raw)
}
// Metadata/work are cumulative; only a temporary allocation's retained charge
// is released. Escaping parser output commits its admission to the query.
fn temporary(budget: Option<&Budget>, bytes: usize) -> Result<Option<RetainedReservation<'_>>> {
    budget
        .map(|budget| {
            let reservation = budget.reserve_retained(bytes)?;
            budget.charge(Cost {
                metadata_bytes: bytes as u64,
                ..Default::default()
            })?;
            Ok(reservation)
        })
        .transpose()
}
fn owned(budget: Option<&Budget>, bytes: usize) -> Result<()> {
    if let Some(reservation) = temporary(budget, bytes)? {
        reservation.commit();
    }
    Ok(())
}
fn version(value: &str, budget: Option<&Budget>) -> Result<[u32; 4]> {
    work(budget, value.len())?;
    // schema::version collects borrowed components in a geometrically grown Vec.
    let capacity = value.split('.').count().next_power_of_two().max(4);
    let _parts = temporary(budget, capacity * size_of::<&str>())?;
    schema::version(value)
}
// A BTreeMap entry may cause a node allocation/split. Reserve a full node per
// insertion, including links/alignment, rather than pretending it is just K+V.
fn map_entry<K, V>() -> usize {
    16 * size_of::<(K, V)>() + 128
}
pub fn installed_rows(raw: &[u8]) -> Result<Vec<InstalledBuild>> {
    installed_rows_budgeted(raw, None)
}
pub fn installed_rows_budgeted(raw: &[u8], budget: Option<&Budget>) -> Result<Vec<InstalledBuild>> {
    if raw.len() > 1 << 20 {
        return Err(exceeds());
    }
    let mut lines = checked_text(raw, budget)?
        .trim_start_matches('\u{feff}')
        .lines();
    let header = lines.next().ok_or_else(invalid)?;
    work(budget, header.len())?;
    let header_count = header.split('|').count();
    if header_count > 128 {
        return Err(exceeds());
    }
    let _headers = temporary(budget, header_count * size_of::<&str>())?;
    let mut headers = Vec::with_capacity(header_count);
    headers.extend(header.split('|'));
    let _columns = temporary(budget, header_count * map_entry::<&str, usize>())?;
    let mut columns = BTreeMap::new();
    for (i, h) in headers.iter().enumerate() {
        work(budget, h.len())?;
        let name = h.split('!').next().unwrap().trim();
        if name.is_empty() || columns.insert(name, i).is_some() {
            return Err(invalid());
        }
    }
    for required in ["Product", "Version", "Build Key", "CDN Key", "Active"] {
        if !columns.contains_key(required) {
            return Err(invalid());
        }
    }
    let mut rows = vec![];
    let mut row_allocation = None;
    for line in lines {
        work(budget, line.len())?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if rows.len() == 4096 {
            return Err(exceeds());
        }
        let value_count = line.split('|').count();
        if value_count != headers.len() {
            return Err(invalid());
        }
        let _values = temporary(budget, value_count * size_of::<&str>())?;
        let mut values = Vec::with_capacity(value_count);
        values.extend(line.split('|'));
        let get = |key: &str| values[columns[key]].trim();
        if !product(get("Product")) || !matches!(get("Active"), "0" | "1") {
            return Err(invalid());
        }
        version(get("Version"), budget)?;
        casc::key(get("Build Key"))?;
        casc::key(get("CDN Key"))?;
        let branch = columns
            .get("Branch")
            .map(|&i| values[i].trim())
            .unwrap_or("");
        if branch.len() > 64
            || !branch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(invalid());
        }
        let profile = lycheedev_core::client::catalog_product(get("Product"));
        if rows.len() == rows.capacity() {
            let capacity = (rows.capacity() * 2).clamp(4, 4096);
            let allocation = temporary(budget, capacity * size_of::<InstalledBuild>())?;
            rows.try_reserve_exact(capacity - rows.len())
                .map_err(|_| exceeds())?;
            row_allocation = allocation;
        }
        owned(
            budget,
            get("Product").len()
                + get("Version").len()
                + get("Build Key").len()
                + get("CDN Key").len()
                + branch.len()
                + profile.map(|p| p.channel.len()).unwrap_or(0)
                + profile
                    .filter(|p| !p.channel.is_empty())
                    .map(|p| p.display_name.len())
                    .unwrap_or(0),
        )?;
        rows.push(InstalledBuild {
            product: get("Product").into(),
            full_build: get("Version").into(),
            build_config: get("Build Key").into(),
            cdn_config: get("CDN Key").into(),
            active: get("Active") == "1",
            metadata_branch: branch.into(),
            channel: profile.map(|p| p.channel).unwrap_or("").into(),
            display_name: profile
                .filter(|p| !p.channel.is_empty())
                .map(|p| p.display_name)
                .unwrap_or("")
                .into(),
        });
    }
    if let Some(reservation) = row_allocation {
        reservation.commit();
    }
    Ok(rows)
}
pub fn parse_configuration(raw: Vec<u8>, key: &str) -> Result<ConfigDocument> {
    parse_configuration_budgeted(raw, key, None)
}
pub fn parse_configuration_budgeted(
    raw: Vec<u8>,
    key: &str,
    budget: Option<&Budget>,
) -> Result<ConfigDocument> {
    let expected = casc::key(key)?;
    if raw.len() > 4 << 20 {
        return Err(exceeds());
    }
    work(budget, raw.len())?;
    if Md5::digest(&raw)[..] != expected {
        return Err(fault(
            "records.configuration_integrity",
            4,
            "Configuration MD5 does not match pinned key",
        ));
    }
    let mut fields = BTreeMap::new();
    for line in checked_text(&raw, budget)?.lines() {
        work(budget, line.len())?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = line.split_once('=').ok_or_else(invalid)?;
        let name = name.trim();
        if !field(name) || fields.contains_key(name) {
            return Err(invalid());
        }
        if fields.len() == 4096 {
            return Err(exceeds());
        }
        work(budget, value.len())?;
        let (count, bytes) = value
            .split_whitespace()
            .fold((0, 0), |(count, bytes), token| {
                (count + 1, bytes + token.len())
            });
        owned(
            budget,
            map_entry::<String, Vec<String>>() + name.len() + count * size_of::<String>() + bytes,
        )?;
        if let Some(budget) = budget {
            budget.charge(Cost::work(count))?;
        }
        let mut tokens = Vec::with_capacity(count);
        for (index, token) in value.split_whitespace().enumerate() {
            if index % 256 == 0
                && let Some(budget) = budget
            {
                budget.check()?;
            }
            tokens.push(token.to_owned());
        }
        fields.insert(name.into(), tokens);
    }
    owned(budget, key.len() + 64)?;
    work(budget, raw.len())?;
    Ok(ConfigDocument {
        key: key.into(),
        sha256: storage::sha256(&raw),
        fields,
        raw,
    })
}
fn read(root: &Path, path: &Path, max: usize, budget: Option<&Budget>) -> Result<Vec<u8>> {
    // Joined/canonical paths and Windows path conversion coexist inside the
    // existing path guard. Their scratch is bounded by this explicit path.
    let _paths = temporary(budget, path.as_os_str().len() * 8 + 512)?;
    storage::guard_path(root, path)?;
    let mut file = File::open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(invalid());
    }
    if meta.len() > max as u64 {
        return Err(exceeds());
    }
    let mut allocation = temporary(budget, meta.len() as usize)?;
    let mut raw = Vec::with_capacity(meta.len() as usize);
    let _scratch = budget
        .map(|budget| budget.reserve_retained(65536))
        .transpose()?;
    let mut chunk = [0; 65536];
    loop {
        work(budget, chunk.len())?;
        let next = chunk.len().min(max + 1 - raw.len());
        let count = match file.read(&mut chunk[..next]) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => result?,
        };
        if count == 0 {
            break;
        }
        let required = raw.len() + count;
        if required > max {
            return Err(exceeds());
        }
        if required > raw.capacity() {
            let capacity = required.max(raw.capacity() * 2).min(max);
            let next = temporary(budget, capacity)?;
            raw.try_reserve_exact(capacity - raw.len())
                .map_err(|_| exceeds())?;
            allocation = next;
        }
        raw.extend_from_slice(&chunk[..count]);
    }
    if raw.len() as u64 != meta.len() {
        return Err(fault(
            "records.pinned_build_changed",
            4,
            "Local metadata changed during read",
        ));
    }
    storage::guard_path(root, path)?;
    checked_text(&raw, budget)?;
    if let Some(reservation) = allocation {
        reservation.commit();
    }
    Ok(raw)
}
pub fn resolve_local(root: &Path, product_code: &str, full_build: &str) -> Result<BuildMetadata> {
    resolve_local_budgeted(root, product_code, full_build, None)
}
pub fn resolve_local_budgeted(
    root: &Path,
    product_code: &str,
    full_build: &str,
    budget: Option<&Budget>,
) -> Result<BuildMetadata> {
    if !product(product_code) {
        return Err(invalid());
    }
    version(full_build, budget)?;
    let _catalog_path = temporary(budget, root.as_os_str().len() * 2 + 64)?;
    let raw = read(root, &root.join(".build.info"), 1 << 20, budget)?;
    let rows = installed_rows_budgeted(&raw, budget)?;
    let mut matches = rows
        .into_iter()
        .filter(|r| r.active && r.product == product_code && r.full_build == full_build);
    let selected = matches.next();
    if matches.next().is_some() {
        return Err(fault(
            "records.build_ambiguous",
            4,
            "Multiple active exact local builds",
        ));
    }
    let installed = selected.ok_or_else(|| {
        fault(
            "records.build_unavailable",
            3,
            "Exact active local product/build not installed",
        )
    })?;
    let config = |key: &str| {
        let _path = temporary(budget, root.as_os_str().len() * 2 + 256)?;
        parse_configuration_budgeted(
            read(
                root,
                &root.join(format!("Data/config/{}/{}/{}", &key[..2], &key[2..4], key)),
                4 << 20,
                budget,
            )?,
            key,
            budget,
        )
    };
    let build_document = config(&installed.build_config)?;
    let cdn_document = config(&installed.cdn_config)?;
    let fields = &build_document.fields;
    let root_keys = fields.get("root").ok_or_else(invalid)?;
    let encoding = fields.get("encoding").ok_or_else(invalid)?;
    let sizes = fields.get("encoding-size").ok_or_else(invalid)?;
    if root_keys.len() != 1 || encoding.len() != 2 || sizes.len() != 2 {
        return Err(invalid());
    }
    for key in root_keys.iter().chain(encoding) {
        casc::key(key)?;
    }
    if fields
        .get("build-uid")
        .is_some_and(|uid| uid.as_slice() != [installed.product.as_str()])
    {
        return Err(invalid());
    }
    let size = |s: &str| -> Result<usize> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        let n = s.parse().map_err(|_| invalid())?;
        if n == 0 || n > 1usize << 40 {
            return Err(invalid());
        }
        Ok(n)
    };
    owned(
        budget,
        64 + root_keys[0].len() + encoding[0].len() + encoding[1].len(),
    )?;
    work(budget, raw.len())?;
    Ok(BuildMetadata {
        installed,
        catalog_sha256: storage::sha256(&raw),
        root_content_key: root_keys[0].clone(),
        encoding_content_key: encoding[0].clone(),
        encoding_key: encoding[1].clone(),
        content_bytes: size(&sizes[0])?,
        encoding_bytes: size(&sizes[1])?,
        build_document,
        cdn_document,
    })
}
/// Resolve an explicit client directory using its flavor. The parent catalog
/// is used only when the selected directory has a valid one-column identity.
/// The returned code is the observed catalog slot. Track resolution is separate.
pub fn installation_root(client: &Path) -> Result<(PathBuf, String)> {
    installation_root_budgeted(client, None)
}
fn installation_root_budgeted(client: &Path, budget: Option<&Budget>) -> Result<(PathBuf, String)> {
    let _paths = temporary(budget, client.as_os_str().len() * 4 + 128)?;
    let raw = read(client, &client.join(".flavor.info"), 65536, budget)?;
    let mut lines = checked_text(&raw, budget)?
        .lines()
        .filter(|s| !s.trim().is_empty());
    if lines.next() != Some("Product Flavor!STRING:0") {
        return Err(invalid());
    }
    let product_code = lines.next().ok_or_else(invalid)?.trim();
    if !product(product_code) || lines.next().is_some() {
        return Err(invalid());
    }
    owned(budget, client.as_os_str().len() + product_code.len())?;
    let root = if client.join(".build.info").is_file() {
        client.to_owned()
    } else {
        client.parent().ok_or_else(invalid)?.to_owned()
    };
    Ok((root, product_code.into()))
}
pub fn open_encoding(
    root: &Path,
    metadata: &BuildMetadata,
    limits: casc::Limits,
    keys: &BTreeMap<u64, Vec<u8>>,
    cancel: Option<&AtomicBool>,
) -> Result<blte::Content> {
    if metadata.content_bytes > 256 << 20 || metadata.encoding_bytes > limits.encoded_bytes {
        return Err(exceeds());
    }
    let object = casc::open_local(
        root,
        &metadata.encoding_key,
        metadata.encoding_bytes,
        limits,
        cancel,
    )?;
    let content = object.decode(
        &metadata.encoding_content_key,
        blte::Limits {
            decoded_bytes: metadata.content_bytes,
            ..Default::default()
        },
        keys,
    )?;
    if content.bytes.len() != metadata.content_bytes {
        return Err(invalid());
    }
    Ok(content)
}
pub fn read_content(
    root: &Path,
    encoding: &Encoding<'_>,
    record: &EncodingRecord,
    limits: casc::Limits,
    keys: &BTreeMap<u64, Vec<u8>>,
    cancel: Option<&AtomicBool>,
) -> Result<(casc::LocalObject, blte::Content)> {
    let mut unavailable = None;
    for ekey in &record.encoding_keys {
        let physical = encoding.find_encoding(ekey, limits, cancel)?;
        let size = usize::try_from(physical.encoded_bytes).map_err(|_| exceeds())?;
        match casc::open_local(root, ekey, size, limits, cancel) {
            Ok(object) => {
                let content = object.decode(
                    &record.content_key,
                    blte::Limits {
                        decoded_bytes: usize::try_from(record.decoded_bytes)
                            .map_err(|_| exceeds())?,
                        ..Default::default()
                    },
                    keys,
                )?;
                if content.bytes.len() as u64 != record.decoded_bytes {
                    return Err(invalid());
                }
                return Ok((object, content));
            }
            Err(e) if e.code == "records.local_object_unavailable" => unavailable = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(unavailable.unwrap_or_else(|| {
        fault(
            "records.local_object_unavailable",
            3,
            "No local encoding alternative available",
        )
    }))
}
