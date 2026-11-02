use super::*;
use crate::control::{OperationControl, TrackedCancellation};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

struct Fixture {
    root: PathBuf,
    invocation: RequestContext,
    snapshot: Option<String>,
    selected_project: Option<String>,
    pin: PinnedSet,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-project-controlled-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let home = root.join("home");
        let project = root.join("project");
        crate::workspace::initialize(&home).unwrap();
        fs::create_dir_all(project.join("nested")).unwrap();
        let mut pin = PinnedSet {
            schema: "lycheedev.selection.v1".into(),
            id: String::new(),
            spec: SelectionSpec {
                source: Some(SourcePin {
                    repository: "wow-ui-source".into(),
                    product: "retail".into(),
                    requested_ref: String::new(),
                    exact_commit: "a".repeat(40),
                    parser_revision: "fixture".into(),
                }),
                ..Default::default()
            },
        };
        pin.id = identity(&pin).unwrap();
        fs::write(
            project.join("lycheedev.json"),
            br#"{"schema":"lycheedev.project.v1","product":"retail"}"#,
        )
        .unwrap();
        fs::write(
            project.join("lycheedev.lock.json"),
            serde_json::to_vec(&json!({"schema":"lycheedev.project-lock.v1","selection":pin}))
                .unwrap(),
        )
        .unwrap();
        let invocation = RequestContext {
            home,
            cwd: project.join("nested"),
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(30)),
        };
        Self {
            root,
            invocation,
            snapshot: None,
            selected_project: None,
            pin,
        }
    }
    fn request(&self) -> PinSelectionRequest<'_> {
        PinSelectionRequest { home: &self.invocation.home, cwd: &self.invocation.cwd,
            snapshot: self.snapshot.as_deref(), project: self.selected_project.as_deref() }
    }
    fn project(&self) -> PathBuf {
        self.root.join("project")
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.root).unwrap();
    }
}
#[test]
fn project_default_imports_and_authenticates_persisted_pin_without_raw_token() {
    let f = Fixture::new();
    let actual = resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(())).unwrap();
    assert_eq!(actual.id, f.pin.id);
    let again = resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(())).unwrap();
    assert_eq!(again.id, actual.id);
    assert_eq!(
        load_pinned_controlled(&f.invocation.home, &actual.id, &f.invocation.control)
            .unwrap()
            .id,
        actual.id
    );
    f.invocation
        .control
        .ordered_child(f.invocation.control.deadline())
        .unwrap();
    f.cleanup();
}
#[test]
fn malformed_nearer_project_is_not_skipped_for_valid_parent() {
    let f = Fixture::new();
    fs::write(
        f.invocation.cwd.join("lycheedev.json"),
        br#"{"schema":"lycheedev.project.v1","SCHEMA":"lycheedev.project.v1","product":"retail"}"#,
    )
    .unwrap();
    let error = resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(())).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("selection.project_format", 4)
    );
    f.cleanup();
}
#[test]
fn explicit_project_is_resolved_relative_to_invocation_cwd() {
    let mut f = Fixture::new();
    f.selected_project = Some("..".into());
    assert_eq!(
        resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(()))
            .unwrap()
            .id,
        f.pin.id
    );
    f.cleanup();
}
#[test]
fn project_invalid_utf8_oversize_and_nonregular_fail_before_pin_import() {
    let f = Fixture::new();
    let path = f.project().join("lycheedev.json");
    for raw in [vec![0xff], vec![b' '; 65537], b"{\"schema\":1}".to_vec()] {
        fs::write(&path, raw).unwrap();
        assert_eq!(
            resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(()))
                .unwrap_err()
                .code,
            "selection.project_format"
        );
    }
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    assert_eq!(
        resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(()))
            .unwrap_err()
            .code,
        "selection.project_format"
    );
    f.cleanup();
}
#[test]
fn project_missing_lock_unlocked_and_wrong_product_conflict_have_reference_codes() {
    let f = Fixture::new();
    let lock = f.project().join("lycheedev.lock.json");
    fs::remove_file(&lock).unwrap();
    assert_eq!(
        resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(()))
            .unwrap_err()
            .code,
        "selection.project_unlocked"
    );
    fs::write(
        &lock,
        serde_json::to_vec(&json!({"schema":"lycheedev.project-lock.v1","selection":f.pin}))
            .unwrap(),
    )
    .unwrap();
    fs::write(
        f.project().join("lycheedev.json"),
        br#"{"schema":"lycheedev.project.v1","product":"classic"}"#,
    )
    .unwrap();
    assert_eq!(
        resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || Ok(()))
            .unwrap_err()
            .code,
        "selection.project_conflict"
    );
    f.cleanup();
}
#[test]
fn project_typed_documents_reject_unknown_fields_and_require_exact_schema() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let mut scope = Scope {
        control: &control,
        supplemental: || Ok(()),
    };
    let valid = br#"{"schema":"lycheedev.project.v1","product":"retail"}"#;
    assert_eq!(wire::project(valid, &mut scope).unwrap(), "retail");
    for raw in [
        br#"{"schema":"lycheedev.project.v1","product":"retail","unknown":null}"#.as_slice(),
        br#"{"schema":"wrong","product":"retail"}"#.as_slice(),
        br#"{"schema":"lycheedev.project.v1","product":"Retail"}"#.as_slice(),
    ] {
        assert!(
            wire::project(raw, &mut scope).is_err(),
            "{}",
            String::from_utf8_lossy(raw)
        );
    }
    let lock = serde_json::to_vec(
        &json!({"schema":"lycheedev.project-lock.v1","selection":f.pin}),
    )
    .unwrap();
    assert_eq!(wire::lock(&lock, &mut scope).unwrap().id, f.pin.id);
    let unknown = serde_json::to_vec(
        &json!({"schema":"lycheedev.project-lock.v1","selection":f.pin,"unknown":1}),
    )
    .unwrap();
    assert!(wire::lock(&unknown, &mut scope).is_err());
    assert!(
        wire::lock(
            br#"{"schema":"lycheedev.project-lock.v1","selection":null}"#,
            &mut scope
        )
        .is_err()
    );
}
#[test]
fn project_precancel_preserves_original_scope_and_independent_budget_failure() {
    let f = Fixture::new();
    let stop = TrackedCancellation::new();
    let mut inv = f.invocation.clone();
    inv.control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    stop.cancel();
    let error = resolve_pin_request_controlled_with_check(PinSelectionRequest { home: &inv.home, cwd: &inv.cwd, snapshot: None, project: None }, &inv.control, || {
        Err(ToolkitError::new(
            "records.query_resource_budget",
            "independent deadline",
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
    inv.control.stop_cause().unwrap();
    f.cleanup();
}
#[test]
fn project_import_checks_independent_budget_inside_real_sqlite_busy_wait() {
    let f = Fixture::new();
    let metadata = Metadata::open(&f.invocation.home, true).unwrap();
    metadata.close().unwrap();
    let blocker =
        rusqlite::Connection::open(f.invocation.home.join("state/toolkit.sqlite")).unwrap();
    blocker.busy_timeout(Duration::ZERO).unwrap();
    blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let deadline = Instant::now() + Duration::from_millis(300);
    let started = Instant::now();
    let error = resolve_pin_request_controlled_with_check(f.request(), &f.invocation.control, || {
        if Instant::now() >= deadline {
            Err(ToolkitError::new(
                "records.query_resource_budget",
                "independent deadline",
                3,
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("records.query_resource_budget", 3)
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    f.invocation.control.check().unwrap();
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    f.cleanup();
}
#[test]
fn project_shared_file_survives_atomic_lock_replacement() {
    let f = Fixture::new();
    let path = f.project();
    let name = OsStr::new("lycheedev.lock.json");
    let mut scope = Scope {
        control: &f.invocation.control,
        supplemental: || Ok(()),
    };
    let directory = storage::Directory::open_shared_controlled(&path, &mut scope).unwrap();
    let mut old = directory
        .open_optional_project_file_controlled(name, &mut scope)
        .unwrap()
        .unwrap();
    let original = fs::read(path.join(name)).unwrap();
    let mut spec = f.pin.spec.clone();
    spec.source.as_mut().unwrap().exact_commit = "b".repeat(40);
    let replacement =
        pin_selection_controlled(&f.invocation.home, spec, &f.invocation.control).unwrap();
    execute_project(&f.invocation, &ProjectRequest::Lock { path: Some(path.clone()), snapshot: replacement.id.clone() }).unwrap();
    let mut bytes = Vec::new();
    old.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, original);
    let current: Value = serde_json::from_slice(&fs::read(path.join(name)).unwrap()).unwrap();
    assert_eq!(current["selection"]["id"], replacement.id);
    assert_ne!(replacement.id, f.pin.id);
    storage::close_file(old).unwrap();
    directory.close().unwrap();
    f.cleanup();
}

#[path = "request_tests.rs"]
mod request_tests;
