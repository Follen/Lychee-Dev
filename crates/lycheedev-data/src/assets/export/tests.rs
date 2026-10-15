use super::*;
use std::collections::BTreeMap;

fn invocation() -> Invocation {
    Invocation {
        command: "asset export".into(),
        positional: None,
        flags: BTreeMap::from([
            ("--snapshot".into(), "PIN-invalid-but-present".into()),
            ("--cdn".into(), "true".into()),
            ("--file-id".into(), "7".into()),
            ("--output".into(), "unused-output".into()),
        ]),
        home: "unopened-asset-workspace".into(),
        cwd: "unopened-asset-workspace".into(),
        control: Default::default(),
    }
}
#[test]
fn default_asset_arguments_reject_missing_snapshot_zero_id_and_source_before_workspace() {
    for (flag, value) in [("--snapshot", ""), ("--file-id", "0"), ("--cdn", "false")] {
        let mut inv = invocation();
        inv.flags.insert(flag.into(), value.into());
        let error = run(&inv).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert!(error.message.contains("requires --snapshot"), "{error:?}");
    }
    let mut inv = invocation();
    inv.flags.insert("--file-id".into(), "4294967296".into());
    assert_eq!(run(&inv).unwrap_err().message, "invalid --file-id");
    inv.flags.insert("--file-id".into(), "7".into());
    inv.flags.insert("--installation".into(), "source".into());
    assert!(run(&inv).unwrap_err().message.contains("choose either"));
}
#[test]
fn default_asset_image_options_and_original_stop_precede_destination_open() {
    let mut inv = invocation();
    inv.flags.insert("--channels".into(), "r".into());
    assert_eq!(run(&inv).unwrap_err().code, "records.export_options");
    inv.control.tracked_cancel_handle().unwrap().cancel();
    let stopped = inv.control.check().unwrap_err();
    let actual = run(&inv).unwrap_err();
    assert_eq!(actual.code, stopped.code);
    assert_eq!(actual.exit_code, stopped.exit_code);
    assert_eq!(actual.details, stopped.details);
}
#[test]
fn committed_capture_context_preserves_all_typed_cleanup_and_does_not_replay_work() {
    let mut progress = Progress::default();
    let ids: Vec<_> = ['a', 'b', 'c']
        .into_iter()
        .map(|ch| format!("CAP-{}", ch.to_string().repeat(64)))
        .collect();
    for id in &ids {
        progress.committed(&json!({"id":id}));
    }
    progress.output_published = true;
    let mut primary = ToolkitError::new("command.cancelled", "cancelled after publish", 7);
    primary.details = json!({"schema":"lycheedev.admitted-workspace-errors.v1", "causes":[
        {"code":"command.cancelled","message":"cancelled after publish","exit_code":7},
        {"code":"command.io","message":"same cleanup","exit_code":5},
        {"code":"command.io","message":"same cleanup","exit_code":5}
    ]});
    let expected = primary.clone();
    let actual = progress.finish::<()>(Err(primary)).unwrap_err();
    assert_eq!(actual.code, expected.code);
    assert_eq!(actual.exit_code, expected.exit_code);
    assert_eq!(
        actual.details["schema"],
        "lycheedev.asset-committed-error.v1"
    );
    assert_eq!(
        actual.details["primary"],
        serde_json::to_value(&expected).unwrap()
    );
    assert_eq!(actual.details["completedCaptures"], json!(ids));
    assert_eq!(actual.details["outputPublished"], true);
    assert!(actual.message.starts_with(&expected.message));
    for id in &ids {
        assert_eq!(actual.message.matches(id).count(), 1);
    }
    assert!(actual.message.contains("asset output already published"));
    assert_eq!(progress.count, 3);
    assert_eq!(progress.finish(Ok(19)).unwrap(), 19);
}
