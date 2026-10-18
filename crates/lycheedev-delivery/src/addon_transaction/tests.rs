use super::*;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

const VERSION: &str = "3.5.0";
static NEXT: AtomicU64 = AtomicU64::new(0);

struct EquivalentScanCancellation {
    control: OperationControl,
    cancel_after: usize,
    observed: Vec<PathBuf>,
}
thread_local! {
    static EQUIVALENT_SCAN_CANCELLATION: RefCell<Option<EquivalentScanCancellation>> =
        const { RefCell::new(None) };
}

// Called only by the cfg(test) seam after a real record read and schema check.
// Thread-local state cannot cancel a concurrently running fixture's request.
pub(super) fn observe_equivalent_journal_record(path: &Path, control: &OperationControl) {
    EQUIVALENT_SCAN_CANCELLATION.with(|active| {
        let mut active = active.borrow_mut();
        let Some(active) = active.as_mut() else {
            return;
        };
        assert!(control.same_scope(&active.control));
        assert_eq!(control.deadline(), active.control.deadline());
        control.check().unwrap();
        active.observed.push(path.to_path_buf());
        if active.observed.len() == active.cancel_after {
            control.tracked_cancel_handle().unwrap().cancel();
        }
    });
}

fn with_equivalent_scan_cancellation<T>(
    control: &OperationControl,
    cancel_after: usize,
    operation: impl FnOnce() -> T,
) -> (T, Vec<PathBuf>) {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            EQUIVALENT_SCAN_CANCELLATION.with(|active| {
                active.borrow_mut().take();
            });
        }
    }
    assert!(cancel_after > 0);
    control.check().unwrap();
    EQUIVALENT_SCAN_CANCELLATION.with(|active| {
        let mut active = active.borrow_mut();
        assert!(
            active.is_none(),
            "scan cancellation observer already installed"
        );
        *active = Some(EquivalentScanCancellation {
            control: control.clone(),
            cancel_after,
            observed: Vec::new(),
        });
    });
    let _reset = Reset;
    let result = operation();
    let observed =
        EQUIVALENT_SCAN_CANCELLATION.with(|active| active.borrow_mut().take().unwrap().observed);
    (result, observed)
}

// Byte snapshots cannot detect same-byte rewrites. On the shipped platform,
// these retained read-only shares reject writes and replacement of the exact
// pool/journal paths even if their resulting bytes would have been identical.
#[cfg(windows)]
fn hold_without_write_or_delete(path: &Path) -> fs::File {
    use std::os::windows::fs::OpenOptionsExt;
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .unwrap();
    // Check the actual OS sharing restriction without writing or deleting.
    // GENERIC_WRITE and DELETE cover in-place writes and atomic replacement.
    for access in [0x40000000, 0x00010000] {
        let error = fs::OpenOptions::new()
            .access_mode(access)
            .share_mode(7)
            .open(path)
            .expect_err("held read-only share must reject mutation access");
        assert_eq!(error.raw_os_error(), Some(32));
    }
    held
}

