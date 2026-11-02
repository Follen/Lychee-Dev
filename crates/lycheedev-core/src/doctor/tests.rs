use super::*;
use crate::control::{StopCause, TrackedCancellation};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Fixture {
    root: PathBuf,
    inv: RequestContext,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-doctor-r35-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        let inv = RequestContext {
            cwd: root.clone(),
            home,
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(90)),
        };
        Self { root, inv }
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.root).unwrap()
    }
}
fn item<'a>(result: &'a Value, id: &str) -> &'a Value {
    result["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["id"] == id)
        .unwrap()
}
#[test]
fn fresh_doctor_is_readonly_and_assembles_fixed_order_before_providers() {
    let f = Fixture::new();
    let path = f.inv.home.join("state/toolkit.sqlite");
    let report = inspect_with_provider(&f.inv, &[], || {
        assert!(!path.exists());
        assert!(!f.inv.home.join("tmp/.lycheedev-write-probe").exists());
        Ok(vec![check("fixture.provider", true, "ok", "", "last", "")])
    })
    .unwrap();
    let ids: Vec<_> = report.data["checks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        ids,
        vec![
            "vault.workspace",
            "vault.metadata",
            "vault.writable",
            "vault.lock_hygiene",
            "vault.tmp_hygiene",
            "vault.cache_integrity",
            "vault.archive_state",
            "selection.catalog",
            "selection.named_targets",
            "fixture.provider"
        ]
    );
    assert_eq!(item(&report.data, "vault.metadata")["status"], "warn");
    assert_eq!(
        item(&report.data, "selection.named_targets")["detail"],
        "no named target configurations"
    );
    assert!(!path.exists());
    f.cleanup();
}
#[test]
fn invalid_workspace_keeps_catalog_targets_archive_and_provider_diagnostics() {
    let f = Fixture::new();
    fs::write(f.inv.home.join("workspace.json"), b"{}").unwrap();
    let mut calls = 0;
    let error = inspect_with_provider(&f.inv, &[], || {
        calls += 1;
        Ok(vec![check(
            "fixture.provider",
            false,
            "warn",
            "",
            "independent",
            "",
        )])
    })
    .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.code, "doctor.unhealthy");
    let report = &error.details["result"];
    assert_eq!(
        item(report, "vault.workspace")["code"],
        "vault.unsupported_workspace"
    );
    assert_eq!(
        item(report, "selection.named_targets")["code"],
        "selection.target_format"
    );
    assert_eq!(item(report, "selection.catalog")["ok"], true);
    assert_eq!(item(report, "fixture.provider")["status"], "warn");
    assert!(
        !report["checks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["id"] == "vault.writable")
    );
    f.cleanup();
}
#[test]
fn probe_collision_never_deletes_foreign_file_and_hygiene_counts_files_only() {
    let f = Fixture::new();
    let probe = f.inv.home.join("tmp/.lycheedev-write-probe");
    fs::write(&probe, b"foreign").unwrap();
    fs::create_dir_all(f.inv.home.join("tmp/empty/nested")).unwrap();
    fs::create_dir_all(f.inv.home.join("locks/empty/nested")).unwrap();
    let error = inspect(&f.inv).unwrap_err();
    let result = &error.details["result"];
    assert_eq!(
        item(result, "vault.writable")["code"],
        "vault.workspace_readonly"
    );
    assert!(
        item(result, "vault.tmp_hygiene")["detail"]
            .as_str()
            .unwrap()
            .starts_with("1 leftover temporary file(s); oldest modified ")
    );
    assert!(
        item(result, "vault.lock_hygiene")["detail"]
            .as_str()
            .unwrap()
            .starts_with("0 lock files present;")
    );
    assert_eq!(fs::read(probe).unwrap(), b"foreign");
    f.cleanup();
}
#[test]
fn broken_metadata_does_not_hide_named_target_format_or_cache_corruption() {
    let f = Fixture::new();
    fs::write(f.inv.home.join("state/toolkit.sqlite"), b"broken sqlite").unwrap();
    let digest = "a".repeat(64);
    let path = f
        .inv
        .home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"wrong").unwrap();
    let error = inspect(&f.inv).unwrap_err();
    let report = &error.details["result"];
    assert_eq!(item(report, "vault.metadata")["status"], "error");
    assert_eq!(
        item(report, "selection.named_targets")["code"],
        "selection.target_format"
    );
    assert_eq!(
        item(report, "vault.cache_integrity")["detail"],
        "objects=1 bytes=5 missing=0 corrupt=1"
    );
    assert_eq!(fs::read(path).unwrap(), b"wrong");
    f.cleanup();
}
#[test]
fn named_target_check_uses_all_configuration_pages_without_history() {
    let f = Fixture::new();
    Metadata::open(&f.inv.home, true).unwrap().close().unwrap();
    let mut db = rusqlite::Connection::open(f.inv.home.join("state/toolkit.sqlite")).unwrap();
    let tx = db.transaction().unwrap();
    for i in 0..1002 {
        let name = format!("remote{i:04}");
        let value = json!({"schema":"lycheedev.target.v1","name":name,"product":"retail","region":"us","locale":"enUS","source":"remote"});
        tx.execute(
            "INSERT INTO documents(key,generation,value) VALUES(?1,1,?2)",
            rusqlite::params![
                format!("target/{i:04}"),
                serde_json::to_vec(&value).unwrap()
            ],
        )
        .unwrap();
    }
    tx.execute(
        "INSERT INTO documents(key,generation,value) VALUES('target-ref/broken',1,?)",
        [b"not JSON".as_slice()],
    )
    .unwrap();
    tx.commit().unwrap();
    db.close().unwrap();
    let report = inspect(&f.inv).unwrap();
    let named = item(&report.data, "selection.named_targets");
    assert_eq!(named["data"]["remote"].as_array().unwrap().len(), 1002);
    assert!(named["detail"].as_str().unwrap().starts_with("1002 named target(s) validate and resolve locally; remote currency unknown offline for remote0000"));
    f.cleanup();
}
#[test]
fn legacy_candidates_only_sniff_sorted_names_and_preserve_sentinel_bytes() {
    let f = Fixture::new();
    let user = f.root.join("user");
    fs::create_dir_all(user.join(".wowdoc")).unwrap();
    fs::write(user.join(".wowdoc/wowdoc.json"), b"not JSON; never opened").unwrap();
    fs::write(user.join(".wowdoc/bin"), b"sentinel").unwrap();
    fs::write(user.join(".wowdata"), b"not a directory").unwrap();
    let legacy = legacy_candidates(Some(&user), None);
    let report = inspect_with_provider(&f.inv, &legacy, || Ok(vec![])).unwrap();
    let item = item(&report.data, "vault.legacy_roots");
    assert_eq!(
        item["data"]["wowdoc"]["detail"],
        "legacy layout detected (markers: bin, wowdoc.json); isolated, never read"
    );
    assert_eq!(
        item["data"]["wowdata"]["detail"],
        "present but not a directory; left untouched"
    );
    assert_eq!(item["data"]["lycheedev-legacy"]["detected"], false);
    assert_eq!(
        fs::read(user.join(".wowdoc/wowdoc.json")).unwrap(),
        b"not JSON; never opened"
    );
    f.cleanup();
}
#[test]
fn provider_cancellation_retains_completed_checks_and_ordered_scope() {
    let mut f = Fixture::new();
    let stop = TrackedCancellation::new();
    f.inv.control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(60), stop.clone());
    let error = inspect_with_provider(&f.inv, &[], || {
        stop.cancel();
        Err(f.inv.control.check().unwrap_err())
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["result"]["checks"].as_array().unwrap().len(),
        9
    );
    assert_eq!(
        f.inv.control.stop_cause().unwrap(),
        Some(StopCause::Cancelled)
    );
    f.cleanup();
}
#[test]
fn workspace_missing_is_typed_observation_not_a_second_path_guess() {
    let f = Fixture::new();
    fs::remove_file(f.inv.home.join("workspace.json")).unwrap();
    let error = inspect(&f.inv).unwrap_err();
    assert_eq!(
        item(&error.details["result"], "vault.workspace")["code"],
        "vault.legacy_detected"
    );
    let mut inv = f.inv.clone();
    inv.home = f.root.join("absent");
    let error = inspect(&inv).unwrap_err();
    assert_eq!(
        item(&error.details["result"], "vault.workspace")["code"],
        "vault.workspace_missing"
    );
    f.cleanup();
}

#[test]
fn stop_after_successful_probe_is_a_typed_terminal_result() {
    let stop = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    let completed: Result<u8> = Ok(7);
    stop.cancel();
    assert_eq!(
        diagnostic(completed, &control).unwrap_err().code,
        "command.cancelled"
    );
    let error = diagnostic::<u8>(
        Err(ToolkitError::new("external.io", "probe read failed", 5)),
        &control,
    )
    .unwrap_err();
    assert_eq!(error.code, "external.io");
    assert!(
        serde_json::to_string(&error.details)
            .unwrap()
            .contains("command.cancelled")
    );
}
