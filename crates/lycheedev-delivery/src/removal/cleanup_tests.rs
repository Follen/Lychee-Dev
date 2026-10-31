use super::*;
use lycheedev_core::coordination::InstallationMaintenanceLease;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    target: PathBuf,
    archive: PathBuf,
}
impl Fixture {
    fn new(with_slots: bool) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-removal-close-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let parent = root.join("AddOns");
        let target = parent.join("Lychee Dev");
        fs::create_dir_all(&target).unwrap();
        fs::write(target.join("owned.lua"), b"-- fixture").unwrap();
        storage::atomic_json(
            &target.join(installation::MARKER),
            &installation::Receipt {
                schema: "lycheedev.installation.v1".into(),
                component: "addon".into(),
                version: "3.2.0".into(),
                commit: "a".repeat(40),
                resources: vec![installation::Resource {
                    path: "addon/owned.lua".into(),
                    bytes: 10,
                    sha256: storage::sha256(b"-- fixture"),
                }],
            },
        )
        .unwrap();
        if with_slots {
            let control = OperationControl::default();
            let phase = crate::slot_test_diagnostics::SlotPhase::begin(
                "removal.cleanup_fixture_install",
                Some(&control),
            );
            let result = slots::install_controlled(&parent, "3.2.0", &control);
            phase.finish(result.as_ref().err());
            result.unwrap();
        }
        Self {
            archive: root.join("archive"),
            root,
            target,
        }
    }
    fn assert_released(&self) {
        InstallationMaintenanceLease::acquire(&self.target)
            .unwrap()
            .close()
            .unwrap();
        deploy::installation_lock_controlled(&self.target, &Default::default())
            .unwrap()
            .close()
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn move_member(source: &Path, destination: &Path) -> Result<()> {
    storage::durable_move_noreplace(source, destination).map_err(ToolkitError::io)
}
fn close_error(which: &str) -> ToolkitError {
    ToolkitError::new(
        format!("fixture.{which}_close"),
        "injected after actual close",
        5,
    )
    .with_details(json!({"consumed":true}))
}

#[test]
fn installation_busy_failure_closes_maintenance_and_keeps_competing_lock() {
    let fixture = Fixture::new(false);
    let competitor = deploy::installation_lock_controlled(&fixture.target, &Default::default()).unwrap();
    let mut closes = 0;
    let error = addon_with_cleanup(
        &fixture.target,
        &fixture.archive,
        &OperationControl::default(),
        |_, _| panic!("busy target must not move"),
        |_| panic!("lock not acquired"),
        |_| panic!("failed lock acquisition must not fabricate an owner"),
        |guard| {
            closes += 1;
            guard.close()?;
            Err(close_error("maintenance"))
        },
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "workspace.busy");
    assert_eq!(error.details["primary"]["code"], "workspace.busy");
    assert_eq!(
        error.details["cleanup"]["code"],
        "fixture.maintenance_close"
    );
    InstallationMaintenanceLease::acquire(&fixture.target)
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(
        deploy::installation_lock_controlled(&fixture.target, &Default::default())
            .err()
            .unwrap()
            .code,
        "workspace.busy"
    );
    assert!(fixture.target.exists());
    assert!(!fixture.archive.exists());
    competitor.close().unwrap();
    fixture.assert_released();
}

#[test]
fn cancellation_after_lock_or_real_inventory_closes_both_without_moving() {
    for stop_at in [RemovalStep::Locked, RemovalStep::Validated] {
        let fixture = Fixture::new(false);
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let mut lock_closes = 0;
        let mut guard_closes = 0;
        let error = addon_with_cleanup(
            &fixture.target,
            &fixture.archive,
            &control,
            |_, _| panic!("cancellation must prevent move"),
            |step| {
                if step == stop_at {
                    cancel.cancel();
                }
            },
            |lock| {
                lock_closes += 1;
                lock.close()
            },
            |guard| {
                guard_closes += 1;
                guard.close()
            },
        )
        .unwrap_err();
        assert_eq!((lock_closes, guard_closes), (1, 1));
        assert_eq!(error.code, "command.cancelled");
        assert!(error.details.is_null());
        assert_eq!(
            fs::read(fixture.target.join("owned.lua")).unwrap(),
            b"-- fixture"
        );
        assert!(!fixture.archive.exists());
        assert!(!companion(&fixture.archive, ".removal.json").exists());
        fixture.assert_released();
        assert_eq!(
            addon_controlled(&fixture.target, &fixture.archive, &Default::default()).unwrap()["state"],
            "archived"
        );
    }
}

#[test]
fn committed_main_removal_survives_late_cancel_and_both_close_failures() {
    let fixture = Fixture::new(false);
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut lock_closes = 0;
    let mut guard_closes = 0;
    let error = addon_with_cleanup(
        &fixture.target,
        &fixture.archive,
        &control,
        |source, destination| {
            move_member(source, destination)?;
            cancel.cancel();
            Ok(())
        },
        |_| {},
        |lock| {
            lock_closes += 1;
            lock.close()?;
            Err(close_error("installation"))
        },
        |guard| {
            guard_closes += 1;
            guard.close()?;
            Err(close_error("maintenance"))
        },
    )
    .unwrap_err();
    assert_eq!((lock_closes, guard_closes), (1, 1));
    assert_eq!(error.code, "fixture.installation_close");
    let committed = &error.details["primary"]["details"];
    assert_eq!(committed["operationCompleted"], true);
    assert_eq!(committed["result"]["state"], "archived");
    assert_eq!(committed["result"]["receipt"]["commit"], "a".repeat(40));
    assert_eq!(committed["cleanup"]["code"], "fixture.installation_close");
    assert_eq!(
        error.details["cleanup"]["code"],
        "fixture.maintenance_close"
    );
    assert!(!fixture.target.exists());
    assert_eq!(
        fs::read(fixture.archive.join("owned.lua")).unwrap(),
        b"-- fixture"
    );
    fixture.assert_released();
}

#[test]
fn partial_slot_removal_cancel_and_close_failures_keep_exact_intent_for_resume() {
    let fixture = Fixture::new(true);
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut moves = 0;
    let mut lock_closes = 0;
    let mut guard_closes = 0;
    let error = addon_with_cleanup(
        &fixture.target,
        &fixture.archive,
        &control,
        |source, destination| {
            move_member(source, destination)?;
            moves += 1;
            cancel.cancel();
            Ok(())
        },
        |_| {},
        |lock| {
            lock_closes += 1;
            lock.close()?;
            Err(close_error("installation"))
        },
        |guard| {
            guard_closes += 1;
            guard.close()?;
            Err(close_error("maintenance"))
        },
    )
    .unwrap_err();
    assert_eq!(moves, 1);
    assert_eq!((lock_closes, guard_closes), (1, 1));
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["primary"]["details"]["primary"]["code"],
        "command.cancelled"
    );
    assert_eq!(
        error.details["primary"]["details"]["cleanup"]["code"],
        "fixture.installation_close"
    );
    assert_eq!(
        error.details["cleanup"]["code"],
        "fixture.maintenance_close"
    );
    let journal = companion(&fixture.archive, ".removal.json");
    let intent = fs::read(&journal).unwrap();
    let archived_slots = companion(&fixture.archive, ".slots");
    assert!(slots::directory(&archived_slots, 1).exists());
    assert!(slots::directory(fixture.target.parent().unwrap(), 2).exists());
    assert!(fixture.target.exists());
    assert!(!fixture.archive.exists());
    fixture.assert_released();
    assert_eq!(
        addon_controlled(&fixture.target, &fixture.archive, &Default::default()).unwrap()["state"],
        "archived"
    );
    assert_eq!(fs::read(journal).unwrap(), intent);
    assert!(slots::directory(&archived_slots, 500).exists());
    assert!(!fixture.target.exists());
}

