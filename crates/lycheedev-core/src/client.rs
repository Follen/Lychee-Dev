//! Explicit flavor/build identity; directory names and equal builds never select a product.
use crate::{Result, ToolkitError, selection::valid_build, storage};
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

pub struct Product {
    pub id: &'static str,
    pub display_name: &'static str,
    pub catalog_product: &'static str,
    pub series: &'static str,
    pub channel: &'static str,
    pub interface: Option<u32>,
}
/// `forever` is the compatibility id for Forever Asia Beta only.
pub static PRODUCTS: [Product; 5] = [
    Product {
        id: "retail",
        display_name: "Retail",
        catalog_product: "wow",
        series: "12.1.0",
        channel: "",
        interface: Some(120100),
    },
    Product {
        id: "classic",
        display_name: "Classic",
        catalog_product: "wow_classic",
        series: "5.5.4",
        channel: "",
        interface: Some(50504),
    },
    Product {
        id: "titan",
        display_name: "Titan",
        catalog_product: "wow_classic_titan",
        series: "3.80.2",
        channel: "",
        interface: Some(38002),
    },
    Product {
        id: "forever",
        display_name: "Forever Asia Beta",
        catalog_product: "wow_classic_beta",
        series: "1.60.1",
        channel: "asia-beta",
        interface: Some(16001),
    },
    // A publishing build observation does not authorize a Source API interface.
    Product {
        id: "cn-beta",
        display_name: "Forever CN Beta",
        catalog_product: "wow_cn_beta",
        series: "1.60.1",
        channel: "cn-beta",
        interface: None,
    },
];
pub fn product(value: &str) -> Option<&'static Product> {
    PRODUCTS
        .iter()
        .find(|p| p.id == value || p.display_name == value)
}