struct Fixture {
    root: PathBuf,
    target: PathBuf,
    release: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-addon-intent-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(root.join("client/Interface/AddOns")).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let release = root.join("release-a");
        candidate(&release, VERSION, b"-- release A\n");
        Self {
            target: root.join("client/Interface/AddOns/Lychee Dev"),
            root,
            release,
        }
    }
    fn apply(
        &self,
        release: Option<&Path>,
        archive: Option<&Path>,
        version: &str,
        resume: bool,
        control: &OperationControl,
        guard: &InstallationGuard,
    ) -> Result<Value> {
        apply(
            &self.target,
            Request {
                release,
                archive,
                version,
                resume,
                control,
            },
            guard,
        )
    }
    fn intent(&self, release: &Path, archive: Option<&Path>, version: &str) -> PathBuf {
        let wanted = release::receipt(&release::inspect(release, version).unwrap(), "addon");
        journal(&self.target, archive, &wanted).unwrap()
    }
    fn snapshot(&self) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn visit(root: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                // The held OS locks are synchronization state, not addon data.
                if entry.file_name() == ".lycheedev-locks" {
                    continue;
                }
                let path = entry.path();
                let relative = path.strip_prefix(root).unwrap().to_path_buf();
                if entry.file_type().unwrap().is_dir() {
                    files.insert(relative, None);
                    visit(root, &path, files);
                } else {
                    files.insert(relative, Some(fs::read(path).unwrap()));
                }
            }
        }
        let mut files = BTreeMap::new();
        visit(&self.root, &self.root, &mut files);
        files
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }
}
fn candidate(root: &Path, version: &str, body: &[u8]) {
    let files = [
        (
            "addon/Lychee Dev.toc",
            format!("## Interface: 120100\n## Version: {version}\nBridge/SlotRuntime.lua\n")
                .into_bytes(),
        ),
        ("addon/Bridge/SlotRuntime.lua", body.to_vec()),
        ("addon/README.txt", b"fixture addon\n".to_vec()),
        ("skill/SKILL.md", b"fixture skill\n".to_vec()),
    ];
    for (name, bytes) in &files {
        let path = root.join("payload").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let resources: Vec<_> = files.iter().map(|(path, bytes)| {
        json!({"path":path,"bytes":bytes.len(),"sha256":storage::sha256(bytes)})
    }).collect();
    storage::atomic_json(&root.join("release.json"), &json!({
        "schema":"lycheedev.release.v1", "version":version, "commit":"a".repeat(40),
        "binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"b".repeat(64)}},
        "resources":resources,
    })).unwrap();
}
fn reverse_resources(path: &Path) {
    let mut value: Value = storage::read_json(path).unwrap();
    value["resources"].as_array_mut().unwrap().reverse();
    storage::atomic_json(path, &value).unwrap();
}
fn assert_installation_lock_released(target: &Path, control: &OperationControl) {
    deploy::InstallationLease::acquire(target, control)
        .unwrap()
        .finish(Ok(()))
        .unwrap();
}

fn seed_main(f: &Fixture) {
    let seed = f.root.join("seed");
    candidate(&seed, VERSION, b"-- predecessor P\n");
    deploy::fresh_controlled(
        &seed,
        &f.target,
        "addon",
        VERSION,
        &OperationControl::default(),
    )
    .unwrap();
}

fn pause_combined(f: &Fixture, archive: Option<&Path>, cut: Step) -> (PathBuf, Value) {
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let guard = InstallationGuard::acquire(&f.target).unwrap();
    let lease = deploy::InstallationLease::acquire(&f.target, &control).unwrap();
    let path = f.intent(&f.release, archive, VERSION);
    let phase = match cut {
        Step::Prepared => "prepared",
        Step::Main => "main",
        Step::Slots => "slots",
    };
    let mut observed = false;
    let result = apply_locked_observed(
        &f.target,
        Request {
            release: Some(&f.release),
            archive,
            version: VERSION,
            resume: false,
            control: &control,
        },
        &guard,
        &lease,
        &mut |step, checkpoint| {
            if step == cut {
                assert_eq!(checkpoint, path.as_path());
                let durable: Value = storage::read_json(checkpoint).unwrap();
                assert_eq!(durable["phase"], phase);
                observed = true;
                cancel.cancel();
            }
        },
    );
    let error = lease.finish(result).unwrap_err();
    assert!(observed, "combined checkpoint was not reached: {cut:?}");
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    assert_eq!(error.details["result"]["complete"], false);
    assert_eq!(error.details["result"]["phase"], phase);
    assert_eq!(error.details["result"]["journal"], json!(path));
    let record: Value = storage::read_json(&path).unwrap();
    assert_eq!(record["phase"], phase);
    assert_eq!(error.details["result"]["main"], record["result"]);
    guard.close().unwrap();
    assert_installation_lock_released(&f.target, &OperationControl::default());
    (path, record)
}

fn replacement_release(f: &Fixture, commit_only: bool) -> PathBuf {
    let root = f.root.join("release-b");
    change_release(&root, commit_only);
    root
}

fn change_release(root: &Path, commit_only: bool) {
    candidate(
        root,
        VERSION,
        if commit_only {
            b"-- release A\n"
        } else {
            b"-- release B\n"
        },
    );
    if commit_only {
        let manifest = root.join("release.json");
        let mut value: Value = storage::read_json(&manifest).unwrap();
        value["commit"] = json!("c".repeat(40));
        storage::atomic_json(&manifest, &value).unwrap();
    }
}

fn source_change_during_prepared(
    f: &Fixture,
    archive: Option<&Path>,
    commit_only: bool,
) -> (ToolkitError, BTreeMap<PathBuf, Option<Vec<u8>>>, Value) {
    let control = OperationControl::default();
    let guard = InstallationGuard::acquire(&f.target).unwrap();
    let lease = deploy::InstallationLease::acquire(&f.target, &control).unwrap();
    let path = f.intent(&f.release, archive, VERSION);
    let mut frozen = None;
    let mut before = None;
    let result = apply_locked_observed(
        &f.target,
        Request {
            release: Some(&f.release),
            archive,
            version: VERSION,
            resume: false,
            control: &control,
        },
        &guard,
        &lease,
        &mut |step, checkpoint| {
            if step == Step::Prepared {
                assert_eq!(checkpoint, path.as_path());
                let record: Value = storage::read_json(checkpoint).unwrap();
                assert_eq!(record["phase"], "prepared");
                change_release(&f.release, commit_only);
                frozen = Some(record);
                before = Some(f.snapshot());
            }
        },
    );
    let error = lease.finish(result).unwrap_err();
    let record = frozen.expect("durable prepared checkpoint was not reached");
    assert_eq!(error.code, "delivery.installation_conflict");
    assert_eq!(error.exit_code, 4);
    assert_eq!(error.details["result"]["complete"], false);
    assert_eq!(error.details["result"]["phase"], "prepared");
    assert_eq!(error.details["result"]["journal"], json!(path));
    assert_eq!(error.details["result"]["main"], Value::Null);
    assert_eq!(storage::read_json::<Value>(&path).unwrap(), record);
    assert!(
        !installation::same_receipt_value(
            &serde_json::to_value(release::receipt(
                &release::inspect(&f.release, VERSION).unwrap(),
                "addon"
            ))
            .unwrap(),
            &record["wanted"],
        )
        .unwrap()
    );
    guard.close().unwrap();
    assert_installation_lock_released(&f.target, &control);
    (error, before.unwrap(), record)
}

#[test]
fn prepared_fresh_source_change_is_rejected_before_publication() {
    for commit_only in [false, true] {
        let f = Fixture::new();
        let (error, before, record) = source_change_during_prepared(&f, None, commit_only);
        assert_eq!(
            error.message,
            "fresh release differs from frozen combined receipt"
        );
        assert_eq!(error.details["cause"]["wanted"], record["wanted"]);
        assert_eq!(error.details["cause"]["next"]["version"], VERSION);
        assert!(!f.target.exists());
        assert_eq!(f.snapshot(), before);
    }
}

#[test]
fn prepared_archive_source_change_is_rejected_before_publication() {
    for commit_only in [false, true] {
        let f = Fixture::new();
        seed_main(&f);
        let archive = f.root.join("archive-a");
        let (error, before, record) =
            source_change_during_prepared(&f, Some(&archive), commit_only);
        assert_eq!(
            error.message,
            "prepared replacement differs from frozen combined receipt"
        );
        let cause = &error.details["cause"];
        assert_eq!(cause["cause"]["details"]["wanted"], record["wanted"]);
        let retained = PathBuf::from(cause["retainedPreparation"].as_str().unwrap());
        assert_eq!(retained.parent().unwrap(), f.root);
        assert!(
            retained
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(".lycheedev-prepare-")
        );
        assert!(!archive.exists());
        assert!(!retained.join("replacement.json").exists());
        let staged = installation::inspect_main_controlled(
            &retained.join("next"),
            "addon",
            &OperationControl::default(),
        )
        .unwrap();
        assert_eq!(staged["state"], "managed");
        assert_eq!(staged["receipt"], cause["cause"]["details"]["next"]);
        // Only the existing owned-preparation evidence is retained. No main,
        // archive, slot or combined-journal bytes may change after the check.
        let mut after = f.snapshot();
        let relative = retained.strip_prefix(&f.root).unwrap();
        after.retain(|path, _| !path.starts_with(relative));
        assert_eq!(after, before);
    }
}

fn assert_pending_conflict(
    f: &Fixture,
    archive: Option<&Path>,
    path: &Path,
    record: &Value,
    message: &str,
) -> ToolkitError {
    let control = OperationControl::default();
    let guard = InstallationGuard::acquire(&f.target).unwrap();
    let before = f.snapshot();
    let mut last = None;
    for (release, resume) in [
        (Some(f.release.as_path()), false),
        (Some(f.release.as_path()), true),
        (None, true),
    ] {
        let error = f
            .apply(release, archive, VERSION, resume, &control, &guard)
            .unwrap_err();
        assert_eq!(error.code, "delivery.installation_conflict");
        assert_eq!(error.exit_code, 4);
        assert_eq!(error.message, message);
        assert_eq!(error.details["result"]["complete"], false);
        assert_eq!(error.details["result"]["phase"], record["phase"]);
        assert_eq!(error.details["result"]["journal"], json!(path));
        assert_eq!(error.details["result"]["main"], record["result"]);
        assert_eq!(f.snapshot(), before);
        assert_installation_lock_released(&f.target, &control);
        last = Some(error);
    }
    guard.close().unwrap();
    last.unwrap()
}

fn assert_slots_replacement_rejected(commit_only: bool) {
    for archived_original in [false, true] {
        let f = Fixture::new();
        let archive = f.root.join("archive-a");
        let original_archive = archived_original.then_some(archive.as_path());
        if archived_original {
            seed_main(&f);
        }
        let (path, record) = pause_combined(&f, original_archive, Step::Slots);
        let replacement = replacement_release(&f, commit_only);
        let control = OperationControl::default();
        let result_b = deploy::upgrade_controlled(
            Some(&replacement),
            &f.target,
            &f.root.join("archive-b"),
            "addon",
            VERSION,
            false,
            &control,
        )
        .unwrap();
        installation::verify_clean(&f.target, VERSION, slots::TRANSPORT, slots::COUNT).unwrap();
        let installed_b =
            installation::inspect_main_controlled(&f.target, "addon", &control).unwrap();
        assert_eq!(result_b["receipt"], installed_b["receipt"]);
        assert!(
            !installation::same_receipt_value(&installed_b["receipt"], &record["wanted"]).unwrap()
        );
        for reordered in [false, true] {
            if reordered {
                reverse_resources(&f.release.join("release.json"));
                assert_ne!(f.intent(&f.release, original_archive, VERSION), path);
            }
            let error = assert_pending_conflict(
                &f,
                original_archive,
                &path,
                &record,
                "main addon differs from frozen combined receipt after slot installation",
            );
            assert_eq!(error.details["cause"]["wanted"], record["wanted"]);
            assert_eq!(error.details["cause"]["installed"], installed_b);
        }
    }
}

#[test]
fn slots_checkpoint_rejects_same_version_content_replacement_without_mutation() {
    assert_slots_replacement_rejected(false);
}

#[test]
fn slots_checkpoint_rejects_same_version_commit_replacement_without_mutation() {
    assert_slots_replacement_rejected(true);
}

#[test]
fn slots_checkpoint_resumes_matching_and_reordered_receipts_without_replay() {
    let started = Instant::now();
    let progress = |archive: bool, reordered: Option<bool>, request: Option<u8>, stage: &str| {
        let mut stderr = std::io::stderr().lock();
        std::io::Write::write_fmt(&mut stderr, format_args!(
            "slots-recovery archive={archive} reordered={reordered:?} request={request:?} stage={stage} elapsed_ms={}\n",
            started.elapsed().as_millis(),
        )).unwrap();
        std::io::Write::flush(&mut stderr).unwrap();
    };
    let mut completed_cases = 0;
    for archived_original in [false, true] {
        progress(archived_original, None, None, "checkpoint-begin");
        let f = Fixture::new();
        let archive = f.root.join("archive-a");
        let original_archive = archived_original.then_some(archive.as_path());
        if archived_original {
            seed_main(&f);
        }
        let (path, record) = pause_combined(&f, original_archive, Step::Slots);
        // Each origin is produced by a real interrupted installation. Reuse
        // that exact durable checkpoint, never manufacture a phase or slots.
        let checkpoint_bytes = fs::read(&path).unwrap();
        let baseline = f.snapshot();
        let marker_path = f.target.join(installation::MARKER);
        let manifest_path = f.release.join("release.json");
        let marker_bytes = fs::read(&marker_path).unwrap();
        let manifest_bytes = fs::read(&manifest_path).unwrap();
        let mut completed = load(&path, &OperationControl::default()).unwrap();
        completed.phase = "complete".into();
        let completed_bytes = serde_json::to_vec_pretty(&completed).unwrap();
        progress(archived_original, None, None, "checkpoint-end");
        for reordered in [false, true] {
            let mut expected_start = baseline.clone();
            if reordered {
                for (file, bytes) in [
                    (&marker_path, &marker_bytes),
                    (&manifest_path, &manifest_bytes),
                ] {
                    let mut value: Value = serde_json::from_slice(bytes).unwrap();
                    value["resources"].as_array_mut().unwrap().reverse();
                    let bytes = serde_json::to_vec_pretty(&value).unwrap();
                    storage::atomic_write(file, &bytes).unwrap();
                    expected_start.insert(
                        file.strip_prefix(&f.root).unwrap().to_path_buf(),
                        Some(bytes),
                    );
                }
                assert_ne!(f.intent(&f.release, original_archive, VERSION), path);
            }
            let mut expected_complete = expected_start.clone();
            expected_complete.insert(
                path.strip_prefix(&f.root).unwrap().to_path_buf(),
                Some(completed_bytes.clone()),
            );
            for request_kind in 0..3 {
                // The preceding case must have restored all original bytes;
                // only the explicitly expected resource order may differ.
                assert_eq!(fs::read(&path).unwrap(), checkpoint_bytes);
                assert_eq!(f.snapshot(), expected_start);
                let control = OperationControl::default();
                let guard = InstallationGuard::acquire(&f.target).unwrap();
                let release = (request_kind != 2).then_some(f.release.as_path());
                // Re-entering slot installation would rewrite even a ready
                // pool. Deny that replay without blocking the legitimate
                // combined-journal slots -> complete transition below.
                #[cfg(windows)]
                let held_pool =
                    hold_without_write_or_delete(&f.target.parent().unwrap().join(slots::MARKER));
                progress(
                    archived_original,
                    Some(reordered),
                    Some(request_kind),
                    "resume-begin",
                );
                let result = f
                    .apply(
                        release,
                        original_archive,
                        VERSION,
                        request_kind != 0,
                        &control,
                        &guard,
                    )
                    .unwrap();
                progress(
                    archived_original,
                    Some(reordered),
                    Some(request_kind),
                    "resume-end",
                );
                assert_eq!(result, record["result"]);
                assert_eq!(f.snapshot(), expected_complete);
                // A completed retry must not rewrite the completed journal,
                // including rewriting the same bytes, or reinstall the slots.
                #[cfg(windows)]
                let held_complete = hold_without_write_or_delete(&path);
                assert_eq!(
                    f.apply(None, original_archive, VERSION, true, &control, &guard)
                        .unwrap(),
                    result
                );
                assert_eq!(f.snapshot(), expected_complete);
                progress(
                    archived_original,
                    Some(reordered),
                    Some(request_kind),
                    "verify-begin",
                );
                installation::verify_clean(&f.target, VERSION, slots::TRANSPORT, slots::COUNT)
                    .unwrap();
                #[cfg(windows)]
                {
                    storage::finish_file(held_complete, Ok(()), &control).unwrap();
                    storage::finish_file(held_pool, Ok(()), &control).unwrap();
                }
                guard.close().unwrap();
                progress(
                    archived_original,
                    Some(reordered),
                    Some(request_kind),
                    "verify-end",
                );
                // Public apply closed its installation lease, and the held
                // maintenance guard is now closed, before fixture restoration.
                progress(
                    archived_original,
                    Some(reordered),
                    Some(request_kind),
                    "restore-begin",
                );
                storage::atomic_write(&path, &checkpoint_bytes).unwrap();
                progress(
                    archived_original,
                    Some(reordered),
                    Some(request_kind),
                    "restore-end",
                );
                completed_cases += 1;
            }
            if reordered {
                storage::atomic_write(&marker_path, &marker_bytes).unwrap();
                storage::atomic_write(&manifest_path, &manifest_bytes).unwrap();
            }
            assert_eq!(f.snapshot(), baseline);
        }
        progress(archived_original, None, None, "cleanup-begin");
        drop(f);
        progress(archived_original, None, None, "cleanup-end");
    }
    assert_eq!(completed_cases, 12);
}

#[test]
fn prepared_archive_checkpoint_rejects_changed_predecessor_without_mutation() {
    let f = Fixture::new();
    seed_main(&f);
    let archive = f.root.join("archive-a");
    let (path, record) = pause_combined(&f, Some(&archive), Step::Prepared);
    assert!(!archive.exists());
    let replacement = replacement_release(&f, false);
    let control = OperationControl::default();
    deploy::upgrade_controlled(
        Some(&replacement),
        &f.target,
        &f.root.join("archive-b"),
        "addon",
        VERSION,
        false,
        &control,
    )
    .unwrap();
    let installed_b = installation::inspect_main_controlled(&f.target, "addon", &control).unwrap();
    let error = assert_pending_conflict(
        &f,
        Some(&archive),
        &path,
        &record,
        "main addon changed after combined preparation",
    );
    assert_eq!(
        error.details["cause"]["previous"],
        record["previous"]["receipt"]
    );
    assert_eq!(error.details["cause"]["installed"], installed_b);
    assert!(!archive.exists());
}

fn nested_checkpoint(f: &Fixture, release: &Path, archive: &Path, cut: deploy::UpgradeCheckpoint) {
    let result =
        deploy::interrupt_addon_upgrade_for_test(release, &f.target, archive, VERSION, cut);
    match cut {
        deploy::UpgradeCheckpoint::Published => {
            // The completed publication is retained even if the observer cancels
            // afterward; the combined checkpoint has not advanced yet.
            result.unwrap();
            assert!(f.target.exists());
            assert!(!archive.join("next").exists());
        }
        deploy::UpgradeCheckpoint::ArchivePublished => {
            assert_eq!(result.unwrap_err().code, "command.cancelled");
            assert!(f.target.exists());
            assert!(!archive.join("previous").exists());
            assert!(archive.join("next").exists());
        }
        deploy::UpgradeCheckpoint::OldMoved => {
            assert_eq!(result.unwrap_err().code, "command.cancelled");
            assert!(!f.target.exists());
            assert!(archive.join("previous").exists());
            assert!(archive.join("next").exists());
        }
    }
    assert!(archive.join("replacement.json").is_file());
    assert_installation_lock_released(&f.target, &OperationControl::default());
}

const NESTED_CHECKPOINTS: [deploy::UpgradeCheckpoint; 3] = [
    deploy::UpgradeCheckpoint::ArchivePublished,
    deploy::UpgradeCheckpoint::OldMoved,
    deploy::UpgradeCheckpoint::Published,
];

#[test]
fn prepared_archive_checkpoint_rejects_nested_changed_next_before_publication() {
    for cut in NESTED_CHECKPOINTS {
        for commit_only in [false, true] {
            let f = Fixture::new();
            seed_main(&f);
            let archive = f.root.join("archive-a");
            let (path, record) = pause_combined(&f, Some(&archive), Step::Prepared);
            let replacement = replacement_release(&f, commit_only);
            nested_checkpoint(&f, &replacement, &archive, cut);
            let nested: Value = storage::read_json(&archive.join("replacement.json")).unwrap();
            assert!(
                installation::same_receipt_value(
                    &nested["previous"],
                    &record["previous"]["receipt"]
                )
                .unwrap()
            );
            assert!(!installation::same_receipt_value(&nested["next"], &record["wanted"]).unwrap());
            let error = assert_pending_conflict(
                &f,
                Some(&archive),
                &path,
                &record,
                "replacement intent differs from frozen combined receipts",
            );
            assert_eq!(
                error.details["cause"]["previous"],
                record["previous"]["receipt"]
            );
            assert_eq!(error.details["cause"]["wanted"], record["wanted"]);
            assert_eq!(error.details["cause"]["replacement"], nested);
        }
    }
}

#[test]
fn prepared_archive_checkpoint_rejects_nested_changed_previous_before_publication() {
    for cut in NESTED_CHECKPOINTS {
        let f = Fixture::new();
        seed_main(&f);
        let archive = f.root.join("archive-a");
        let (path, record) = pause_combined(&f, Some(&archive), Step::Prepared);
        let replacement = replacement_release(&f, false);
        deploy::upgrade_controlled(
            Some(&replacement),
            &f.target,
            &f.root.join("archive-b"),
            "addon",
            VERSION,
            false,
            &OperationControl::default(),
        )
        .unwrap();
        nested_checkpoint(&f, &f.release, &archive, cut);
        let nested: Value = storage::read_json(&archive.join("replacement.json")).unwrap();
        assert!(
            !installation::same_receipt_value(&nested["previous"], &record["previous"]["receipt"])
                .unwrap()
        );
        assert!(installation::same_receipt_value(&nested["next"], &record["wanted"]).unwrap());
        let error = assert_pending_conflict(
            &f,
            Some(&archive),
            &path,
            &record,
            "replacement intent differs from frozen combined receipts",
        );
        assert_eq!(
            error.details["cause"]["previous"],
            record["previous"]["receipt"]
        );
        assert_eq!(error.details["cause"]["wanted"], record["wanted"]);
        assert_eq!(error.details["cause"]["replacement"], nested);
    }
}

#[test]
fn prepared_archive_checkpoint_resumes_each_matching_nested_state() {
    for cut in NESTED_CHECKPOINTS {
        for reordered in [false, true] {
            let f = Fixture::new();
            seed_main(&f);
            let archive = f.root.join("archive-a");
            let (path, record) = pause_combined(&f, Some(&archive), Step::Prepared);
            nested_checkpoint(&f, &f.release, &archive, cut);
            if reordered {
                let nested_path = archive.join("replacement.json");
                let mut nested: Value = storage::read_json(&nested_path).unwrap();
                nested["previous"]["resources"]
                    .as_array_mut()
                    .unwrap()
                    .reverse();
                nested["next"]["resources"]
                    .as_array_mut()
                    .unwrap()
                    .reverse();
                storage::atomic_json(&nested_path, &nested).unwrap();
                reverse_resources(&f.release.join("release.json"));
                for directory in [&f.target, &archive.join("previous"), &archive.join("next")] {
                    if directory.exists() {
                        reverse_resources(&directory.join(installation::MARKER));
                    }
                }
            }
            let nested_bytes = fs::read(archive.join("replacement.json")).unwrap();
            let control = OperationControl::default();
            let guard = InstallationGuard::acquire(&f.target).unwrap();
            let result = f
                .apply(None, Some(&archive), VERSION, true, &control, &guard)
                .unwrap();
            assert!(
                installation::same_receipt_value(&result["receipt"], &record["wanted"]).unwrap()
            );
            let done: Value = storage::read_json(&path).unwrap();
            assert_eq!(done["phase"], "complete");
            assert_eq!(done["wanted"], record["wanted"]);
            assert_eq!(done["previous"], record["previous"]);
            assert_eq!(done["result"], result);
            assert_eq!(
                fs::read(archive.join("replacement.json")).unwrap(),
                nested_bytes
            );
            assert_eq!(
                fs::read(archive.join("previous/Bridge/SlotRuntime.lua")).unwrap(),
                b"-- predecessor P\n"
            );
            assert_eq!(
                fs::read(f.target.join("Bridge/SlotRuntime.lua")).unwrap(),
                b"-- release A\n"
            );
            installation::verify_clean(&f.target, VERSION, slots::TRANSPORT, slots::COUNT).unwrap();
            let before = f.snapshot();
            assert_eq!(
                f.apply(
                    Some(&f.release),
                    Some(&archive),
                    VERSION,
                    true,
                    &control,
                    &guard
                )
                .unwrap(),
                result
            );
            assert_eq!(f.snapshot(), before);
            guard.close().unwrap();
        }
    }
}

#[test]
fn prepared_checkpoint_resumes_matching_fresh_and_upgrade_without_nested_record() {
    for upgrade in [false, true] {
        let f = Fixture::new();
        let archive = f.root.join("archive-a");
        let selected = upgrade.then_some(archive.as_path());
        if upgrade {
            seed_main(&f);
        }
        let (path, record) = pause_combined(&f, selected, Step::Prepared);
        let control = OperationControl::default();
        let guard = InstallationGuard::acquire(&f.target).unwrap();
        let before = f.snapshot();
        let error = f
            .apply(None, selected, VERSION, true, &control, &guard)
            .unwrap_err();
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(f.snapshot(), before);
        reverse_resources(&f.release.join("release.json"));
        if upgrade {
            reverse_resources(&f.target.join(installation::MARKER));
        }
        let result = f
            .apply(Some(&f.release), selected, VERSION, true, &control, &guard)
            .unwrap();
        let done: Value = storage::read_json(&path).unwrap();
        assert_eq!(done["phase"], "complete");
        assert_eq!(done["wanted"], record["wanted"]);
        assert_eq!(done["previous"], record["previous"]);
        assert_eq!(done["result"], result);
        installation::verify_clean(&f.target, VERSION, slots::TRANSPORT, slots::COUNT).unwrap();
        guard.close().unwrap();
    }
}

#[test]
fn prepared_no_archive_checkpoint_rejects_intervening_managed_install_without_mutation() {
    let f = Fixture::new();
    let (path, record) = pause_combined(&f, None, Step::Prepared);
    let replacement = replacement_release(&f, false);
    deploy::fresh_controlled(
        &replacement,
        &f.target,
        "addon",
        VERSION,
        &OperationControl::default(),
    )
    .unwrap();
    assert_pending_conflict(
        &f,
        None,
        &path,
        &record,
        "main addon changed after combined preparation",
    );
}

#[test]
fn main_checkpoint_rejects_same_version_replacement_without_replay() {
    for commit_only in [false, true] {
        let f = Fixture::new();
        let (path, record) = pause_combined(&f, None, Step::Main);
        let replacement = replacement_release(&f, commit_only);
        deploy::upgrade_controlled(
            Some(&replacement),
            &f.target,
            &f.root.join("archive-b"),
            "addon",
            VERSION,
            false,
            &OperationControl::default(),
        )
        .unwrap();
        assert_pending_conflict(
            &f,
            None,
            &path,
            &record,
            "main addon was edited while slots were pending",
        );
    }
}

#[test]
fn main_checkpoint_resumes_matching_and_reordered_receipts() {
    for reordered in [false, true] {
        let f = Fixture::new();
        let (path, record) = pause_combined(&f, None, Step::Main);
        if reordered {
            reverse_resources(&f.target.join(installation::MARKER));
            reverse_resources(&f.release.join("release.json"));
        }
        let control = OperationControl::default();
        let guard = InstallationGuard::acquire(&f.target).unwrap();
        assert_eq!(
            f.apply(Some(&f.release), None, VERSION, true, &control, &guard)
                .unwrap(),
            record["result"]
        );
        let done: Value = storage::read_json(&path).unwrap();
        assert_eq!(done["phase"], "complete");
        assert_eq!(done["wanted"], record["wanted"]);
        assert_eq!(done["previous"], record["previous"]);
        installation::verify_clean(&f.target, VERSION, slots::TRANSPORT, slots::COUNT).unwrap();
        guard.close().unwrap();
    }
}

#[test]
fn pending_checkpoints_preserve_cancellation_and_deadline_without_mutation() {
    for cut in [Step::Prepared, Step::Main, Step::Slots] {
        let f = Fixture::new();
        let (path, record) = pause_combined(&f, None, cut);
        let guard = InstallationGuard::acquire(&f.target).unwrap();
        let before = f.snapshot();
        let cancelled = OperationControl::default();
        cancelled.tracked_cancel_handle().unwrap().cancel();
        let expired = OperationControl::uncancelable(Instant::now());
        for (stopped, code) in [
            (&cancelled, "command.cancelled"),
            (&expired, "command.deadline"),
        ] {
            let error = f
                .apply(None, None, VERSION, true, stopped, &guard)
                .unwrap_err();
            assert_eq!(error.code, code);
            assert_eq!(f.snapshot(), before);
            let durable: Value = storage::read_json(&path).unwrap();
            assert_eq!(durable, record);
            assert_installation_lock_released(&f.target, &OperationControl::default());
        }
        guard.close().unwrap();
    }
}

#[test]
fn completed_intent_rejects_same_version_replacement_without_replay_or_mutation() {
    for archived_original in [false, true] {
        let f = Fixture::new();
        let control = OperationControl::default();
        let guard = InstallationGuard::acquire(&f.target).unwrap();
        let archive_a = f.root.join("archive-a");
        let original_archive = archived_original.then_some(archive_a.as_path());
        if archived_original {
            let seed = f.root.join("seed");
            candidate(&seed, VERSION, b"-- before A\n");
            f.apply(Some(&seed), None, VERSION, false, &control, &guard)
                .unwrap();
        }
        let result_a = f
            .apply(
                Some(&f.release),
                original_archive,
                VERSION,
                false,
                &control,
                &guard,
            )
            .unwrap();
        let path_a = f.intent(&f.release, original_archive, VERSION);
        let record_a: Value = storage::read_json(&path_a).unwrap();
        assert_eq!(record_a["phase"], "complete");
        assert_eq!(record_a["result"], result_a);
        let release_b = f.root.join("release-b");
        let archive_b = f.root.join("archive-b");
        candidate(
            &release_b,
            VERSION,
            if archived_original {
                b"-- release A\n"
            } else {
                b"-- release B\n"
            },
        );
        if archived_original {
            // A different release identity is a conflict even with identical files.
            let manifest = release_b.join("release.json");
            let mut value: Value = storage::read_json(&manifest).unwrap();
            value["commit"] = json!("c".repeat(40));
            storage::atomic_json(&manifest, &value).unwrap();
        }
        let result_b = f
            .apply(
                Some(&release_b),
                Some(&archive_b),
                VERSION,
                false,
                &control,
                &guard,
            )
            .unwrap();
        assert_ne!(result_b, result_a);
        installation::verify_clean(&f.target, VERSION, slots::TRANSPORT, slots::COUNT).unwrap();
        let installed_b =
            installation::inspect_main_controlled(&f.target, "addon", &control).unwrap();
        let before = f.snapshot();
        for (release, resume) in [(Some(f.release.as_path()), false), (None, true)] {
            let error = f
                .apply(release, original_archive, VERSION, resume, &control, &guard)
                .unwrap_err();
            assert_eq!(error.code, "delivery.installation_conflict");
            assert_eq!(error.exit_code, 4);
            assert_eq!(
                error.message,
                "main addon differs from completed combined receipt"
            );
            assert_eq!(error.details["result"]["complete"], false);
            assert_eq!(error.details["result"]["phase"], "complete");
            assert_eq!(error.details["result"]["journal"], json!(path_a));
            assert_eq!(error.details["result"]["main"], result_a);
            assert_eq!(error.details["cause"]["wanted"], record_a["wanted"]);
            assert_eq!(error.details["cause"]["installed"], installed_b);
            assert_eq!(f.snapshot(), before);
            assert_installation_lock_released(&f.target, &control);
        }
        guard.close().unwrap();
    }
}

#[test]
fn completed_intent_is_read_only_for_matching_and_reordered_receipts() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let guard = InstallationGuard::acquire(&f.target).unwrap();
    let result = f
        .apply(Some(&f.release), None, VERSION, false, &control, &guard)
        .unwrap();
    let original_path = f.intent(&f.release, None, VERSION);
    let original_bytes = fs::read(&original_path).unwrap();
    for reordered in [false, true] {
        if reordered {
            reverse_resources(&f.target.join(installation::MARKER));
            reverse_resources(&f.release.join("release.json"));
            assert_ne!(f.intent(&f.release, None, VERSION), original_path);
        }
        let before = f.snapshot();
        #[cfg(windows)]
        let held_pool =
            hold_without_write_or_delete(&f.target.parent().unwrap().join(slots::MARKER));
        #[cfg(windows)]
        let held_complete = hold_without_write_or_delete(&original_path);
        for (release, resume) in [
            (Some(f.release.as_path()), false),
            (Some(f.release.as_path()), true),
            (None, true),
        ] {
            assert_eq!(
                f.apply(release, None, VERSION, resume, &control, &guard)
                    .unwrap(),
                result
            );
            assert_eq!(f.snapshot(), before);
        }
        assert_eq!(fs::read(&original_path).unwrap(), original_bytes);
        #[cfg(windows)]
        {
            storage::finish_file(held_complete, Ok(()), &control).unwrap();
            storage::finish_file(held_pool, Ok(()), &control).unwrap();
        }
    }
    guard.close().unwrap();
}

