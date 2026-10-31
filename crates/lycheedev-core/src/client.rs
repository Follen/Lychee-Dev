//! Current client identities come from explicit flavor, exact catalog and verified source baselines.
#![forbid(unsafe_code)]
pub(crate) mod baselines;
use crate::{Result, ToolkitError, selection::valid_build, storage};
use baselines::{ClientBaseline, VERIFIED_BASELINES};
use lycheedev_windows::filesystem::destination::{Directory, FileControl, FileCost};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Read, path::Path};

/// Bounded client-metadata inspection costs. I/O operations include filesystem
/// API attempts, possible status translation and reserved closes; they are not
/// a measured count of Win32 syscalls.
#[derive(Clone, Copy, Debug, Default)]
pub struct MetadataCost {
    pub io_operations: u64,
    pub read_calls: u64,
    pub requested_bytes: u64,
    pub allocation_bytes: u64,
    pub work_units: u64,
}
pub trait MetadataControl {
    fn charge(&mut self, cost: MetadataCost) -> Result<()>;
    fn check(&mut self) -> Result<()>;
}
struct Unmetered;
impl MetadataControl for Unmetered {
    fn charge(&mut self, _: MetadataCost) -> Result<()> {
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        Ok(())
    }
}
fn io<T>(
    control: &mut impl MetadataControl,
    operation: impl FnOnce() -> std::io::Result<T>,
) -> Result<T> {
    control.charge(MetadataCost {
        io_operations: 1,
        allocation_bytes: 131072,
        work_units: 32768,
        ..MetadataCost::default()
    })?;
    let value = operation()?;
    control.check()?;
    Ok(value)
}
struct MetadataFiles<'a, C>(&'a mut C);
impl<C: MetadataControl> FileControl for MetadataFiles<'_, C> {
    type Error = ToolkitError;
    fn charge(&mut self, cost: FileCost) -> Result<()> {
        self.0.charge(MetadataCost {
            io_operations: cost.io_operations,
            allocation_bytes: cost.allocation_bytes,
            work_units: cost.work_units,
            ..MetadataCost::default()
        })
    }
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
}
fn exists(directory: &Directory, name: &str, control: &mut impl MetadataControl) -> Result<bool> {
    directory
        .optional_metadata_controlled(name.as_ref(), &mut MetadataFiles(control))
        .map(|metadata| metadata.is_some())
}

/// Delivery identity is separate from the shared UI/API source family.
#[derive(Debug, Serialize)]
pub struct Product {
    pub id: &'static str,
    pub family: &'static str,
    pub display_name: &'static str,
    pub catalog_product: &'static str,
    pub folder: &'static str,
    pub series: &'static str,
    pub channel: &'static str,
    pub interface: Option<u32>,
    /// Accepted user and canonical spellings. Identity mapping only; these
    /// never collapse two deliveries or authorize a build range.
    pub aliases: &'static [&'static str],
}
/// Current supported delivery products; historical records retain their original identities.
pub static PRODUCTS: [Product; 6] = [
    Product {
        id: "retail",
        family: "retail",
        display_name: "Retail",
        catalog_product: "wow",
        folder: "_retail_",
        series: "12.1.5",
        channel: "",
        interface: Some(120105),
        aliases: &["retail", "wow", "正式服"],
    },
    Product {
        id: "classic",
        family: "classic",
        display_name: "Classic",
        catalog_product: "wow_classic",
        folder: "_classic_",
        series: "5.5.4",
        channel: "",
        interface: Some(50504),
        aliases: &["classic", "wow_classic", "怀旧服", "经典版"],
    },
    Product {
        id: "classic-era",
        family: "classic-era",
        display_name: "Classic Era",
        catalog_product: "wow_classic_era",
        folder: "_classic_era_",
        series: "1.15.9",
        channel: "",
        interface: Some(11509),
        aliases: &["classic-era", "classic_era", "永久60", "经典旧世"],
    },
    Product {
        id: "titan",
        family: "titan",
        display_name: "Titan",
        catalog_product: "wow_classic_titan",
        folder: "_classic_titan_",
        series: "3.80.2",
        channel: "",
        interface: Some(38002),
        aliases: &["titan", "wow_classic_titan", "泰坦", "经典泰坦", "时光服"],
    },
    Product {
        id: "forever",
        family: "forever",
        display_name: "Forever",
        catalog_product: "wowf",
        folder: "_forever_",
        series: "1.60.1",
        channel: "",
        interface: Some(16001),
        aliases: &["forever", "wowf", "无限服"],
    },
    Product {
        id: "anniversary",
        family: "anniversary",
        display_name: "Anniversary",
        catalog_product: "wow_anniversary",
        folder: "_anniversary_",
        series: "2.5.6",
        channel: "",
        interface: Some(20506),
        aliases: &["anniversary", "wow_anniversary", "周年服"],
    },
];
pub fn product(value: &str) -> Option<&'static Product> {
    PRODUCTS.iter().find(|p| p.id == value)
}
/// Current product identities for CLI discovery and the first-party Skill quick
/// table. Series and TOC interfaces come from the verified baselines; this is
/// identity mapping, not a claimed build range or runtime acceptance.
pub fn discovery_products() -> Vec<serde_json::Value> {
    PRODUCTS
        .iter()
        .map(|product| {
            let baselines: Vec<_> = VERIFIED_BASELINES
                .iter()
                .filter(|baseline| {
                    baseline.product == product.id || baseline.product == product.family
                })
                .collect();
            let mut series: Vec<_> = baselines.iter().map(|b| b.series).collect();
            series.sort_unstable();
            series.dedup();
            if series.is_empty() {
                series.push(product.series);
            }
            let mut interfaces: Vec<u32> = baselines.iter().map(|b| b.interface).collect();
            interfaces.sort_unstable();
            interfaces.dedup();
            if interfaces.is_empty() {
                interfaces.extend(product.interface);
            }
            serde_json::json!({
                "id": product.id,
                "family": product.family,
                "displayName": product.display_name,
                "catalogProduct": product.catalog_product,
                "channel": product.channel,
                "commonNames": product.aliases,
                "buildSeries": series,
                "tocInterfaces": interfaces,
                "delivery": true,
                "sharedSourceFamily": product.family == "forever",
                "ambiguousLocally": false,
                "note": "",
            })
        })
        .collect()
}
pub fn catalog_product(value: &str) -> Option<&'static Product> {
    PRODUCTS.iter().find(|p| p.catalog_product == value)
}
pub fn canonical_product(value: &str) -> Option<&'static str> {
    product(value).map(|p| p.id)
}
/// Historical pin validation keeps family relationships without making retired
/// delivery IDs eligible for a new target or changing their serialized identity.
pub fn source_product_family(product: &str) -> &str {
    match product {
        "forever-asia-beta" | "forever-cn-beta" => "forever",
        _ => PRODUCTS
            .iter()
            .find(|profile| profile.id == product)
            .map_or(product, |profile| profile.family),
    }
}
/// Metadata identifies a delivery track even when the client API family is shared.
pub fn client_data_product(client: &ClientInstallation) -> &str {
    PRODUCTS
        .iter()
        .find(|profile| {
            profile.catalog_product == client.catalog_product
                || profile.catalog_product == client.product_code
        })
        .map_or(&client.product, |profile| profile.id)
}
pub fn client_matches_product(client: &ClientInstallation, product: &str) -> bool {
    product.is_empty()
        || self::product(product).is_some_and(|profile| {
            client.product == profile.family
                && (client.product_code == profile.catalog_product
                    || client.catalog_product == profile.catalog_product)
        })
}
pub fn client_folders() -> Vec<&'static str> {
    PRODUCTS.iter().map(|profile| profile.folder).collect()
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInstallation {
    pub directory: String,
    pub product: String,
    pub product_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub catalog_product: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub channel: String,
    /// Publishing metadata branch, not a claim about player geography.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub metadata_branch: String,
    pub full_build: String,
    pub interface: u32,
    /// Legacy serialized records remain unknown until freshly inspected.
    #[serde(default)]
    pub interface_known: bool,
    pub toc: String,
    pub identity_source: String,
}
/// Convert a canonical Windows path for public display, preserving UNC roots.
/// Filesystem handles continue to use the original extended path internally.
pub fn display_path(path: &Path) -> String {
    let value = path.to_string_lossy();
    if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else if let Some(drive) = value.strip_prefix(r"\\?\") {
        let bytes = drive.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\'
        {
            drive.into()
        } else {
            value.into_owned()
        }
    } else {
        value.into_owned()
    }
}

