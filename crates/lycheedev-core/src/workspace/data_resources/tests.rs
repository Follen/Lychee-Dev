use super::*;
use crate::control::TrackedCancellation;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
fn home() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/data-admission-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    crate::workspace::initialize(&home).unwrap();
    home
}
fn short() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_millis(60))
}
fn configured(home: &Path, workers: usize) {
    let mut config = crate::workspace::default_config();
    config["download"]["workers"] = serde_json::json!(workers);
    storage::atomic_json(&home.join("config.json"), &config).unwrap();
}
#[test]
fn actual_kernel_slots_and_configured_per_query_width() {
    for (workers, width, slots) in [
        (0, 8, 2),
        (1, 1, 1),
        (2, 1, 2),
        (3, 1, 2),
        (4, 2, 2),
        (8, 4, 2),
        (256, 8, 2),
    ] {
        let home = home();
        configured(&home, workers);
        let control = OperationControl::default();
        let first = acquire(&home, Policy::Ordinary, &control).unwrap();
        assert_eq!(first.download_workers(), width);
        let second = if slots == 2 {
            Some(acquire(&home, Policy::Ordinary, &control).unwrap())
        } else {
            None
        };
        assert_eq!(
            acquire(&home, Policy::Ordinary, &short())
                .err()
                .unwrap()
                .code,
            "command.deadline"
        );
        first.close().unwrap();
        if let Some(second) = second {
            second.close().unwrap();
        }
        acquire(&home, Policy::Ordinary, &control)
            .unwrap()
            .close()
            .unwrap();
    }
}
#[test]
fn measurement_holds_both_slots_and_blocks_new_ordinary_without_metadata_lock() {
    let home = home();
    let control = OperationControl::default();
    let first = acquire(&home, Policy::Ordinary, &control).unwrap();
    let second = acquire(&home, Policy::Ordinary, &control).unwrap();
    assert_eq!(
        acquire(&home, Policy::Measurement, &short())
            .err()
            .unwrap()
            .code,
        "command.deadline"
    );
    first.close().unwrap();
    second.close().unwrap();
    let measurement = acquire(&home, Policy::Measurement, &control).unwrap();
    std::thread::scope(|scope| {
        let child = scope.spawn(|| acquire(&home, Policy::Ordinary, &short()));
        let mut metadata =
            crate::metadata::Metadata::open_controlled(&home, true, &control).unwrap();
        metadata
            .create_controlled(
                "admission-test",
                &serde_json::json!("metadata remains writable"),
                &control,
            )
            .unwrap();
        metadata.close().unwrap();
        assert_eq!(
            child.join().unwrap().err().unwrap().code,
            "command.deadline"
        );
    });
    measurement.close().unwrap();
}
#[test]
fn independent_workspaces_do_not_share_admission() {
    let a = home();
    let b = home();
    let control = OperationControl::default();
    let held = acquire(&a, Policy::Measurement, &control).unwrap();
    acquire(&b, Policy::Ordinary, &control)
        .unwrap()
        .close()
        .unwrap();
    held.close().unwrap();
}
#[test]
fn original_and_supplemental_stops_do_not_leave_admission_owned() {
    let home = home();
    let control = OperationControl::default();
    let holder = acquire(&home, Policy::Measurement, &control).unwrap();
    for independent in [false, true] {
        let token = TrackedCancellation::new();
        let cancellable =
            OperationControl::tracked(Instant::now() + Duration::from_secs(2), token.clone());
        let original = if independent {
            OperationControl::default()
        } else {
            cancellable.clone()
        };
        std::thread::scope(|scope| {
            let worker = scope.spawn(|| {
                acquire_with_check(&home, Policy::Ordinary, &original, || cancellable.check())
            });
            std::thread::sleep(Duration::from_millis(30));
            token.cancel();
            assert_eq!(
                worker.join().unwrap().err().unwrap().code,
                "command.cancelled"
            );
        });
        assert_eq!(original.check().is_ok(), independent);
    }
    holder.close().unwrap();
    acquire(&home, Policy::Measurement, &control)
        .unwrap()
        .close()
        .unwrap();
}
#[test]
fn consuming_close_releases_locks_even_after_original_cancellation() {
    let home = home();
    let control = OperationControl::default();
    let held = acquire(&home, Policy::Measurement, &control).unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(held.close().unwrap_err().code, "command.cancelled");
    acquire(&home, Policy::Measurement, &OperationControl::default())
        .unwrap()
        .close()
        .unwrap();
}
#[test]
fn compound_errors_preserve_primary_cleanup_and_original_typed_stop() {
    let original = ToolkitError::new("external.io", "original", 5)
        .with_details(serde_json::json!({"original":7}));
    let cleanup =
        ToolkitError::new("external.io", "close", 5).with_details(serde_json::json!({"close":11}));
    let stopped = ToolkitError::new("command.cancelled", "stop", 7);
    let error = finish::<()>(
        Err(original.clone()),
        [cleanup.clone()],
        Err(stopped.clone()),
        Err(stopped),
    )
    .unwrap_err();
    assert_eq!(error.code, original.code);
    assert_eq!(error.details["causes"].as_array().unwrap().len(), 3);
    assert_eq!(
        error.details["causes"][0],
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(
        error.details["causes"][1],
        serde_json::to_value(cleanup).unwrap()
    );
    assert!(!pure_busy(
        &ToolkitError::new("workspace.busy", "busy", 6)
            .with_details(serde_json::json!({"cleanup":"failed"}))
    ));
}
#[test]
fn equal_close_failures_preserve_each_event_but_repeated_stops_are_deduplicated() {
    let cleanup = ToolkitError::new("external.io", "identical close failure", 5)
        .with_details(serde_json::json!({"osError":6}));
    let stopped = ToolkitError::new("command.cancelled", "same observed stop", 7);
    let error = finish::<()>(
        Ok(()),
        [cleanup.clone(), cleanup.clone()],
        Err(stopped.clone()),
        Err(stopped.clone()),
    )
    .unwrap_err();
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 3);
    assert_eq!(causes[0], serde_json::to_value(&cleanup).unwrap());
    assert_eq!(causes[1], serde_json::to_value(&cleanup).unwrap());
    assert_eq!(causes[2], serde_json::to_value(&stopped).unwrap());
}
#[test]
fn config_file_is_bounded_and_missing_defaults_never_create_it() {
    let home = home();
    let control = OperationControl::default();
    assert_eq!(config::read(&home, &control, &mut || Ok(())).unwrap(), 0);
    assert!(!home.join("config.json").exists());
    fs::write(home.join("config.json"), vec![b' '; 65537]).unwrap();
    assert_eq!(
        acquire(&home, Policy::Ordinary, &control)
            .err()
            .unwrap()
            .code,
        "vault.config_format"
    );
    assert!(!home.join("locks/data-resources-v1").exists());
}

