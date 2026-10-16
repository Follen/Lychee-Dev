use super::*;
use crate::control::{StopCause, TrackedCancellation};
use std::{
    collections::BTreeMap,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    inv: Invocation,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-project-management-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(root.join("project/nested")).unwrap();
        crate::workspace::initialize(&root.join("home")).unwrap();
        Self {
            inv: Invocation {
                command: "project init".into(),
                positional: None,
                flags: BTreeMap::new(),
                home: root.join("home"),
                cwd: root.join("project"),
                control: OperationControl::default(),
            },
            root,
        }
    }
    fn command(&self, command: &str, flags: &[(&str, &str)]) -> Invocation {
        let mut inv = self.inv.clone();
        inv.command = command.into();
        inv.flags = flags
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        inv
    }
    fn init(&self, product: &str) -> Value {
        dispatch(&self.command("project init", &[("--product", product)]))
            .unwrap()
            .data
    }
    fn pin(&self, product: &str, letter: char) -> PinnedSet {
        pin_selection_controlled(
            &self.inv.home,
            SelectionSpec {
                source: Some(SourcePin {
                    repository: "wow-ui-source".into(),
                    product: product.into(),
                    requested_ref: String::new(),
                    exact_commit: letter.to_string().repeat(40),
                    parser_revision: "fixture".into(),
                }),
                ..Default::default()
            },
            &self.inv.control,
        )
        .unwrap()
    }
    fn done(self) {
        fs::remove_dir_all(self.root).unwrap();
    }
}
#[test]
fn default_init_requires_existing_directory_and_validates_product_first() {
    let f = Fixture::new();
    let missing = f.root.join("missing");
    let text = missing.to_str().unwrap();
    let bad = dispatch(&f.command(
        "project init",
        &[("--path", text), ("--product", "INVALID")],
    ))
    .unwrap_err();
    assert_eq!(bad.code, "selection.project_format");
    let absent = dispatch(&f.command("project init", &[("--path", text), ("--product", "retail")]))
        .unwrap_err();
    assert_eq!(absent.code, "external.io");
    assert!(!missing.exists());
    f.done();
}
#[test]
fn default_init_status_and_lock_keep_public_shapes_and_checked_owners() {
    let f = Fixture::new();
    let value = f.init("retail");
    assert_eq!(value["state"], "unlocked");
    assert!(value.get("lock").is_none());
    assert_eq!(
        fs::read(f.inv.cwd.join("lycheedev.json")).unwrap(),
        b"{\n  \"schema\": \"lycheedev.project.v1\",\n  \"product\": \"retail\"\n}\n"
    );
    assert!(!f.inv.cwd.join("lycheedev.lock.json").exists());
    let pin = f.pin("retail", 'a');
    let out = dispatch(&f.command("project lock", &[("--snapshot", &pin.id)])).unwrap();
    assert_eq!(out.data["lock"]["selection"]["id"], pin.id);
    assert_eq!(out.selection, json!({"snapshot":pin.id}));
    let mut nested = f.command("project status", &[]);
    nested.cwd = nested.cwd.join("nested");
    assert_eq!(dispatch(&nested).unwrap().data["state"], "locked");
    f.inv
        .control
        .ordered_child(f.inv.control.deadline())
        .unwrap();
    assert_eq!(f.inv.control.stop_cause().unwrap(), None);
    f.done();
}
#[test]
fn existing_declaration_and_orphan_lock_are_never_blessed_or_replaced() {
    let f = Fixture::new();
    f.init("retail");
    let original = fs::read(f.inv.cwd.join("lycheedev.json")).unwrap();
    assert_eq!(
        dispatch(&f.command("project init", &[("--product", "classic")]))
            .unwrap_err()
            .code,
        "selection.project_conflict"
    );
    assert_eq!(
        fs::read(f.inv.cwd.join("lycheedev.json")).unwrap(),
        original
    );
    fs::remove_file(f.inv.cwd.join("lycheedev.json")).unwrap();
    fs::write(f.inv.cwd.join("lycheedev.lock.json"), b"foreign").unwrap();
    assert_eq!(
        dispatch(&f.command("project init", &[("--product", "retail")]))
            .unwrap_err()
            .code,
        "selection.project_conflict"
    );
    assert!(!f.inv.cwd.join("lycheedev.json").exists());
    f.done();
}
#[test]
fn lock_uses_wrapper_find_then_pin_then_explicit_directory_order() {
    let f = Fixture::new();
    let mut nearest = f.command("project lock", &[("--snapshot", "missing")]);
    nearest.cwd = f.root.join("project");
    assert_eq!(
        dispatch(&nearest).unwrap_err().code,
        "selection.project_missing"
    );
    let missing = f.root.join("missing");
    assert_eq!(
        dispatch(&f.command(
            "project lock",
            &[
                ("--snapshot", "missing"),
                ("--path", missing.to_str().unwrap())
            ]
        ))
        .unwrap_err()
        .code,
        "vault.record_missing"
    );
    let pin = f.pin("retail", 'a');
    assert_eq!(
        dispatch(&f.command(
            "project lock",
            &[
                ("--snapshot", &pin.id),
                ("--path", missing.to_str().unwrap())
            ]
        ))
        .unwrap_err()
        .code,
        "external.io"
    );
    f.done();
}
#[test]
fn lock_replaces_valid_other_product_but_preserves_malformed_previous_lock() {
    let f = Fixture::new();
    f.init("retail");
    let old = f.pin("classic", 'a');
    let new = f.pin("retail", 'b');
    let path = f.inv.cwd.join("lycheedev.lock.json");
    fs::write(&path, serde_json::to_vec(&locked(old)).unwrap()).unwrap();
    assert_eq!(
        inspect_value(&f.inv.cwd, &f.inv.control).unwrap_err().code,
        "selection.project_conflict"
    );
    dispatch(&f.command("project lock", &[("--snapshot", &new.id)])).unwrap();
    assert_eq!(
        inspect_value(&f.inv.cwd, &f.inv.control).unwrap()["lock"]["selection"]["id"],
        new.id
    );
    fs::write(&path, b"{\"schema\":\"foreign\"}").unwrap();
    assert_eq!(
        dispatch(&f.command("project lock", &[("--snapshot", &new.id)]))
            .unwrap_err()
            .code,
        "selection.project_format"
    );
    assert_eq!(fs::read(&path).unwrap(), b"{\"schema\":\"foreign\"}");
    f.done();
}
#[test]
fn real_project_lease_wait_checks_original_cancel_without_exposing_token() {
    let f = Fixture::new();
    let held = ExclusiveLock::resource(&f.inv.cwd.join(".lycheedev-locks"), "project").unwrap();
    let stop = TrackedCancellation::new();
    let mut inv = f.command("project init", &[("--product", "retail")]);
    inv.control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    ExclusiveLock::set_blob_wait_observer(move || stop.cancel());
    let error = dispatch(&inv).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert_eq!(
        inv.control.stop_cause().unwrap(),
        Some(StopCause::Cancelled)
    );
    inv.control.ordered_child(inv.control.deadline()).unwrap();
    assert!(!f.inv.cwd.join("lycheedev.json").exists());
    held.close().unwrap();
    f.done();
}
#[test]
fn project_status_is_workspace_independent_and_rejects_nearer_invalid_file() {
    let f = Fixture::new();
    f.init("retail");
    let mut inv = f.command("project status", &[]);
    inv.home = f.root.join("absent-home");
    assert_eq!(dispatch(&inv).unwrap().data["state"], "unlocked");
    assert!(!inv.home.exists());
    inv.cwd = inv.cwd.join("nested");
    fs::write(inv.cwd.join("lycheedev.json"), b"invalid").unwrap();
    assert_eq!(dispatch(&inv).unwrap_err().code, "selection.project_format");
    f.done();
}
#[test]
fn optional_default_consumer_imports_same_pin_and_preserves_cancellation() {
    let f = Fixture::new();
    assert!(optional(&f.inv).unwrap().is_none());
    f.init("retail");
    let pin = f.pin("retail", 'a');
    dispatch(&f.command("project lock", &[("--snapshot", &pin.id)])).unwrap();
    let (path, actual) = optional(&f.inv).unwrap().unwrap();
    assert_eq!(path, f.inv.cwd);
    assert_eq!(actual.id, pin.id);
    let mut inv = f.inv.clone();
    inv.flags.insert("--snapshot".into(), String::new());
    assert_eq!(resolve_pinned(&inv).unwrap().id, pin.id);
    let stop = TrackedCancellation::new();
    inv.control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    stop.cancel();
    assert_eq!(optional(&inv).unwrap_err().code, "command.cancelled");
    assert_eq!(resolve_pinned(&inv).unwrap_err().code, "command.cancelled");
    f.done();
}
#[test]
fn import_authenticates_duplicate_historical_winner_under_same_metadata_owner() {
    let f = Fixture::new();
    f.init("retail");
    let pin = f.pin("retail", 'a');
    dispatch(&f.command("project lock", &[("--snapshot", &pin.id)])).unwrap();
    let mut raw = serde_json::to_string(&pin).unwrap();
    raw.insert_str(1, "\"schema\":\"ignored-earlier\",");
    let mut metadata = Metadata::open(&f.inv.home, true).unwrap();
    let key = format!("pin/{}", pin.id);
    let generation = metadata.generation(&key).unwrap().expect("existing pin");
    metadata
        .put(
            &key,
            &serde_json::value::RawValue::from_string(raw).unwrap(),
            Some(generation),
        )
        .unwrap();
    metadata.close().unwrap();
    assert_eq!(resolve_pinned(&f.inv).unwrap().id, pin.id);
    f.done();
}
#[test]
fn publication_failure_keeps_real_commit_fact_primary_and_all_close_causes() {
    let control = OperationControl::default();
    let mut scope = Scope {
        control: &control,
        supplemental: || Ok(()),
    };
    let first = ToolkitError::new("selection.project_format", "bad", 4)
        .with_details(json!({"original":true}));
    let result: Result<()> = scope.finish(
        Err(first.clone()),
        [
            ToolkitError::new("fixture.close", "stage", 5),
            ToolkitError::new("fixture.close", "directory", 5),
        ],
    );
    let error = Publication {
        path: Some(PathBuf::from("project/lycheedev.lock.json")),
        state: Some(StagedPublication::Published),
        stage_name: Some("owned-stage".into()),
    }
    .finish(result)
    .unwrap_err();
    assert_eq!(error.code, first.code);
    assert_eq!(error.details["publication"]["state"], "published");
    let text = serde_json::to_string(&error.details).unwrap();
    for expected in ["original", "stage", "directory"] {
        assert!(text.contains(expected));
    }
    assert!(error.message.contains("publication published"));
    let unknown = Publication {
        path: Some(PathBuf::from("project/lycheedev.lock.json")),
        state: Some(StagedPublication::Unknown),
        stage_name: Some("retained-stage".into()),
    }
    .finish::<()>(Err(first))
    .unwrap_err();
    assert_eq!(unknown.details["publication"]["state"], "unknown");
    assert!(unknown.message.contains("publication unknown"));
}
#[test]
fn required_project_arguments_are_checked_before_any_filesystem_work() {
    let f = Fixture::new();
    for (command, flag, message) in [
        (
            "project init",
            "--product",
            "project init requires --product",
        ),
        (
            "project lock",
            "--snapshot",
            "project lock requires --snapshot",
        ),
    ] {
        let error = dispatch(&f.command(command, &[(flag, "")])).unwrap_err();
        assert_eq!(error.exit_code, 2);
        assert_eq!(error.message, message);
    }
    f.done();
}

#[test]
fn project_lock_pin_identity_precedes_invalid_nested_spec_and_digit_only_build() {
    let control = OperationControl::default();
    let mut scope = Scope {
        control: &control,
        supplemental: || Ok(()),
    };
    let raw=br#"{"schema":"lycheedev.project-lock.v1","selection":{"schema":"bad","id":"bad","source":{}}}"#;
    let error = wire::lock(raw, &mut scope).unwrap_err();
    assert_eq!(
        error.message,
        "selection.project_format: selection.invalid_pin"
    );
    let raw = format!(
        r#"{{"schema":"lycheedev.project-lock.v1","selection":{{"schema":"lycheedev.selection.v1","id":"PIN-{}","data":{{"product":"retail","region":"cn","language":"zhCN","fullBuild":"+12.1.5.70000","buildConfig":"{}","cdnConfig":"{}","definitionCommit":"{}"}}}}}}"#,
        "a".repeat(64),
        "b".repeat(32),
        "c".repeat(32),
        "d".repeat(40)
    );
    let error = wire::lock(raw.as_bytes(), &mut scope).unwrap_err();
    assert_eq!(
        error.message,
        "selection.project_format: selection.unresolved_data"
    );
}
