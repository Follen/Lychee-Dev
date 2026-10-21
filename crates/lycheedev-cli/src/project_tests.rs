use super::*;
use lycheedev_core::{selection, workspace};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-project-cli-{}-{}-{}",
            std::process::id(),
            lycheedev_core::storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(path.join("project")).unwrap();
        workspace::initialize(&path.join("home")).unwrap();
        Self(path)
    }
    fn inv(&self, args: &[&str]) -> Invocation {
        let mut args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        // Go 3.2.3 command_contract.go: only project lock admits --home.
        // Init/status use the project path and must not touch a workspace.
        if args.get(1).is_some_and(|verb| verb == "lock") {
            args.extend([
                "--home".into(),
                self.0.join("home").to_string_lossy().into_owned(),
            ]);
        }
        args.extend(["--format".into(), "json".into()]);
        let mut inv = parse(&args).unwrap();
        inv.cwd = self.0.join("project");
        inv
    }
    fn done(self) {
        std::fs::remove_dir_all(self.0).unwrap();
    }
}
#[test]
fn project_actual_argv_init_status_lock_publish_go_shapes_and_snapshot_context() {
    let f = Fixture::new();
    let inv = f.inv(&["project", "init", "--product", "retail"]);
    let out = dispatch(&inv).unwrap();
    let envelope = envelope(&Ok(out));
    assert_eq!(envelope["result"]["state"], "unlocked");
    assert!(envelope["result"].get("lock").is_none());
    assert_eq!(envelope["context"], json!({}));
    let pin = selection::pin_selection_controlled(
        &f.0.join("home"),
        selection::SelectionSpec {
            source: Some(selection::SourcePin {
                repository: "wow-ui-source".into(),
                product: "retail".into(),
                exact_commit: "a".repeat(40),
                parser_revision: "fixture".into(),
                requested_ref: String::new(),
            }),
            ..Default::default()
        },
        &inv.control,
    )
    .unwrap();
    let lock = f.inv(&["project", "lock", "--snapshot", &pin.id]);
    let out = crate::envelope(&Ok(dispatch(&lock).unwrap()));
    assert_eq!(out["context"]["snapshot"], pin.id);
    assert_eq!(out["captures"], json!([]));
    let status = f.inv(&["project", "status"]);
    assert_eq!(dispatch(&status).unwrap().data["state"], "locked");
    lock.control.ordered_child(lock.control.deadline()).unwrap();
    f.done();
}
#[test]
fn project_actual_format_fault_retains_selection_stage_and_plain_io_is_dispatch_failure() {
    let f = Fixture::new();
    std::fs::write(f.0.join("project/lycheedev.json"), b"invalid").unwrap();
    let inv = f.inv(&["project", "status"]);
    let error = command_fault(&inv, dispatch(&inv).unwrap_err());
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("selection.project_format", 4)
    );
    assert_eq!(crate::envelope(&Err(error))["error"]["stage"], "selection");
    let missing = f.0.join("missing");
    let inv = f.inv(&[
        "project",
        "init",
        "--path",
        missing.to_str().unwrap(),
        "--product",
        "retail",
    ]);
    let error = command_fault(&inv, dispatch(&inv).unwrap_err());
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.failed", 5)
    );
    assert!(!missing.exists());
    f.done();
}
#[test]
fn project_cli_keeps_published_and_unknown_facts_in_existing_error_details() {
    let f = Fixture::new();
    let inv = f.inv(&["project", "lock", "--snapshot", "PIN-fixture"]);
    for state in ["published", "unknown"] {
        let details = json!({"schema":"lycheedev.project-file-error.v1","primary":{"code":"external.io","message":"checked close","exit_code":5,"details":{"cleanup":"preserved"}},"publication":{"state":state,"path":"project/lycheedev.lock.json","stageName":"stage"}});
        let error = command_fault(
            &inv,
            ToolkitError::new(
                "external.io",
                format!("close\nproject file publication {state}: project/lycheedev.lock.json"),
                5,
            )
            .with_details(details.clone()),
        );
        assert_eq!(error.code, "command.failed");
        assert_eq!(crate::envelope(&Err(error))["error"]["details"], details);
    }
    f.done();
}
#[test]
fn project_cli_required_and_empty_values_preserve_parser_then_dispatch_priority() {
    let f = Fixture::new();
    for (verb, flag, message) in [
        ("init", "--product", "project init requires --product"),
        ("lock", "--snapshot", "project lock requires --snapshot"),
    ] {
        let args = ["project", verb, flag, ""].map(str::to_owned);
        let error = parse(&args).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(error.code, "command.invalid_arguments");
        assert_eq!(error.details["parser"], "clap");
        assert_eq!(error.details["kind"], "InvalidValue");
        assert!(error.details["diagnostic"].as_str().unwrap().contains(flag));
        let inv = f.inv(&["project", verb]);
        let error = dispatch(&inv).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(error.message, message);
    }
    f.done();
}

#[test]
fn project_init_and_status_reject_home_before_dispatch() {
    for verb in ["init", "status"] {
        let args = ["project", verb, "--home", "fixture-home"].map(str::to_owned);
        let error = parse(&args).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(
            error.message,
            format!("--home is not supported by project {verb}")
        );
        assert_eq!(error.details, json!({"parser":"clap","kind":"UnknownArgument","flag":"--home"}));
    }
}