#[test]
fn equivalent_intent_ambiguity_stops_without_creating_a_third_journal() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let guard = InstallationGuard::acquire(&f.target).unwrap();
    f.apply(Some(&f.release), None, VERSION, false, &control, &guard)
        .unwrap();
    let original_path = f.intent(&f.release, None, VERSION);
    let mut second = load(&original_path, &control).unwrap();
    second.wanted.resources.reverse();
    let second_path = journal(&f.target, None, &second.wanted).unwrap();
    assert_ne!(second_path, original_path);
    storage::atomic_json(&second_path, &second).unwrap();
    let manifest = f.release.join("release.json");
    let mut value: Value = storage::read_json(&manifest).unwrap();
    value["resources"].as_array_mut().unwrap().rotate_left(1);
    storage::atomic_json(&manifest, &value).unwrap();
    let third_path = f.intent(&f.release, None, VERSION);
    assert_ne!(third_path, original_path);
    assert_ne!(third_path, second_path);
    let before = f.snapshot();
    let error = f
        .apply(Some(&f.release), None, VERSION, false, &control, &guard)
        .unwrap_err();
    assert_eq!(error.code, "delivery.installation_conflict");
    assert_eq!(error.message, "ambiguous equivalent combined addon intent");
    assert_eq!(f.snapshot(), before);
    assert!(!third_path.exists());
    assert_installation_lock_released(&f.target, &control);
    guard.close().unwrap();
}

