use super::*;
fn request(path: &Path, file: &str) -> Invocation {
    let mut inv = crate::parse(&words(&[
        "target",
        "resolve",
        "--file",
        file,
        "--home",
        path.to_str().unwrap(),
        "--format",
        "json",
    ]))
    .unwrap();
    inv.cwd = path.into();
    inv
}
fn source_spec() -> Value {
    json!({"source":{"repository":"wow-ui-source","product":"retail","requestedRef":"live","exactCommit":"a".repeat(40),"parserRevision":"file-fixture"}})
}
#[test]
fn selection_file_actual_argv_dispatch_envelope_has_pin_without_added_snapshot_or_capture() {
    let path = home();
    workspace::initialize(&path).unwrap();
    std::fs::write(
        path.join("selection.json"),
        serde_json::to_vec(&source_spec()).unwrap(),
    )
    .unwrap();
    let inv = request(&path, "selection.json");
    let out = crate::dispatch(&inv).unwrap();
    let value = crate::envelope(&Ok(out));
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["schema"], "lycheedev.selection.v1");
    let id = value["result"]["id"].as_str().unwrap();
    assert!(id.starts_with("PIN-"));
    assert_eq!(value["context"], json!({}));
    assert_eq!(value["captures"], json!([]));
    assert_eq!(
        selection::load_pinned_controlled(&path, id, &inv.control)
            .unwrap()
            .id,
        id
    );
    assert!(inv.control.stop_cause().unwrap().is_none());
    assert!(
        inv.control
            .ordered_child(Instant::now() + Duration::from_secs(30))
            .is_ok()
    );
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn selection_file_actual_failure_is_plain_go_error_with_dispatch_stage_and_no_pin() {
    let path = home();
    workspace::initialize(&path).unwrap();
    let inv = request(&path, "selection.json");
    for (raw, message) in [
        (
            br#"{"unknown":true}"#.as_slice(),
            "json: unknown field \"unknown\"",
        ),
        (b"null".as_slice(), "selection.empty"),
    ] {
        std::fs::write(path.join("selection.json"), raw).unwrap();
        let error = crate::command_fault(&inv, crate::dispatch(&inv).unwrap_err());
        assert_eq!(
            (error.code.as_str(), error.exit_code),
            ("command.failed", 5)
        );
        assert_eq!(error.message, message);
        let value = crate::envelope(&Err(error));
        assert_eq!(value["error"]["stage"], "dispatch");
        assert_eq!(value["context"], json!({}));
        assert_eq!(value["captures"], json!([]));
    }
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn selection_file_cli_projection_preserves_known_committed_pin_and_all_cleanup_causes() {
    let inv = request(Path::new("unused"), "selection.json");
    let details = json!({"schema":"lycheedev.selection-file-error.v1","completedPin":"PIN-known","primary":{"code":"external.io","message":"close","exit_code":5,"details":{"schema":"lycheedev.file-cleanup.v1","primary":{"code":"selection.empty"},"cleanup":{"code":"external.io"}}}});
    let error = crate::command_fault(
        &inv,
        ToolkitError::new(
            "external.io",
            "close\nselection pin already committed: PIN-known",
            5,
        )
        .with_details(details.clone()),
    );
    assert_eq!(error.code, "command.failed");
    assert_eq!(error.details, details);
    let value = crate::envelope(&Err(error));
    assert_eq!(value["error"]["details"], details);
    assert!(
        value["error"]["message"]
            .as_str()
            .unwrap()
            .contains("PIN-known")
    );
}
