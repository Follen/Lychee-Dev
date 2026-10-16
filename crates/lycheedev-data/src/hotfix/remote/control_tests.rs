use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(super) struct Fixture {
    pub inv: Invocation,
    pub root: PathBuf,
}
impl Fixture {
    pub(super) fn new(source: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-hotfix-controls-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace(':', "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let home = root.join("home");
        lycheedev_core::workspace::initialize(&home).unwrap();
        Self {
            inv: Invocation {
                command: "data hotfix".into(),
                positional: None,
                home,
                cwd: root.clone(),
                control: OperationControl::default(),
                flags: [
                    ("--source", source),
                    ("--product", "retail"),
                    ("--build", "12.0.7.68974"),
                    ("--region", "us"),
                    ("--locale", "enUS"),
                ]
                .into_iter()
                .map(|(k, v)| (k.into(), v.into()))
                .collect(),
            },
            root,
        }
    }
    pub(super) fn budget(&self) -> Budget {
        Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        )
    }
    pub(super) fn cache(&mut self) -> Vec<u8> {
        let mut raw = b"XFTH".to_vec();
        raw.extend(9u32.to_le_bytes());
        raw.extend(68974u32.to_le_bytes());
        raw.resize(44, 0);
        let path = self.root.join("cache.bin");
        std::fs::write(&path, &raw).unwrap();
        self.inv
            .flags
            .insert("--raidbots".into(), path.to_string_lossy().into());
        raw
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            std::fs::remove_dir_all(&self.root).unwrap()
        } else {
            eprintln!("retained {}", self.root.display())
        }
    }
}
#[test]
fn raidbots_default_chain_keeps_original_scope_and_publishes_exact_change_pin() {
    let mut f = Fixture::new("raidbots");
    let raw = f.cache();
    let budget = f.budget();
    let result = run(&f.inv, &budget).unwrap();
    assert!(result.capture.is_none());
    assert_eq!(result.change.sha256, storage::sha256(&raw));
    let pin =
        selection::load_pinned_controlled(&f.inv.home, &result.snapshot, &f.inv.control).unwrap();
    assert_eq!(pin.changes.as_ref(), Some(&result.change));
    assert!(f.inv.control.stop_cause().unwrap().is_none());
    f.inv
        .control
        .ordered_child(f.inv.control.deadline())
        .unwrap()
        .check()
        .unwrap();
}
#[test]
fn raidbots_independent_budget_stops_before_file_and_pin() {
    let mut f = Fixture::new("raidbots");
    f.cache();
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() - Duration::from_secs(1),
        None,
    );
    let error = run(&f.inv, &budget).err().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    f.inv.control.check().unwrap();
    match lycheedev_core::metadata::Metadata::open(&f.inv.home, false) {
        Ok(owner) => {
            assert!(owner.list::<Value>("pin/", 1).unwrap().is_empty());
            owner.close().unwrap();
        }
        Err(error) => assert_eq!(error.code, "vault.record_missing"),
    }
}
#[test]
fn raidbots_file_validator_stop_closes_before_return_and_original_wins() {
    let mut f = Fixture::new("raidbots");
    f.cache();
    let budget = f.budget();
    let path = PathBuf::from(f.inv.flag("--raidbots").unwrap());
    let cancel = f.inv.control.tracked_cancel_handle().unwrap();
    let error = super::super::local::read_source_file_controlled(
        &path,
        512 << 20,
        &budget,
        &f.inv.control,
        |_| {
            cancel.cancel();
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    budget.check().unwrap();
    std::fs::rename(&path, path.with_extension("closed")).unwrap();
}
#[test]
fn parent_pin_observes_independent_budget_without_a_child_deadline() {
    let f = Fixture::new("raidbots");
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() - Duration::from_secs(1),
        None,
    );
    let change = selection::ChangePin {
        provider: "raidbots".into(),
        sha256: "a".repeat(64),
        fetched_at: "2026-10-16T00:00:00Z".into(),
        ..Default::default()
    };
    let error = pin(&f.inv, change, &budget).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    f.inv.control.check().unwrap();
}
#[test]
fn snapshot_scanner_observes_original_callback_inside_version_eight_probes() {
    let mut f = Fixture::new("raidbots");
    let mut raw = f.cache();
    raw[4..8].copy_from_slice(&8u32.to_le_bytes());
    let mut row = vec![0u8; 28];
    row[..4].copy_from_slice(b"XFTH");
    row[12..16].copy_from_slice(&42u32.to_le_bytes());
    row[16..20].copy_from_slice(&7u32.to_le_bytes());
    row[24] = 1;
    raw.extend(row);
    let budget = f.budget();
    let cancel = f.inv.control.tracked_cancel_handle().unwrap();
    let mut checks = 0;
    let result = super::super::inspect_budgeted_with_check(
        &raw,
        super::super::CacheFilter {
            build: 68974,
            limit: 1,
            ..Default::default()
        },
        Some(&budget),
        || {
            checks += 1;
            if checks == 3 {
                cancel.cancel();
            }
            f.inv.control.check()
        },
    );
    assert_eq!(result.unwrap_err().code, "command.cancelled");
    assert!(checks >= 3);
    budget.check().unwrap();
}
#[test]
fn output_failure_retains_committed_capture_and_original_typed_cause() {
    let f = Fixture::new("wago");
    let budget = f.budget();
    let committed = evidence::commit_capture_budgeted(
        &f.inv.home,
        evidence::CaptureInput {
            bytes: b"{}",
            maximum_bytes: 2,
            media_type: "application/json",
            provenance: json!({"kind":"test"}),
            complete: true,
            truncated: false,
        },
        &f.inv.control,
        &budget,
    )
    .unwrap();
    let query = Query::from_invocation(&f.inv).unwrap();
    let mut reading = RemoteResult::new(
        "wago",
        query.clone(),
        vec![],
        Coverage::new("wago", &query),
        String::new(),
    )
    .unwrap();
    reading.capture = Some(committed.clone());
    let original = ToolkitError::new("records.query_resource_budget", "stopped during output", 3)
        .with_details(json!({"reason":"fixture"}));
    let error =
        committed_result::<()>(Err(output_failure(original.clone())), &reading).unwrap_err();
    assert_eq!(error.details["completedCaptures"], json!([committed["id"]]));
    assert_eq!(
        error.details["primary"]["details"]["originalCause"],
        serde_json::to_value(original).unwrap()
    );
}
