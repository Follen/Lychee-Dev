use super::*;
use lycheedev_core::{VERSION, request::RequestContext};
use std::{
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    old: PathBuf,
    next: PathBuf,
    target: PathBuf,
    archive: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-deploy-control-{}-{}-{}",
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
        let next = root.join("next-release");
        candidate(&old, b"old", 'a');
        candidate(&next, b"next", 'b');
        fs::create_dir(root.join("skills")).unwrap();
        Self {
            target: root.join("skills/lycheedev"),
            archive: root.join("archive"),
            root,
            old,
            next,
        }
    }
    fn installed(&self) {
        fresh(&self.old, &self.target, "skill", VERSION).unwrap();
    }
    fn assert_old(&self) {
        assert_eq!(fs::read(self.target.join("SKILL.md")).unwrap(), b"old");
    }
    fn assert_next(&self) {
        assert_eq!(fs::read(self.target.join("SKILL.md")).unwrap(), b"next");
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn candidate(root: &Path, body: &[u8], commit: char) {
    let files = [
        ("addon/Lychee Dev.toc", b"addon".as_slice()),
        ("skill/SKILL.md", body),
        ("skill/references/a.md", b"reference".as_slice()),
    ];
    for (name, bytes) in files {
        let path = root.join("payload").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
    let resources: Vec<_> = files.iter().map(|(path, bytes)| json!({"path":path,"bytes":bytes.len(),"sha256":storage::sha256(bytes)})).collect();
    storage::atomic_json(&root.join("release.json"), &json!({"schema":"lycheedev.release.v1","version":VERSION,"commit":commit.to_string().repeat(40),"binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"c".repeat(64)}},"resources":resources})).unwrap();
}
fn reverse_receipt(path: &Path) {
    let mut value: Value = storage::read_json(path).unwrap();
    value["resources"].as_array_mut().unwrap().reverse();
    storage::atomic_json(path, &value).unwrap();
}
#[test]
fn public_skill_mutations_preserve_invocation_control_before_writes() {
    let f = Fixture::new();
    for request in [
        crate::Request::SkillInstall { path: f.target.clone(), mode: crate::InstallMode::Release { root: f.old.clone(), archive: None } },
        crate::Request::SkillInstall { path: f.target.clone(), mode: crate::InstallMode::Release { root: f.old.clone(), archive: Some(f.archive.clone()) } },
        crate::Request::SkillRemove { path: f.target.clone(), archive: f.archive.clone() },
    ] {
        let ctx = RequestContext { home: f.root.join("home"), cwd: f.root.clone(), control: OperationControl::default() };
        ctx.control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(crate::dispatch(&ctx, &request).unwrap_err().code, "command.cancelled");
        assert!(!f.target.exists());
        assert!(!f.archive.exists());
    }
}
#[test]
fn cancellation_after_real_staging_prevents_fresh_publication() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut observed = false;
    let error = fresh_observed(
        &f.old,
        &f.target,
        "skill",
        VERSION,
        &control,
        &mut |step, _| {
            if step == Step::Staged {
                observed = true;
                cancel.cancel();
            }
        },
    )
    .unwrap_err();
    assert!(observed);
    assert_eq!(error.code, "command.cancelled");
    assert!(!f.target.exists());
    fresh(&f.old, &f.target, "skill", VERSION).unwrap();
    f.assert_old();
}
#[test]
fn cancelled_upgrade_preparation_never_publishes_archive_and_is_retryable() {
    for cut in [Step::Staged, Step::Prepared] {
        let f = Fixture::new();
        f.installed();
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let mut observed = false;
        let error = upgrade_observed(
            Some(&f.next),
            &f.target,
            &f.archive,
            "skill",
            VERSION,
            false,
            &control,
            &mut |step, _| {
                if step == cut {
                    observed = true;
                    cancel.cancel();
                }
            },
        )
        .unwrap_err();
        assert!(observed);
        assert_eq!(error.code, "command.cancelled");
        f.assert_old();
        assert!(!f.archive.exists());
        upgrade(
            Some(&f.next),
            &f.target,
            &f.archive,
            "skill",
            VERSION,
            false,
        )
        .unwrap();
        f.assert_next();
    }
}
#[test]
fn cancellation_after_old_move_keeps_exact_recovery_and_resumes_without_release() {
    let f = Fixture::new();
    f.installed();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let error = upgrade_observed(
        Some(&f.next),
        &f.target,
        &f.archive,
        "skill",
        VERSION,
        false,
        &control,
        &mut |step, _| {
            if step == Step::OldMoved {
                cancel.cancel();
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(!f.target.exists());
    assert_eq!(
        fs::read(f.archive.join("previous/SKILL.md")).unwrap(),
        b"old"
    );
    assert_eq!(fs::read(f.archive.join("next/SKILL.md")).unwrap(), b"next");
    let journal = fs::read(f.archive.join("replacement.json")).unwrap();
    let done = upgrade(None, &f.target, &f.archive, "skill", VERSION, true).unwrap();
    f.assert_next();
    assert_eq!(done["receipt"]["commit"], "b".repeat(40));
    assert_eq!(
        fs::read(f.archive.join("replacement.json")).unwrap(),
        journal
    );
}
#[test]
fn cancellation_after_successful_moves_keeps_fresh_upgrade_and_removal_facts() {
    for operation in ["fresh", "upgrade", "remove"] {
        let f = Fixture::new();
        if operation != "fresh" {
            f.installed();
        }
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let mut observed = false;
        let mut observe = |step, path: &Path| {
            if path == f.target.as_path() && matches!(step, Step::Published | Step::Removed) {
                observed = true;
                cancel.cancel();
            }
        };
        match operation {
            "fresh" => {
                fresh_observed(&f.old, &f.target, "skill", VERSION, &control, &mut observe)
                    .unwrap();
                f.assert_old();
            }
            "upgrade" => {
                let done = upgrade_observed(
                    Some(&f.next),
                    &f.target,
                    &f.archive,
                    "skill",
                    VERSION,
                    false,
                    &control,
                    &mut observe,
                )
                .unwrap();
                assert_eq!(done["receipt"]["commit"], "b".repeat(40));
                f.assert_next();
            }
            _ => {
                let done = remove_observed(&f.target, &f.archive, "skill", &control, &mut observe)
                    .unwrap();
                assert_eq!(done["state"], "archived");
                assert!(!f.target.exists());
                assert_eq!(fs::read(f.archive.join("SKILL.md")).unwrap(), b"old");
            }
        }
        assert!(observed);
        assert_eq!(control.check().unwrap_err().code, "command.cancelled");
    }
}
#[test]
fn cancellation_after_removal_inventory_preserves_original_tree() {
    let f = Fixture::new();
    f.installed();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let error = remove_observed(&f.target, &f.archive, "skill", &control, &mut |step, _| {
        if step == Step::RemoveReady {
            cancel.cancel();
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    f.assert_old();
    assert!(!f.archive.exists());
}
#[test]
fn failed_replacement_move_preserves_foreign_target_and_both_recovery_trees() {
    let f = Fixture::new();
    f.installed();
    let error = upgrade_observed(
        Some(&f.next),
        &f.target,
        &f.archive,
        "skill",
        VERSION,
        false,
        &OperationControl::default(),
        &mut |step, _| {
            if step == Step::OldMoved {
                fs::create_dir(&f.target).unwrap();
                fs::write(f.target.join("foreign.txt"), b"owner").unwrap();
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.exit_code, 5);
    assert_eq!(fs::read(f.target.join("foreign.txt")).unwrap(), b"owner");
    assert_eq!(
        fs::read(f.archive.join("previous/SKILL.md")).unwrap(),
        b"old"
    );
    assert_eq!(fs::read(f.archive.join("next/SKILL.md")).unwrap(), b"next");
    assert_eq!(
        upgrade(None, &f.target, &f.archive, "skill", VERSION, true)
            .unwrap_err()
            .code,
        "delivery.installation_conflict"
    );
    fs::remove_file(f.target.join("foreign.txt")).unwrap();
    fs::remove_dir(&f.target).unwrap();
    upgrade(None, &f.target, &f.archive, "skill", VERSION, true).unwrap();
    f.assert_next();
}
#[test]
fn receipt_order_is_immaterial_for_fresh_and_interrupted_upgrade() {
    let f = Fixture::new();
    f.installed();
    reverse_receipt(&f.target.join(installation::MARKER));
    let before = fs::read(f.target.join(installation::MARKER)).unwrap();
    fresh(&f.old, &f.target, "skill", VERSION).unwrap();
    assert_eq!(
        fs::read(f.target.join(installation::MARKER)).unwrap(),
        before
    );
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    upgrade_observed(
        Some(&f.next),
        &f.target,
        &f.archive,
        "skill",
        VERSION,
        false,
        &control,
        &mut |step, _| {
            if step == Step::OldMoved {
                cancel.cancel();
            }
        },
    )
    .unwrap_err();
    reverse_receipt(&f.archive.join("previous").join(installation::MARKER));
    reverse_receipt(&f.archive.join("next").join(installation::MARKER));
    upgrade(None, &f.target, &f.archive, "skill", VERSION, true).unwrap();
    f.assert_next();
    fs::write(f.target.join("SKILL.md"), b"edited").unwrap();
    assert_eq!(
        fresh(&f.next, &f.target, "skill", VERSION)
            .unwrap_err()
            .code,
        "delivery.installation_conflict"
    );
}
#[test]
fn preparation_failure_retains_structured_checked_close_uncertainty() {
    let f = Fixture::new();
    let stage = f.root.join("retained");
    fs::create_dir(&stage).unwrap();
    fs::write(stage.join("uncertain.tmp"), b"evidence").unwrap();
    let primary=ToolkitError::new("fixture.close","injected close boundary failure",5).with_details(json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":null,"cleanup":{"code":"fixture.close"},"control":null}));
    let expected = serde_json::to_value(&primary).unwrap();
    let error = finish_preparation::<()>(&stage, Err(primary)).unwrap_err();
    assert_eq!(error.code, "fixture.close");
    assert_eq!(error.details["cause"], expected);
    assert_eq!(fs::read(stage.join("uncertain.tmp")).unwrap(), b"evidence");
}

#[test]
fn checked_installation_close_failure_keeps_committed_receipt_and_consumes_once() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let lease = InstallationLease::acquire(&f.target, &control).unwrap();
    let result = fresh_locked(&f.old, &lease, "skill", VERSION, &control);
    assert!(result.is_ok());
    let mut closes = 0;
    let error = lease
        .finish_with(result, |lock| {
            closes += 1;
            lock.close()?;
            // Delivery finish-boundary injection, not a claimed kernel failure.
            Err(ToolkitError::new(
                "fixture.close",
                "injected checked close failure",
                5,
            ))
        })
        .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "fixture.close");
    assert_eq!(error.details["operationCompleted"], true);
    assert_eq!(error.details["result"]["commit"], "a".repeat(40));
    f.assert_old();
    // The original lock was consumed, so retry can acquire the same owner.
    fresh(&f.old, &f.target, "skill", VERSION).unwrap();
}
#[test]
fn checked_installation_close_failure_preserves_primary_error_and_its_details() {
    let f = Fixture::new();
    f.installed();
    let control = OperationControl::default();
    let lease = InstallationLease::acquire(&f.target, &control).unwrap();
    let mut primary = fresh_locked(&f.next, &lease, "skill", VERSION, &control).unwrap_err();
    assert_eq!(primary.code, "delivery.installation_conflict");
    primary.details = json!({"originalDiagnostic": "retained"});
    let expected = serde_json::to_value(&primary).unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    let mut closes = 0;
    let error = lease
        .finish_with::<Receipt>(Err(primary), |lock| {
            closes += 1;
            lock.close()?;
            Err(ToolkitError::new(
                "fixture.close",
                "injected checked close failure",
                5,
            ))
        })
        .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "delivery.installation_conflict");
    assert_eq!(error.details["primary"], expected);
    assert_eq!(error.details["cleanup"]["code"], "fixture.close");
    f.assert_old();
    InstallationLease::acquire(&f.target, &OperationControl::default())
        .unwrap()
        .finish(Ok(()))
        .unwrap();
}
