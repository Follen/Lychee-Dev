use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-coordination-close-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn target(&self) -> PathBuf {
        self.0.join("Lychee Dev")
    }
    fn registry(&self) -> PathBuf {
        self.0.join(".lycheedev-claims.json")
    }
    fn assert_released(&self) {
        InstallationMaintenanceLease::acquire(&self.target())
            .unwrap()
            .close()
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn cleanup_error() -> ToolkitError {
    ToolkitError::new(
        "fixture.maintenance_close",
        "injected after actual close",
        5,
    )
    .with_details(serde_json::json!({"consumed":true}))
}

#[test]
fn failed_registry_parse_schema_and_read_close_maintenance_once() {
    let excessive = serde_json::to_string(&ClaimRegistry {
        schema: "lycheedev.claims.v1".into(),
        claims: vec![
            Claim {
                owner: "fixture-owner".into(),
                kind: "native-live".into(),
                resource: "fixture-resource".into(),
                process_id: 123,
                created_at: "fixture".into(),
                lifecycle_ref: "fixture-lifecycle".into(),
                pending: false,
            };
            1025
        ],
    })
    .unwrap();
    for (raw, expected) in [
        (Some("{"), "input.invalid"),
        (
            Some(r#"{"schema":"unknown","claims":[]}"#),
            "coordination.claim_format",
        ),
        (Some(excessive.as_str()), "coordination.claim_format"),
        (None, "external.io"),
    ] {
        let fixture = Fixture::new();
        if let Some(raw) = raw {
            fs::write(fixture.registry(), raw).unwrap();
        } else {
            fs::create_dir(fixture.registry()).unwrap();
        }
        let mut closes = 0;
        let error = InstallationGuard::acquire_with(
            &fixture.target(),
            &OperationControl::default(),
            |_| {},
            |lease| {
                closes += 1;
                lease.close()?;
                Err(cleanup_error())
            },
        )
        .err()
        .unwrap();
        assert_eq!(closes, 1);
        assert_eq!(error.code, expected);
        assert_eq!(error.details["schema"], "lycheedev.coordination-cleanup.v1");
        assert_eq!(error.details["primary"]["code"], expected);
        assert_eq!(error.details["primary"]["message"], error.message);
        assert_eq!(error.details["primary"]["exit_code"], error.exit_code);
        assert!(error.details["primary"]["details"].is_null());
        assert_eq!(
            error.details["cleanup"]["code"],
            "fixture.maintenance_close"
        );
        assert_eq!(error.details["cleanup"]["details"]["consumed"], true);
        fixture.assert_released();
    }
}

#[cfg(windows)]
#[test]
fn rejected_registry_redirect_explicitly_closes_acquired_maintenance() {
    use std::os::windows::fs::MetadataExt;

    let foreign_fixture = Fixture::new();
    let fixture = Fixture::new();
    let foreign = foreign_fixture.0.join("foreign.json");
    let raw = br#"{"schema":"lycheedev.claims.v1","claims":[]}"#;
    fs::write(&foreign, raw).unwrap();
    // Match the existing Windows junction fixture in tests/rust-parity/oracle.test.mjs.
    // A directory reparse point at the registry leaf must be rejected before any read;
    // unlike a file symlink, creating this fixture does not require symlink privilege.
    let junction = std::process::Command::new("node")
        .args([
            "-e",
            "require('node:fs').symlinkSync(process.argv[1], process.argv[2], 'junction');",
        ])
        .arg(&foreign_fixture.0)
        .arg(fixture.registry())
        .output()
        .expect("create Windows registry junction fixture with Node");
    assert!(
        junction.status.success(),
        "junction fixture failed: {}",
        String::from_utf8_lossy(&junction.stderr)
    );
    assert_ne!(
        fs::symlink_metadata(fixture.registry())
            .unwrap()
            .file_attributes()
            & 0x400,
        0
    );
    assert_eq!(
        fs::read(fixture.registry().join("foreign.json")).unwrap(),
        raw
    );
    let mut acquisitions = 0;
    let mut closes = 0;
    let error = InstallationGuard::acquire_with(
        &fixture.target(),
        &OperationControl::default(),
        |step| {
            assert_eq!(step, AcquireStep::Maintenance);
            acquisitions += 1;
            let busy = InstallationMaintenanceLease::acquire(&fixture.target())
                .err()
                .unwrap();
            assert_eq!(busy.code, "workspace.busy");
        },
        |lease| {
            closes += 1;
            lease.close()
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "input.invalid");
    assert_eq!(error.message, "reparse point in managed path");
    assert_eq!(acquisitions, 1);
    assert_eq!(closes, 1);
    assert!(error.details.is_null());
    fixture.assert_released();
    assert_eq!(fs::read(&foreign).unwrap(), raw);
    assert_eq!(fs::read_dir(&foreign_fixture.0).unwrap().count(), 1);
    fs::remove_dir(fixture.registry()).unwrap(); // Remove only the junction.
    assert_eq!(fs::read(&foreign).unwrap(), raw);
}

#[test]
fn cancelled_acquisition_before_maintenance_creates_no_lock_directory() {
    let fixture = Fixture::new();
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    let error = InstallationGuard::acquire_controlled(&fixture.target(), &control)
        .err()
        .unwrap();
    assert_eq!(error.code, "command.cancelled");
    assert!(!fixture.0.join(".lycheedev-locks").exists());
}

#[test]
fn cancellation_after_maintenance_and_during_registry_read_preserves_stop_and_closes() {
    for stop_at in [AcquireStep::Maintenance, AcquireStep::RegistryRead] {
        let fixture = Fixture::new();
        let raw = format!(
            "{}{}",
            r#"{"schema":"lycheedev.claims.v1","claims":[]}"#,
            " ".repeat(128 << 10)
        );
        fs::write(fixture.registry(), &raw).unwrap();
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let mut reads = 0;
        let mut closes = 0;
        let error = InstallationGuard::acquire_with(
            &fixture.target(),
            &control,
            |step| {
                if step == AcquireStep::RegistryRead {
                    reads += 1;
                }
                if step == stop_at {
                    cancel.cancel();
                }
            },
            |lease| {
                closes += 1;
                lease.close()?;
                Err(cleanup_error())
            },
        )
        .err()
        .unwrap();
        assert_eq!(reads, usize::from(stop_at == AcquireStep::RegistryRead));
        assert_eq!(closes, 1);
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
        assert_eq!(error.details["primary"]["code"], "command.cancelled");
        assert_eq!(
            error.details["cleanup"]["code"],
            "fixture.maintenance_close"
        );
        fixture.assert_released();
        // The original file handle must also have been consumed before retry.
        let moved = fixture.0.join("retained-claims.json");
        fs::rename(fixture.registry(), &moved).unwrap();
        assert_eq!(fs::read_to_string(moved).unwrap(), raw);
    }
}

#[test]
fn successful_guard_acquisition_transfers_same_maintenance_until_consuming_close() {
    let fixture = Fixture::new();
    let guard = InstallationGuard::acquire_with(
        &fixture.target(),
        &OperationControl::default(),
        |_| {},
        |_| panic!("failed-acquisition close must not consume a successful owner"),
    )
    .unwrap();
    let error = InstallationMaintenanceLease::acquire(&fixture.target())
        .err()
        .unwrap();
    assert_eq!(error.code, "workspace.busy");
    guard.require_idle().unwrap();
    guard.close().unwrap();
    fixture.assert_released();
}
