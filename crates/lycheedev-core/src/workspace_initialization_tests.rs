//! COR-4 reconstructed revision. These regressions have not been executed.
use super::*;
use crate::control::TrackedCancellation;
use std::{
    path::PathBuf,
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
            "lychee-init-control-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        let home = base.join("home");
        fs::create_dir_all(&home).unwrap();
        Self { base, home }
    }
    fn stages(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.base)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".lycheedev-init-")
            })
            .collect()
    }
    fn assert_lease_released(&self) {
        ExclusiveLock::resource(&self.base.join(".lycheedev-locks"), "initialize:home")
            .unwrap()
            .close()
            .unwrap();
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
    assert_eq!(parent.deadline(), child.deadline());
    (child, signal)
}
fn error_contains(error: &ToolkitError, code: &str) -> bool {
    serde_json::to_string(error).unwrap().contains(code)
}
#[test]
fn original_control_stops_every_initialization_phase_without_rolling_back_published_root() {
    for phase in [
        InitPhase::ParentPrepared,
        InitPhase::LeaseAcquired,
        InitPhase::StageCreated,
        InitPhase::DirectoryCreated,
        InitPhase::ManifestWriting,
        InitPhase::ManifestPublished,
        InitPhase::RemoveEmptyRoot,
        InitPhase::EmptyRootRemoved,
        InitPhase::RootRename,
        InitPhase::RootPublished,
        InitPhase::LeaseClosed,
    ] {
        let f = Fixture::new();
        let (control, signal) = original_control();
        let mut reached = false;
        let error = initialize_observed(&f.home, InitControl(Some(&control)), &mut |current, _| {
            if current == phase {
                reached = true;
                signal.cancel();
            }
            Ok(())
        })
        .unwrap_err();
        assert!(reached, "{phase:?}");
        assert_eq!(error.code, "command.cancelled", "{phase:?}: {error:?}");
        let published = matches!(phase, InitPhase::RootPublished | InitPhase::LeaseClosed);
        assert_eq!(
            f.home.join("workspace.json").exists(),
            published,
            "{phase:?}"
        );
        assert!(f.stages().is_empty(), "{phase:?}");
        if published {
            assert_eq!(error.details["result"]["identity"], open(&f.home).unwrap());
        }
        if matches!(
            phase,
            InitPhase::EmptyRootRemoved | InitPhase::RootRename | InitPhase::RootPublished
        ) {
            assert_eq!(error.details["initialization"]["emptyTargetRemoved"], true);
        }
        f.assert_lease_released();
        let identity = initialize(&f.home).unwrap();
        assert_eq!(identity, open(&f.home).unwrap());
    }
}
#[test]
fn initialization_business_error_cleanup_failure_and_cancellation_are_all_retained() {
    let f = Fixture::new();
    let (control, signal) = original_control();
    let mut cleanups = 0;
    let error = initialize_unlocked_observed(
        &f.home,
        InitControl(Some(&control)),
        &mut |phase, _| {
            if phase == InitPhase::StageCreated {
                signal.cancel();
                return Err(ToolkitError::new(
                    "fixture.stage_failure",
                    "stage failure",
                    5,
                ));
            }
            Ok(())
        },
        |_| {
            cleanups += 1;
            Err(ToolkitError::new(
                "fixture.cleanup_failure",
                "cleanup failure",
                5,
            ))
        },
    )
    .unwrap_err();
    assert_eq!(cleanups, 1);
    assert_eq!(error.code, "fixture.stage_failure");
    assert!(error_contains(&error, "fixture.cleanup_failure"));
    assert!(error_contains(&error, "command.cancelled"));
    assert_eq!(error.details["initialization"]["published"], false);
    assert_eq!(error.details["initialization"]["stageRetained"], true);
    assert_eq!(f.stages().len(), 1);
    assert!(!f.home.join("workspace.json").exists());
}
#[test]
fn uncertain_manifest_publication_preserves_private_stage_and_original_atomic_evidence() {
    let f = Fixture::new();
    let (control, _) = original_control();
    let mut stage = None;
    let error = initialize_observed(&f.home, InitControl(Some(&control)), &mut |phase, path| {
        if phase == InitPhase::ManifestWriting {
            stage = Some(path.to_path_buf());
            // A real durable-replacement failure, after the temporary has
            // been written and closed. This is not a simulated close fault.
            fs::create_dir(path.join("workspace.json"))?;
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, "vault.publish");
    assert!(error_contains(
        &error,
        "lycheedev.atomic-temporary-retained.v1"
    ));
    assert_eq!(error.details["initialization"]["stageRetained"], true);
    let stage = stage.unwrap();
    assert!(stage.join("workspace.json").is_dir());
    assert!(fs::read_dir(&stage).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".lycheedev-")
    }));
    f.assert_lease_released();
    let identity = initialize(&f.home).unwrap();
    assert_eq!(open(&f.home).unwrap(), identity);
    assert!(
        stage.exists(),
        "later initialization must not adopt uncertain old stages"
    );
}
#[test]
fn terminal_failure_after_real_lease_close_preserves_published_identity_and_releases_lock() {
    let f = Fixture::new();
    let (control, _) = original_control();
    let mut closed = false;
    let error = initialize_observed(&f.home, InitControl(Some(&control)), &mut |phase, _| {
        if phase == InitPhase::LeaseClosed {
            // This observer runs after the actual checked close. It proves
            // terminal error composition, not a native close-fault result.
            f.assert_lease_released();
            closed = true;
            return Err(ToolkitError::new("fixture.terminal", "terminal failure", 5));
        }
        Ok(())
    })
    .unwrap_err();
    assert!(closed);
    assert_eq!(error.code, "fixture.terminal");
    assert_eq!(error.details["result"]["identity"], open(&f.home).unwrap());
    assert!(f.stages().is_empty());
    f.assert_lease_released();
}
