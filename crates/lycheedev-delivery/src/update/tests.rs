use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    old: PathBuf,
    next: PathBuf,
    other: PathBuf,
    state: PathBuf,
    target: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-update-recovery-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let old = root.join("old");
        let next = root.join("next");
        let other = root.join("other");
        candidate(&old, b"old", 'a');
        candidate(&next, b"next", 'b');
        candidate(&other, b"other", 'c');
        fs::create_dir(root.join("skills")).unwrap();
        Self {
            state: root.join("state"),
            target: root.join("skills/lycheedev"),
            root,
            old,
            next,
            other,
        }
    }
    fn targets(&self) -> Vec<UpdateTarget> {
        vec![UpdateTarget {
            component: "skill".into(),
            path: self.target.clone(),
        }]
    }
    fn install(&self) {
        deploy::fresh_controlled(&self.old, &self.target, "skill", VERSION, &Default::default()).unwrap();
    }
    fn pending(&self, name: &str) -> (PathBuf, Value) {
        let archive = self.root.join(".lycheedev-updates").join(name);
        fs::create_dir_all(&archive).unwrap();
        let previous = installation::inspect_main_controlled(&self.target, "skill", &Default::default()).unwrap()["receipt"].clone();
        let next = deploy::fresh_controlled(&self.next, &archive.join("next"), "skill", VERSION, &Default::default()).unwrap();
        let record = json!({"schema":"lycheedev.replacement.v1","target":self.target,"previous":previous,"next":next});
        storage::atomic_json(&archive.join("replacement.json"), &record).unwrap();
        fs::rename(&self.target, archive.join("previous")).unwrap();
        (archive, record)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn candidate(root: &Path, body: &[u8], commit: char) {
    let files = [
        ("addon/Lychee Dev.toc", b"fixture".as_slice()),
        ("skill/SKILL.md", body),
        ("skill/reference.md", b"reference".as_slice()),
    ];
    for (name, bytes) in files {
        let path = root.join("payload").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let resources:Vec<_>=files.iter().map(|(path,bytes)|json!({"path":path,"bytes":bytes.len(),"sha256":storage::sha256(bytes)})).collect();
    storage::atomic_json(&root.join("release.json"),&json!({"schema":"lycheedev.release.v1","version":VERSION,"commit":commit.to_string().repeat(40),"binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"d".repeat(64)}},"resources":resources})).unwrap();
}
#[test]
fn remembered_removed_skill_is_not_reinstalled_but_explicit_target_is() {
    let f = Fixture::new();
    let targets = f.targets();
    managed_with_control(&f.old, VERSION, &f.state, &targets, false, &Default::default()).unwrap();
    let ctx = RequestContext {
        home: f.root.join("home"),
        cwd: f.root.clone(),
        control: OperationControl::default(),
    };
    crate::dispatch(
        &ctx,
        &crate::Request::SkillRemove {
            path: f.target.clone(),
            archive: f.root.join("removed"),
        },
    )
    .unwrap();
    assert!(!f.target.exists());
    let control = OperationControl::default();
    let remembered = read_targets(&f.state, &control).unwrap();
    assert_eq!(remembered.len(), 1);
    let discovered = discover_targets(remembered, &control).unwrap();
    assert!(discovered.is_empty());
    let result = managed_with_control(&f.next, VERSION, &f.state, &discovered, false, &Default::default()).unwrap();
    assert_eq!(result.data["complete"], true);
    assert!(!f.target.exists());
    managed_with_control(&f.next, VERSION, &f.state, &targets, false, &Default::default()).unwrap();
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
}
#[test]
fn absent_target_with_original_recovery_is_discovered_and_other_release_is_rejected() {
    let f = Fixture::new();
    f.install();
    let (archive, record) = f.pending("replace-old-selector-hash");
    let control = OperationControl::default();
    assert_eq!(discover_targets(f.targets(), &control).unwrap().len(), 1);
    for plan in [true, false] {
        let error = managed_with_control(&f.other, VERSION, &f.state, &f.targets(), plan, &Default::default()).unwrap_err();
        assert_eq!(error.code, "delivery.installation_conflict");
        assert_eq!(
            error.details["recovery"],
            serde_json::to_value(&archive).unwrap()
        );
        assert!(!f.target.exists());
        assert_eq!(
            storage::read_json::<Value>(&archive.join("replacement.json")).unwrap(),
            record
        );
    }
    let plan = managed_with_control(&f.next, VERSION, &f.state, &f.targets(), true, &Default::default()).unwrap();
    assert_eq!(
        plan.data["targets"][0]["recovery"],
        serde_json::to_value(&archive).unwrap()
    );
    assert!(!f.state.exists());
    let result = managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap();
    assert_eq!(result.data["complete"], true);
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
    assert!(!archive.exists());
}
#[test]
fn cleanup_marker_without_archive_freezes_original_release_and_recovers() {
    let f = Fixture::new();
    f.install();
    let (archive, record) = f.pending("replace-original");
    deploy::upgrade_controlled(None, &f.target, &archive, "skill", VERSION, true, &Default::default()).unwrap();
    let marker = cleanup_marker(&archive);
    storage::atomic_json(&marker, &record).unwrap();
    fs::remove_dir_all(&archive).unwrap();
    let error = managed_with_control(&f.other, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap_err();
    assert_eq!(error.code, "delivery.installation_conflict");
    assert!(marker.exists());
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
    let result = managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap();
    assert_eq!(result.data["complete"], true);
    assert!(!marker.exists());
}
#[test]
fn duplicate_recoveries_conflict_but_archive_and_its_cleanup_marker_deduplicate() {
    let f = Fixture::new();
    f.install();
    let (archive, record) = f.pending("replace-one");
    let control = OperationControl::default();
    storage::atomic_json(&cleanup_marker(&archive), &record).unwrap();
    assert_eq!(
        pending_recoveries(&f.target, "skill", &control)
            .unwrap()
            .len(),
        1
    );
    let other = f.root.join(".lycheedev-updates/replace-two.cleanup.json");
    storage::atomic_json(&other, &record).unwrap();
    let error = managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap_err();
    assert_eq!(error.code, "delivery.installation_conflict");
    assert_eq!(error.details["recoveries"].as_array().unwrap().len(), 2);
    assert!(!f.target.exists());
    assert!(archive.join("next").exists());
}
#[test]
fn changed_pending_inventory_is_rejected_before_any_target_changes() {
    let f = Fixture::new();
    f.install();
    let (archive, record) = f.pending("replace-original");
    fs::write(archive.join("next/SKILL.md"), b"foreign edit").unwrap();
    let error = managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap_err();
    assert_eq!(error.code, "delivery.installation_conflict");
    assert_eq!(error.details["result"]["targets"][0]["state"], "pending");
    assert!(!f.target.exists());
    assert_eq!(fs::read(archive.join("previous/SKILL.md")).unwrap(), b"old");
    assert_eq!(
        storage::read_json::<Value>(&archive.join("replacement.json")).unwrap(),
        record
    );
}
#[test]
fn same_release_recovery_accepts_receipt_order_but_rejects_changed_resources() {
    let f = Fixture::new();
    f.install();
    let (archive, mut record) = f.pending("replace-original");
    record["next"]["resources"]
        .as_array_mut()
        .unwrap()
        .reverse();
    record["previous"]["resources"]
        .as_array_mut()
        .unwrap()
        .reverse();
    storage::atomic_json(&archive.join("replacement.json"), &record).unwrap();
    let mut changed = record.clone();
    changed["next"]["resources"][0]["sha256"] = json!("e".repeat(64));
    storage::atomic_json(&archive.join("replacement.json"), &changed).unwrap();
    assert_eq!(
        managed_with_control(&f.next, VERSION, &f.state, &f.targets(), true, &Default::default())
            .unwrap_err()
            .code,
        "delivery.installation_conflict"
    );
    assert!(!f.target.exists());
    storage::atomic_json(&archive.join("replacement.json"), &record).unwrap();
    managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap();
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
    assert!(!archive.exists());
}
#[test]
fn cancelled_second_target_reports_and_preserves_first_committed_target() {
    let f = Fixture::new();
    f.install();
    let mut targets = f.targets();
    let other = f.root.join("second/lycheedev");
    fs::create_dir(other.parent().unwrap()).unwrap();
    deploy::fresh_controlled(&f.old, &other, "skill", VERSION, &Default::default()).unwrap();
    targets.push(UpdateTarget {
        component: "skill".into(),
        path: other.clone(),
    });
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let error = managed_observed(
        &f.next,
        VERSION,
        &f.state,
        &targets,
        false,
        &control,
        &mut |index| {
            if index == UpdateStep::BeforeTarget(1) {
                cancel.cancel();
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.details["result"]["complete"], false);
    assert_eq!(error.details["result"]["targets"][0]["state"], "current");
    assert_eq!(error.details["result"]["targets"][1]["state"], "pending");
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
    assert_eq!(fs::read(other.join("SKILL.md")).unwrap(), b"old");
    managed_with_control(&f.next, VERSION, &f.state, &targets, false, &Default::default()).unwrap();
    assert_eq!(fs::read(other.join("SKILL.md")).unwrap(), b"next");
}
#[test]
fn empty_default_update_does_not_create_state_and_cancelled_update_never_creates_it() {
    let f = Fixture::new();
    let result = managed_with_control(&f.next, VERSION, &f.state, &[], false, &Default::default()).unwrap();
    assert_eq!(result.data["complete"], true);
    assert!(!f.state.exists());
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &control)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(!f.state.exists());
    assert!(!f.target.exists());
}

#[test]
fn recovery_appearing_after_preflight_blocks_new_release_under_target_lease() {
    let f = Fixture::new();
    f.install();
    let mut retained = None;
    let error = managed_observed(
        &f.other,
        VERSION,
        &f.state,
        &f.targets(),
        false,
        &OperationControl::default(),
        &mut |step| {
            if step == UpdateStep::BeforeTarget(0) {
                // A second invocation completed its old-moved checkpoint after
                // B's read-only preflight and before B acquired target ownership.
                retained = Some(f.pending("replace-racing-original"));
            }
        },
    )
    .unwrap_err();
    let (archive, record) = retained.unwrap();
    assert_eq!(error.code, "delivery.installation_conflict");
    assert_eq!(error.details["result"]["targets"][0]["state"], "pending");
    assert_eq!(
        error.details["cause"]["recovery"],
        serde_json::to_value(&archive).unwrap()
    );
    assert!(!f.target.exists());
    assert_eq!(fs::read(archive.join("previous/SKILL.md")).unwrap(), b"old");
    assert_eq!(fs::read(archive.join("next/SKILL.md")).unwrap(), b"next");
    assert_eq!(
        storage::read_json::<Value>(&archive.join("replacement.json")).unwrap(),
        record
    );
    managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap();
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
}
#[test]
fn authoritative_recovery_scan_and_moves_exclude_all_competing_deploy_entries() {
    let f = Fixture::new();
    f.install();
    let mut blocked = 0;
    managed_observed(
        &f.next,
        VERSION,
        &f.state,
        &f.targets(),
        false,
        &OperationControl::default(),
        &mut |step| {
            if step != UpdateStep::LockedTarget(0) {
                return;
            }
            let archive = f.root.join("competing-archive");
            assert_eq!(
                deploy::fresh_controlled(&f.other, &f.target, "skill", VERSION, &Default::default())
                    .unwrap_err()
                    .code,
                "workspace.busy"
            );
            assert_eq!(
                deploy::upgrade_controlled(Some(&f.other), &f.target, &archive, "skill", VERSION, false, &Default::default())
                    .unwrap_err()
                    .code,
                "workspace.busy"
            );
            assert_eq!(
                deploy::remove_controlled(&f.target, &archive, "skill", &Default::default())
                    .unwrap_err()
                    .code,
                "workspace.busy"
            );
            assert!(!archive.exists());
            assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"old");
            blocked += 3;
        },
    )
    .unwrap();
    assert_eq!(blocked, 3);
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
}
#[test]
fn checked_update_state_close_failure_keeps_completed_business_result() {
    let f = Fixture::new();
    f.install();
    let mut closes = 0;
    let error = managed_with_close(
        &f.next,
        VERSION,
        &f.state,
        &f.targets(),
        false,
        &OperationControl::default(),
        &mut |_| {},
        |lock| {
            closes += 1;
            lock.close()?;
            Err(ToolkitError::new(
                "fixture.close",
                "injected state close failure",
                5,
            ))
        },
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "fixture.close");
    assert_eq!(error.details["operationCompleted"], true);
    assert_eq!(error.details["result"]["data"]["complete"], true);
    assert_eq!(
        error.details["result"]["data"]["targets"][0]["state"],
        "current"
    );
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
    managed_with_control(&f.next, VERSION, &f.state, &f.targets(), false, &Default::default()).unwrap();
}
#[test]
fn checked_update_state_close_failure_preserves_cancellation_and_prior_commits() {
    let f = Fixture::new();
    f.install();
    let mut targets = f.targets();
    let second = f.root.join("second/lycheedev");
    fs::create_dir(second.parent().unwrap()).unwrap();
    deploy::fresh_controlled(&f.old, &second, "skill", VERSION, &Default::default()).unwrap();
    targets.push(UpdateTarget {
        component: "skill".into(),
        path: second.clone(),
    });
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut closes = 0;
    let error = managed_with_close(
        &f.next,
        VERSION,
        &f.state,
        &targets,
        false,
        &control,
        &mut |step| {
            if step == UpdateStep::BeforeTarget(1) {
                cancel.cancel();
            }
        },
        |lock| {
            closes += 1;
            lock.close()?;
            Err(ToolkitError::new(
                "fixture.close",
                "injected state close failure",
                5,
            ))
        },
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["primary"]["details"]["result"]["targets"][0]["state"],
        "current"
    );
    assert_eq!(error.details["cleanup"]["code"], "fixture.close");
    assert_eq!(fs::read(f.target.join("SKILL.md")).unwrap(), b"next");
    assert_eq!(fs::read(second.join("SKILL.md")).unwrap(), b"old");
    managed_with_control(&f.next, VERSION, &f.state, &targets, false, &Default::default()).unwrap();
}

fn seed_configured_targets(home: &Path, rows: &[(String, Vec<u8>)]) {
    use lycheedev_core::metadata::{Metadata, OwnedMutation};
    let control = OperationControl::default();
    lycheedev_core::workspace::initialize_controlled(home, &control).unwrap();
    Metadata::with_owned_write(home, &control, |writer| {
        for chunk in rows.chunks(64) {
            let changes: Vec<_> = chunk
                .iter()
                .map(|(key, bytes)| OwnedMutation {
                    key,
                    expected_generation: 0,
                    bytes,
                })
                .collect();
            writer.commit(&changes)?;
        }
        Ok(())
    })
    .unwrap();
}
#[test]
fn configured_addon_discovery_stopped_control_never_reads_workspace_targets() {
    let f = Fixture::new();
    let home = f.root.join("configured-home");
    let client = f.root.join("configured-client");
    seed_configured_targets(
        &home,
        &[(
            "target/actual".into(),
            serde_json::to_vec(&json!({"source":"installation","installation":client})).unwrap(),
        )],
    );
    assert!(home.join("state/toolkit.sqlite").is_file());
    let cancelled = OperationControl::default();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    let expired = OperationControl::uncancelable(std::time::Instant::now());
    for (control, expected) in [
        (cancelled, "command.cancelled"),
        (expired, "command.deadline"),
    ] {
        let error = configured_addon_targets(&home, &control).unwrap_err();
        assert_eq!(
            error.code, expected,
            "the production discovery owner consumes the original stop before reading configured addon targets"
        );
    }
}
#[test]
fn configured_addon_discovery_keeps_prefix_source_key_order_and_original_row_limit() {
    let f = Fixture::new();
    let home = f.root.join("configured-home");
    let first = f.root.join("z-first-by-key");
    let second = f.root.join("a-second-by-key");
    let mut rows = Vec::new();
    rows.push((
        "other/unrelated".into(),
        serde_json::to_vec(&json!({"source":"installation","installation":"unrelated"})).unwrap(),
    ));
    for index in 0..10002 {
        let value = match index {
            0 => json!({"source":"repository","installation":"filtered-source"}),
            1 => json!({"source":"installation","installation":17}),
            2 => json!({"source":"installation","installation":first}),
            3 => json!({"source":"installation","installation":second}),
            _ => {
                json!({"source":"installation","installation":f.root.join(format!("client-{index:05}"))})
            }
        };
        rows.push((
            format!("target/{index:05}"),
            serde_json::to_vec(&value).unwrap(),
        ));
    }
    seed_configured_targets(&home, &rows);
    let actual = configured_addon_targets(&home, &OperationControl::default()).unwrap();
    assert_eq!(
        actual.len(),
        9999,
        "the 10001-row source limit applies before source/installation filtering"
    );
    assert!(actual.iter().all(|target| target.component == "addon"));
    assert_eq!(actual[0].path, first);
    assert_eq!(
        actual[1].path, second,
        "metadata key order is preserved rather than path sorting"
    );
    assert_eq!(actual.last().unwrap().path, f.root.join("client-10000"));
    assert!(
        !actual
            .iter()
            .any(|target| target.path == f.root.join("client-10001"))
    );
}
