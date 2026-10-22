use super::*;
fn request(home: &Path, verb: &str, name: Option<&str>) -> Invocation {
    let mut args = vec!["target".to_owned(), verb.to_owned()];
    if let Some(name) = name {
        args.push(name.into());
    }
    if verb == "add" {
        args.extend(words(&[
            "--product",
            "retail",
            "--region",
            "us",
            "--locale",
            "ptPT",
            "--remote",
        ]));
    }
    args.extend(words(&[
        "--home",
        home.to_str().unwrap(),
        "--format",
        "json",
    ]));
    crate::parse(&args).unwrap()
}
#[test]
fn target_management_actual_cli_lifecycle_uses_query_stage_and_fixed_report() {
    let path = home();
    workspace::initialize(&path).unwrap();
    let add = request(&path, "add", Some("retail"));
    let first = crate::dispatch(&add).unwrap();
    assert_eq!(first.data["locale"], "ptPT");
    let duplicate = crate::command_fault(&add, crate::dispatch(&add).unwrap_err());
    assert_eq!(
        (duplicate.code.as_str(), duplicate.exit_code),
        ("selection.target_exists", 3)
    );
    assert_eq!(duplicate.details["stage"], "query");
    let show = crate::dispatch(&request(&path, "show", Some("RETAIL"))).unwrap();
    assert_eq!(show.data["references"], json!([]));
    assert!(show.data.get("resolvedPins").is_none());
    let removed = crate::dispatch(&request(&path, "remove", Some("ret"))).unwrap();
    assert_eq!(removed.data, json!({"name":"retail","removedRefCount":0}));
    assert_eq!(
        crate::dispatch(&request(&path, "list", None)).unwrap().data,
        json!({"targets":[]})
    );
    std::fs::remove_dir_all(path).unwrap();
}
#[test]
fn target_management_parser_rejects_origin_conflict_and_preserves_typed_cleanup() {
    for args in [
        words(&["target", "add", "name"]),
        words(&[
            "target",
            "add",
            "name",
            "--installation",
            "game",
            "--remote",
        ]),
    ] {
        let error = crate::parse(&args).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.message,
            "target add requires exactly one of --installation or --remote"
        );
    }
    assert!(crate::parse(&words(&["target", "add", "name", "--help"])).is_ok());
    let inv = request(Path::new("unused"), "remove", Some("name"));
    let details = json!({"schema":"lycheedev.file-cleanup.v1","primary":{"code":"selection.target_in_use"},"cleanup":{"code":"external.io","message":"close"}});
    let error = crate::command_fault(
        &inv,
        ToolkitError::new(
            "selection.target_in_use",
            "selection.target_in_use: OP-one\nclose",
            3,
        )
        .with_details(details.clone()),
    );
    assert_eq!(error.details["primary"], details["primary"]);
    assert_eq!(error.details["cleanup"], details["cleanup"]);
    assert_eq!(error.details["stage"], "query");
    assert!(error.message.contains("OP-one\nclose"));
}