#[test]
fn reordered_intent_lookup_preserves_control_and_scan_limit_without_mutation() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let guard = InstallationGuard::acquire(&f.target).unwrap();
    f.apply(Some(&f.release), None, VERSION, false, &control, &guard)
        .unwrap();
    let original_path = f.intent(&f.release, None, VERSION);
    reverse_resources(&f.release.join("release.json"));
    let reordered_path = f.intent(&f.release, None, VERSION);
    assert_ne!(reordered_path, original_path);
    assert!(!reordered_path.exists());
    let before = f.snapshot();
    // Pre-stopped requests are rejected during public lease acquisition;
    // the separate observation below proves cancellation inside the scan.
    let cancelled = OperationControl::default();
    cancelled.tracked_cancel_handle().unwrap().cancel();
    let expired = OperationControl::uncancelable(Instant::now());
    for (stopped, code) in [
        (&cancelled, "command.cancelled"),
        (&expired, "command.deadline"),
    ] {
        let error = f
            .apply(Some(&f.release), None, VERSION, true, stopped, &guard)
            .unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(f.snapshot(), before);
    }
    let mut unrelated = load(&original_path, &control).unwrap();
    for index in 0..1000 {
        unrelated.target = f.root.join(format!("unrelated-{index}"));
        let path = journal(&unrelated.target, None, &unrelated.wanted).unwrap();
        // Keep unrelated records in this discovery namespace so the real
        // original-release lookup must account for every directory entry.
        storage::atomic_json(
            &original_path
                .parent()
                .unwrap()
                .join(path.file_name().unwrap()),
            &unrelated,
        )
        .unwrap();
    }
    let before = f.snapshot();
    let mid_scan = OperationControl::default();
    let (result, observed) = with_equivalent_scan_cancellation(&mid_scan, 2, || {
        f.apply(Some(&f.release), None, VERSION, true, &mid_scan, &guard)
    });
    let error = result.unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    assert_eq!(observed.len(), 2);
    assert_ne!(observed[0], observed[1]);
    for path in &observed {
        assert_eq!(path.parent(), original_path.parent());
        assert!(path.is_file());
    }
    assert!(mid_scan.is_cancelled());
    assert!(!reordered_path.exists());
    assert_eq!(f.snapshot(), before);
    assert_installation_lock_released(&f.target, &control);
    EQUIVALENT_SCAN_CANCELLATION.with(|active| assert!(active.borrow().is_none()));
    // The scoped observer is now gone. The original live control must still
    // reach the real 1001-entry limit instead of inheriting the cancellation.
    let error = f
        .apply(Some(&f.release), None, VERSION, true, &control, &guard)
        .unwrap_err();
    assert_eq!(error.code, "delivery.transaction_limit");
    assert_eq!(error.exit_code, 3);
    assert_eq!(f.snapshot(), before);
    assert_installation_lock_released(&f.target, &control);
    guard.close().unwrap();
}