pub fn catalog_product(value: &str) -> Option<&'static Product> {
    PRODUCTS.iter().find(|p| p.catalog_product == value)
}
pub fn canonical_product(value: &str) -> Option<&'static str> {
    product(value).map(|p| p.id)
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
fn identity_error(message: &str) -> ToolkitError {
    ToolkitError::new("selection.client_identity", message, 4)
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
        return Err(identity_error("metadata handle is a reparse point"));
    }
    if !info.is_file() || info.len() > maximum as u64 {
        return Err(ToolkitError::new(
            "selection.client_identity",
            "identity file is not bounded regular text",
            4,
        ));
    }
    control.charge(MetadataCost {
        allocation_bytes: (maximum as u64 + 1) * 3,
        work_units: (maximum as u64 + 1) * 4,
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
    if info.len() != raw.len() as u64 {
        return Err(identity_error("metadata changed while reading"));
    }
    root.verify_controlled(&mut MetadataFiles(&mut *control))?;
    if raw.len() > maximum || raw.contains(&0) {
        return Err(identity_error(
            "identity text exceeds bounds or contains NUL",
        ));
    }
    Ok(String::from_utf8(raw)
        .map_err(|e| ToolkitError::invalid(e.to_string()))?
        .trim_start_matches('\u{feff}')
        .trim()
        .into())
}
fn flavor(root: &Directory, control: &mut impl MetadataControl) -> Result<(String, String)> {
    let mut selected: Option<(String, String)> = None;
    for name in [".flavor.info", "flavor.info"] {
        let value = text(root, name, 4096, control)?;
        if value.is_empty() {
            continue;
        }
        let mut lines = value.lines();
        let first = lines.next().unwrap_or("").trim();
        let code = if first == "Product Flavor!STRING:0" {
            lines.next().unwrap_or("").trim()
        } else {
            first
        };
        if lines.next().is_some() || catalog_product(code).is_none() {
            return Err(identity_error(
                "flavor metadata is missing, ambiguous or unsupported",
            ));
        }
        if selected.as_ref().is_some_and(|(old, _)| old != code) {
            return Err(identity_error("flavor metadata files disagree"));
        }
        if selected.is_none() {
            selected = Some((code.into(), name.into()));
        }
    }
    selected.ok_or_else(|| identity_error("explicit flavor metadata is required"))
}
fn catalog_identity(
    raw: &str,
    code: &str,
    control: &mut impl MetadataControl,
) -> Result<(String, String)> {
    control.charge(MetadataCost {
        allocation_bytes: raw.len() as u64 * 64 + 65536,
        work_units: raw.len() as u64 * 64 + 65536,
        ..MetadataCost::default()
    })?;
    let mut lines = raw.lines();
    let header: Vec<_> = lines
        .next()
        .ok_or_else(|| identity_error("empty build catalog"))?
        .split('|')
        .take(129)
        .collect();
    if header.len() > 128 {
        return Err(identity_error("build catalog exceeds column budget"));
    }
    let mut columns = BTreeMap::new();
    for (i, field) in header.iter().enumerate() {
        let name = field.split('!').next().unwrap_or("").trim();
        if name.is_empty() || columns.insert(name, i).is_some() {
            return Err(identity_error(
                "build catalog has duplicate or empty columns",
            ));
        }
    }
    for required in ["Product", "Version", "Active"] {
        if !columns.contains_key(required) {
            return Err(identity_error("build catalog identity columns are missing"));
        }
    }
    let mut selected = None;
    let mut rows = 0usize;
    for line in lines {
        control.check()?;
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        rows += 1;
        if rows > 4096 {
            return Err(identity_error("build catalog exceeds row budget"));
        }
        let values: Vec<_> = line.split('|').take(129).collect();
        if values.len() != header.len() {
            return Err(identity_error("build catalog row shape differs"));
        }
        let get = |name: &str| values[columns[name]].trim();
        if !matches!(get("Active"), "0" | "1") || !valid_build(get("Version")) {
            return Err(identity_error("invalid build catalog identity"));
        }
        if get("Active") != "1" || get("Product") != code {
            continue;
        }
        let branch = columns
            .get("Branch")
            .map(|&i| values[i].trim())
            .unwrap_or("");
        if branch.len() > 64
            || !branch
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(identity_error("invalid publishing metadata branch"));
        }
        if selected
            .replace((get("Version").into(), branch.into()))
            .is_some()
        {
            return Err(identity_error(
                "multiple active rows for exact flavor product",
            ));
        }
    }
    selected.ok_or_else(|| identity_error("no active build row matches exact flavor product"))
}
pub fn inspect(directory: &Path) -> Result<ClientInstallation> {
    inspect_with(directory, &mut Unmetered)
}
pub fn inspect_with(
    directory: &Path,
    control: &mut impl MetadataControl,
) -> Result<ClientInstallation> {
    control.check()?;
    if directory.as_os_str().len() > 32768 {
        return Err(identity_error("client directory exceeds bound"));
    }
    let root = Directory::open_controlled(directory, &mut MetadataFiles(&mut *control))?;
    let (code, mut source) = flavor(&root, control)?;
    let profile =
        catalog_product(&code).ok_or_else(|| identity_error("unsupported product metadata"))?;
    let version = text(&root, "version.txt", 4096, control)?;
    if !version.is_empty() && !valid_build(&version) {
        return Err(ToolkitError::new(
            "selection.client_identity",
            "version.txt requires exact full build",
            4,
        ));
    }
    control.charge(MetadataCost {
        allocation_bytes: 262144,
        work_units: 65536,
        ..MetadataCost::default()
    })?;
    let parent;
    let catalog_root = if exists(&root, ".build.info", control)? {
        Some(&root)
    } else {
        parent = root
            .path()
            .parent()
            .map(|path| Directory::open_controlled(path, &mut MetadataFiles(&mut *control)))
            .transpose()?;
        match parent.as_ref() {
            Some(parent) if exists(parent, ".build.info", control)? => Some(parent),
            _ => None,
        }
    };
    let (build, branch) = if let Some(parent) = catalog_root {
        let raw = text(parent, ".build.info", 1 << 20, control)?;
        let (build, branch) = catalog_identity(&raw, &code, control)?;
        if !version.is_empty() && version != build {
            return Err(identity_error(
                "version.txt contradicts exact flavor catalog row",
            ));
        }
        source.push_str("+.build.info");
        (build, branch)
    } else {
        (version, String::new())
    };
    if !build.starts_with(&format!("{}.", profile.series)) {
        return Err(ToolkitError::new(
            "selection.client_identity",
            "missing or contradictory product/build identity",
            4,
        ));
    }
    control.charge(MetadataCost {
        allocation_bytes: 131072,
        work_units: 32768,
        ..MetadataCost::default()
    })?;
    let result = ClientInstallation {
        directory: root
            .path()
            .to_string_lossy()
            .trim_start_matches("\\\\?\\")
            .into(),
        product: profile.id.into(),
        product_code: code.clone(),
        catalog_product: code,
        display_name: profile.display_name.into(),
        channel: profile.channel.into(),
        metadata_branch: branch,
        full_build: build,
        interface: profile.interface.unwrap_or(0),
        interface_known: profile.interface.is_some(),
        toc: "Lychee Dev.toc".into(),
        identity_source: source,
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
            fs::write(
                self.0.join(".build.info"),
                format!("Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0\n{rows}"),
            )
            .unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    const ASIA: &str = "us|1|1.60.1.70338|wow_classic_beta\n";
    const CN: &str = "cn|1|1.60.1.70338|wow_cn_beta\n";
    #[derive(Default)]
    struct Meter {
        cost: MetadataCost,
        reads: Vec<u64>,
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
    fn controlled_parent_catalog_inspection_is_metered_and_fresh() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wow_classic_beta");
        fixture.catalog(ASIA);
        let mut meter = Meter::default();
        let first = inspect_with(&path, &mut meter).unwrap();
        assert_eq!(first, inspect(&path).unwrap());
        assert!(meter.cost.io_operations > meter.cost.read_calls);
        assert!(meter.cost.allocation_bytes > 0 && meter.cost.work_units > 0);
        assert!(meter.reads.iter().all(|n| *n <= 65536));
        let before = meter.cost.read_calls;
        fixture.catalog("us|1|1.60.1.70339|wow_classic_beta\n");
        let second = inspect_with(&path, &mut meter).unwrap();
        assert_ne!(first, second);
        assert!(meter.cost.read_calls > before);
    }
    #[test]
    fn controlled_inspection_cancellation_and_read_budget_keep_original_cause() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wow_classic_beta");
        fixture.catalog(ASIA);
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
        let path = fixture.client("renamed", "wow_classic_beta");
        fs::write(path.join("version.txt"), vec![b'1'; 4097]).unwrap();
        assert!(inspect_with(&path, &mut Meter::default()).is_err());
        fs::remove_file(path.join("version.txt")).unwrap();
        fs::write(fixture.0.join(".build.info"), vec![b'1'; (1 << 20) + 1]).unwrap();
        assert!(inspect_with(&path, &mut Meter::default()).is_err());
        fs::write(fixture.0.join(".build.info"), "x|".repeat(129)).unwrap();
        assert!(inspect_with(&path, &mut Meter::default()).is_err());
    }
    #[test]
    fn controlled_multichunk_read_has_no_request_larger_than_64k() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wow_classic_beta");
        fixture.catalog(&format!("#{}\n{ASIA}", "x".repeat(140000)));
        let mut meter = Meter::default();
        assert_eq!(
            inspect_with(&path, &mut meter).unwrap().full_build,
            "1.60.1.70338"
        );
        assert!(meter.reads.iter().filter(|n| **n == 65536).count() >= 3);
        assert!(meter.reads.iter().all(|n| *n <= 65536));
    }
    #[test]
    fn controlled_inspection_refuses_metadata_directory_and_missing_identity() {
        let fixture = Fixture::new();
        let path = fixture.client("renamed", "wow_classic_beta");
        fs::create_dir(path.join("version.txt")).unwrap();
        assert!(inspect_with(&path, &mut Meter::default()).is_err());
        let missing = fixture.0.join("missing");
        assert!(inspect_with(&missing, &mut Meter::default()).is_err());
    }
    #[test]
    fn controlled_metadata_keeps_client_and_catalog_ancestry_held_until_completion() {
        let fixture = Fixture::new();
        let path = fixture.client("client", "wow_classic_beta");
        fixture.catalog(ASIA);
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
        assert_eq!(identity.catalog_product, "wow_classic_beta");
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
    fn equal_build_beta_clients_keep_exact_products_without_version_files() {
        let fixture = Fixture::new();
        let asia = fixture.client("renamed-one", "wow_classic_beta");
        let cn = fixture.client("renamed-two", "wow_cn_beta");
        fixture.catalog(&format!("{ASIA}{CN}"));
        let asia = inspect(&asia).unwrap();
        let cn = inspect(&cn).unwrap();
        assert_eq!(asia.full_build, cn.full_build);
        assert_eq!(
            (
                asia.product.as_str(),
                asia.product_code.as_str(),
                asia.catalog_product.as_str()
            ),
            ("forever", "wow_classic_beta", "wow_classic_beta")
        );
        assert_eq!(
            (
                cn.product.as_str(),
                cn.product_code.as_str(),
                cn.catalog_product.as_str()
            ),
            ("cn-beta", "wow_cn_beta", "wow_cn_beta")
        );
        assert_eq!(
            (asia.display_name.as_str(), cn.display_name.as_str()),
            ("Forever Asia Beta", "Forever CN Beta")
        );
        assert_eq!(
            (asia.metadata_branch.as_str(), cn.metadata_branch.as_str()),
            ("us", "cn")
        );
        assert_eq!(
            (asia.channel.as_str(), cn.channel.as_str()),
            ("asia-beta", "cn-beta")
        );
        assert!(asia.interface_known);
        assert!(!cn.interface_known);
        assert_eq!(cn.interface, 0);
        assert_eq!(cn.identity_source, ".flavor.info+.build.info");
        let value = serde_json::to_value(&asia).unwrap();
        assert!(
            value.get("region").is_none(),
            "metadata branch is not player geography"
        );
    }
    #[test]
    fn beta_missing_wrong_duplicate_and_conflicting_identity_refuse() {
        let fixture = Fixture::new();
        let cn = fixture.client("_cn_beta_", "wow_cn_beta");
        assert!(inspect(&cn).is_err());
        fixture.catalog(ASIA);
        assert!(
            inspect(&cn).is_err(),
            "equal build from another product cannot supply identity"
        );
        fixture.catalog(&format!("{CN}{CN}"));
        assert!(
            inspect(&cn).is_err(),
            "identical duplicate active rows remain ambiguous"
        );
        fixture.catalog(&format!("{CN}us|1|1.60.1.70338|wow_cn_beta\n"));
        assert!(
            inspect(&cn).is_err(),
            "a second publishing branch cannot be silently chosen"
        );
        fixture.catalog(&format!("{CN}cn|1|1.60.1.70339|wow_cn_beta\n"));
        assert!(inspect(&cn).is_err());
        fixture.catalog(CN);
        fs::write(cn.join("version.txt"), "1.60.1.70339").unwrap();
        assert!(inspect(&cn).is_err());
        fs::remove_file(cn.join("version.txt")).unwrap();
        fs::write(cn.join("flavor.info"), "wow_classic_beta").unwrap();
        assert!(inspect(&cn).is_err());
        fs::remove_file(cn.join("flavor.info")).unwrap();
        fs::write(
            cn.join(".flavor.info"),
            "Product Flavor!STRING:0\nwow_cn_beta\nwow_classic_beta\n",
        )
        .unwrap();
        assert!(inspect(&cn).is_err());
        let unknown = fixture.0.join("_beta_");
        fs::create_dir_all(&unknown).unwrap();
        fs::write(unknown.join("version.txt"), "1.60.1.70338").unwrap();
        assert!(
            inspect(&unknown).is_err(),
            "folder and build are not a product selector"
        );
    }
    #[test]
    fn formal_aliases_are_shared_and_cn_has_no_borrowed_source_authority() {
        assert_eq!(canonical_product("forever"), Some("forever"));
        assert_eq!(canonical_product("Forever Asia Beta"), Some("forever"));
        assert_eq!(canonical_product("Forever CN Beta"), Some("cn-beta"));
        assert_eq!(canonical_product("cn-beta"), Some("cn-beta"));
        assert_eq!(canonical_product("Forever"), None);
        let mut source = crate::selection::SourcePin {
            repository: "wow-ui-source".into(),
            product: "forever".into(),
            exact_commit: "4d5d706b8e01c5ebe01c8dd9b7a07151d8d37069".into(),
            ..Default::default()
        };
        assert_eq!(crate::selection::source_interface(&source), Some(16001));
        source.product = "cn-beta".into();
        assert_eq!(crate::selection::source_interface(&source), None);
        let old = serde_json::json!({"directory":"fixture","product":"forever","productCode":"wow_forever","fullBuild":"1.60.1.70338","interface":16001,"toc":"Lychee Dev.toc","identitySource":"version.txt"});
        let old: ClientInstallation = serde_json::from_value(old).unwrap();
        assert!(!old.interface_known);
        assert_eq!(
            old.product_code, "wow_forever",
            "deserialization never rewrites old evidence"
        );
    }
}