fn identity_error(message: &str) -> ToolkitError {
    ToolkitError::new("selection.client_identity", message, 4)
}
fn bare_identity_error() -> ToolkitError {
    identity_error("selection.client_identity")
}
pub fn addon_destination(client: &ClientInstallation) -> Result<std::path::PathBuf> {
    let root = Path::new(&client.directory);
    let target = root.join("Interface/AddOns/Lychee Dev");
    storage::guard_path(root, &target)?;
    if !root.join("Interface/AddOns").is_dir() {
        return Err(ToolkitError::new(
            "delivery.invalid_installation",
            "addon discovery parent does not exist",
            4,
        ));
    }
    Ok(target)
}
fn metadata_text_error(name: &str, limit: bool, message: &str) -> ToolkitError {
    if name == ".build.info" {
        ToolkitError::new(
            if limit {
                "records.metadata_limit"
            } else {
                "records.invalid_metadata"
            },
            message,
            if limit { 3 } else { 4 },
        )
    } else {
        // Go clientText returns the sentinel without additional prose for
        // non-regular files, byte bounds and invalid UTF-8.
        bare_identity_error()
    }
}
fn text(
    root: &Directory,
    name: &str,
    maximum: usize,
    control: &mut impl MetadataControl,
) -> Result<String> {
    control.charge(MetadataCost {
        allocation_bytes: 131072,
        work_units: 32768,
        ..MetadataCost::default()
    })?;
    let Some(mut file) =
        root.open_optional_file_controlled(name.as_ref(), &mut MetadataFiles(&mut *control))?
    else {
        return Ok(String::new());
    };
    let info = io(control, || file.metadata())?;
    use std::os::windows::fs::MetadataExt;
    if info.file_attributes() & 0x400 != 0 {
        return Err(bare_identity_error());
    }
    if !info.is_file() {
        return Err(metadata_text_error(
            name,
            false,
            "identity file is not regular text",
        ));
    }
    if info.len() > maximum as u64 {
        return Err(metadata_text_error(
            name,
            true,
            "identity text exceeds byte bound",
        ));
    }
    let raw = read_text_bytes(&mut file, name, maximum, control)?;
    if info.len() != raw.len() as u64 {
        return Err(bare_identity_error());
    }
    root.verify_controlled(&mut MetadataFiles(&mut *control))?;
    if name == ".build.info" && raw.contains(&0) {
        return Err(metadata_text_error(name, false, "catalog contains NUL"));
    }
    let value = String::from_utf8(raw)
        .map_err(|_| metadata_text_error(name, false, "identity text is not UTF-8"))?;
    let value = value.strip_prefix('\u{feff}').unwrap_or(&value);
    // Preserve the catalog's line structure; a header plus newline is valid
    // even when it contains no rows (records.installedRows at fixed Go).
    Ok(if name == ".build.info" {
        value
    } else {
        value.trim()
    }
    .into())
}
// Keep one authenticated file handle. The separate helper only makes bounded
// read/admission ordering testable without changing identity or freshness checks.
fn read_text_bytes(
    file: &mut impl Read,
    name: &str,
    maximum: usize,
    control: &mut impl MetadataControl,
) -> Result<Vec<u8>> {
    control.charge(MetadataCost {
        allocation_bytes: (maximum as u64 + 1) * 3,
        // The entire buffer is actually zero-initialized below. Only this
        // pass visits unused capacity; text validation/conversion visits the
        // bounded bytes read, admitted separately before each read.
        work_units: maximum as u64 + 1,
        ..MetadataCost::default()
    })?;
    let mut raw = vec![0; maximum + 1];
    let mut used = 0;
    while used < raw.len() {
        let end = (used + 65536).min(raw.len());
        control.charge(MetadataCost {
            io_operations: 1,
            read_calls: 1,
            requested_bytes: (end - used) as u64,
            // At most three byte passes after reading: catalog NUL + UTF-8
            // validation + output copy, or UTF-8 + trim + output copy for
            // identity text. Charge the requested extent before IO, including
            // short reads and the EOF probe. No work is refunded on failure.
            work_units: (end - used) as u64 * 3,
            ..MetadataCost::default()
        })?;
        let count = file.read(&mut raw[used..end])?;
        control.check()?;
        if count == 0 {
            break;
        }
        used += count;
    }
    raw.truncate(used);
    if raw.len() > maximum {
        return Err(metadata_text_error(
            name,
            true,
            "identity text exceeds byte bound",
        ));
    }
    Ok(raw)
}

fn flavor(
    root: &Directory,
    control: &mut impl MetadataControl,
) -> Result<Option<(String, String)>> {
    for name in [".flavor.info", "flavor.info"] {
        let value = text(root, name, 4096, control)?;
        if value.is_empty() {
            continue;
        }
        // Go's ASCII-word regexp is \bwow[a-z0-9_]*\b after lowercasing.
        // The first nonempty file is authoritative; do not even read the next.
        let lower = value.to_lowercase();
        let mut matches = lower
            .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .filter(|word| word.starts_with("wow"));
        let code = matches
            .next()
            .ok_or_else(|| identity_error(&format!("invalid {name}")))?;
        if matches.next().is_some() {
            return Err(identity_error(&format!("invalid {name}")));
        }
        return Ok(Some((code.into(), name.into())));
    }
    Ok(None)
}

