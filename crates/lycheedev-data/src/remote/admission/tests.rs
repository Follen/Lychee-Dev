use super::*;
use crate::{fault, resource::Cost};
use lycheedev_core::{control::TrackedCancellation, storage};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
fn home() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/remote-admitted-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    lycheedev_core::workspace::initialize(&home).unwrap();
    home
}
#[test]
fn admitted_workspace_holds_root_and_real_slots_without_preparing_metadata_or_cdn() {
    let home = home();
    let control = OperationControl::default();
    let owner = AdmittedWorkspace::acquire(&home, Policy::Ordinary, &control, None).unwrap();
    assert_eq!(owner.home(), std::path::absolute(&home).unwrap());
    assert_eq!(owner.download_workers(), 8);
    assert!(!home.join("state/toolkit.sqlite").exists());
    fs::write(home.join("own-metadata-update"), b"allowed").unwrap();
    let moved = home.with_extension("moved");
    assert!(fs::rename(&home, &moved).is_err());
    owner.finish(Ok(())).unwrap();
    fs::rename(&home, &moved).unwrap();
    fs::rename(&moved, &home).unwrap();
    AdmittedWorkspace::acquire(&home, Policy::Measurement, &control, None)
        .unwrap()
        .finish(Ok(()))
        .unwrap();
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn failed_admission_and_independent_budget_stop_release_root_and_slots() {
    let home = home();
    let control = OperationControl::default();
    let token = TrackedCancellation::new();
    let independent =
        OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    let budget = Budget::controlled(Cost::defaults(), independent.deadline(), independent);
    let owner =
        AdmittedWorkspace::acquire(&home, Policy::Ordinary, &control, Some(&budget)).unwrap();
    token.cancel();
    assert_eq!(owner.check().unwrap_err().code, "command.cancelled");
    assert_eq!(owner.finish(Ok(())).unwrap_err().code, "command.cancelled");
    let moved = home.with_extension("moved");
    fs::rename(&home, &moved).unwrap();
    fs::rename(&moved, &home).unwrap();
    AdmittedWorkspace::acquire(&home, Policy::Measurement, &control, None)
        .unwrap()
        .finish(Ok(()))
        .unwrap();
    fs::write(home.join("config.json"), b"invalid json").unwrap();
    assert!(AdmittedWorkspace::acquire(&home, Policy::Ordinary, &control, None).is_err());
    fs::rename(&home, &moved).unwrap();
    fs::rename(&moved, &home).unwrap();
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn admitted_finish_preserves_each_cleanup_event_and_deduplicates_stop_observations() {
    let primary = fault("test.operation", 5, "operation");
    let close = fault("test.close", 5, "close");
    let stop = fault("command.cancelled", 7, "cancelled");
    let error = finish::<()>(
        Err(primary),
        [close.clone(), close],
        Err(stop.clone()),
        Err(stop),
    )
    .unwrap_err();
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 4);
    assert_eq!(causes[1], causes[2]);
    assert_eq!(causes[3]["code"], "command.cancelled");
}
