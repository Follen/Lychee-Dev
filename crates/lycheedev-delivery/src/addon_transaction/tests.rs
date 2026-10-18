use super::*;
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

const VERSION: &str = "3.5.0";
static NEXT: AtomicU64 = AtomicU64::new(0);

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
    let before = f.snapshot();
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