#[test]
fn completed_slot_removal_preserves_success_when_cancel_arrives_after_final_move() {
    let fixture = Fixture::new(true);
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut moves = 0;
    let mut lock_closes = 0;
    let mut guard_closes = 0;
    let result = addon_with_cleanup(
        &fixture.target,
        &fixture.archive,
        &control,
        |source, destination| {
            move_member(source, destination)?;
            moves += 1;
            if source == fixture.target {
                cancel.cancel();
            }
            Ok(())
        },
        |_| {},
        |lock| {
            lock_closes += 1;
            lock.close()
        },
        |guard| {
            guard_closes += 1;
            guard.close()
        },
    )
    .unwrap();
    assert_eq!(moves, 502);
    assert_eq!((lock_closes, guard_closes), (1, 1));
    assert_eq!(result["state"], "archived");
    assert_eq!(result["receipt"]["commit"], "a".repeat(40));
    assert!(control.check().is_err());
    assert!(!fixture.target.exists());
    assert!(fixture.archive.exists());
    assert!(slots::directory(&companion(&fixture.archive, ".slots"), 500).exists());
    fixture.assert_released();
}

#[test]
fn real_move_failure_preserves_primary_details_foreign_archive_and_both_close_errors() {
    let fixture = Fixture::new(false);
    let mut lock_closes = 0;
    let mut guard_closes = 0;
    let error = addon_with_cleanup(
        &fixture.target,
        &fixture.archive,
        &OperationControl::default(),
        |source, destination| {
            fs::create_dir(destination)?;
            fs::write(destination.join("foreign"), b"do not overwrite")?;
            move_member(source, destination).map_err(|error| {
                error.with_details(json!({"failedMove":"main","originalSource":source}))
            })
        },
        |_| {},
        |lock| {
            lock_closes += 1;
            lock.close()?;
            Err(close_error("installation"))
        },
        |guard| {
            guard_closes += 1;
            guard.close()?;
            Err(close_error("maintenance"))
        },
    )
    .unwrap_err();
    assert_eq!((lock_closes, guard_closes), (1, 1));
    assert_eq!(error.code, "external.io");
    assert_eq!(error.exit_code, 5);
    let original = &error.details["primary"]["details"]["primary"];
    assert_eq!(original["code"], "external.io");
    assert_eq!(original["message"], error.message);
    assert_eq!(original["details"]["failedMove"], "main");
    assert_eq!(
        error.details["primary"]["details"]["cleanup"]["code"],
        "fixture.installation_close"
    );
    assert_eq!(
        error.details["cleanup"]["code"],
        "fixture.maintenance_close"
    );
    assert_eq!(
        fs::read(fixture.target.join("owned.lua")).unwrap(),
        b"-- fixture"
    );
    assert_eq!(
        fs::read(fixture.archive.join("foreign")).unwrap(),
        b"do not overwrite"
    );
    assert!(!fixture.archive.join("owned.lua").exists());
    fixture.assert_released();
}
