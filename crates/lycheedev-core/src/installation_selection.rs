//! Fixed Go 3.2 records installation discovery and selection. No recursive scans.
#![forbid(unsafe_code)]
use crate::{
    Result, ToolkitError,
    client::{self, ClientInstallation, MetadataControl, MetadataCost},
    control::OperationControl,
};
use lycheedev_windows::filesystem::destination::{Directory, FileControl, FileCost};
use serde::{Serialize, Serializer};
use std::{fs, io, os::windows::fs::MetadataExt, path::Path};

/// The public Go client shape. Internal enrichment must never leak through this DTO.
#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Client {
    pub directory: String,
    pub product: String,
    pub product_code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub catalog_product: String,
    pub full_build: String,
    pub interface: u32,
    pub toc: String,
    pub identity_source: String,
}
impl From<&ClientInstallation> for Client {
    fn from(value: &ClientInstallation) -> Self {
        Self {
            directory: value.directory.clone(),
            product: value.product.clone(),
            product_code: value.product_code.clone(),
            catalog_product: value.catalog_product.clone(),
            full_build: value.full_build.clone(),
            interface: value.interface,
            toc: value.toc.clone(),
            identity_source: value.identity_source.clone(),
        }
    }
}
impl From<ClientInstallation> for Client {
    fn from(value: ClientInstallation) -> Self {
        Self {
            directory: value.directory,
            product: value.product,
            product_code: value.product_code,
            catalog_product: value.catalog_product,
            full_build: value.full_build,
            interface: value.interface,
            toc: value.toc,
            identity_source: value.identity_source,
        }
    }
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallationIssue {
    pub directory: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub product_code: String,
    pub reason: String,
}
#[derive(Clone, Debug, Default, Serialize)]
pub struct InstallationDiscovery {
    pub directory: String,
    #[serde(serialize_with = "public_candidates")]
    pub candidates: Vec<ClientInstallation>,
    pub issues: Vec<InstallationIssue>,
}
fn public_candidates<S: Serializer>(
    clients: &[ClientInstallation],
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    use serde::ser::SerializeSeq;
    let mut sequence = serializer.serialize_seq(Some(clients.len()))?;
    for client in clients {
        sequence.serialize_element(&Client::from(client))?;
    }
    sequence.end()
}
/// Adapt one invocation's cancellation and deadline without creating a new budget.
pub struct InvocationMetadata<'a>(pub &'a OperationControl);
impl MetadataControl for InvocationMetadata<'_> {
    fn charge(&mut self, _: MetadataCost) -> Result<()> {
        self.0.check()
    }
    fn check(&mut self) -> Result<()> {
        self.0.check()
    }
}
struct Files<'a, C>(&'a mut C);
impl<C: MetadataControl> FileControl for Files<'_, C> {
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
fn controlled_io<T>(
    control: &mut impl MetadataControl,
    operation: impl FnOnce() -> io::Result<T>,
) -> Result<T> {
    control.check()?;
    control.charge(MetadataCost {
        io_operations: 1,
        allocation_bytes: 131072,
        work_units: 32768,
        ..MetadataCost::default()
    })?;
    let result = operation();
    control.check()?;
    result.map_err(Into::into)
}
fn spelling(path: &Path) -> String {
    client::display_path(path)
}
fn known_folder(path: &Path) -> bool {
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    client::client_folders()
        .iter()
        .any(|folder| name.eq_ignore_ascii_case(folder))
}
fn present(root: &Directory, name: &str, control: &mut impl MetadataControl) -> Result<bool> {
    root.optional_metadata_controlled(name.as_ref(), &mut Files(control))
        .map(|info| info.is_some())
}
fn markers(root: &Directory, control: &mut impl MetadataControl) -> Result<bool> {
    for name in [".flavor.info", "flavor.info", "version.txt"] {
        if present(root, name, control)? {
            return Ok(true);
        }
    }
    Ok(false)
}
fn issue_reason(error: &ToolkitError) -> String {
    if error.message == error.code || error.code == "selection.installation_metadata_conflict" {
        error.code.clone()
    } else {
        error.to_string()
    }
}
fn selection_error(kind: &str, product: &str, report: &InstallationDiscovery) -> ToolkitError {
    let exit = match kind {
        "installation_missing" => 3,
        "installation_ambiguous" | "installation_product_mismatch" => 2,
        _ => 4,
    };
    let code = format!("selection.{kind}");
    ToolkitError::new(&code, format!("{code}: scope {:?} product {product:?}; inspect candidates/issues and select a verified client directory", report.directory), exit)
        .with_details(serde_json::json!({"__commandContext":{"installationSelection":report,"requestedProduct":product}}))
}

