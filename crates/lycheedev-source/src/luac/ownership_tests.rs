use super::super::*;

fn fixture(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "lycheedev-owned-luac-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    root
}
fn runtime(root: &Path) -> Runtime {
    // Authenticated bytes deliberately cannot be executed as a program.
    fs::write(root.join("never.exe"), b"not executed").unwrap();
    fs::write(root.join("LICENSE"), b"license").unwrap();
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
fn output(status: i32) -> process::Output {
    process::Output {
        status,
        stdout: vec![],
        stderr: vec![],
    }
}
#[test]
fn ownership_empty_compile_cleans_only_its_scratch_and_releases_runtime() {
    let root = fixture("empty");
    let runtime = runtime(&root);
    let staging = root.join("staging");
    fs::create_dir(&staging).unwrap();
    fs::write(staging.join("other-owner"), b"keep").unwrap();
    let checked = runtime.check_with_control(&staging, &BTreeMap::new(), &Default::default()).unwrap();
    assert_eq!(checked["checkedLua"], 0);
    assert_eq!(checked["complete"], true);
    assert_eq!(fs::read_dir(&staging).unwrap().count(), 1);
    assert_eq!(fs::read(staging.join("other-owner")).unwrap(), b"keep");
    // On Windows this fails if a deny-delete runtime handle survived return.
    fs::rename(root.join("never.exe"), root.join("renamed.exe")).unwrap();
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ownership_entered_spawn_failure_is_conservatively_retained_and_mapped() {
    let root = fixture("spawn");
    let runtime = runtime(&root);
    let staging = root.join("staging");
    let error = runtime
        .check_with_control(
            &staging,
            &BTreeMap::from([("a.lua".into(), b"local value = 1".to_vec())]), &Default::default(),
        )
        .unwrap_err();
    assert_eq!(error.code, "luac.unavailable");
    assert_eq!(error.exit_code, 3);
    assert!(requires_scratch_retention(&error));
    let retained = PathBuf::from(error.details["retainedScratch"].as_str().unwrap());
    assert_eq!(retained.parent(), Some(staging.as_path()));
    assert!(retained.join("retained-owner.json").is_file());
    assert_eq!(error.details["cause"]["code"], "luac.unavailable");
    assert_eq!(error.details["cause"]["message"], error.message);
    let record: Value =
        serde_json::from_slice(&fs::read(retained.join("retained-owner.json")).unwrap()).unwrap();
    assert_eq!(record, serde_json::to_value(&error).unwrap());
    fs::rename(root.join("never.exe"), root.join("renamed.exe")).unwrap();
    // This fixed non-executable fixture cannot have started a child. The
    // product intentionally retains even this error rather than infer proof.
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ownership_runner_errors_keep_primary_and_cannot_claim_terminal_proof() {
    for (code, message, exit_code) in [
        ("process.cancelled", "cancelled", 7),
        ("luac.check_failed", "deadline", 5),
        ("luac.unavailable", "attach failed", 3),
        ("external.io", "pipe failed", 5),
    ] {
        let root = fixture("error");
        fs::write(root.join("evidence"), b"keep").unwrap();
        let mut frozen = Frozen(root.clone(), true);
        // Even all-true diagnostic flags are not a successful runner return.
        let original = ToolkitError::new(code, message, exit_code).with_details(json!({
            "terminated": true,
            "reaped": true,
            "jobEmpty": true,
            "stdinWriterComplete": true,
            "outputReadersComplete": true,
            "complete": true
        }));
        let result = run_with_frozen(&mut frozen, || Err(original.clone()));
        let error = frozen.finish(result).unwrap_err();
        assert_eq!(error.code, code);
        assert_eq!(error.message, message);
        assert_eq!(error.exit_code, exit_code);
        assert_eq!(
            error.details["cause"],
            serde_json::to_value(&original).unwrap()
        );
        assert!(requires_scratch_retention(&error));
        assert_eq!(fs::read(root.join("evidence")).unwrap(), b"keep");
        assert!(root.join("retained-owner.json").is_file());
        // These modeled runner returns create no process or worker.
        fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn ownership_retention_record_failure_keeps_primary_marker_and_evidence() {
    let root = fixture("record-failure");
    // A directory at the record filename forces atomic publication to fail.
    fs::create_dir(root.join("retained-owner.json")).unwrap();
    fs::write(root.join("evidence"), b"keep").unwrap();
    let mut frozen = Frozen(root.clone(), true);
    let result = run_with_frozen(&mut frozen, || {
        Err(ToolkitError::new("process.cancelled", "cancelled", 7))
    });
    let error = frozen.finish(result).unwrap_err();
    assert_eq!(error.code, "process.cancelled");
    assert_eq!(error.exit_code, 7);
    assert_eq!(error.details["schema"], storage::FILE_CLEANUP_SCHEMA);
    assert!(requires_scratch_retention(&error));
    assert!(!error.details["cleanup"].is_null());
    assert_eq!(fs::read(root.join("evidence")).unwrap(), b"keep");
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ownership_successful_runner_outputs_allow_cleanup_for_both_exit_statuses() {
    for status in [0, 1] {
        let root = fixture("success");
        let mut frozen = Frozen(root.clone(), true);
        let result = run_with_frozen(&mut frozen, || Ok(output(status)));
        let actual = frozen.finish(result).unwrap();
        assert_eq!(actual.status, status);
        assert!(!root.exists());
    }
}
#[test]
fn ownership_later_runner_failure_does_not_reuse_an_earlier_success() {
    let root = fixture("later-failure");
    let mut frozen = Frozen(root.clone(), true);
    run_with_frozen(&mut frozen, || Ok(output(0))).unwrap();
    let result = run_with_frozen(&mut frozen, || {
        Err(ToolkitError::new("process.cancelled", "later runner", 7))
    });
    let error = frozen.finish(result).unwrap_err();
    assert!(requires_scratch_retention(&error));
    assert!(root.is_dir());
    fs::remove_dir_all(root).unwrap();
}
#[test]
fn ownership_pre_run_or_post_success_error_preserves_primary_and_cleanup() {
    for started in [false, true] {
        let root = fixture("safe-error");
        let mut frozen = Frozen(root.clone(), true);
        if started {
            run_with_frozen(&mut frozen, || Ok(output(0))).unwrap();
        }
        let original = ToolkitError::new("luac.input_budget", "preflight", 3)
            .with_details(json!({"original": "unchanged"}));
        let error = frozen
            .finish::<()>(Err(original.clone()))
            .unwrap_err();
        assert_eq!(
            serde_json::to_value(&error).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        assert!(!requires_scratch_retention(&error));
        assert!(!root.exists());
    }
}
#[test]
fn ownership_unwind_retains_only_after_runner_entry() {
    for entered in [false, true] {
        let root = fixture("unwind");
        fs::write(root.join("evidence"), b"keep").unwrap();
        let caught = std::panic::catch_unwind(|| {
            let mut frozen = Frozen(root.clone(), true);
            if entered {
                let _ = run_with_frozen(&mut frozen, || panic!("runner unwind"));
            } else {
                panic!("setup unwind");
            }
        });
        assert!(caught.is_err());
        assert_eq!(root.exists(), entered);
        if entered {
            assert_eq!(fs::read(root.join("evidence")).unwrap(), b"keep");
            // Unwind preservation does not pretend that ordinary finalization
            // ran or that a retention receipt was successfully published.
            assert!(!root.join("retained-owner.json").exists());
            fs::remove_dir_all(root).unwrap();
        }
    }
}
