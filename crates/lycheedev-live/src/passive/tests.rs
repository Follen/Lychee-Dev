use super::*;
use serde_json::json;
use std::{
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-passive-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn client(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(&path).unwrap();
        fs::write(path.join(".flavor.info"), "wow").unwrap();
        fs::write(path.join("version.txt"), "12.1.0.69933").unwrap();
        // No valid executable content is needed by passive discovery.
        fs::write(path.join("Wow.exe"), []).unwrap();
        path
    }
    fn context(&self) -> LiveContext {
        LiveContext { home: self.0.join("absent-home"), control: Default::default() }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn window(path: &Path, pid: u32) -> Window {
    Window {
        identity: lycheedev_windows::WindowIdentity {
            process: lycheedev_windows::ProcessIdentity {
                pid,
                created: u64::MAX - u64::from(pid),
            },
            hwnd: pid as usize + 100,
        },
        executable: path.join("Wow.exe").to_string_lossy().into_owned(),
        class: "GxWindowClass".into(),
        title: "Same title is not a character".into(),
        visible: true,
    }
}
struct Fake {
    windows: Vec<Window>,
    calls: Cell<usize>,
}
impl Fake {
    fn new(windows: Vec<Window>) -> Self {
        Self {
            windows,
            calls: Cell::new(0),
        }
    }
}
impl Source for Fake {
    fn windows(&self, control: &OperationControl) -> Result<Vec<Window>> {
        control.check()?;
        self.calls.set(self.calls.get() + 1);
        Ok(self.windows.clone())
    }
}
#[test]
fn passive_discovery_keeps_twins_distinct_without_native_capabilities() {
    let fixture = Fixture::new();
    let client = fixture.client("client");
    let mut first = window(&client, 11);
    first.identity.hwnd = 18_446_744_073_709_551_000usize;
    let source = Fake::new(vec![first, window(&client, 12)]);
    let ctx = fixture.context();
    let default = serde_json::to_value(discover(&ctx, &InstancesRequest::default(), &source).unwrap()).unwrap();
    assert_eq!(source.calls.get(), 1);
    assert_eq!(default.as_object().unwrap().len(), 2);
    let candidates = default["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0]["window"]["handle"], "18446744073709551000");
    assert_eq!(
        candidates[1]["window"]["processStartedAt"],
        (u64::MAX - 12).to_string()
    );
    for candidate in candidates {
        assert_eq!(candidate["state"], "unobserved");
        assert_eq!(candidate["inputReady"], false);
        assert_eq!(candidate["notReadyReason"], "passive_discovery");
        assert!(candidate.get("character").is_none());
        assert!(candidate.get("runtimeRelease").is_none());
        assert!(candidate.get("foreignOwner").is_none());
    }
    assert_eq!(default["installations"].as_array().unwrap().len(), 1);
    assert_eq!(default["installations"][0]["state"], "running");
    assert!(!ctx.home.exists());
    assert!(!client.join("Interface").exists());
}

#[test]
fn scans_only_known_roots_and_does_not_filter_other_running_installations() {
    let fixture = Fixture::new();
    fixture.client("game/_retail_");
    fixture.client("game/_ptr_");
    fixture.client("game/unrelated");
    let running = fixture.client("elsewhere");
    let source = Fake::new(vec![window(&running, 20)]);
    let ctx = fixture.context();
    let mut request = InstancesRequest { installation: Some(fixture.0.join("game")) };
    let report = discover(&ctx, &request, &source).unwrap();
    assert_eq!(report.candidates.unwrap().len(), 1);
    assert_eq!(report.installations.len(), 2);
    assert_eq!(
        report
            .installations
            .iter()
            .filter(|item| item.state == "running")
            .count(),
        1
    );
    assert!(
        report
            .installations
            .iter()
            .any(|item| item.client.directory.ends_with("_retail_"))
    );
    request.installation = Some(running.clone());
    assert_eq!(discover(&ctx, &request, &source).unwrap().installations.len(), 1);
}

#[test]
fn empty_output_and_one_broken_window_keep_public_inventory_shape() {
    let fixture = Fixture::new();
    let empty =
        serde_json::to_value(discover(&fixture.context(), &InstancesRequest::default(), &Fake::new(vec![])).unwrap()).unwrap();
    assert_eq!(empty, json!({"installations":[],"candidates":null}));
    let good = fixture.client("good");
    let broken = fixture.client("broken");
    fs::write(broken.join("version.txt"), "broken").unwrap();
    let mut hidden = window(&good, 13);
    hidden.visible = false;
    let mut foreign = window(&good, 14);
    foreign.executable = good.join("Other.exe").to_string_lossy().into_owned();
    let mut beta = window(&good, 15);
    fs::write(good.join("WowB.exe"), []).unwrap();
    beta.executable = good.join("WowB.exe").to_string_lossy().into_owned();
    let source = Fake::new(vec![window(&broken, 11), hidden, foreign, beta]);
    let report = serde_json::to_value(discover(&fixture.context(), &InstancesRequest::default(), &source).unwrap()).unwrap();
    assert_eq!(report["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(report["candidates"][0]["state"], "identity_unreadable");
    assert_eq!(
        report["candidates"][0]["client"],
        json!({"directory":"","product":"","productCode":"","fullBuild":"","interface":0,"toc":"","identitySource":""})
    );
    assert_eq!(report["candidates"][1]["state"], "unobserved");
    assert_eq!(report["installations"].as_array().unwrap().len(), 1);
}

fn marker(client: &Path, window: &Window) -> PathBuf {
    let resource = format!(
        "window/{}/{}/{}",
        window.identity.process.pid, window.identity.process.created, window.identity.hwnd
    );
    client
        .join("Interface/AddOns/.lycheedev-window-owners")
        .join(format!("{}.json", storage::sha256(resource.as_bytes())))
}
#[test]
fn current_connection_owner_markers_are_read_only_and_unreadable_ownership_stays_occupied() {
    let fixture = Fixture::new();
    let client = fixture.client("client");
    let window = window(&client, 17);
    let path = marker(&client, &window);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let owner = WindowOwner {
        schema: "lycheedev.window-owner.v1".into(),
        workspace_id: "a".repeat(32),
        resource: format!("window/17/{}/117", u64::MAX - 17),
        operation_id: format!("CON-{}", "b".repeat(32)),
        intent_sha256: String::new(),
    };
    let original = storage::identity_json(&owner).unwrap();
    fs::write(&path, &original).unwrap();
    let ctx = fixture.context();
    let source = Fake::new(vec![window]);
    let candidate = discover(&ctx, &InstancesRequest::default(), &source)
        .unwrap()
        .candidates
        .unwrap()
        .remove(0);
    assert_eq!(candidate.state, "occupied");
    assert!(candidate.foreign_owner);
    assert_eq!(candidate.busy_operation_id, owner.operation_id);
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!ctx.home.exists());
    fs::create_dir(&ctx.home).unwrap();
    fs::write(ctx.home.join("workspace.json"), serde_json::to_vec(&json!({"schema":"lycheedev.workspace.v1","workspaceId":owner.workspace_id,"createdAt":"2026-10-13T00:00:00Z"})).unwrap()).unwrap();
    assert!(!discover(&ctx, &InstancesRequest::default(), &source).unwrap().candidates.unwrap()[0].foreign_owner);
    for bad in [
        b"invalid".to_vec(),
        vec![b' '; 4097],
        [original, b"\n".to_vec()].concat(),
    ] {
        fs::write(&path, &bad).unwrap();
        let candidate = discover(&ctx, &InstancesRequest::default(), &source)
            .unwrap()
            .candidates
            .unwrap()
            .remove(0);
        assert_eq!(candidate.state, "occupied");
        assert!(candidate.foreign_owner);
        assert!(!candidate.reason.is_empty());
        assert_eq!(fs::read(&path).unwrap(), bad);
    }
    assert!(!client.join("Interface/AddOns/.lycheedev-locks").exists());
}

#[test]
fn cancellation_and_deadline_are_not_swallowed_as_candidate_or_installation_errors() {
    let fixture = Fixture::new();
    let source = Fake::new(vec![]);
    let mut ctx = fixture.context();
    ctx.control.tracked_cancel_handle().unwrap().cancel();
    let error = discover(&ctx, &InstancesRequest::default(), &source).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert_eq!(source.calls.get(), 0);
    ctx.control = OperationControl::uncancelable(Instant::now());
    let error = discover(&ctx, &InstancesRequest::default(), &source).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("live.deadline_pending", 6)
    );
    struct CancelAfterList;
    impl Source for CancelAfterList {
        fn windows(&self, control: &OperationControl) -> Result<Vec<Window>> {
            control.tracked_cancel_handle().unwrap().cancel();
            Ok(vec![])
        }
    }
    ctx.control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
    assert_eq!(discover(&ctx, &InstancesRequest::default(), &CancelAfterList).unwrap_err().exit_code, 7);
    assert!(!ctx.home.exists());
}

#[test]
fn enumeration_failure_is_an_external_failure_not_an_empty_inventory() {
    struct Failed;
    impl Source for Failed {
        fn windows(&self, _: &OperationControl) -> Result<Vec<Window>> {
            Err(ToolkitError::new(
                "command.failed",
                "fixture enumeration failure",
                5,
            ))
        }
    }
    let fixture = Fixture::new();
    let error = discover(&fixture.context(), &InstancesRequest::default(), &Failed).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.failed", 5)
    );
    assert_eq!(error.message, "fixture enumeration failure");
    assert!(!fixture.context().home.exists());
}

#[test]
fn catalog_scan_keeps_approved_cn_product_distinct() {
    let fixture = Fixture::new();
    for (folder, product) in [
        ("_classic_beta_", "wow_classic_beta"),
        ("_cn_beta_", "wow_cn_beta"),
    ] {
        let path = fixture.0.join(folder);
        fs::create_dir(&path).unwrap();
        fs::write(path.join(".flavor.info"), product).unwrap();
    }
    fs::write(fixture.0.join(".build.info"), "Branch!STRING:0|Active!DEC:1|Version!STRING:0|Product!STRING:0|Build Key!HEX:16|CDN Key!HEX:16\nus|1|1.60.1.70338|wow_classic_beta|0123456789abcdef0123456789abcdef|fedcba9876543210fedcba9876543210\ncn|1|1.60.1.70338|wow_cn_beta|0123456789abcdef0123456789abcdef|fedcba9876543210fedcba9876543210\n").unwrap();
    let ctx = fixture.context();
    let request = InstancesRequest { installation: Some(fixture.0.clone()) };
    let report = discover(&ctx, &request, &Fake::new(vec![])).unwrap();
    assert_eq!(report.installations.len(), 2);
    assert!(
        report
            .installations
            .iter()
            .any(|item| item.client.product == "forever"
                && item.client.catalog_product == "wow_classic_beta")
    );
    assert!(report.installations.iter().any(
        |item| item.client.product == "forever" && item.client.catalog_product == "wow_cn_beta"
    ));
}

#[test]
fn passive_known_folders_include_era_and_anniversary_without_public_enrichment() {
    let fixture = Fixture::new();
    for (folder, flavor, version) in [
        ("_classic_era_", "wow_classic_era", "1.15.9.1"),
        ("_anniversary_", "wow_anniversary", "2.5.6.1"),
    ] {
        let path = fixture.client(folder);
        fs::write(path.join(".flavor.info"), flavor).unwrap();
        fs::write(path.join("version.txt"), version).unwrap();
    }
    // An ambiguous root is skipped, then Go's fixed folder list finds both.
    // Unrelated malformed markers remain an issue for root selection, not an
    // error that drops the independent valid fixed-folder observations.
    let broken = fixture.client("unrelated");
    fs::write(broken.join(".flavor.info"), "invalid").unwrap();
    let ctx = fixture.context();
    let request = InstancesRequest { installation: Some(fixture.0.clone()) };
    let report = serde_json::to_value(discover(&ctx, &request, &Fake::new(vec![])).unwrap()).unwrap();
    let installations = report["installations"].as_array().unwrap();
    assert_eq!(installations.len(), 2);
    assert!(
        installations
            .iter()
            .any(|entry| entry["client"]["product"] == "classic-era")
    );
    assert!(
        installations
            .iter()
            .any(|entry| entry["client"]["product"] == "anniversary")
    );
    for entry in installations {
        // Go selection.ClientInstallation omits catalogProduct for ordinary slots.
        let keys = entry["client"]
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
        for name in ["displayName", "channel", "metadataBranch", "interfaceKnown"] {
            assert!(entry["client"].get(name).is_none());
        }
    }
}

#[test]
fn passive_root_add_preserves_single_renamed_client_selection() {
    let fixture = Fixture::new();
    let client = fixture.client("renamed-client");
    let ctx = fixture.context();
    let request = InstancesRequest { installation: Some(fixture.0.clone()) };
    let report = discover(&ctx, &request, &Fake::new(vec![])).unwrap();
    assert_eq!(report.installations.len(), 1);
    assert_eq!(
        path_key(Path::new(&report.installations[0].client.directory)),
        path_key(&client)
    );
}

#[test]
fn passive_root_and_known_child_preserve_scope_deduplication() {
    let fixture = Fixture::new();
    let client = fixture.client("_retail_");
    let ctx = fixture.context();
    let request = InstancesRequest { installation: Some(fixture.0.clone()) };
    let report = discover(&ctx, &request, &Fake::new(vec![])).unwrap();
    assert_eq!(report.installations.len(), 2);
    for entry in report.installations {
        assert_eq!(
            path_key(Path::new(&entry.client.directory)),
            path_key(&client)
        );
        assert_eq!(entry.state, "installed");
    }
}