/// Project the fixed selection error from an already observed discovery.
/// Native window selection may retain multiple candidates for PID filtering;
/// this helper does not rescan, choose a client, or reject that valid case.
pub fn require_discovered_candidates(report: &InstallationDiscovery) -> Result<()> {
    if !report.candidates.is_empty() {
        return Ok(());
    }
    Err(selection_error(
        if report.issues.is_empty() {
            "installation_missing"
        } else {
            "installation_metadata_conflict"
        },
        "",
        report,
    ))
}

/// Inspect an explicit client or at most 256 immediate entries under a game root.
/// Catalog rows never invent installed clients. Directory ownership guards remain
/// held during the scan, and reparse entries are not traversed from a root scan.
pub fn discover_with(
    directory: &Path,
    control: &mut impl MetadataControl,
) -> Result<InstallationDiscovery> {
    control.check()?;
    if directory.as_os_str().len() > 32768 {
        return Err(ToolkitError::new(
            "records.metadata_limit",
            "installation path exceeds bound",
            3,
        ));
    }
    let absolute = controlled_io(control, || std::path::absolute(directory))?;
    let mut report = InstallationDiscovery {
        directory: spelling(&absolute),
        ..Default::default()
    };
    // Capture NotFound before conversion into ToolkitError so absent scopes have
    // the selection error contract, while access and other OS errors propagate.
    let canonical = controlled_io(control, || match fs::canonicalize(&absolute) {
        Ok(path) => Ok(Some(path)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    })?;
    let Some(canonical) = canonical else {
        return Err(selection_error("installation_missing", "", &report));
    };
    report.directory = spelling(&canonical);
    let root = Directory::open_controlled(&canonical, &mut Files(&mut *control))?;
    let has_markers = markers(&root, control)?;
    let has_catalog = present(&root, ".build.info", control)?;
    let exact = has_markers || (!has_catalog && known_folder(&canonical));
    let mut paths = Vec::new();
    if exact {
        paths.push(canonical.clone());
    } else {
        control.charge(MetadataCost {
            // Reserve the enumeration handle's close and bounded entry storage.
            io_operations: 1,
            allocation_bytes: 257 * 131072,
            work_units: 257,
            ..MetadataCost::default()
        })?;
        let mut entries = controlled_io(control, || fs::read_dir(root.path()))?;
        let mut bounded = Vec::with_capacity(257);
        while bounded.len() < 257 {
            let entry = controlled_io(control, || entries.next().transpose())?;
            let Some(entry) = entry else {
                break;
            };
            bounded.push(entry);
        }
        // As Go ReadDir(257), enforce the scope bound before inspecting clients.
        if bounded.len() > 256 {
            return Err(ToolkitError::new(
                "records.metadata_limit",
                "installation root exceeds 256 entries",
                3,
            ));
        }
        for entry in bounded {
            control.check()?;
            let kind = controlled_io(control, || entry.file_type())?;
            if !kind.is_dir() || kind.is_symlink() {
                continue;
            }
            let name = entry.file_name();
            let info = root.metadata_controlled(&name, &mut Files(&mut *control))?;
            if info.file_attributes() & 0x400 != 0 {
                continue;
            }
            let child_path = root.path().join(name);
            let child = Directory::open_controlled(&child_path, &mut Files(&mut *control))?;
            let mut has_client = markers(&child, control)?;
            if !has_client && known_folder(&child_path) {
                for name in ["Wow.exe", "WowClassic.exe", "WowB.exe"] {
                    if let Some(info) = child
                        .optional_metadata_controlled(name.as_ref(), &mut Files(&mut *control))?
                    {
                        has_client |= info.is_file() && info.file_attributes() & 0x400 == 0;
                    }
                }
            }
            if has_client {
                paths.push(child_path);
            }
            child.verify_controlled(&mut Files(&mut *control))?;
        }
        paths.sort();
    }
    for path in paths {
        control.check()?;
        match client::inspect_observed_with(&path, control) {
            Ok(client) => {
                if !exact
                    && Path::new(&client.directory).parent().is_none_or(|parent| {
                        !spelling(parent).eq_ignore_ascii_case(&report.directory)
                    })
                {
                    report.issues.push(InstallationIssue {
                        directory: spelling(&path),
                        product_code: String::new(),
                        reason: "client resolves outside selected game root".into(),
                    });
                } else {
                    report.candidates.push(client);
                }
            }
            Err((error, observed))
                if matches!(
                    error.code.as_str(),
                    "selection.client_identity" | "selection.installation_metadata_conflict"
                ) =>
            {
                report.issues.push(InstallationIssue {
                    directory: spelling(&path),
                    product_code: observed,
                    reason: issue_reason(&error),
                });
            }
            Err((error, _)) => return Err(error),
        }
    }
    root.verify_controlled(&mut Files(&mut *control))?;
    control.check()?;
    Ok(report)
}
pub fn select_with(
    directory: &Path,
    product: &str,
    control: &mut impl MetadataControl,
) -> Result<ClientInstallation> {
    let profile = if product.is_empty() {
        None
    } else {
        Some(client::product(product).ok_or_else(|| {
            ToolkitError::new(
                "selection.unsupported_data_product",
                "selection.unsupported_data_product",
                2,
            )
        })?)
    };
    let report = discover_with(directory, control)?;
    let mut kind = if !report.candidates.is_empty() && !product.is_empty() {
        "installation_product_mismatch"
    } else {
        "installation_missing"
    };
    let mut matches = report
        .candidates
        .iter()
        .filter(|client| client::client_matches_product(client, product));
    let first = matches.next();
    let ambiguous = matches.next().is_some();
    let conflict = report.issues.iter().any(|issue| {
        product.is_empty()
            || issue.product_code.is_empty()
            || profile.is_some_and(|profile| {
                issue.product_code == profile.catalog_product
                    || (product == "forever"
                        && matches!(issue.product_code.as_str(), "wow_forever" | "wow_cn_beta"))
            })
    });
    control.check()?;
    if ambiguous {
        kind = "installation_ambiguous";
    } else if conflict {
        kind = "installation_metadata_conflict";
    } else if let Some(client) = first {
        return Ok(client.clone());
    }
    Err(selection_error(kind, product, &report))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::TrackedCancellation;
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, Instant},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "lychee-installation-selection-{}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
        fn client(&self, name: &str, flavor: &str, version: &str) -> PathBuf {
            let path = self.0.join(name);
            fs::create_dir_all(&path).unwrap();
            if !flavor.is_empty() {
                fs::write(path.join(".flavor.info"), flavor).unwrap();
            }
            if !version.is_empty() {
                fs::write(path.join("version.txt"), version).unwrap();
            }
            path
        }
        fn catalog(&self, rows: &[(&str, &str)]) {
            let mut text = String::from(
                "Product!STRING:0|Version!STRING:0|Build Key!HEX:16|CDN Key!HEX:16|Active!DEC:1\n",
            );
            for (product, build) in rows {
                text.push_str(&format!("{product}|{build}|0123456789abcdef0123456789abcdef|fedcba9876543210fedcba9876543210|1\n"));
            }
            fs::write(self.0.join(".build.info"), text).unwrap();
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[derive(Default)]
    struct Meter {
        calls: u64,
        reads: u64,
        stop_at: Option<u64>,
        checks: u64,
    }
    impl MetadataControl for Meter {
        fn charge(&mut self, cost: MetadataCost) -> Result<()> {
            self.calls += cost.io_operations;
            self.reads += cost.read_calls;
            self.check()
        }
        fn check(&mut self) -> Result<()> {
            self.checks += 1;
            if self.stop_at.is_some_and(|stop| self.checks >= stop) {
                return Err(ToolkitError::new(
                    "command.cancelled",
                    "fixture cancellation",
                    7,
                ));
            }
            Ok(())
        }
    }
    fn select(path: &Path, product: &str) -> Result<ClientInstallation> {
        select_with(path, product, &mut Meter::default())
    }
    fn discover(path: &Path) -> Result<InstallationDiscovery> {
        discover_with(path, &mut Meter::default())
    }
    fn error_code(path: &Path, product: &str) -> String {
        select(path, product).unwrap_err().code
    }

    #[test]
    fn root_and_exact_client_resolve_same_titan_and_public_shape() {
        let fixture = Fixture::new();
        let path = fixture.client("_classic_titan_", "wow_classic_titan", "3.80.2.69874");
        fixture.catalog(&[("wow_classic_titan", "3.80.2.69874")]);
        let exact = select(&path, "").unwrap();
        assert_eq!(select(&fixture.0, "").unwrap(), exact);
        let public = serde_json::to_value(Client::from(&exact)).unwrap();
        // Go selection.ClientInstallation omits catalogProduct for ordinary slots.
        let keys = public
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            keys,
            [
                "directory",
                "product",
                "productCode",
                "fullBuild",
                "interface",
                "toc",
                "identitySource"
            ]
            .into_iter()
            .collect()
        );
        for name in [
            "interfaceKnown",
            "displayName",
            "channel",
            "metadataBranch",
            "catalogProduct",
        ] {
            assert!(public.get(name).is_none(), "{name}");
        }
        assert_eq!(
            serde_json::to_value(discover(&fixture.0).unwrap()).unwrap()["candidates"][0],
            public
        );
        assert_eq!(
            error_code(&path, "retail"),
            "selection.installation_product_mismatch"
        );
        let error = select(&path, "retail").unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.details["__commandContext"]["requestedProduct"],
            "retail"
        );
    }
    #[test]
    fn does_not_invent_clients_or_override_explicit_metadata() {
        for scenario in [
            "multiple",
            "catalog-only",
            "missing-flavor",
            "missing-version",
            "conflict",
            "empty-folder",
            "foreign-unsupported",
            "unknown-sibling",
        ] {
            let fixture = Fixture::new();
            fixture.catalog(&[
                ("wow_classic_titan", "3.80.2.69874"),
                ("wow", "12.1.0.69875"),
            ]);
            if scenario != "catalog-only" {
                fixture.client(
                    "_classic_titan_",
                    if matches!(scenario, "missing-flavor" | "empty-folder") {
                        ""
                    } else {
                        "wow_classic_titan"
                    },
                    if matches!(scenario, "missing-version" | "empty-folder") {
                        ""
                    } else if scenario == "conflict" {
                        "3.80.2.69873"
                    } else {
                        "3.80.2.69874"
                    },
                );
            }
            if scenario == "multiple" {
                fixture.client("_retail_", "wow", "12.1.0.69875");
                assert_eq!(
                    error_code(&fixture.0, ""),
                    "selection.installation_ambiguous"
                );
            }
            if matches!(scenario, "foreign-unsupported" | "unknown-sibling") {
                fixture.client(
                    "_beta_",
                    if scenario == "foreign-unsupported" {
                        "wow_beta"
                    } else {
                        "invalid"
                    },
                    "",
                );
                let report = discover(&fixture.0).unwrap();
                assert_eq!(report.issues.len(), 1);
                assert_eq!(
                    report.issues[0].product_code,
                    if scenario == "foreign-unsupported" {
                        "wow_beta"
                    } else {
                        ""
                    }
                );
            }
            let result = select(&fixture.0, "titan");
            match scenario {
                "catalog-only" | "empty-folder" => assert_eq!(
                    result.unwrap_err().code,
                    "selection.installation_missing",
                    "{scenario}"
                ),
                "conflict" | "unknown-sibling" => assert_eq!(
                    result.unwrap_err().code,
                    "selection.installation_metadata_conflict",
                    "{scenario}"
                ),
                _ => assert_eq!(result.unwrap().product, "titan", "{scenario}"),
            }
        }
    }
    #[test]
    fn ambiguity_precedes_conflict_and_invalid_product_precedes_io() {
        let fixture = Fixture::new();
        fixture.client("a", "wow", "12.1.0.1");
        fixture.client("b", "wow", "12.1.0.1");
        fixture.client("unknown", "invalid", "");
        let error = select(&fixture.0, "retail").unwrap_err();
        assert_eq!(error.code, "selection.installation_ambiguous");
        assert_eq!(
            error.details["__commandContext"]["installationSelection"]["issues"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        let mut meter = Meter::default();
        assert_eq!(
            select_with(&fixture.0, "unsupported", &mut meter)
                .unwrap_err()
                .code,
            "selection.unsupported_data_product"
        );
        assert_eq!(meter.calls, 0);
    }

    #[test]
    fn regional_clients_stay_separate_and_related_failures_block_family_selection() {
        let fixture = Fixture::new();
        let cn = fixture.client("_cn_beta_", "wow_cn_beta", "");
        fixture.client("_classic_beta_", "wow_classic_beta", "");
        fixture.catalog(&[
            ("wow_cn_beta", "1.60.1.70245"),
            ("wow_classic_beta", "1.60.1.70124"),
        ]);
        assert_eq!(
            error_code(&fixture.0, "forever"),
            "selection.installation_ambiguous"
        );
        assert_eq!(
            select(&fixture.0, "forever-cn-beta")
                .unwrap()
                .catalog_product,
            "wow_cn_beta"
        );
        assert_eq!(
            select(&fixture.0, "forever-asia-beta").unwrap().full_build,
            "1.60.1.70124"
        );
        fs::write(cn.join("version.txt"), "5.5.4.69934").unwrap();
        assert_eq!(
            error_code(&fixture.0, "forever"),
            "selection.installation_metadata_conflict"
        );
        let report = discover(&fixture.0).unwrap();
        assert_eq!(report.issues[0].product_code, "wow_cn_beta");
        assert!(select(&fixture.0, "forever-asia-beta").is_ok());
        fs::write(cn.join("version.txt"), "1.60.1.70000").unwrap();
        assert_eq!(
            error_code(&cn, ""),
            "selection.installation_metadata_conflict"
        );
        assert_eq!(discover(&cn).unwrap().issues[0].product_code, "wow_forever");
    }
    #[test]
    fn marker_and_known_executable_discovery_is_one_level_and_sorted() {
        let fixture = Fixture::new();
        fixture.client("z-renamed", "wow_classic_titan", "3.80.2.69874");
        fixture.client("a-renamed", "wow", "12.1.0.69875");
        fixture.client("nested/client", "wow", "12.1.0.69875");
        let path = fixture.client("_classic_era_", "", "");
        fs::write(path.join("WowClassic.exe"), []).unwrap();
        fixture.catalog(&[("wow_classic_era", "1.15.9.1")]);
        let report = discover(&fixture.0).unwrap();
        assert_eq!(report.candidates.len(), 3);
        let names: Vec<_> = report
            .candidates
            .iter()
            .map(|client| {
                Path::new(&client.directory)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["_classic_era_", "a-renamed", "z-renamed"]);
        assert_eq!(report.candidates[0].product, "classic-era");
    }
    #[test]
    fn discovery_entry_bound_and_cancellation_are_not_swallowed() {
        let fixture = Fixture::new();
        for n in 0..256 {
            fs::write(fixture.0.join(format!("file-{n}")), []).unwrap();
        }
        assert!(discover(&fixture.0).unwrap().candidates.is_empty());
        let malformed = fixture.client("entry-257", "wow", "12.1.0.1");
        fs::remove_file(malformed.join("version.txt")).unwrap();
        fs::create_dir(malformed.join("version.txt")).unwrap();
        // The root bound is authoritative before any client metadata read.
        let mut bound_meter = Meter::default();
        let error = discover_with(&fixture.0, &mut bound_meter).unwrap_err();
        assert_eq!(bound_meter.reads, 0);
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("records.metadata_limit", 3)
        );
        let mut meter = Meter {
            stop_at: Some(1),
            ..Default::default()
        };
        assert_eq!(
            discover_with(&fixture.0, &mut meter).unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(meter.calls, 0);
        let mut meter = Meter {
            stop_at: Some(40),
            ..Default::default()
        };
        assert_eq!(
            discover_with(&fixture.0, &mut meter).unwrap_err().code,
            "command.cancelled"
        );
        assert!(meter.calls > 0);
        let cancel = TrackedCancellation::new();
        cancel.cancel();
        let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), cancel);
        assert_eq!(
            select_with(&fixture.0, "", &mut InvocationMetadata(&control))
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        let missing = select(&fixture.0.join("missing"), "titan").unwrap_err();
        assert_eq!(missing.code, "selection.installation_missing");
        // Discover's missing-scope error predates selection, so the fixed
        // reference leaves the requested product empty but still emits the
        // context field.
        assert_eq!(missing.details["__commandContext"]["requestedProduct"], "");
        assert!(
            missing.details["__commandContext"]
                .get("installationSelection")
                .is_some()
        );
    }
    #[test]
    fn root_discovery_does_not_follow_foreign_directory_symlink() {
        let fixture = Fixture::new();
        let foreign = Fixture::new();
        let client = foreign.client("client", "wow_classic_titan", "3.80.2.69874");
        let expected = select(&client, "titan").unwrap();
        let members = [".flavor.info", "version.txt"]
            .map(|name| (name, fs::read(client.join(name)).unwrap()));
        let link = fixture.0.join("_classic_titan_");
        // A junction exercises the actual Windows directory-reparse boundary
        // without requiring file/directory-symlink privilege or skipping it.
        let junction = std::process::Command::new("node")
            .args([
                "-e",
                "require('node:fs').symlinkSync(process.argv[1], process.argv[2], 'junction');",
            ])
            .arg(&client)
            .arg(&link)
            .output()
            .expect("create Windows installation junction fixture with Node");
        assert!(
            junction.status.success(),
            "junction fixture failed: {}",
            String::from_utf8_lossy(&junction.stderr)
        );
        let attributes = fs::symlink_metadata(&link).unwrap().file_attributes();
        assert_ne!(attributes & 0x400, 0, "fixture must be a reparse point");
        assert_ne!(attributes & 0x10, 0, "fixture must redirect a directory");
        assert!(fs::metadata(&link).unwrap().is_dir());
        assert_eq!(
            fs::canonicalize(&link).unwrap(),
            fs::canonicalize(&client).unwrap()
        );
        for (name, bytes) in &members {
            assert_eq!(fs::read(link.join(name)).unwrap(), *bytes);
        }
        let report = discover(&fixture.0).unwrap();
        assert!(report.candidates.is_empty());
        assert!(report.issues.is_empty());
        // An explicitly selected alias can resolve to the client, unlike a root traversal.
        let selected = select(&link, "titan").unwrap();
        assert_eq!(selected.product, "titan");
        assert_eq!(selected, expected);
        for (name, bytes) in &members {
            assert_eq!(fs::read(client.join(name)).unwrap(), *bytes);
        }
        fs::remove_dir(&link).unwrap(); // Remove only the junction.
        assert_eq!(fs::read_dir(&foreign.0).unwrap().count(), 1);
        assert_eq!(fs::read_dir(&client).unwrap().count(), members.len());
        for (name, bytes) in &members {
            assert_eq!(fs::read(client.join(name)).unwrap(), *bytes);
        }
    }
    #[test]
    fn issue_reasons_match_reference_identity_errors_and_precedence() {
        struct Case {
            name: &'static str,
            folder: &'static str,
            flavor_file: &'static str,
            flavor: &'static str,
            version: &'static str,
            rows: &'static [(&'static str, &'static str)],
            observed: &'static str,
            reason: &'static str,
        }
        let cases = [
            Case {
                name: "no flavor match",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "invalid",
                version: "",
                rows: &[],
                observed: "",
                reason: "selection.client_identity: invalid .flavor.info",
            },
            Case {
                name: "multiple flavor matches",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow wow_classic",
                version: "",
                rows: &[],
                observed: "",
                reason: "selection.client_identity: invalid .flavor.info",
            },
            Case {
                name: "alternate no match",
                folder: "custom",
                flavor_file: "flavor.info",
                flavor: "invalid",
                version: "",
                rows: &[],
                observed: "",
                reason: "selection.client_identity: invalid flavor.info",
            },
            Case {
                name: "alternate multiple matches",
                folder: "custom",
                flavor_file: "flavor.info",
                flavor: "wow wow",
                version: "",
                rows: &[],
                observed: "",
                reason: "selection.client_identity: invalid flavor.info",
            },
            Case {
                name: "invalid version",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow",
                version: "bad",
                rows: &[],
                observed: "wow",
                reason: "selection.client_identity: invalid version.txt",
            },
            Case {
                name: "unsupported Asia slot",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow_classic_beta",
                version: "",
                rows: &[],
                observed: "wow_classic_beta",
                reason: r#"selection.client_identity: unsupported build for slot "wow_classic_beta""#,
            },
            Case {
                name: "unsupported CN slot",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow_cn_beta",
                version: "",
                rows: &[],
                observed: "wow_cn_beta",
                reason: r#"selection.client_identity: unsupported build for slot "wow_cn_beta""#,
            },
            Case {
                name: "unsupported explicit product",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow_beta",
                version: "",
                rows: &[],
                observed: "wow_beta",
                reason: r#"selection.client_identity: unsupported product "wow_beta""#,
            },
            Case {
                name: "unsupported empty product",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "",
                version: "",
                rows: &[],
                observed: "",
                reason: r#"selection.client_identity: unsupported product """#,
            },
            Case {
                name: "missing build",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow",
                version: "",
                rows: &[],
                observed: "wow",
                reason: "selection.client_identity: missing or unsupported product/build pair",
            },
            Case {
                name: "unknown ordinary build",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "",
                version: "9.9.9.1",
                rows: &[],
                observed: "",
                reason: "selection.client_identity: unsupported build",
            },
            Case {
                name: "unknown CN folder build",
                folder: "_cn_beta_",
                flavor_file: ".flavor.info",
                flavor: "",
                version: "9.9.9.1",
                rows: &[],
                observed: "",
                reason: "selection.client_identity: unsupported test track",
            },
            Case {
                name: "unknown Asia folder build",
                folder: "_classic_beta_",
                flavor_file: ".flavor.info",
                flavor: "",
                version: "9.9.9.1",
                rows: &[],
                observed: "",
                reason: "selection.client_identity: unsupported test track",
            },
            Case {
                name: "unknown CN slot build",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow_cn_beta",
                version: "9.9.9.1",
                rows: &[],
                observed: "wow_cn_beta",
                reason: "selection.client_identity: unsupported test track",
            },
            Case {
                name: "unknown Asia slot build",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow_classic_beta",
                version: "9.9.9.1",
                rows: &[],
                observed: "wow_classic_beta",
                reason: "selection.client_identity: unsupported build",
            },
            Case {
                name: "regular catalog ambiguous",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow",
                version: "",
                rows: &[("wow", "12.1.0.1"), ("wow", "12.1.0.2")],
                observed: "wow",
                reason: "selection.client_identity: ambiguous active builds",
            },
            Case {
                name: "slot catalog ambiguous",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow_classic_beta",
                version: "",
                rows: &[
                    ("wow_classic_beta", "1.60.1.1"),
                    ("wow_classic_beta", "5.5.4.1"),
                ],
                observed: "wow_classic_beta",
                reason: "selection.client_identity: ambiguous active builds",
            },
            Case {
                name: "late test track",
                folder: "_classic_beta_",
                flavor_file: ".flavor.info",
                flavor: "wow_classic",
                version: "5.5.4.1",
                rows: &[],
                observed: "wow_classic",
                reason: "selection.client_identity: unsupported test track",
            },
            Case {
                name: "catalog conflict",
                folder: "custom",
                flavor_file: ".flavor.info",
                flavor: "wow_cn_beta",
                version: "1.60.1.1",
                rows: &[("wow_cn_beta", "1.60.1.2")],
                observed: "wow_forever",
                reason: "selection.installation_metadata_conflict",
            },
        ];
        for case in cases {
            let fixture = Fixture::new();
            let path = fixture.client(case.folder, "", case.version);
            fs::write(path.join(case.flavor_file), case.flavor).unwrap();
            if !case.rows.is_empty() {
                fixture.catalog(case.rows);
            }
            let report = discover(&path).unwrap();
            assert!(report.candidates.is_empty(), "{}", case.name);
            assert_eq!(report.issues.len(), 1, "{}", case.name);
            assert_eq!(report.issues[0].reason, case.reason, "{}", case.name);
            assert_eq!(
                report.issues[0].product_code, case.observed,
                "{}",
                case.name
            );
        }
    }

    #[test]
    fn client_text_issues_publish_retained_sentinel_for_kind_size_and_utf8() {
        for name in [".flavor.info", "flavor.info", "version.txt"] {
            for invalid in ["directory", "oversized", "invalid-utf8"] {
                let fixture = Fixture::new();
                let path = fixture.client("custom", "", "");
                if name == "version.txt" {
                    fs::write(path.join(".flavor.info"), "wow").unwrap();
                }
                match invalid {
                    "directory" => fs::create_dir(path.join(name)).unwrap(),
                    "oversized" => fs::write(path.join(name), vec![b'x'; 4097]).unwrap(),
                    _ => fs::write(path.join(name), [0xff]).unwrap(),
                }
                let report = discover(&path).unwrap();
                assert!(report.candidates.is_empty());
                assert_eq!(report.issues.len(), 1);
                assert_eq!(
                    report.issues[0].reason, "selection.client_identity",
                    "{name}/{invalid}"
                );
                assert_eq!(
                    report.issues[0].product_code,
                    if name == "version.txt" { "wow" } else { "" }
                );
            }
        }
    }

    #[test]
    fn observed_product_survives_the_exact_reference_failure_stage() {
        for (name, flavor, version, expected) in [
            ("custom", "wow_beta", "", "wow_beta"),
            ("custom", "invalid", "", ""),
            ("custom", "wow_cn_beta", "bad-version", "wow_cn_beta"),
            ("_retail_", "", "", "wow"),
            ("_cn_beta_", "", "5.5.4.1", ""),
            ("custom", "wow", "5.5.4.1", "wow"),
        ] {
            let fixture = Fixture::new();
            let path = fixture.client(name, flavor, version);
            let (error, observed) =
                client::inspect_observed_with(&path, &mut Meter::default()).unwrap_err();
            assert_eq!(
                error.code, "selection.client_identity",
                "{name}/{flavor}/{version}"
            );
            assert_eq!(observed, expected, "{name}/{flavor}/{version}");
            assert!(
                error.details.is_null(),
                "partial observations stay internal"
            );
        }
    }
}