#[cfg(windows)]
struct ChildOwner(Option<std::process::Child>);
#[cfg(windows)]
impl ChildOwner {
    fn stop(&mut self) -> std::result::Result<(), String> {
        let Some(mut child) = self.0.take() else {
            return Ok(());
        };
        let kill = match child.try_wait() {
            Ok(Some(_)) => Ok(()),
            _ => child.kill(),
        };
        let wait = child.wait();
        match (kill, wait) {
            (Ok(()), Ok(_)) => Ok(()),
            (kill, wait) => Err(format!("child cleanup: kill={kill:?}, reap={wait:?}")),
        }
    }
}
#[cfg(windows)]
impl Drop for ChildOwner {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
#[cfg(windows)]
#[test]
fn cross_process_measurement_and_killed_owner_release_actual_kernel_locks() {
    let home = home();
    let marker = home.join("admission-helper-ready");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "workspace::data_resources::tests::data_resource_helper_process",
            "--nocapture",
        ])
        .env("LYCHEEDEV_DATA_ADMISSION_HELPER", &home)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let mut child = ChildOwner(Some(child));
    let until = Instant::now() + Duration::from_secs(5);
    while !marker.try_exists().unwrap() {
        assert!(Instant::now() < until, "helper readiness deadline");
        assert!(
            child.0.as_mut().unwrap().try_wait().unwrap().is_none(),
            "helper exited before owning lease"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read(marker).unwrap(), b"ready\n");
    assert_eq!(
        acquire(&home, Policy::Ordinary, &short())
            .err()
            .unwrap()
            .code,
        "command.deadline"
    );
    child.stop().unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(2));
    acquire(&home, Policy::Ordinary, &control)
        .unwrap()
        .close()
        .unwrap();
}
#[cfg(windows)]
#[test]
fn data_resource_helper_process() {
    let Some(home) = std::env::var_os("LYCHEEDEV_DATA_ADMISSION_HELPER") else {
        return;
    };
    let home = PathBuf::from(home);
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(20));
    let owner = acquire(&home, Policy::Measurement, &control).unwrap();
    fs::write(home.join("admission-helper-ready"), b"ready\n").unwrap();
    std::thread::sleep(Duration::from_secs(10));
    owner.close().unwrap();
}

#[test]
fn queued_measurement_keeps_admission_until_both_slots_are_released() {
    let home = home();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let first = acquire(&home, Policy::Ordinary, &control).unwrap();
    let second = acquire(&home, Policy::Ordinary, &control).unwrap();
    std::thread::scope(|scope| {
        let (events_tx, events_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let measure_tx = events_tx.clone();
        let measure_home = &home;
        let measure_control = &control;
        let measure = scope.spawn(move || {
            let held = acquire(measure_home, Policy::Measurement, measure_control)?;
            measure_tx.send("measurement").unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(2));
            held.close()
        });
        let until = Instant::now() + Duration::from_secs(1);
        loop {
            match ExclusiveLock::resource(&home.join("locks/data-resources-v1"), "admission") {
                Err(error) if pure_busy(&error) => break,
                Ok(lock) => lock.close().unwrap(),
                Err(error) => panic!("{error}"),
            }
            assert!(Instant::now() < until, "measurement did not acquire gate");
            std::thread::sleep(Duration::from_millis(5));
        }
        let ordinary_home = &home;
        let ordinary_control = &control;
        let ordinary = scope.spawn(move || {
            let held = acquire(ordinary_home, Policy::Ordinary, ordinary_control)?;
            events_tx.send("ordinary").unwrap();
            held.close()
        });
        first.close().unwrap();
        second.close().unwrap();
        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "measurement"
        );
        assert!(events_rx.try_recv().is_err());
        release_tx.send(()).unwrap();
        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(1)).unwrap(),
            "ordinary"
        );
        measure.join().unwrap().unwrap();
        ordinary.join().unwrap().unwrap();
    });
}