#[cfg(windows)]
#[test]
fn pending_original_release_reorders_resume_the_same_journal_and_changed_bytes_do_not() {
    use std::os::windows::fs::OpenOptionsExt;
    let f = Fixture::new();
    let control = OperationControl::default();
    let guard = InstallationGuard::acquire(&f.target).unwrap();
    let old = f.root.join("old");
    candidate(&old, "3.4.9", b"-- old\n");
    f.apply(Some(&old), None, "3.4.9", false, &control, &guard)
        .unwrap();
    let archive = f.root.join("archive");
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(slots::directory(f.target.parent().unwrap(), 1).join("Lychee Dev Slot 01.toc"))
        .unwrap();
    let first = f
        .apply(
            Some(&f.release),
            Some(&archive),
            VERSION,
            false,
            &control,
            &guard,
        )
        .unwrap_err();
    assert_eq!(first.code, "external.io");
    assert_eq!(first.details["cause"]["rawOsError"], 32);
    assert_eq!(first.details["result"]["phase"], "main");
    let original_path = f.intent(&f.release, Some(&archive), VERSION);
    assert_eq!(first.details["result"]["journal"], json!(original_path));
    let different = f.root.join("different");
    candidate(&different, VERSION, b"-- different content\n");
    let before = f.snapshot();
    let error = f
        .apply(
            Some(&different),
            Some(&archive),
            VERSION,
            false,
            &control,
            &guard,
        )
        .unwrap_err();
    assert_eq!(error.code, "delivery.addon_transaction_pending");
    assert_eq!(error.details["journal"], json!(original_path));
    assert_eq!(f.snapshot(), before);
    reverse_resources(&f.release.join("release.json"));
    let reordered_path = f.intent(&f.release, Some(&archive), VERSION);
    assert_ne!(reordered_path, original_path);
    let before = f.snapshot();
    let retry = f
        .apply(
            Some(&f.release),
            Some(&archive),
            VERSION,
            false,
            &control,
            &guard,
        )
        .unwrap_err();
    assert_eq!(retry.code, "external.io");
    assert_eq!(retry.details["cause"]["rawOsError"], 32);
    assert_eq!(retry.details["result"]["phase"], "main");
    assert_eq!(retry.details["result"]["journal"], json!(original_path));
    assert_eq!(f.snapshot(), before);
    drop(held);
    let result = f
        .apply(
            Some(&f.release),
            Some(&archive),
            VERSION,
            true,
            &control,
            &guard,
        )
        .unwrap();
    let record = load(&original_path, &control).unwrap();
    assert_eq!(record.phase, "complete");
    assert_eq!(record.result, result);
    assert!(!reordered_path.exists());
    assert_eq!(
        fs::read(f.target.join("Bridge/SlotRuntime.lua")).unwrap(),
        b"-- release A\n"
    );
    assert_eq!(
        fs::read(archive.join("previous/Bridge/SlotRuntime.lua")).unwrap(),
        b"-- old\n"
    );
    installation::verify_clean(&f.target, VERSION, slots::TRANSPORT, slots::COUNT).unwrap();
    guard.close().unwrap();
}
