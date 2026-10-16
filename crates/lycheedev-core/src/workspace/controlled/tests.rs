use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
fn home() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "lychee-workspace-controlled-{}-{}-{}",
        std::process::id(),
        storage::utc_now().replace([':', '.'], "_"),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    initialize(&path).unwrap();
    path
}
#[test]
fn controlled_marker_preserves_existing_wire_and_original_ordered_scope() {
    let home = home();
    let control = OperationControl::default();
    let expected = open(&home).unwrap();
    assert_eq!(open_controlled(&home, &control).unwrap(), expected);
    let raw = fs::read(home.join("workspace.json")).unwrap();
    let mut value: String = String::from_utf8(raw).unwrap();
    value.pop();
    value.push_str(&format!(",\"ignored\":\"{}\"}}", "a".repeat(2 * 65536 + 3)));
    fs::write(home.join("workspace.json"), value).unwrap();
    assert_eq!(open_controlled(&home, &control).unwrap(), expected);
    control.stop_cause().unwrap();
    control.ordered_child(control.deadline()).unwrap();
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn independent_stop_in_marker_stream_remains_typed_and_closes_owner() {
    let home = home();
    let control = OperationControl::default();
    let mut raw = fs::read(home.join("workspace.json")).unwrap();
    raw.pop();
    raw.extend_from_slice(format!(",\"ignored\":\"{}\"}}", "a".repeat(4 * 65536)).as_bytes());
    let path = home.join("workspace.json");
    fs::write(&path, &raw).unwrap();
    let mut calls = 0;
    // Entry + alternating pre/post read checkpoints: call eight follows the
    // third delivered 64 KiB chunk, before parsing the completed document.
    let error = open_controlled_with_check(&home, &control, || {
        calls += 1;
        if calls == 8 {
            Err(ToolkitError::new(
                "records.query_resource_budget",
                "independent body stop",
                3,
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(control.check().is_ok());
    assert_eq!(fs::read(&path).unwrap(), raw);
    fs::remove_file(path).unwrap();
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn callback_input_invalid_is_not_misclassified_as_bad_workspace() {
    let home = home();
    let control = OperationControl::default();
    let mut calls = 0;
    let error = open_controlled_with_check(&home, &control, || {
        calls += 1;
        if calls >= 7 {
            Err(ToolkitError::invalid("caller callback identity"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "input.invalid");
    assert_eq!(error.message, "caller callback identity");
    fs::remove_dir_all(home).unwrap();
}
#[test]
fn controlled_missing_marker_preserves_empty_versus_occupied_classification() {
    let home = home();
    fs::remove_file(home.join("workspace.json")).unwrap();
    let control = OperationControl::default();
    let (error, missing) = inspect_controlled(&home, &control);
    assert!(!missing);
    assert_eq!(error.unwrap_err().code, "vault.legacy_detected");
    let empty = home.join("empty");
    fs::create_dir(&empty).unwrap();
    let (error, missing) = inspect_controlled(&empty, &control);
    assert!(missing);
    assert_eq!(error.unwrap_err().code, "external.io");
    fs::remove_dir_all(home).unwrap();
}

#[test]
fn original_and_independent_stops_are_both_retained_without_opening_marker() {
    let home = home();
    let stop = crate::control::TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    stop.cancel();
    let error = open_controlled_with_check(&home, &control, || {
        Err(ToolkitError::new(
            "records.query_resource_budget",
            "independent cancellation",
            3,
        ))
    })
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(
        serde_json::to_string(&error.details)
            .unwrap()
            .contains("records.query_resource_budget")
    );
    fs::remove_dir_all(home).unwrap();
}
