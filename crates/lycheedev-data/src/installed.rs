//! Explicit local build identity and authenticated configurations. Reading a
//! client never selects a region, locale or definition revision implicitly.
use crate::{
    casc, fault,
    resource::{Budget, Cost, RetainedReservation},
    schema,
};
use lycheedev_core::{
    Result, ToolkitError,
    client::{self, ClientInstallation, MetadataControl, MetadataCost},
    installation_selection, storage,
};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
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
/// Fixed Go records.InstalledBuild wire shape. Enrichment stays internal.
#[derive(Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct InstalledBuildPublic<'a> {
    product: &'a str,
    full_build: &'a str,
    build_config: &'a str,
    #[serde(rename = "cdnConfig")]
    cdn_config: &'a str,
    active: bool,
}
impl InstalledBuild {
    fn public(&self) -> InstalledBuildPublic<'_> {
        InstalledBuildPublic {
            product: &self.product,
            full_build: &self.full_build,
            build_config: &self.build_config,
            cdn_config: &self.cdn_config,
            active: self.active,
        }
    }
}
impl Serialize for InstalledBuild {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.public().serialize(serializer)
    }
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
    /// Exact toolkit delivery product id; never inferred from a build.
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
    if request.installation.as_os_str().is_empty() {
        return Err(fault(
            "records.target_installation_required",
            2,
            "Installation is required",
        ));
    }
    if requested_product.is_none() && request.full_build.as_deref().is_some_and(|s| !s.is_empty()) {
        return Err(fault(
            "records.target_product_required_for_build",
            2,
            "An exact build requires a product",
        ));
    }
    // Target preparation selects a real client even when passed a game root.
    // A catalog alone is sufficient only for a reader with an existing pin.
    let selected = select_client_budgeted(
        &request.installation,
        requested_product.unwrap_or(""),
        budget,
    )?;
    if request
        .full_build
        .as_deref()
        .filter(|s| !s.is_empty())
        .is_some_and(|build| build != selected.full_build)
    {
        return Err(fault(
            "selection.target_build_unavailable",
            3,
            "Requested build differs from the selected client",
        ));
    }
    let installation = PathBuf::from(&selected.directory);
    let root = installation.parent().ok_or_else(invalid)?.to_owned();
    let product_code = observed_slot(&selected);
    let track = client::client_data_product(&selected);
    let build = selected.full_build.clone();
    let flavor_sha256 = flavor_digest(&installation, budget)?;
    let metadata = resolve_local_budgeted(&root, product_code, &build, budget)?;
    // The selector and configuration reads have independent file lifetimes.
    // Recheck both observations before returning a pin, just as Go target.go.
    let current = select_client_budgeted(
        &request.installation,
        requested_product.unwrap_or(""),
        budget,
    )?;
    if !same_client_identity(&current, &selected) {
        return Err(pinned_changed(
            "Selected client changed during target preparation",
        ));
    }
    let after = resolve_local_budgeted(&root, product_code, &build, budget)?;
    if after.installed.public() != metadata.installed.public() {
        return Err(pinned_changed(
            "Active catalog identity changed during target preparation",
        ));
    }
    let metadata = after;
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
        installation,
        common_root: root,
        flavor_sha256,
    })
}
/// Resolve an already-pinned reader without preparing a new target. This is
/// the records.file_source path: catalog-only CASC roots are legitimate here.
pub fn resolve_pinned_target_budgeted(
    installation: &Path,
    pin: &lycheedev_core::selection::DataPin,
    budget: Option<&Budget>,
) -> Result<ResolvedLocalTarget> {
    let profile = client::product(&pin.product).ok_or_else(|| {
        fault(
            "selection.unsupported_data_product",
            2,
            "Unknown toolkit product",
        )
    })?;
    if profile.reused_slot
        && (!lycheedev_core::selection::valid_build(&pin.full_build)
            || !pin.full_build.starts_with(&format!("{}.", profile.series)))
    {
        return Err(fault(
            "selection.data_product_build_mismatch",
            4,
            "Pinned build is outside the verified reused-slot series",
        ));
    }
    let product_code = if pin.product == "forever" {
        "wow_forever"
    } else {
        profile.catalog_product
    };
    let (common_root, product_code) =
        data_installation_root_budgeted(installation, product_code, &pin.full_build, budget)?;
    let metadata = resolve_local_budgeted(&common_root, &product_code, &pin.full_build, budget)?;
    if metadata.installed.build_config != pin.build_config
        || metadata.installed.cdn_config != pin.cdn_config
    {
        return Err(pinned_changed(
            "Local configurations differ from fixed DataPin",
        ));
    }
    owned(
        budget,
        installation.as_os_str().len()
            + pin.product.len()
            + pin.region.len()
            + pin.full_build.len()
            + pin.build_config.len()
            + pin.cdn_config.len()
            + pin.language.len()
            + pin.definition_commit.len(),
    )?;
    Ok(ResolvedLocalTarget {
        data: pin.clone(),
        metadata,
        installation: installation.to_owned(),
        common_root,
        flavor_sha256: None,
    })
}
/// Filesystem/path/parser allocation admissions are transient retained memory,
/// not bytes read from metadata. Keep conservative reservations until the whole
/// inspection ends; only owned output is charged beyond that scope.
struct InstallationBudget<'a> {
    budget: Option<&'a Budget>,
    allocations: Vec<RetainedReservation<'a>>,
}
impl<'a> InstallationBudget<'a> {
    fn new(budget: Option<&'a Budget>) -> Self {
        Self {
            budget,
            allocations: Vec::new(),
        }
    }
}
impl MetadataControl for InstallationBudget<'_> {
    fn charge(&mut self, cost: MetadataCost) -> Result<()> {
        if let Some(budget) = self.budget {
            budget.charge(Cost {
                metadata_bytes: cost.requested_bytes,
                decode_work: cost
                    .work_units
                    .saturating_add(cost.io_operations)
                    .saturating_add(cost.read_calls),
                ..Default::default()
            })?;
            if cost.allocation_bytes != 0 {
                let bytes = usize::try_from(cost.allocation_bytes)
                    .ok()
                    .and_then(|bytes| bytes.checked_add(4 * size_of::<RetainedReservation<'_>>()))
                    .ok_or_else(exceeds)?;
                // Includes the vector's geometric growth and minimum initial capacity.
                let reservation = budget.reserve_retained(bytes)?;
                self.allocations.try_reserve(1).map_err(|_| exceeds())?;
                self.allocations.push(reservation);
            }
        }
        Ok(())
    }
    fn check(&mut self) -> Result<()> {
        self.budget.map_or(Ok(()), Budget::check)
    }
}
fn select_client_budgeted(
    directory: &Path,
    product: &str,
    budget: Option<&Budget>,
) -> Result<ClientInstallation> {
    let mut control = InstallationBudget::new(budget);
    let client = installation_selection::select_with(directory, product, &mut control)?;
    owned(
        budget,
        size_of::<ClientInstallation>()
            + client.directory.len()
            + client.product.len()
            + client.product_code.len()
            + client.catalog_product.len()
            + client.display_name.len()
            + client.channel.len()
            + client.metadata_branch.len()
            + client.full_build.len()
            + client.toc.len()
            + client.identity_source.len(),
    )?;
    Ok(client)
}
impl storage::FileControl for InstallationBudget<'_> {
    type Error = ToolkitError;
    fn charge(&mut self, cost: storage::FileCost) -> Result<()> {
        MetadataControl::charge(
            self,
            MetadataCost {
                io_operations: cost.io_operations,
                allocation_bytes: cost.allocation_bytes,
                work_units: cost.work_units,
                ..Default::default()
            },
        )
    }
    fn check(&mut self) -> Result<()> {
        MetadataControl::check(self)
    }
}
enum RootProbeError {
    Io(std::io::Error),
    Operation(ToolkitError),
}
impl From<std::io::Error> for RootProbeError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl From<RootProbeError> for ToolkitError {
    fn from(error: RootProbeError) -> Self {
        match error {
            RootProbeError::Io(error) => error.into(),
            RootProbeError::Operation(error) => error,
        }
    }
}
struct RootProbe<'a>(InstallationBudget<'a>);
impl storage::FileControl for RootProbe<'_> {
    type Error = RootProbeError;
    fn charge(&mut self, cost: storage::FileCost) -> std::result::Result<(), Self::Error> {
        storage::FileControl::charge(&mut self.0, cost).map_err(RootProbeError::Operation)
    }
    fn check(&mut self) -> std::result::Result<(), Self::Error> {
        MetadataControl::check(&mut self.0).map_err(RootProbeError::Operation)
    }
}
fn has_casc_catalog(directory: &Path, budget: Option<&Budget>) -> Result<bool> {
    let mut control = RootProbe(InstallationBudget::new(budget));
    let root = match storage::Directory::open_controlled(directory, &mut control) {
        Ok(root) => root,
        Err(RootProbeError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(false);
        }
        Err(error) => return Err(error.into()),
    };
    Ok(root
        .optional_metadata_controlled(".build.info".as_ref(), &mut control)?
        .is_some())
}
fn same_client_identity(left: &ClientInstallation, right: &ClientInstallation) -> bool {
    // Go compares its public ClientInstallation, not optional internal enrichment.
    left.directory == right.directory
        && left.product == right.product
        && left.product_code == right.product_code
        && left.catalog_product == right.catalog_product
        && left.full_build == right.full_build
        && left.interface == right.interface
        && left.toc == right.toc
        && left.identity_source == right.identity_source
}
fn observed_slot(client: &ClientInstallation) -> &str {
    if client.catalog_product.is_empty() {
        &client.product_code
    } else {
        &client.catalog_product
    }
}
fn pinned_changed(message: &str) -> ToolkitError {
    fault("records.pinned_build_changed", 4, message)
}
fn flavor_digest(directory: &Path, budget: Option<&Budget>) -> Result<Option<String>> {
    let mut control = InstallationBudget::new(budget);
    let root = storage::Directory::open_controlled(directory, &mut control)?;
    for name in [".flavor.info", "flavor.info"] {
        if root
            .optional_metadata_controlled(name.as_ref(), &mut control)?
            .is_none()
        {
            continue;
        }
        let raw = read(directory, &directory.join(name), 65536, budget)?;
        if checked_text(&raw, budget)?
            .trim_start_matches('\u{feff}')
            .trim()
            .is_empty()
        {
            continue;
        }
        owned(budget, 64)?;
        work(budget, raw.len())?;
        return Ok(Some(storage::sha256(&raw)));
    }
    Ok(None)
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
    schema::version(value).map_err(|_| invalid())
}
// A BTreeMap entry may cause a node allocation/split. Reserve a full node per
// insertion, including links/alignment, rather than pretending it is just K+V.
fn map_entry<K, V>() -> usize {
    16 * size_of::<(K, V)>() + 128
}
pub fn installed_rows_budgeted(raw: &[u8], budget: Option<&Budget>) -> Result<Vec<InstalledBuild>> {
    if raw.len() > 1 << 20 {
        return Err(exceeds());
    }
    let raw_text = checked_text(raw, budget)?;
    if !raw_text.contains('\n') {
        return Err(invalid());
    }
    let mut lines = raw_text.trim_start_matches('\u{feff}').lines();
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
        casc::key(get("Build Key")).map_err(|_| invalid())?;
        casc::key(get("CDN Key")).map_err(|_| invalid())?;
        let branch = columns
            .get("Branch")
            .map(|&i| values[i].trim())
            .unwrap_or("");
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
pub fn parse_configuration_budgeted(
    raw: Vec<u8>,
    key: &str,
    budget: Option<&Budget>,
) -> Result<ConfigDocument> {
    if raw.len() > 4 << 20 {
        return Err(exceeds());
    }
    let expected = casc::key(key).map_err(|_| invalid())?;
    // Go rejects malformed text before considering its content hash.
    checked_text(&raw, budget)?;
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
    // A lexical child plus held non-reparse ancestry and handle-relative file
    // open provides confinement without unchecked stat/canonicalize traversals.
    let _paths = temporary(budget, path.as_os_str().len() * 8 + 512)?;
    let mut control = InstallationBudget::new(budget);
    let held_root = storage::Directory::open_controlled(root, &mut control)?;
    let relative = path.strip_prefix(root).map_err(|_| invalid())?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(invalid());
    }
    let parent =
        storage::Directory::open_controlled(path.parent().ok_or_else(invalid)?, &mut control)?;
    let mut file =
        parent.open_file_controlled(path.file_name().ok_or_else(invalid)?, &mut control)?;
    MetadataControl::charge(
        &mut control,
        MetadataCost {
            io_operations: 1,
            ..Default::default()
        },
    )?;
    let meta = file.metadata()?;
    MetadataControl::check(&mut control)?;
    use std::os::windows::fs::MetadataExt;
    if !meta.is_file() || meta.file_attributes() & 0x400 != 0 {
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
    parent.verify_controlled(&mut control)?;
    held_root.verify_controlled(&mut control)?;
    checked_text(&raw, budget)?;
    if let Some(reservation) = allocation {
        reservation.commit();
    }
    Ok(raw)
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
    let (root, product_code) =
        data_installation_root_budgeted(root, product_code, full_build, budget)?;
    let root = root.as_path();
    let product_code = product_code.as_str();
    let _catalog_path = temporary(budget, root.as_os_str().len() * 2 + 64)?;
    let catalog_path = root.join(".build.info");
    let (raw, rows) = (|| {
        let raw = read(root, &catalog_path, 1 << 20, budget)?;
        let rows = installed_rows_budgeted(&raw, budget)?;
        Ok((raw, rows))
    })().map_err(|mut error: ToolkitError| {
        if error.code == "records.invalid_metadata" {
            if error.details.is_null() {
                error.details = serde_json::json!({});
            }
            if let Some(details) = error.details.as_object_mut() {
                details.entry("sourcePath").or_insert_with(|| serde_json::json!(catalog_path));
                details.entry("formatHint").or_insert_with(|| serde_json::json!("Pipe-delimited launcher build catalog with a header and row boundary"));
            }
        }
        error
    })?;
    let exact = rows
        .iter()
        .any(|row| row.active && row.product == product_code && row.full_build == full_build);
    // Fixed Go catalog.go permits the canonical Forever family only when an
    // exact row is absent. An observed regional slot never enters this fallback.
    let legacy_slots = !exact && product_code == "wow_forever" && full_build.starts_with("1.60.1.");
    let mut matches = rows.into_iter().filter(|row| {
        row.active
            && row.full_build == full_build
            && (row.product == product_code
                || (legacy_slots
                    && matches!(row.product.as_str(), "wow_classic_beta" | "wow_cn_beta")))
    });
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
        casc::key(key).map_err(|_| invalid())?;
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
        if n == 0 || n > 1usize << 50 {
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
/// Go records.dataInstallationRoot: an explicit CASC root retains the supplied
/// pin, while a selected client must match canonical product or actual slot and
/// full build before its parent archives can be used. Never borrow a sibling.
pub fn data_installation_root_budgeted(
    directory: &Path,
    product_code: &str,
    full_build: &str,
    budget: Option<&Budget>,
) -> Result<(PathBuf, String)> {
    if has_casc_catalog(directory, budget)? {
        owned(budget, directory.as_os_str().len() + product_code.len())?;
        return Ok((directory.to_owned(), product_code.to_owned()));
    }
    let selected = select_client_budgeted(directory, "", budget).map_err(|error| {
        if error.code == "selection.installation_metadata_conflict" {
            pinned_changed(&error.to_string()).with_details(error.details)
        } else {
            error
        }
    })?;
    if (selected.product_code != product_code && selected.catalog_product != product_code)
        || selected.full_build != full_build
    {
        return Err(pinned_changed(
            "Selected client differs from pinned product/slot/build",
        ));
    }
    let root = Path::new(&selected.directory)
        .parent()
        .ok_or_else(invalid)?
        .to_owned();
    owned(
        budget,
        root.as_os_str().len() + observed_slot(&selected).len(),
    )?;
    Ok((root, observed_slot(&selected).to_owned()))
}

mod target;
pub use target::{LocalTarget, TargetRequest, prepare_target};
