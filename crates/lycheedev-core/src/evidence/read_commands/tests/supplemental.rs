use super::*;
fn stopped() -> ToolkitError {
    ToolkitError::new(
        "records.query_resource_budget",
        "independent budget stopped",
        3,
    )
}
#[test]
fn independent_capture_read_reuses_historical_identity_and_never_creates_evidence() {
    let f = Fixture::new();
    let (id, raw) = f.manifest(2, "supplemental");
    let mut checks = 0;
    let value =
        read_capture_controlled_with_check(&f.home, &id, &OperationControl::default(), || {
            checks += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(value, serde_json::from_str::<Value>(&raw).unwrap());
    assert!(checks > 5);
    let m = Metadata::open(&f.home, false).unwrap();
    assert_eq!(m.list::<Value>("capture/", 100).unwrap().len(), 1);
    m.close().unwrap();
    f.cleanup();
}
#[test]
fn independent_pre_stop_precedes_workspace_and_keeps_original_invocation_live() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let error = read_capture_controlled_with_check(&f.home, "missing", &control, || Err(stopped()))
        .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(error.exit_code, 3);
    control.check().unwrap();
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.cleanup();
}
#[test]
fn held_sqlite_wait_observes_independent_deadline_without_reclassifying_it_as_command_deadline() {
    let f = Fixture::new();
    let (id, _) = f.manifest(4, "wait");
    let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(30));
    let until = Instant::now() + Duration::from_millis(200);
    let start = Instant::now();
    let error = read_capture_controlled_with_check(&f.home, &id, &control, || {
        if Instant::now() >= until {
            Err(stopped())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(error.exit_code, 3);
    control.check().unwrap();
    assert!(start.elapsed() < Duration::from_secs(2));
    db.execute_batch("ROLLBACK").unwrap();
    db.close().unwrap();
    f.cleanup();
}
#[test]
fn original_and_independent_stops_are_both_retained_after_no_open() {
    let f = Fixture::new();
    let stop = TrackedCancellation::new();
    stop.cancel();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop);
    let error = read_capture_controlled_with_check(&f.home, "missing", &control, || Err(stopped()))
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(error.message.contains("independent budget stopped"));
    assert!(!error.details.is_null());
    assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
    f.cleanup();
}
#[test]
fn original_parse_failure_keeps_priority_when_independent_stop_is_also_observed() {
    let control = OperationControl::default();
    let primary = ToolkitError::invalid("invalid capture JSON");
    let error =
        dependencies::finish_capture_checks::<()>(Err(primary.clone()), &control, &mut || {
            Err(stopped())
        })
        .unwrap_err();
    assert_eq!(error.code, primary.code);
    assert_eq!(error.exit_code, primary.exit_code);
    assert_eq!(error.details["primary"]["message"], primary.message);
    assert_eq!(
        error.details["cleanup"]["code"],
        "records.query_resource_budget"
    );
}
#[test]
fn typed_capture_reads_retain_stop_observation_without_a_normalization_pass() {
    let stop = TrackedCancellation::new();
    stop.cancel();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop);
    let raw = br#"{"schema":"lycheedev.capture.v1"}"#;
    let error = dependencies::decode_capture_ref(raw, &control).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(control.stop_cause().unwrap(), Some(StopCause::Cancelled));
}
