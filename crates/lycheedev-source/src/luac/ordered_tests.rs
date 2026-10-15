use super::*;
use lycheedev_core::control::{OperationControl, StopCause, TrackedCancellation};
fn runtime(root: &Path) -> Runtime {
    Runtime {
        root: root.into(),
        manifest: Manifest {
            schema: "lycheedev.luac-runtime.v1".into(),
            version: "5.1.5".into(),
            platform: "windows-amd64".into(),
            executable: "never.exe".into(),
            sha256: storage::sha256(b"not executed"),
            bytes: 12,
            license: "LICENSE".into(),
            license_sha256: storage::sha256(b"license"),
        },
    }
}
#[test]
fn ordered_cancel_before_io_retains_caller_lineage() {
    let cancellation = TrackedCancellation::new();
    let control = OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        cancellation.clone(),
    );
    cancellation.cancel();
    let error = runtime(Path::new("nonexistent-owned-fixture"))
        .check_with_ordered_control(Path::new("must-not-exist"), &BTreeMap::new(), &control)
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
}
#[test]
fn ordered_expired_short_parent_wins_over_later_cancel() {
    let cancellation = TrackedCancellation::new();
    let parent = OperationControl::tracked(
        Instant::now() - Duration::from_millis(1),
        cancellation.clone(),
    );
    cancellation.cancel();
    let error = runtime(Path::new("nonexistent-owned-fixture"))
        .check_with_ordered_control(Path::new("must-not-exist"), &BTreeMap::new(), &parent)
        .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert_eq!(
        parent.stop_cause().unwrap(),
        Some(StopCause::DeadlineExceeded)
    );
}
#[test]
fn ordered_empty_compile_checks_closes_and_keeps_parent_ordered() {
    let root = std::env::temp_dir().join(format!(
        "lycheedev-ordered-luac-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("never.exe"), b"not executed").unwrap();
    fs::write(root.join("LICENSE"), b"license").unwrap();
    let control = OperationControl::default();
    let checked = runtime(&root)
        .check_with_ordered_control(&root.join("staging"), &BTreeMap::new(), &control)
        .unwrap();
    assert_eq!(checked["checkedLua"], 0);
    assert_eq!(control.stop_cause().unwrap(), None);
    assert_eq!(fs::read_dir(root.join("staging")).unwrap().count(), 0);
    // On Windows this fails if a deny-delete runtime handle survived return.
    fs::rename(root.join("never.exe"), root.join("renamed.exe")).unwrap();
    fs::remove_dir_all(&root).unwrap();
}
#[test]
fn ordered_identity_failure_releases_already_acquired_runtime() {
    let root = std::env::temp_dir().join(format!(
        "lycheedev-ordered-luac-failure-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    fs::write(root.join("never.exe"), b"not executed").unwrap();
    fs::write(root.join("LICENSE"), b"changed").unwrap();
    let error = runtime(&root)
        .check_with_ordered_control(
            &root.join("staging"),
            &BTreeMap::new(),
            &OperationControl::default(),
        )
        .unwrap_err();
    assert_eq!(error.code, "luac.unavailable");
    assert!(!root.join("staging").exists());
    fs::rename(root.join("never.exe"), root.join("renamed.exe")).unwrap();
    fs::remove_dir_all(&root).unwrap();
}
fn retained_fixture(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lycheedev-luac-retention-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    root
}
#[test]
fn ordered_runner_unknown_lifetime_retains_owned_scratch_and_original_error() {
    // A modeled runner return creates no process; deleting this test fixture
    // after assertions is therefore independent of product recovery policy.
    let root = retained_fixture("unknown");
    fs::write(root.join("evidence"), b"keep").unwrap();
    let mut frozen = Frozen(root.clone(), true);
    let error = run_with_frozen(&mut frozen, || {
        Err(ToolkitError::new(
            "luac.unavailable",
            "ambiguous spawn or job-attach failure",
            3,
        ))
    })
    .unwrap_err();
    let error = frozen.finish::<()>(Err(error)).unwrap_err();
    assert_eq!(error.code, "luac.unavailable");
    assert_eq!(error.exit_code, 3);
    assert!(requires_scratch_retention(&error));
    assert_eq!(fs::read(root.join("evidence")).unwrap(), b"keep");
    assert!(root.join("retained-owner.json").is_file());
    assert_eq!(
        error.details["cause"]["message"],
        "ambiguous spawn or job-attach failure"
    );
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ordered_runner_partial_pipe_or_reap_proof_never_allows_cleanup() {
    for missing in [
        "terminated",
        "reaped",
        "stdinWriterComplete",
        "outputReadersComplete",
    ] {
        let root = retained_fixture(missing);
        let mut frozen = Frozen(root.clone(), true);
        let mut proof = json!({"terminated":true,"reaped":true,"stdinWriterComplete":true,"outputReadersComplete":true});
        proof[missing] = json!(false);
        let error = run_with_frozen(&mut frozen, || {
            Err(ToolkitError::new("process.cancelled", "cancelled", 7).with_details(proof.clone()))
        })
        .unwrap_err();
        let error = frozen.finish::<()>(Err(error)).unwrap_err();
        assert!(requires_scratch_retention(&error));
        assert_eq!(error.exit_code, 7);
        assert_eq!(error.details["cause"]["details"], proof);
        assert!(root.is_dir());
        fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn ordered_no_runner_can_cleanup_but_error_metadata_is_not_terminal_proof() {
    let root = retained_fixture("not-started");
    let error = Frozen(root.clone(), true)
        .finish::<()>(Err(ToolkitError::new(
            "luac.input_budget",
            "preflight rejected",
            3,
        )))
        .unwrap_err();
    assert!(!root.exists());
    assert!(!requires_scratch_retention(&error));
    let root = retained_fixture("terminal");
    let mut frozen = Frozen(root.clone(), true);
    let error=run_with_frozen(&mut frozen,||Err(ToolkitError::new("process.cancelled","cancelled",7).with_details(json!({"terminated":true,"reaped":true,"stdinWriterComplete":true,"outputReadersComplete":true})))).unwrap_err();
    let error = frozen.finish::<()>(Err(error)).unwrap_err();
    assert_eq!(error.exit_code, 7);
    assert!(root.exists());
    assert!(requires_scratch_retention(&error));
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ordered_retention_marker_survives_checked_close_cause_tree() {
    let original = retain_scratch(
        ToolkitError::new("process.cancelled", "cancelled", 7),
        Path::new("owned/compiler"),
    );
    let mut combined = original.clone();
    combined.details = json!({"schema":storage::FILE_CLEANUP_SCHEMA,"primary":original,"cleanup":ToolkitError::io(std::io::Error::other("close failed")),"control":null});
    assert!(requires_scratch_retention(&combined));
    assert_eq!(combined.exit_code, 7);
    assert_eq!(combined.details["cleanup"]["message"], "close failed");
}

// Keep the shared ownership regression family under the existing CI selector.
#[path = "legacy_tests.rs"]
mod legacy_tests;
