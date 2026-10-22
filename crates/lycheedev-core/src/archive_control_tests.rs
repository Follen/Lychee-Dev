//! COR-4 reconstructed revision. Filesystem phase regressions remain NOT_RUN.
use super::*;
use crate::{control::TrackedCancellation, storage::ExclusiveLock};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    base: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir().join(format!(
            "lychee-archive-control-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let home = base.join("home");
        fs::create_dir_all(home.join("state")).unwrap();
        fs::write(home.join("config.json"), b"opaque legacy, not JSON\0").unwrap();
        fs::write(home.join("state/record"), b"opaque legacy state").unwrap();
        Self { base, home }
    }
    fn assert_lease_released(&self) {
        ExclusiveLock::resource(&self.base.join(".lycheedev-locks"), "initialize:home")
            .unwrap()
            .close()
            .unwrap();
    }
    fn assert_no_stages(&self) {
        assert!(!fs::read_dir(&self.base).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".lycheedev-init-")
        }));
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}
fn original_control() -> (OperationControl, TrackedCancellation) {
    let parent = OperationControl::default();
    let signal = parent.tracked_cancel_handle().unwrap();
    let child = parent
        .ordered_child(Instant::now() + Duration::from_secs(300))
        .unwrap();
    assert_eq!(child.deadline(), parent.deadline());
    (child, signal)
}
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(base: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(base).unwrap().to_path_buf();
            if entry.file_type().unwrap().is_dir() {
                out.insert(relative, None);
                visit(base, &path, out);
            } else {
                out.insert(relative, Some(fs::read(path).unwrap()));
            }
        }
    }
    let mut out = BTreeMap::new();
    visit(root, root, &mut out);
    out
}
#[test]
fn archive_plan_checks_original_control_through_read_only_resolution() {
    for phase in [
        ArchivePhase::InspectJournal,
        ArchivePhase::InspectRoot,
        ArchivePhase::SelectArchive,
        ArchivePhase::PlanReady,
    ] {
        let f = Fixture::new();
        let before = snapshot(&f.base);
        let (control, signal) = original_control();
        let mut reached = false;
        let error = plan_observed(&f.home, InitControl(Some(&control)), &mut |current, _| {
            if current == phase {
                reached = true;
                signal.cancel();
            }
            Ok(())
        })
        .unwrap_err();
        assert!(reached, "{phase:?}");
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(snapshot(&f.base), before);
    }
}
#[test]
fn archive_cancellation_keeps_exact_journal_and_move_facts_and_allows_safe_resume() {
    let cases = [
        (ArchivePhase::ParentPrepared, None, false, false),
        (ArchivePhase::LeaseAcquired, None, false, false),
        (ArchivePhase::PlanReady, None, false, false),
        (ArchivePhase::JournalWriting, None, false, false),
        (ArchivePhase::JournalRecorded, Some("planned"), false, false),
        (ArchivePhase::Move, Some("planned"), false, false),
        (ArchivePhase::Moved, Some("planned"), true, false),
        (
            ArchivePhase::MovedJournalWriting,
            Some("planned"),
            true,
            false,
        ),
        (
            ArchivePhase::MovedJournalRecorded,
            Some("moved"),
            true,
            false,
        ),
        (
            ArchivePhase::Workspace(InitPhase::StageCreated),
            Some("moved"),
            true,
            false,
        ),
        (
            ArchivePhase::Workspace(InitPhase::DirectoryCreated),
            Some("moved"),
            true,
            false,
        ),
        (
            ArchivePhase::Workspace(InitPhase::ManifestWriting),
            Some("moved"),
            true,
            false,
        ),
        (
            ArchivePhase::Workspace(InitPhase::ManifestPublished),
            Some("moved"),
            true,
            false,
        ),
        (
            ArchivePhase::Workspace(InitPhase::RootRename),
            Some("moved"),
            true,
            false,
        ),
        (
            ArchivePhase::Workspace(InitPhase::RootPublished),
            Some("moved"),
            true,
            true,
        ),
        (ArchivePhase::WorkspaceReady, Some("moved"), true, true),
        (
            ArchivePhase::CompleteJournalWriting,
            Some("moved"),
            true,
            true,
        ),
        (
            ArchivePhase::CompleteJournalRecorded,
            Some("complete"),
            true,
            true,
        ),
        (ArchivePhase::LeaseClosed, Some("complete"), true, true),
    ];
    for (phase, expected_journal, moved, published) in cases {
        let f = Fixture::new();
        let legacy = snapshot(&f.home);
        let (control, signal) = original_control();
        let mut reached = false;
        let error = archive_observed(
            &f.home,
            false,
            InitControl(Some(&control)),
            &mut |current, _| {
                if current == phase {
                    reached = true;
                    signal.cancel();
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(reached, "{phase:?}");
        assert_eq!(error.code, "command.cancelled", "{phase:?}: {error:?}");
        let journal = journal_path(&f.home).unwrap();
        let recorded: Option<Value> = expected_journal.map(|expected| {
            let value: Value = storage::read_json(&journal).unwrap();
            assert_eq!(value["state"], expected, "{phase:?}");
            value
        });
        if expected_journal.is_none() {
            assert!(!journal.exists(), "{phase:?}");
        }
        assert_eq!(
            f.home.join("workspace.json").exists(),
            published,
            "{phase:?}"
        );
        if moved {
            let path = PathBuf::from(recorded.as_ref().unwrap()["archivePath"].as_str().unwrap());
            assert_eq!(snapshot(&path), legacy, "{phase:?}");
        } else {
            assert_eq!(snapshot(&f.home), legacy, "{phase:?}");
        }
        if phase != ArchivePhase::ParentPrepared {
            let result = &error.details["result"];
            assert_eq!(result["complete"], false);
            assert_eq!(result["archiveMoved"], moved, "{phase:?}");
            assert_eq!(result["journalState"], json!(expected_journal), "{phase:?}");
            if let Some(expected) = expected_journal {
                assert_eq!(result["plan"]["state"], expected, "{phase:?}");
            }
            if published {
                assert_eq!(
                    result["summary"]["identity"],
                    workspace::open(&f.home).unwrap()
                );
            } else {
                assert!(result["summary"].is_null());
            }
        }
        f.assert_no_stages();
        f.assert_lease_released();
        let recovered = if journal.exists() {
            resume(&f.home)
        } else {
            fresh(&f.home)
        }
        .unwrap();
        assert_eq!(recovered["plan"]["state"], "complete");
        let archive = PathBuf::from(recovered["plan"]["archivePath"].as_str().unwrap());
        assert_eq!(snapshot(&archive), legacy);
        assert_eq!(
            recovered["summary"]["identity"],
            workspace::open(&f.home).unwrap()
        );
        assert_eq!(resume(&f.home).unwrap(), recovered, "resume is idempotent");
    }
}
#[test]
fn archive_business_failure_after_move_keeps_first_error_and_separate_stop_with_recovery() {
    let f = Fixture::new();
    let legacy = snapshot(&f.home);
    let (control, signal) = original_control();
    let error = archive_observed(
        &f.home,
        false,
        InitControl(Some(&control)),
        &mut |phase, _| {
            if phase == ArchivePhase::Moved {
                signal.cancel();
                return Err(ToolkitError::new(
                    "fixture.after_move",
                    "failure after real move",
                    5,
                ));
            }
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "fixture.after_move");
    assert!(
        serde_json::to_string(&error)
            .unwrap()
            .contains("command.cancelled")
    );
    assert_eq!(error.details["result"]["archiveMoved"], true);
    assert_eq!(error.details["result"]["journalState"], "planned");
    assert_eq!(error.details["result"]["plan"]["state"], "planned");
    let recorded: Value = storage::read_json(&journal_path(&f.home).unwrap()).unwrap();
    assert_eq!(recorded["state"], "planned");
    assert!(!f.home.exists());
    f.assert_lease_released();
    let recovered = resume(&f.home).unwrap();
    assert_eq!(recovered["plan"]["state"], "complete");
    let archive = PathBuf::from(recovered["plan"]["archivePath"].as_str().unwrap());
    assert_eq!(snapshot(&archive), legacy);
}
#[test]
fn stopped_resume_and_missing_resume_do_not_create_paths_or_rewrite_journal() {
    let f = Fixture::new();
    let planned = plan(&f.home).unwrap();
    storage::atomic_json(&journal_path(&f.home).unwrap(), &planned).unwrap();
    let before = snapshot(&f.base);
    let (control, signal) = original_control();
    signal.cancel();
    assert_eq!(
        resume_controlled(&f.home, &control).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(snapshot(&f.base), before);
    let absent = f.base.join("missing-parent/home");
    let error = resume_controlled(&absent, &OperationControl::default()).unwrap_err();
    assert_eq!(error.code, "vault.archive_state");
    assert_eq!(error.exit_code, 3);
    assert_eq!(snapshot(&f.base), before);
}
