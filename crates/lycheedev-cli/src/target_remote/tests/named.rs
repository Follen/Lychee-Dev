use super::*;
fn named(home: &Path, name: &str) -> Invocation {
    super::super::super::parse(&words(&[
        "target",
        "resolve",
        "--target",
        name,
        "--offline",
        "--home",
        home.to_str().unwrap(),
        "--format",
        "json",
    ]))
    .unwrap()
}
fn config(home: &Path, name: &str, build: &str) {
    let mut metadata = Metadata::open(home, true).unwrap();
    metadata.put(&format!("target/{name}"),&json!({"schema":"lycheedev.target.v1","name":name,"product":"retail","region":"cn","locale":"zhCN","source":"remote","fullBuild":build,"definitions":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}),None).unwrap();
    metadata.close().unwrap();
}
#[test]
fn actual_named_remote_keeps_config_pin_context_without_direct_provenance_or_history_reads() {
    let path = home();
    seed(&path);
    config(&path, "retail-cn", "");
    let mut metadata = Metadata::open(&path, true).unwrap();
    metadata
        .put(
            "target-ref/retail-cn/operation/bad",
            &json!({"schema":"bad"}),
            None,
        )
        .unwrap();
    metadata.close().unwrap();
    let inv = named(&path, "RETAIL-C");
    let result = super::super::super::dispatch(&inv).unwrap();
    assert_eq!(result.data["config"]["name"], "retail-cn");
    assert_eq!(result.data["pin"]["data"]["fullBuild"], "12.1.5.70000");
    assert_eq!(
        result.selection,
        json!({"target":"retail-cn","snapshot":result.data["pin"]["id"]})
    );
    assert!(result.provenance.is_empty());
    assert!(result.warnings.is_empty());
    let metadata = Metadata::open(&path, false).unwrap();
    assert_eq!(metadata.list::<Value>("capture/", 100).unwrap().len(), 1);
    assert_eq!(
        metadata
            .read::<Value>(&format!(
                "target-ref/retail-cn/pin/{}",
                result.data["pin"]["id"].as_str().unwrap()
            ))
            .unwrap()["kind"],
        "pin"
    );
    metadata.close().unwrap();
    let second = super::super::super::dispatch(&inv).unwrap();
    assert_eq!(second.data["pin"], result.data["pin"]);
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn actual_named_remote_exact_unavailable_build_is_target_error_and_has_no_pin_reference() {
    let path = home();
    seed(&path);
    config(&path, "exact", "12.1.5.70001");
    let error = super::super::super::dispatch(&named(&path, "exact")).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("selection.target_build_unavailable", 3)
    );
    assert!(error.message.contains("12.1.5.70001"));
    let metadata = Metadata::open(&path, false).unwrap();
    assert!(
        metadata
            .list::<Value>("target-ref/", 10)
            .unwrap()
            .is_empty()
    );
    metadata.close().unwrap();
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn actual_named_definition_override_and_parent_preserve_exact_input_and_ordered_control() {
    let path = home();
    seed(&path);
    config(&path, "named", "");
    let mut inv = named(&path, "named");
    inv.flags.insert("--definitions".into(), "b".repeat(40));
    let result = super::super::super::dispatch(&inv).unwrap();
    assert_eq!(
        result.data["pin"]["data"]["definitionCommit"],
        "b".repeat(40)
    );
    assert!(inv.control.stop_cause().is_ok());
    assert!(inv.control.ordered_child(inv.control.deadline()).is_ok());
    inv.flags.insert(
        "--from".into(),
        result.data["pin"]["id"].as_str().unwrap().into(),
    );
    let inherited = super::super::super::dispatch(&inv).unwrap();
    assert_eq!(inherited.data["pin"]["parent"], result.data["pin"]["id"]);
    std::fs::remove_dir_all(path).unwrap();
}

#[test]
fn named_public_error_uses_selection_stage_and_keeps_typed_cleanup() {
    let path = home();
    workspace::initialize(&path).unwrap();
    let inv = named(&path, "missing");
    let error = super::super::super::dispatch(&inv).unwrap_err();
    let error = super::super::super::command_fault(&inv, error);
    assert_eq!(error.code, "selection.target_missing");
    assert_eq!(error.exit_code, 3);
    assert_eq!(
        super::super::super::envelope(&Err(error))["error"]["stage"],
        "selection"
    );
    let details =
        json!({"primary":{"code":"selection.target_format"},"cleanup":{"code":"io.error"}});
    let error = super::super::super::command_fault(
        &inv,
        ToolkitError::new("selection.target_format", "bad config and cleanup", 2)
            .with_details(details.clone()),
    );
    assert_eq!(error.details["primary"], details["primary"]);
    assert_eq!(error.details["cleanup"], details["cleanup"]);
    std::fs::remove_dir_all(path).unwrap();
}