fn identity_build(build: &str) -> bool {
    valid_build(build)
        && build
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
}
fn version_profile(version: &str) -> Option<&'static ClientBaseline> {
    VERIFIED_BASELINES
        .iter()
        .find(|profile| version.starts_with(&format!("{}.", profile.series)))
}
fn family_code(profile: &Product) -> &'static str {
    profile.catalog_product
}
#[derive(Debug)]
struct ClientBuild {
    product_code: String,
    full_build: String,
    branch: String,
}
#[derive(Debug)]
struct ClientIdentity {
    profile: &'static ClientBaseline,
    product_code: String,
    catalog_product: String,
    full_build: String,
    source: String,
}
// Client selection preserves evidence precedence. Exact installation
// catalog consistency is checked after resolving the observed identity.
#[cfg(test)]
fn resolve_identity(
    folder: &str,
    flavor: Option<(String, String)>,
    version: &str,
    active: &[ClientBuild],
    control: &mut impl MetadataControl,
) -> Result<ClientIdentity> {
    resolve_identity_observed(folder, flavor, version, active, control, &mut String::new())
}
fn resolve_identity_observed(
    folder: &str,
    flavor: Option<(String, String)>,
    version: &str,
    active: &[ClientBuild],
    control: &mut impl MetadataControl,
    observed: &mut String,
) -> Result<ClientIdentity> {
    control.check()?;
    let (mut code, mut source) = flavor.unwrap_or_default();
    *observed = code.clone();
    let explicit_forever = code == "wowf";
    let mut version = version.to_owned();
    if !version.is_empty() && !identity_build(&version) {
        return Err(identity_error("invalid version.txt"));
    }
    if matches!(
        code.as_str(),
        "wow_classic_beta" | "wow_cn_beta" | "wow_forever"
    ) || folder.eq_ignore_ascii_case("_classic_beta_")
        || folder.eq_ignore_ascii_case("_cn_beta_")
    {
        return Err(identity_error("unsupported retired Forever product"));
    }
    if code.is_empty() && !version.is_empty() {
        let profile =
            version_profile(&version).ok_or_else(|| identity_error("unsupported build"))?;
        code = profile.product_code.into();
        source = "version.txt".into();
    }
    if code.is_empty() {
        if let Some(profile) = PRODUCTS
            .iter()
            .find(|profile| folder.eq_ignore_ascii_case(profile.folder))
        {
            code = family_code(profile).into();
        }
        source = "folder".into();
    }
    if observed.is_empty() {
        *observed = code.clone();
    }
    if version.is_empty() {
        for candidate in active {
            control.check()?;
            if candidate.product_code != code {
                continue;
            }
            if !version.is_empty() && version != candidate.full_build {
                return Err(identity_error("ambiguous active builds"));
            }
            version = candidate.full_build.clone();
        }
    }
    if !VERIFIED_BASELINES
        .iter()
        .any(|profile| profile.product_code == code)
    {
        return Err(identity_error(&format!("unsupported product {code:?}")));
    }
    let profile = VERIFIED_BASELINES
        .iter()
        .find(|profile| {
            profile.product_code == code && version.starts_with(&format!("{}.", profile.series))
        })
        .filter(|_| identity_build(&version))
        .ok_or_else(|| identity_error("missing or unsupported product/build pair"))?;
    // The historical beta shares this series. A version alone cannot relabel it
    // as the formal delivery; explicit wowf or an exact wowf catalog is required.
    if profile.product == "forever"
        && !explicit_forever
        && !active
            .iter()
            .any(|row| row.product_code == "wowf" && row.full_build == version)
    {
        return Err(identity_error(
            "Forever requires wowf flavor or exact wowf catalog evidence",
        ));
    }
    Ok(ClientIdentity {
        profile,
        product_code: code,
        catalog_product: if profile.product == "forever" {
            "wowf".into()
        } else {
            String::new()
        },
        full_build: version,
        source,
    })
}
fn validate_installation_catalog(identity: &ClientIdentity, active: &[ClientBuild]) -> Result<()> {
    // Exact installation validation uses only the observed supported product.
    // Another launcher row never supplies its build or overrides this identity.
    let slot = if identity.catalog_product.is_empty() {
        &identity.product_code
    } else {
        &identity.catalog_product
    };
    if active.iter().any(|candidate| {
        (candidate.product_code == identity.product_code || candidate.product_code == slot.as_str())
            && candidate.full_build != identity.full_build
    }) {
        return Err(ToolkitError::new(
            "selection.installation_metadata_conflict",
            "active launcher metadata contradicts selected client build",
            4,
        ));
    }
    Ok(())
}
fn catalog_builds(raw: &str, control: &mut impl MetadataControl) -> Result<Vec<ClientBuild>> {
    control.charge(MetadataCost {
        allocation_bytes: raw.len() as u64 * 64 + 65536,
        work_units: raw.len() as u64 * 64 + 65536,
        ..MetadataCost::default()
    })?;
    if !raw.contains('\n') {
        return Err(metadata_text_error(
            ".build.info",
            false,
            "build catalog requires header and row boundary",
        ));
    }
    let mut lines = raw.lines();
    let header: Vec<_> = lines
        .next()
        .ok_or_else(|| metadata_text_error(".build.info", false, "empty build catalog"))?
        .split('|')
        .take(129)
        .collect();
    if header.len() > 128 {
        return Err(metadata_text_error(
            ".build.info",
            true,
            "build catalog exceeds column budget",
        ));
    }
    let mut columns = BTreeMap::new();
    for (i, field) in header.iter().enumerate() {
        let name = field.split('!').next().unwrap_or("").trim();
        if name.is_empty() || columns.insert(name, i).is_some() {
            return Err(metadata_text_error(
                ".build.info",
                false,
                "build catalog has duplicate or empty columns",
            ));
        }
    }
    for required in ["Product", "Version", "Build Key", "CDN Key", "Active"] {
        if !columns.contains_key(required) {
            return Err(metadata_text_error(
                ".build.info",
                false,
                "build catalog identity columns are missing",
            ));
        }
    }
    let mut selected = Vec::new();
    let mut rows = 0usize;
    for line in lines {
        control.check()?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        rows += 1;
        if rows > 4096 {
            return Err(metadata_text_error(
                ".build.info",
                true,
                "build catalog exceeds row budget",
            ));
        }
        let values: Vec<_> = line.split('|').take(129).collect();
        if values.len() != header.len() {
            return Err(metadata_text_error(
                ".build.info",
                false,
                "build catalog row shape differs",
            ));
        }
        let get = |name: &str| values[columns[name]].trim();
        let product = get("Product");
        let valid_product = !product.is_empty()
            && product.len() <= 128
            && product
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
        let valid_key = |key: &str| {
            key.len() == 32
                && key
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        };
        // Validate inactive and unknown-product rows too, as Go records does.
        if !matches!(get("Active"), "0" | "1")
            || !identity_build(get("Version"))
            || !valid_product
            || !valid_key(get("Build Key"))
            || !valid_key(get("CDN Key"))
        {
            return Err(metadata_text_error(
                ".build.info",
                false,
                "invalid build catalog identity",
            ));
        }
        if get("Active") != "1" {
            continue;
        }
        let branch = columns
            .get("Branch")
            .map(|&i| values[i].trim())
            .unwrap_or("");
        selected.push(ClientBuild {
            product_code: get("Product").into(),
            full_build: get("Version").into(),
            branch: branch.into(),
        });
    }
    Ok(selected)
}
pub fn inspect(directory: &Path) -> Result<ClientInstallation> {
    inspect_with(directory, &mut Unmetered)
}
pub fn inspect_with(
    directory: &Path,
    control: &mut impl MetadataControl,
) -> Result<ClientInstallation> {
    inspect_observed_with(directory, control).map_err(|(error, _)| error)
}
/// Preserve Go's partial identity privately, without adding fields to public errors.
pub(crate) fn inspect_observed_with(
    directory: &Path,
    control: &mut impl MetadataControl,
) -> std::result::Result<ClientInstallation, (ToolkitError, String)> {
    let mut observed = String::new();
    inspect_inner(directory, control, &mut observed).map_err(|error| (error, observed))
}
fn inspect_inner(
    directory: &Path,
    control: &mut impl MetadataControl,
    observed: &mut String,
) -> Result<ClientInstallation> {
    control.check()?;
    if directory.as_os_str().len() > 32768 {
        return Err(identity_error("client directory exceeds bound"));
    }
    let root = Directory::open_controlled(directory, &mut MetadataFiles(&mut *control))?;
    control.charge(MetadataCost {
        allocation_bytes: 262144,
        work_units: 65536,
        ..MetadataCost::default()
    })?;
    // records.inspectExactClient always reads the selected client's parent.
    // A catalog placed inside that client cannot override launcher evidence.
    let parent = root
        .path()
        .parent()
        .map(|path| Directory::open_controlled(path, &mut MetadataFiles(&mut *control)))
        .transpose()?;
    let catalog_root = match parent.as_ref() {
        Some(parent) if exists(parent, ".build.info", control)? => Some(parent),
        _ => None,
    };
    let active = if let Some(parent) = catalog_root {
        text(parent, ".build.info", 1 << 20, control)
            .and_then(|raw| catalog_builds(&raw, control))
            .map_err(|mut error| {
                if error.code == "records.invalid_metadata" {
                    if error.details.is_null() {
                        error.details = serde_json::json!({});
                    }
                    if let Some(details) = error.details.as_object_mut() {
                        details.entry("sourcePath").or_insert_with(|| serde_json::json!(parent.path().join(".build.info")));
                        details.entry("formatHint").or_insert_with(|| serde_json::json!("Pipe-delimited launcher build catalog with a header and row boundary"));
                    }
                }
                error
            })?
    } else {
        Vec::new()
    };
    let flavor = flavor(&root, control)?;
    *observed = flavor
        .as_ref()
        .map(|(code, _)| code.clone())
        .unwrap_or_default();
    let version = text(&root, "version.txt", 4096, control)?;
    let folder = root
        .path()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    let identity = resolve_identity_observed(folder, flavor, &version, &active, control, observed)?;
    *observed = identity.product_code.clone();
    validate_installation_catalog(&identity, &active)?;
    let profile = identity.profile;
    let delivery = catalog_product(&identity.catalog_product)
        .filter(|delivery| delivery.family == profile.product)
        .or_else(|| product(profile.product))
        .ok_or_else(|| identity_error("verified baseline has no delivery product"))?;
    let catalog_code = if identity.catalog_product.is_empty() {
        &identity.product_code
    } else {
        &identity.catalog_product
    };
    let branch = active
        .iter()
        .find(|candidate| {
            candidate.product_code == *catalog_code && candidate.full_build == identity.full_build
        })
        .map(|candidate| candidate.branch.clone())
        .unwrap_or_default();
    control.charge(MetadataCost {
        allocation_bytes: 131072,
        work_units: 32768,
        ..MetadataCost::default()
    })?;
    let result = ClientInstallation {
        directory: display_path(root.path()),
        product: profile.product.into(),
        product_code: identity.product_code,
        catalog_product: identity.catalog_product,
        display_name: delivery.display_name.into(),
        channel: delivery.channel.into(),
        metadata_branch: branch,
        full_build: identity.full_build,
        interface: profile.interface,
        interface_known: true,
        toc: profile.toc.into(),
        identity_source: identity.source,
    };
    root.verify_controlled(&mut MetadataFiles(&mut *control))?;
    if let Some(catalog) = catalog_root {
        catalog.verify_controlled(&mut MetadataFiles(&mut *control))?;
    }
    control.check()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    #[test]
    fn public_path_spelling_preserves_unc_roots_and_only_strips_drive_prefixes() {
        for (input, expected) in [
            (r"\\?\C:\Games\WoW", r"C:\Games\WoW"),
            (r"\\?\UNC\server\share\WoW", r"\\server\share\WoW"),
            (r"\\server\share\WoW", r"\\server\share\WoW"),
            (r"C:\Games\WoW", r"C:\Games\WoW"),
            (r"relative\WoW", r"relative\WoW"),
            (r"\\?\Volume{fixture}\WoW", r"\\?\Volume{fixture}\WoW"),
            (r"\\.\pipe\fixture", r"\\.\pipe\fixture"),
        ] {
            assert_eq!(display_path(Path::new(input)), expected, "{input}");
        }
    }
    #[test]
    fn catalog_schema_matches_reference_records_even_for_inactive_rows() {
        const HEADER: &str =
            "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n";
        const KEY: &str = "0123456789abcdef0123456789abcdef";
        let parse = |row: &str| catalog_builds(&format!("{HEADER}{row}"), &mut Unmetered);
        assert!(parse("").unwrap().is_empty());
        assert!(catalog_builds(HEADER.trim_end(), &mut Unmetered).is_err());
        for active in ["0", "1"] {
            let row = |product: &str, build: &str, key: &str| {
                format!("{product}|{build}|{key}|{KEY}|{active}\n")
            };
            assert_eq!(
                parse(&row("unknown_product", "12.1.0.69875", KEY))
                    .unwrap()
                    .len(),
                usize::from(active == "1")
            );
            for product in ["", "WoW", "wow-test", "wow/evil", "中文"] {
                assert!(parse(&row(product, "12.1.0.69875", KEY)).is_err());
            }
            for key in [
                "",
                "not-md5",
                "0123456789ABCDEF0123456789ABCDEF",
                "0123456789abcdef0123456789abcdeg",
            ] {
                assert!(parse(&row("wow", "12.1.0.69875", key)).is_err());
            }
            for build in ["12.1.0", "12.1.0.+1", "12.1.0.4294967296", "12.1.x.1"] {
                assert!(parse(&row("wow", build, KEY)).is_err());
            }
            assert!(parse(&row(&"x".repeat(128), "12.1.0.1", KEY)).is_ok());
            assert!(parse(&row(&"x".repeat(129), "12.1.0.1", KEY)).is_err());
        }
        for raw in [
            "Product|Product|Version|Build Key|CDN Key|Active\n",
            "Product|Version|Active\nwow|12.1.0.1|1\n",
        ] {
            assert!(catalog_builds(raw, &mut Unmetered).is_err());
        }
        let invalid = parse(&format!("wow|12.1.0.1|{KEY}|{KEY}|maybe\n")).unwrap_err();
        assert_eq!(
            (invalid.code.as_str(), invalid.exit_code),
            ("records.invalid_metadata", 4)
        );
        let columns = format!(
            "{}\n",
            (0..129)
                .map(|n| format!("Column{n}"))
                .collect::<Vec<_>>()
                .join("|")
        );
        let exceeded = catalog_builds(&columns, &mut Unmetered).unwrap_err();
        assert_eq!(
            (exceeded.code.as_str(), exceeded.exit_code),
            ("records.metadata_limit", 3)
        );
        let rows = format!(
            "{HEADER}{}",
            format!("wow|12.1.0.1|{KEY}|{KEY}|0\n").repeat(4097)
        );
        let exceeded = catalog_builds(&rows, &mut Unmetered).unwrap_err();
        assert_eq!(
            (exceeded.code.as_str(), exceeded.exit_code),
            ("records.metadata_limit", 3)
        );
        for (limit, code, exit) in [
            (false, "records.invalid_metadata", 4),
            (true, "records.metadata_limit", 3),
        ] {
            let error = metadata_text_error(".build.info", limit, "fixture");
            assert_eq!((error.code.as_str(), error.exit_code), (code, exit));
        }
        assert!(parse(&format!("wow|12.1.0.1|{KEY}|{KEY}\n")).is_err());
    }
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lychee-client-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn client(&self, name: &str, product: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(
                path.join(".flavor.info"),
                format!("Product Flavor!STRING:0\n{product}\n"),
            )
            .unwrap();
            path
        }
        fn catalog(&self, rows: &str) {
            let rows = rows
                .lines()
                .map(|row| {
                    format!(
                        "{row}|0123456789abcdef0123456789abcdef|fedcba9876543210fedcba9876543210\n"
                    )
                })
                .collect::<String>();
            fs::write(
                self.0.join(".build.info"),
                format!("Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0|Build Key!HEX:16|CDN Key!HEX:16\n{rows}"),
            )
            .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    const FORMAL: &str = "cn|1|1.60.1.70658|wowf\n";
    const ASIA: &str = "us|1|1.60.1.70338|wow_classic_beta\n";
    const CN: &str = "cn|1|1.60.1.70338|wow_cn_beta\n";
    #[derive(Default)]
    struct Meter {
        cost: MetadataCost,
        reads: Vec<u64>,
        read_work: Vec<u64>,
        allocations: Vec<(u64, u64)>,
        deny_read: bool,
        cancel: bool,
    }
    impl MetadataControl for Meter {
        fn charge(&mut self, c: MetadataCost) -> Result<()> {
            self.check()?;
            if self.deny_read && c.read_calls > 0 {
                return Err(identity_error("fixture.read_budget"));
            }
            self.cost.io_operations += c.io_operations;
            self.cost.read_calls += c.read_calls;
            self.cost.requested_bytes += c.requested_bytes;
            self.cost.allocation_bytes += c.allocation_bytes;
            self.cost.work_units += c.work_units;
            if c.read_calls > 0 {
                self.reads.push(c.requested_bytes);
                self.read_work.push(c.work_units);
            }
            if c.allocation_bytes != 0 {
                self.allocations.push((c.allocation_bytes, c.work_units));
            }
            Ok(())
        }
        fn check(&mut self) -> Result<()> {
            if self.cancel {
                Err(identity_error("fixture.cancelled"))
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn text_read_work_is_admitted_before_io_and_preserves_primary_failure() {
        struct Reads(usize);
        impl Read for Reads {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                self.0 += 1;
                Err(std::io::Error::other("fixture.read_failed"))
            }
        }
        let mut reader = Reads(0);
        let mut meter = Meter {
            deny_read: true,
            ..Meter::default()
        };
        assert_eq!(
            read_text_bytes(&mut reader, ".build.info", 1 << 20, &mut meter)
                .unwrap_err()
                .message,
            "fixture.read_budget"
        );
        assert_eq!(reader.0, 0);
        assert_eq!(meter.cost.read_calls, 0);
        meter.deny_read = false;
        let error = read_text_bytes(&mut reader, ".build.info", 1 << 20, &mut meter).unwrap_err();
        assert!(error.to_string().contains("fixture.read_failed"));
        assert_eq!(reader.0, 1);
        assert_eq!(meter.read_work, vec![3 * 65536]);
        meter.cancel = true;
        assert_eq!(
            read_text_bytes(&mut reader, ".build.info", 1 << 20, &mut meter)
                .unwrap_err()
                .message,
            "fixture.cancelled"
        );
        assert_eq!(reader.0, 1);
    }

    #[test]
    fn text_read_sentinel_rejects_growth_and_cancellation_stops_after_current_read() {
        const MAXIMUM: usize = 140_000;
        let mut grown = std::io::Cursor::new(vec![b'x'; MAXIMUM + 1]);
        let mut meter = Meter::default();
        let error = read_text_bytes(&mut grown, ".build.info", MAXIMUM, &mut meter).unwrap_err();
        assert_eq!(error.code, "records.metadata_limit");
        assert_eq!(grown.position(), (MAXIMUM + 1) as u64);
        assert_eq!(
            meter.read_work.iter().sum::<u64>(),
            (MAXIMUM as u64 + 1) * 3
        );
        struct CancelAfterRead {
            read_started: bool,
        }
        impl MetadataControl for CancelAfterRead {
            fn charge(&mut self, cost: MetadataCost) -> Result<()> {
                self.check()?;
                if cost.read_calls > 0 {
                    assert_eq!(cost.work_units, cost.requested_bytes * 3);
                    self.read_started = true;
                }
                Ok(())
            }
            fn check(&mut self) -> Result<()> {
                if self.read_started {
                    Err(identity_error("fixture.cancelled"))
                } else {
                    Ok(())
                }
            }
        }
        let mut reader = std::io::Cursor::new(vec![b'x'; MAXIMUM]);
        let error = read_text_bytes(
            &mut reader,
            ".build.info",
            MAXIMUM,
            &mut CancelAfterRead {
                read_started: false,
            },
        )
        .unwrap_err();
        assert_eq!(error.message, "fixture.cancelled");
        assert_eq!(reader.position(), 65536);
    }

    #[test]
    fn text_work_admits_initialization_and_actual_read_extents_separately() {
        const MAXIMUM: usize = 1 << 20;
        let fixture = Fixture::new();
        // Small, exact-limit, and multi-chunk bodies all use the same public
        // cap. Test text directly so catalog parsing does not obscure costs.
        for length in [0, 257, 140_000, MAXIMUM] {
            let bytes = vec![b'x'; length];
            fs::write(fixture.0.join(".build.info"), &bytes).unwrap();
            let root = Directory::open(&fixture.0).unwrap();
            let mut meter = Meter::default();
            let actual = text(&root, ".build.info", MAXIMUM, &mut meter).unwrap();
            assert_eq!(actual.as_bytes(), bytes);
            assert!(
                meter
                    .allocations
                    .contains(&((MAXIMUM as u64 + 1) * 3, MAXIMUM as u64 + 1,))
            );
            let expected_reads = length.div_ceil(65536) + 1;
            assert_eq!(meter.reads.len(), expected_reads);
            for (request, work) in meter.reads.iter().zip(&meter.read_work) {
                assert!(*request > 0 && *request <= 65536);
                assert_eq!(*work, *request * 3);
            }
            if length > 0 && length < 65536 {
                // No phantom 3 MiB scan over unused initialized capacity.
                assert_eq!(meter.read_work.iter().sum::<u64>(), 6 * 65536);
            }
        }
    }

    #[test]
    fn controlled_parent_catalog_inspection_is_metered_and_fresh() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wowf");
        fixture.catalog(FORMAL);
        let mut meter = Meter::default();
        let first = inspect_with(&path, &mut meter).unwrap();
        assert_eq!(first, inspect(&path).unwrap());
        assert!(meter.cost.io_operations > meter.cost.read_calls);
        assert!(meter.cost.allocation_bytes > 0 && meter.cost.work_units > 0);
        assert!(meter.reads.iter().all(|n| *n <= 65536));
        let before = meter.cost.read_calls;
        fixture.catalog("us|1|1.60.1.70707|wowf\n");
        let second = inspect_with(&path, &mut meter).unwrap();
        assert_ne!(first, second);
        assert!(meter.cost.read_calls > before);
    }
    #[test]
    fn controlled_inspection_cancellation_and_read_budget_keep_original_cause() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wowf");
        fixture.catalog(FORMAL);
        let mut meter = Meter {
            cancel: true,
            ..Meter::default()
        };
        assert_eq!(
            inspect_with(&path, &mut meter).unwrap_err().message,
            "fixture.cancelled"
        );
        assert_eq!(meter.cost.io_operations, 0);
        meter.cancel = false;
        meter.deny_read = true;
        assert_eq!(
            inspect_with(&path, &mut meter).unwrap_err().message,
            "fixture.read_budget"
        );
        assert_eq!(meter.cost.read_calls, 0);
    }
    #[test]
    fn controlled_inspection_retains_file_and_catalog_bounds() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wowf");
        fs::write(path.join("version.txt"), vec![b'1'; 4097]).unwrap();
        assert_eq!(
            inspect_with(&path, &mut Meter::default()).unwrap_err().code,
            "selection.client_identity"
        );
        fs::remove_file(path.join("version.txt")).unwrap();
        fs::write(fixture.0.join(".build.info"), vec![b'1'; (1 << 20) + 1]).unwrap();
        assert_eq!(
            inspect_with(&path, &mut Meter::default()).unwrap_err().code,
            "records.metadata_limit"
        );
        fs::write(fixture.0.join(".build.info"), "x|".repeat(129)).unwrap();
        let error = inspect_with(&path, &mut Meter::default()).unwrap_err();
        assert_eq!(error.code, "records.invalid_metadata");
        assert_eq!(error.exit_code, 4);
        assert_eq!(
            error.details["sourcePath"],
            serde_json::json!(fixture.0.join(".build.info"))
        );
        assert!(
            error.details["formatHint"]
                .as_str()
                .unwrap()
                .contains("Pipe-delimited")
        );
        fs::write(fixture.0.join(".build.info"), b"bad\0catalog\n").unwrap();
        let error = inspect_with(&path, &mut Meter::default()).unwrap_err();
        assert_eq!(error.code, "records.invalid_metadata");
        assert_eq!(error.message, "catalog contains NUL");
        assert_eq!(
            error.details["sourcePath"],
            serde_json::json!(fixture.0.join(".build.info"))
        );
    }
    #[test]
    fn controlled_multichunk_read_has_no_request_larger_than_64k() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wowf");
        fixture.catalog(&format!("#{}\n{FORMAL}", "x".repeat(140000)));
        let mut meter = Meter::default();
        assert_eq!(
            inspect_with(&path, &mut meter).unwrap().full_build,
            "1.60.1.70658"
        );
        assert!(meter.reads.iter().filter(|n| **n == 65536).count() >= 3);
        assert!(meter.reads.iter().all(|n| *n <= 65536));
    }
    #[test]
    fn controlled_inspection_refuses_metadata_directory_and_missing_identity() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wowf");
        fs::create_dir(path.join("version.txt")).unwrap();
        assert!(inspect_with(&path, &mut Meter::default()).is_err());
        let missing = fixture.0.join("missing");
        assert!(inspect_with(&missing, &mut Meter::default()).is_err());
    }
    #[test]
    fn controlled_metadata_keeps_client_and_catalog_ancestry_held_until_completion() {
        let fixture = Fixture::new();
        let path = fixture.client("client", "wowf");
        fixture.catalog(FORMAL);
        struct RenameAttempt {
            client: PathBuf,
            catalog: PathBuf,
            reads: usize,
        }
        impl MetadataControl for RenameAttempt {
            fn charge(&mut self, cost: MetadataCost) -> Result<()> {
                if cost.read_calls != 0 {
                    self.reads += 1;
                    assert!(
                        fs::rename(&self.client, self.client.with_file_name("client-moved"))
                            .is_err()
                    );
                    assert!(
                        fs::rename(&self.catalog, self.catalog.with_extension("moved")).is_err()
                    );
                }
                Ok(())
            }
            fn check(&mut self) -> Result<()> {
                Ok(())
            }
        }
        let mut control = RenameAttempt {
            client: path.clone(),
            catalog: fixture.0.clone(),
            reads: 0,
        };
        let identity = inspect_with(&path, &mut control).unwrap();
        assert_eq!(identity.catalog_product, "wowf");
        assert!(
            control.reads >= 4,
            "flavor and parent catalog each perform data and EOF reads"
        );
        // All directory handles are gone on return; ownership is not leaked.
        let moved = path.with_file_name("client-moved");
        fs::rename(&path, &moved).unwrap();
        fs::rename(&moved, &path).unwrap();
    }
    #[test]
    fn explicit_legacy_flavor_and_version_remain_usable_without_catalog() {
        let fixture = Fixture::new();
        let client = fixture.client("arbitrary-name", "wow");
        fs::write(client.join("version.txt"), "12.1.0.69933").unwrap();
        let observed = inspect(&client).unwrap();
        assert_eq!(observed.product, "retail");
        assert_eq!(observed.product_code, "wow");
        assert_eq!(observed.full_build, "12.1.0.69933");
        assert!(observed.metadata_branch.is_empty());
    }
    #[test]
    fn formal_forever_uses_exact_wowf_catalog_without_version_file() {
        for folder in ["_forever_", "arbitrarily-renamed"] {
            let fixture = Fixture::new();
            let path = fixture.client(folder, "wowf");
            fixture.catalog(&format!("{FORMAL}{ASIA}{CN}"));
            let observed = inspect(&path).unwrap();
            assert!(!path.join("version.txt").exists());
            assert_eq!(
                (
                    observed.product.as_str(),
                    observed.product_code.as_str(),
                    observed.catalog_product.as_str()
                ),
                ("forever", "wowf", "wowf")
            );
            assert_eq!(observed.full_build, "1.60.1.70658");
            assert_eq!(observed.interface, 16001);
            assert!(observed.interface_known);
            assert_eq!(observed.display_name, "Forever");
            assert!(observed.channel.is_empty());
            assert_eq!(observed.metadata_branch, "cn");
            assert!(client_matches_product(&observed, "forever"));
            assert!(!client_matches_product(&observed, "forever-cn-beta"));
            assert!(
                serde_json::to_value(observed)
                    .unwrap()
                    .get("region")
                    .is_none()
            );
        }
    }
    #[test]
    fn formal_forever_catalog_conflicts_and_retired_slots_never_supply_identity() {
        let fixture = Fixture::new();
        let path = fixture.client("_forever_", "wowf");
        fixture.catalog(&format!("{ASIA}{CN}"));
        assert!(
            inspect(&path).is_err(),
            "beta rows never supply wowf's missing version"
        );
        fixture.catalog(&format!("{FORMAL}{FORMAL}"));
        assert_eq!(inspect(&path).unwrap().full_build, "1.60.1.70658");
        fixture.catalog(&format!("{FORMAL}us|1|1.60.1.70658|wowf\n"));
        assert_eq!(inspect(&path).unwrap().catalog_product, "wowf");
        fixture.catalog(&format!("{FORMAL}cn|1|1.60.1.70707|wowf\n"));
        assert!(inspect(&path).is_err());
        fixture.catalog(FORMAL);
        fs::write(path.join("version.txt"), "1.60.1.70707").unwrap();
        assert_eq!(
            inspect(&path).unwrap_err().code,
            "selection.installation_metadata_conflict"
        );
        fs::remove_file(path.join("version.txt")).unwrap();
        fs::write(path.join("flavor.info"), "wow_cn_beta").unwrap();
        assert_eq!(
            inspect(&path).unwrap().product_code,
            "wowf",
            "authoritative flavor prevents lower-priority borrowing"
        );
        fs::write(path.join(".flavor.info"), "wowf wow_cn_beta").unwrap();
        assert!(inspect(&path).is_err());
        for (folder, flavor) in [
            ("renamed-one", "wow_classic_beta"),
            ("renamed-two", "wow_cn_beta"),
            ("renamed-three", "wow_forever"),
            ("_classic_beta_", "wowf"),
            ("_cn_beta_", "wowf"),
        ] {
            let retired = fixture.client(folder, flavor);
            fs::write(retired.join("version.txt"), "1.60.1.70658").unwrap();
            assert_eq!(
                inspect(&retired).unwrap_err().message,
                "unsupported retired Forever product"
            );
        }
    }
    #[test]
    fn exact_products_and_source_family_do_not_conflate_delivery_tracks() {
        for profile in &PRODUCTS {
            assert_eq!(canonical_product(profile.id), Some(profile.id));
            assert_eq!(
                catalog_product(profile.catalog_product).unwrap().id,
                profile.id
            );
            assert_eq!(source_product_family(profile.id), profile.family);
        }
        assert_eq!(canonical_product("forever"), Some("forever"));
        for unsupported in ["cn-beta", "Forever", "Forever Asia Beta", "Forever CN Beta"] {
            assert_eq!(canonical_product(unsupported), None);
        }
        assert_eq!(source_product_family("custom"), "custom");
        assert_eq!(client_folders().len(), 6);
        let old = serde_json::json!({"directory":"fixture","product":"forever","productCode":"wow_forever","fullBuild":"1.60.1.70338","interface":16001,"toc":"Lychee Dev.toc","identitySource":"version.txt"});
        let old: ClientInstallation = serde_json::from_value(old).unwrap();
        assert!(!old.interface_known);
        assert_eq!(client_data_product(&old), "forever");
        assert!(
            !client_matches_product(&old, "forever"),
            "historical family code is not wowf evidence"
        );
        assert!(!client_matches_product(&old, "forever-asia-beta"));
        assert!(!client_matches_product(&old, "forever-cn-beta"));
    }
    #[test]
    fn every_explicit_product_metadata_uses_its_own_slot() {
        for profile in &PRODUCTS {
            let fixture = Fixture::new();
            let path = fixture.client("arbitrarily-renamed", profile.catalog_product);
            fixture.catalog(&format!(
                "us|1|{}.70338|{}\n",
                profile.series, profile.catalog_product
            ));
            let observed = inspect(&path).unwrap();
            assert_eq!(client_data_product(&observed), profile.id);
            assert_eq!(observed.product, profile.family);
            assert_eq!(observed.interface, profile.interface.unwrap());
        }
        let fixture = Fixture::new();
        let path = fixture.client("_cn_beta_", "wow_forever");
        fs::write(path.join("version.txt"), "1.60.1.70338").unwrap();
        assert!(
            inspect(&path).is_err(),
            "legacy flavor cannot authorize a formal client"
        );
    }

    // Fixed-reference evidence-precedence cases plus retained 12.1.0.
    #[test]
    fn retail_series_resolve_by_product_and_build() {
        for (build, interface) in [("12.1.5.70391", 120105), ("12.1.0.69933", 120100)] {
            for (folder, flavor, version, from_catalog) in [
                ("_retail_", "wow", "", true),
                ("arbitrarily-renamed", "wow", build, false),
                ("arbitrarily-renamed", "", build, false),
                ("_retail_", "", "", true),
            ] {
                let fixture = Fixture::new();
                let path = fixture.client(folder, flavor);
                if flavor.is_empty() {
                    fs::write(path.join(".flavor.info"), "").unwrap();
                }
                if !version.is_empty() {
                    fs::write(path.join("version.txt"), version).unwrap();
                }
                if from_catalog {
                    fixture.catalog(&format!("us|1|{build}|wow\n"));
                }
                let observed = inspect(&path).unwrap();
                assert_eq!(observed.product, "retail");
                assert_eq!(observed.full_build, build);
                assert_eq!(observed.interface, interface);
                assert!(observed.interface_known);
            }
        }
        assert_eq!(product("retail").unwrap().series, "12.1.5");
        assert_eq!(product("retail").unwrap().interface, Some(120105));
    }

    #[test]
    fn retail_catalog_rejects_unsupported_conflicting_and_ambiguous_builds() {
        for (flavor, version, rows, message) in [
            (
                "wow",
                "12.1.6.70391",
                "",
                "missing or unsupported product/build pair",
            ),
            (
                "wow",
                "12.1.5.70391",
                "us|1|12.1.0.69933|wow\n",
                "active launcher metadata contradicts selected client build",
            ),
            (
                "wow",
                "",
                "us|1|12.1.0.69933|wow\nus|1|12.1.5.70391|wow\n",
                "ambiguous active builds",
            ),
            (
                "wow_classic",
                "12.1.5.70391",
                "",
                "missing or unsupported product/build pair",
            ),
            (
                "wow_cn_beta",
                "12.1.5.70391",
                "",
                "unsupported retired Forever product",
            ),
        ] {
            let fixture = Fixture::new();
            let path = fixture.client("arbitrarily-renamed", flavor);
            if !version.is_empty() {
                fs::write(path.join("version.txt"), version).unwrap();
            }
            if !rows.is_empty() {
                fixture.catalog(rows);
            }
            assert_eq!(inspect(&path).unwrap_err().message, message);
        }
    }

    // Fixed-reference evidence-precedence cases pinned to commit
    // 3057a9666ab078270b63c7f06de748069b0d1f4b, through the installation wrapper.
    // Additional cases cover metadata-backed folder fallback and the records
    // layer's contradiction gate; pure selection precedence is tested separately.
    #[test]
    fn reference_client_evidence_precedence() {
        struct Case {
            name: &'static str,
            folder: &'static str,
            flavor: &'static str,
            version: &'static str,
            rows: &'static str,
            want: Option<(&'static str, &'static str)>,
        }
        let cases = [
            Case {
                name: "retail catalog",
                folder: "_retail_",
                flavor: "wow",
                version: "",
                rows: "us|1|12.1.0.69875|wow\n",
                want: Some(("retail", "retail")),
            },
            Case {
                name: "anniversary catalog",
                folder: "_anniversary_",
                flavor: "wow_anniversary",
                version: "",
                rows: "us|1|2.5.6.70006|wow_anniversary\n",
                want: Some(("anniversary", "anniversary")),
            },
            Case {
                name: "anniversary version",
                folder: "custom",
                flavor: "wow_anniversary",
                version: "2.5.6.70006",
                rows: "",
                want: Some(("anniversary", "anniversary")),
            },
            Case {
                name: "legacy Forever in reused slot",
                folder: "_classic_beta_",
                flavor: "wow_forever",
                version: "1.60.1.69893",
                rows: "",
                want: None,
            },
            Case {
                name: "slot resolved by catalog",
                folder: "_classic_beta_",
                flavor: "wow_classic_beta",
                version: "",
                rows: "us|1|1.60.1.70009|wow_classic_beta\n",
                want: None,
            },
            Case {
                name: "slot wrong series",
                folder: "_classic_beta_",
                flavor: "wow_classic_beta",
                version: "",
                rows: "us|1|5.5.4.69934|wow_classic_beta\n",
                want: None,
            },
            Case {
                name: "CN slot catalog",
                folder: "_cn_beta_",
                flavor: "wow_cn_beta",
                version: "",
                rows: "cn|1|1.60.1.70245|wow_cn_beta\nus|1|1.60.1.70124|wow_classic_beta\n",
                want: None,
            },
            Case {
                name: "CN slot version",
                folder: "_cn_beta_",
                flavor: "wow_cn_beta",
                version: "1.60.1.70245",
                rows: "",
                want: None,
            },
            Case {
                name: "CN slot wrong series",
                folder: "_cn_beta_",
                flavor: "wow_cn_beta",
                version: "",
                rows: "cn|1|5.5.4.69934|wow_cn_beta\n",
                want: None,
            },
            Case {
                name: "renamed CN slot wrong series",
                folder: "custom",
                flavor: "wow_cn_beta",
                version: "5.5.4.69934",
                rows: "",
                want: None,
            },
            Case {
                name: "CN cannot borrow international build",
                folder: "_cn_beta_",
                flavor: "wow_cn_beta",
                version: "",
                rows: "us|1|1.60.1.70124|wow_classic_beta\n",
                want: None,
            },
            Case {
                name: "CN folder alone is not identity",
                folder: "_cn_beta_",
                flavor: "",
                version: "",
                rows: "cn|1|1.60.1.70245|wow_cn_beta\n",
                want: None,
            },
            Case {
                name: "version before folder",
                folder: "_classic_beta_",
                flavor: "",
                version: "1.60.1.69893",
                rows: "",
                want: None,
            },
            Case {
                name: "product before folder",
                folder: "custom",
                flavor: "wow_classic_titan",
                version: "3.80.2.12345",
                rows: "",
                want: Some(("titan", "titan")),
            },
            Case {
                name: "unknown product no fallback",
                folder: "_retail_",
                flavor: "wow_beta",
                version: "12.1.0.69875",
                rows: "",
                want: None,
            },
            Case {
                name: "contradictory product build",
                folder: "_retail_",
                flavor: "wow",
                version: "1.60.1.69893",
                rows: "",
                want: None,
            },
            Case {
                name: "MoP test slot",
                folder: "_classic_beta_",
                flavor: "",
                version: "5.5.4.69585",
                rows: "",
                want: None,
            },
            Case {
                name: "unidentified reused slot",
                folder: "_classic_beta_",
                flavor: "",
                version: "",
                rows: "us|1|1.60.1.69893|wow_forever\n",
                want: None,
            },
            Case {
                name: "ambiguous catalog",
                folder: "_retail_",
                flavor: "wow",
                version: "",
                rows: "us|1|12.1.0.1|wow\nus|1|12.1.0.2|wow\n",
                want: None,
            },
            Case {
                name: "missing build",
                folder: "_retail_",
                flavor: "wow",
                version: "",
                rows: "",
                want: None,
            },
            Case {
                name: "unsupported series",
                folder: "_retail_",
                flavor: "wow",
                version: "12.2.0.12345",
                rows: "",
                want: None,
            },
            // Additional boundaries of the version-only path.
            Case {
                name: "renamed legacy version",
                folder: "custom",
                flavor: "",
                version: "1.60.1.70338",
                rows: "",
                want: None,
            },
            Case {
                name: "CN folder does not assign region",
                folder: "_cn_beta_",
                flavor: "",
                version: "1.60.1.70338",
                rows: CN,
                want: None,
            },
            Case {
                name: "equal beta builds do not assign region",
                folder: "custom",
                flavor: "",
                version: "1.60.1.70338",
                rows: "us|1|1.60.1.70338|wow_classic_beta\ncn|1|1.60.1.70338|wow_cn_beta\n",
                want: None,
            },
            Case {
                name: "local version needs no unrelated catalog row",
                folder: "custom",
                flavor: "wow_cn_beta",
                version: "1.60.1.70338",
                rows: ASIA,
                want: None,
            },
            Case {
                name: "regular folder and exact catalog supply identity",
                folder: "_retail_",
                flavor: "",
                version: "",
                rows: "us|1|12.1.0.69875|wow\n",
                want: Some(("retail", "retail")),
            },
            Case {
                name: "unsupported version cannot use folder",
                folder: "_retail_",
                flavor: "",
                version: "12.2.0.12345",
                rows: "",
                want: None,
            },
            Case {
                name: "malformed version cannot use folder",
                folder: "_retail_",
                flavor: "",
                version: "12.1.0",
                rows: "",
                want: None,
            },
            Case {
                name: "version-only exact catalog conflict",
                folder: "custom",
                flavor: "",
                version: "1.60.1.70338",
                rows: "us|1|1.60.1.70339|wow_forever\n",
                want: None,
            },
            Case {
                name: "version-only identical catalog duplicates",
                folder: "custom",
                flavor: "",
                version: "1.60.1.70338",
                rows: "us|1|1.60.1.70338|wow_forever\nus|1|1.60.1.70338|wow_forever\n",
                want: None,
            },
        ];
        for case in cases {
            let fixture = Fixture::new();
            let path = fixture.0.join(case.folder);
            fs::create_dir(&path).unwrap();
            if !case.flavor.is_empty() {
                fs::write(
                    path.join(".flavor.info"),
                    format!("Product Flavor!STRING:0\n{}\n", case.flavor),
                )
                .unwrap();
            }
            if !case.version.is_empty() {
                fs::write(path.join("version.txt"), case.version).unwrap();
            }
            if !case.rows.is_empty() {
                fixture.catalog(case.rows);
            }
            let observed = inspect(&path);
            if let Some((family, delivery)) = case.want {
                let observed = observed.unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
                assert_eq!(observed.product, family, "{}", case.name);
                assert_eq!(client_data_product(&observed), delivery, "{}", case.name);
                assert!(
                    observed.interface_known && observed.interface > 0,
                    "{}",
                    case.name
                );
                assert!(!observed.toc.is_empty(), "{}", case.name);
                assert!(client_matches_product(&observed, family), "{}", case.name);
                assert!(client_matches_product(&observed, delivery), "{}", case.name);
                let source = if case.flavor.is_empty() && case.version.is_empty() {
                    "folder"
                } else if case.flavor.is_empty() {
                    "version.txt"
                } else {
                    ".flavor.info"
                };
                let serialized = serde_json::to_value(&observed).unwrap();
                assert_eq!(
                    serialized["productCode"],
                    family_code(product(family).unwrap()),
                    "{}",
                    case.name
                );
                assert_eq!(serialized["identitySource"], source, "{}", case.name);
                assert!(serialized.get("catalogProduct").is_none(), "{}", case.name);
            } else {
                assert_eq!(
                    observed.unwrap_err().code,
                    "selection.client_identity",
                    "{}",
                    case.name
                );
            }
        }
    }
    #[test]
    fn version_only_families_and_empty_flavor_fallback_remain_bounded() {
        for profile in &PRODUCTS {
            let fixture = Fixture::new();
            let path = fixture.0.join("custom");
            fs::create_dir(&path).unwrap();
            fs::write(path.join(".flavor.info"), "\u{feff}\n ").unwrap();
            fs::write(path.join("flavor.info"), "\n").unwrap();
            fs::write(
                path.join("version.txt"),
                format!("{}.70338", profile.series),
            )
            .unwrap();
            if profile.id == "forever" {
                fixture.catalog("cn|1|1.60.1.70338|wowf\n");
            }
            let mut meter = Meter::default();
            let observed = inspect_with(&path, &mut meter).unwrap();
            assert_eq!(observed.product, profile.family);
            assert_eq!(client_data_product(&observed), profile.id);
            assert_eq!(observed.identity_source, "version.txt");
            assert!(meter.cost.read_calls > 0);
            assert!(meter.reads.iter().all(|bytes| *bytes <= 65536));
            meter.deny_read = true;
            assert_eq!(
                inspect_with(&path, &mut meter).unwrap_err().message,
                "fixture.read_budget"
            );
        }
        let fixture = Fixture::new();
        let path = fixture.client("custom", "wowf");
        fs::write(path.join("version.txt"), "1.60.1.70338").unwrap();
        fs::write(path.join("flavor.info"), "wow_classic_beta").unwrap();
        assert_eq!(inspect(&path).unwrap().catalog_product, "wowf");
        fs::write(path.join(".flavor.info"), "").unwrap();
        fs::write(path.join("flavor.info"), "wow_beta").unwrap();
        assert!(
            inspect(&path).is_err(),
            "unknown alternate flavor must not fall back to version"
        );
        fs::remove_file(path.join("flavor.info")).unwrap();
        fs::write(path.join("version.txt"), vec![b'1'; 4097]).unwrap();
        assert!(inspect_with(&path, &mut Meter::default()).is_err());
    }

    #[test]
    fn selection_precedence_and_installation_consistency_are_distinct_resolution_layers() {
        struct Case {
            name: &'static str,
            folder: &'static str,
            flavor: &'static str,
            version: &'static str,
            builds: &'static [(&'static str, &'static str)],
            // family, canonical code, observed slot, build, identity source
            want: Option<(
                &'static str,
                &'static str,
                &'static str,
                &'static str,
                &'static str,
            )>,
            installation_ok: bool,
        }
        let cases = [
            Case {
                name: "local version outranks matching catalog for selection",
                folder: "custom",
                flavor: "wow",
                version: "12.1.0.1",
                builds: &[("wow", "12.1.0.2")],
                want: Some(("retail", "wow", "", "12.1.0.1", ".flavor.info")),
                installation_ok: false,
            },
            Case {
                name: "slot version outranks matching catalog for selection",
                folder: "custom",
                flavor: "wow_cn_beta",
                version: "1.60.1.1",
                builds: &[("wow_cn_beta", "1.60.1.2")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "legacy version checks default data slot at installation",
                folder: "custom",
                flavor: "",
                version: "1.60.1.1",
                builds: &[("wow_classic_beta", "1.60.1.2")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "CN version never borrows Asia row",
                folder: "custom",
                flavor: "wow_cn_beta",
                version: "1.60.1.1",
                builds: &[("wow_classic_beta", "1.60.1.2")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "same-family slot rows select last before installation check",
                folder: "custom",
                flavor: "wow_classic_beta",
                version: "",
                builds: &[
                    ("wow_classic_beta", "1.60.1.1"),
                    ("wow_classic_beta", "1.60.1.2"),
                ],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "identical slot rows allowed",
                folder: "custom",
                flavor: "wow_classic_beta",
                version: "",
                builds: &[
                    ("wow_classic_beta", "1.60.1.1"),
                    ("wow_classic_beta", "1.60.1.1"),
                ],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "different-family slot rows ambiguous",
                folder: "custom",
                flavor: "wow_classic_beta",
                version: "",
                builds: &[
                    ("wow_classic_beta", "1.60.1.1"),
                    ("wow_classic_beta", "5.5.4.1"),
                ],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "renamed Asia slot resolves Classic by version",
                folder: "custom",
                flavor: "wow_classic_beta",
                version: "5.5.4.1",
                builds: &[],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "renamed Asia slot resolves Classic by catalog",
                folder: "custom",
                flavor: "wow_classic_beta",
                version: "",
                builds: &[("wow_classic_beta", "5.5.4.1")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "renamed CN slot still requires Forever",
                folder: "custom",
                flavor: "wow_cn_beta",
                version: "5.5.4.1",
                builds: &[],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "known beta folder still requires Forever",
                folder: "_classic_beta_",
                flavor: "wow_classic",
                version: "5.5.4.1",
                builds: &[],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "identical regular rows allowed",
                folder: "custom",
                flavor: "wow",
                version: "",
                builds: &[("wow", "12.1.0.1"), ("wow", "12.1.0.1")],
                want: Some(("retail", "wow", "", "12.1.0.1", ".flavor.info")),
                installation_ok: true,
            },
            Case {
                name: "different regular builds ambiguous",
                folder: "custom",
                flavor: "wow",
                version: "",
                builds: &[("wow", "12.1.0.1"), ("wow", "12.1.0.2")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "known folder with exact active build",
                folder: "_ReTaIl_",
                flavor: "",
                version: "",
                builds: &[("wow", "12.1.0.1")],
                want: Some(("retail", "wow", "", "12.1.0.1", "folder")),
                installation_ok: true,
            },
            Case {
                name: "legacy folder with exact active build",
                folder: "_forever_",
                flavor: "",
                version: "",
                builds: &[("wow_forever", "1.60.1.1")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "legacy folder cannot borrow beta slot",
                folder: "_forever_",
                flavor: "",
                version: "",
                builds: &[("wow_classic_beta", "1.60.1.1")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "arbitrary folder cannot choose catalog identity",
                folder: "custom",
                flavor: "",
                version: "",
                builds: &[("wow", "12.1.0.1")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "known folder requires a build",
                folder: "_retail_",
                flavor: "",
                version: "",
                builds: &[],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "unknown explicit product cannot fall back to folder",
                folder: "_retail_",
                flavor: "wow_beta",
                version: "",
                builds: &[("wow", "12.1.0.1")],
                want: None,
                installation_ok: false,
            },
            Case {
                name: "plus sign is not a reference build digit",
                folder: "custom",
                flavor: "wow",
                version: "12.1.0.+1",
                builds: &[],
                want: None,
                installation_ok: false,
            },
        ];
        for case in cases {
            let active: Vec<_> = case
                .builds
                .iter()
                .map(|&(product, build)| ClientBuild {
                    product_code: product.into(),
                    full_build: build.into(),
                    branch: String::new(),
                })
                .collect();
            let flavor =
                (!case.flavor.is_empty()).then(|| (case.flavor.into(), ".flavor.info".into()));
            let result =
                resolve_identity(case.folder, flavor, case.version, &active, &mut Unmetered);
            if let Some((family, code, slot, build, source)) = case.want {
                let identity = result.unwrap_or_else(|error| panic!("{}: {error:?}", case.name));
                assert_eq!(
                    (
                        identity.profile.product,
                        identity.product_code.as_str(),
                        identity.catalog_product.as_str(),
                        identity.full_build.as_str(),
                        identity.source.as_str()
                    ),
                    (family, code, slot, build, source),
                    "{}",
                    case.name
                );
                let installation = validate_installation_catalog(&identity, &active);
                assert_eq!(installation.is_ok(), case.installation_ok, "{}", case.name);
                if !case.installation_ok {
                    assert_eq!(
                        installation.unwrap_err().code,
                        "selection.installation_metadata_conflict",
                        "{}",
                        case.name
                    );
                }
            } else {
                assert_eq!(
                    result.unwrap_err().code,
                    "selection.client_identity",
                    "{}",
                    case.name
                );
            }
        }
    }
    #[test]
    fn first_nonempty_flavor_uses_reference_word_matching_and_does_not_read_lower_priority_file() {
        for value in [
            "wow",
            "WOW",
            "Product Flavor!STRING:0\nWoW\n",
            "label: wow; ignored prose",
            "wow-other",
            "\0wow\0",
            "éwowé",
        ] {
            let fixture = Fixture::new();
            let path = fixture.client("custom", "wow");
            fs::write(path.join(".flavor.info"), value).unwrap();
            fs::write(path.join("version.txt"), "12.1.0.1").unwrap();
            fs::create_dir(path.join("flavor.info")).unwrap();
            let observed = inspect(&path).unwrap();
            assert_eq!(observed.product_code, "wow", "{value:?}");
            assert_eq!(observed.identity_source, ".flavor.info", "{value:?}");
        }
        for value in [
            "notwow",
            "_wow",
            "wow wow",
            "wow_cn_beta wow_classic_beta",
            "no identity",
        ] {
            let fixture = Fixture::new();
            let path = fixture.client("_retail_", "wow");
            fs::write(path.join(".flavor.info"), value).unwrap();
            fs::write(path.join("flavor.info"), "wow").unwrap();
            fs::write(path.join("version.txt"), "12.1.0.1").unwrap();
            assert_eq!(
                inspect(&path).unwrap_err().code,
                "selection.client_identity",
                "{value:?}"
            );
        }
        let fixture = Fixture::new();
        let path = fixture.client("custom", "wow");
        fs::write(path.join(".flavor.info"), "\u{feff}\n ").unwrap();
        fs::write(path.join("flavor.info"), "WoWf").unwrap();
        fixture.catalog(FORMAL);
        let observed = inspect(&path).unwrap();
        assert_eq!(observed.product_code, "wowf");
        assert_eq!(observed.catalog_product, "wowf");
        assert_eq!(observed.identity_source, "flavor.info");
    }
    #[test]
    fn parent_catalog_is_authoritative_and_folder_fallback_requires_exact_active_build() {
        let fixture = Fixture::new();
        let path = fixture.0.join("_retail_");
        fs::create_dir(&path).unwrap();
        fixture.catalog("us|1|12.1.0.1|wow\n");
        fs::write(
            path.join(".build.info"),
            "malformed local catalog must not replace parent",
        )
        .unwrap();
        let observed = inspect(&path).unwrap();
        assert_eq!(observed.product_code, "wow");
        assert_eq!(observed.full_build, "12.1.0.1");
        assert_eq!(observed.identity_source, "folder");
        fixture.catalog("us|0|12.1.0.1|wow\n");
        assert_eq!(
            inspect(&path).unwrap_err().code,
            "selection.client_identity"
        );
        fs::remove_file(fixture.0.join(".build.info")).unwrap();
        assert_eq!(
            inspect(&path).unwrap_err().code,
            "selection.client_identity"
        );
    }
}
