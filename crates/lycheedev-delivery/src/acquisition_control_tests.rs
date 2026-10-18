//! Public Delivery consumers stop at the entry to the real Core acquisition.
//! Core's own coordination tests cover cancellation after maintenance and during
//! registry reads, including checked-close faults. These hooks do not claim to
//! inject inside those Core phases, replace acquisition, or refresh its budget.
use lycheedev_core::{
    Invocation, ToolkitError, VERSION,
    control::OperationControl,
    coordination::{ClaimRegistry, InstallationMaintenanceLease},
    storage,
};
use serde_json::json;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AcquirePoint {
    Install,
    UpdatePreflight,
    UpdateMutation,
}
type Observer = Box<dyn FnMut(AcquirePoint, &OperationControl)>;
thread_local! {
    static OBSERVER: RefCell<Option<Observer>> = RefCell::new(None);
}
pub(crate) fn before_acquire(point: AcquirePoint, control: &OperationControl) {
    OBSERVER.with_borrow_mut(|observer| {
        if let Some(observer) = observer {
            observer(point, control);
        }
    });
}
struct Observation;
impl Observation {
    fn install(observer: impl FnMut(AcquirePoint, &OperationControl) + 'static) -> Self {
        OBSERVER.with_borrow_mut(|current| {
            assert!(current.is_none(), "acquisition observer already owned");
            *current = Some(Box::new(observer));
        });
        Self
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        OBSERVER.with_borrow_mut(|observer| *observer = None);
    }
}

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    client: PathBuf,
    target: PathBuf,
    archive: PathBuf,
    state: PathBuf,
    skill: PathBuf,
    next: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-delivery-consumer-control-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let client = root.join("_classic_titan_");
        let target = client.join("Interface/AddOns/Lychee Dev");
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(client.join(".flavor.info"), "wow_classic_titan").unwrap();
        fs::write(client.join("version.txt"), "3.80.2.69874").unwrap();
        let archive = root.join("retained-archive");
        fs::create_dir(&archive).unwrap();
        fs::write(archive.join("owned-evidence"), b"retain exactly").unwrap();
        let next = root.join("next");
        candidate(&next, b"next skill", 'b');
        fs::create_dir(root.join("skills")).unwrap();
        Self {
            client,
            target,
            archive,
            state: root.join("home.delivery"),
            skill: root.join("skills/lycheedev"),
            next,
            root,
        }
    }
    fn registry(&self) -> PathBuf {
        self.target.parent().unwrap().join(".lycheedev-claims.json")
    }
    fn invocation(&self, point: AcquirePoint, plan: bool) -> Invocation {
        let mut flags = BTreeMap::from([
            (
                "--installation".into(),
                self.client.to_string_lossy().into_owned(),
            ),
            ("--release".into(), self.next.to_string_lossy().into_owned()),
        ]);
        if point == AcquirePoint::Install {
            flags.insert(
                "--output".into(),
                self.archive.to_string_lossy().into_owned(),
            );
        } else if point == AcquirePoint::UpdateMutation {
            flags.insert("--path".into(), self.skill.to_string_lossy().into_owned());
        }
        if plan {
            flags.insert("--plan".into(), "true".into());
        }
        Invocation {
            command: if point == AcquirePoint::Install {
                "addon install"
            } else {
                "update"
            }
            .into(),
            positional: None,
            flags,
            home: self.root.join("home"),
            cwd: self.root.clone(),
            control: OperationControl::default(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn candidate(root: &Path, skill: &[u8], commit: char) {
    let files = [
        (
            "addon/Lychee Dev.toc",
            b"## Interface: 38002\nBridge/SlotRuntime.lua\n".as_slice(),
        ),
        (
            "addon/Bridge/SlotRuntime.lua",
            b"-- fixture runtime\n".as_slice(),
        ),
        ("skill/SKILL.md", skill),
    ];
    for (name, body) in files {
        let path = root.join("payload").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    let resources: Vec<_> = files
        .iter()
        .map(|(path, body)| json!({"path":path,"bytes":body.len(),"sha256":storage::sha256(body)}))
        .collect();
    storage::atomic_json(&root.join("release.json"), &json!({
        "schema":"lycheedev.release.v1", "version":VERSION,
        "commit":commit.to_string().repeat(40),
        "binaries":{"windows-amd64":{"path":"native/windows-amd64/lycheedev.exe","bytes":1,"sha256":"d".repeat(64)}},
        "resources":resources,
    })).unwrap();
}
// Include directories and every file byte, including the lock-file namespace.
// Capture immediately after the intentional fixture edit, before acquisition.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    fn visit(root: &Path, path: &Path, values: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap().to_path_buf();
            if entry.file_type().unwrap().is_dir() {
                values.insert(relative, None);
                visit(root, &path, values);
            } else {
                // The update owns an empty kernel-lock file at this cutpoint.
                // Its observed length proves the empty bytes without issuing a
                // competing Windows read against that locked byte range.
                let bytes = if entry.metadata().unwrap().len() == 0 {
                    Vec::new()
                } else {
                    fs::read(&path).unwrap()
                };
                values.insert(relative, Some(bytes));
            }
        }
    }
    let mut values = BTreeMap::new();
    visit(root, root, &mut values);
    values
}

#[derive(Clone, Copy)]
enum Stop {
    Cancel,
    Deadline,
    None,
}
fn public_acquisition(point: AcquirePoint, stop: Stop, plan: bool) {
    let f = Fixture::new();
    if matches!(stop, Stop::None) {
        // A live acquisition can legitimately create its empty lock file before
        // detecting malformed JSON. Seed that namespace for the byte comparison.
        InstallationMaintenanceLease::acquire(&f.target)
            .unwrap()
            .close()
            .unwrap();
    }
    if point == AcquirePoint::UpdateMutation {
        let old = f.root.join("old");
        candidate(&old, b"old skill", 'a');
        crate::deploy::fresh(&old, &f.skill, "skill", VERSION).unwrap();
        // Preflight must succeed so the public update reaches its second guard
        // acquisition after committing the first (skill) target.
        fs::write(
            f.registry(),
            br#"{"schema":"lycheedev.claims.v1","claims":[]}"#,
        )
        .unwrap();
    }
    let mut inv = f.invocation(point, plan);
    if matches!(stop, Stop::Deadline) {
        inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    }
    let original = inv.control.clone();
    let cancel = original.tracked_cancel_handle().unwrap();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let expected = Rc::new(RefCell::new(None));
    let seen = observed.clone();
    let saved = expected.clone();
    let registry = f.registry();
    let root = f.root.clone();
    let observer = Observation::install(move |at, control| {
        assert!(control.same_scope(&original));
        assert_eq!(control.deadline(), original.deadline());
        control.check().unwrap();
        seen.borrow_mut().push(at);
        if at != point {
            return;
        }
        // With the old default-control wrapper this exact public consumer
        // returns input.invalid, rather than the original request's stop.
        fs::write(&registry, b"{").unwrap();
        *saved.borrow_mut() = Some(snapshot(&root));
        match stop {
            Stop::Cancel => cancel.cancel(),
            Stop::Deadline => {
                std::thread::sleep(control.remaining().unwrap() + Duration::from_millis(1));
            }
            Stop::None => (),
        }
    });
    let error = crate::dispatch(&inv).unwrap_err();
    drop(observer);
    assert_eq!(
        observed.borrow().as_slice(),
        if point == AcquirePoint::UpdateMutation {
            &[AcquirePoint::UpdatePreflight, AcquirePoint::UpdateMutation][..]
        } else {
            std::slice::from_ref(&point)
        },
    );
    let expected_error = match stop {
        Stop::Cancel | Stop::Deadline => inv.control.check().unwrap_err(),
        Stop::None => {
            ToolkitError::from(serde_json::from_slice::<ClaimRegistry>(b"{").unwrap_err())
        }
    };
    assert_eq!(error.code, expected_error.code);
    assert_eq!(error.message, expected_error.message);
    assert_eq!(error.exit_code, expected_error.exit_code);
    assert_eq!(
        snapshot(&f.root),
        expected.borrow().as_ref().unwrap().clone()
    );
    assert!(!f.target.exists());
    assert!(!f.root.join("home").exists());
    if point == AcquirePoint::UpdateMutation {
        assert_eq!(error.details["result"]["complete"], false);
        assert_eq!(error.details["result"]["targets"][0]["state"], "current");
        assert_eq!(
            error.details["result"]["targets"][0]["activation"],
            "new_agent_context_required"
        );
        assert_eq!(error.details["result"]["targets"][1]["state"], "pending");
        assert!(error.details["result"]["targets"][1]["activation"].is_null());
        assert_eq!(error.details["cause"], expected_error.details);
        assert_eq!(fs::read(f.skill.join("SKILL.md")).unwrap(), b"next skill");
        let release = crate::release::inspect(&f.next, VERSION).unwrap();
        assert_eq!(
            crate::installation::inspect(&f.skill, "skill").unwrap()["receipt"],
            serde_json::to_value(crate::release::receipt(&release, "skill")).unwrap(),
        );
        let remembered: Vec<crate::update::UpdateTarget> =
            storage::read_json(&f.state.join("targets.json")).unwrap();
        assert_eq!(remembered.len(), 2);
        assert_eq!(remembered[0].component, "skill");
        assert_eq!(remembered[1].component, "addon");
        storage::ExclusiveLock::resource(&f.state, "managed-update")
            .unwrap()
            .close()
            .unwrap();
    } else {
        assert_eq!(error.details, expected_error.details);
        assert!(!f.state.exists());
        assert!(!f.skill.exists());
        if !matches!(stop, Stop::None) {
            assert!(!f.target.parent().unwrap().join(".lycheedev-locks").exists());
        }
    }
    InstallationMaintenanceLease::acquire(&f.target)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn public_addon_install_acquisition_uses_original_cancellation_without_mutation() {
    public_acquisition(AcquirePoint::Install, Stop::Cancel, false);
}
#[test]
fn public_addon_install_acquisition_uses_original_deadline_without_mutation() {
    public_acquisition(AcquirePoint::Install, Stop::Deadline, false);
}
#[test]
fn public_update_preflight_acquisition_uses_original_cancellation_for_plan_and_apply() {
    for plan in [true, false] {
        public_acquisition(AcquirePoint::UpdatePreflight, Stop::Cancel, plan);
    }
}
#[test]
fn public_update_mutation_acquisition_stop_preserves_prior_commit_and_inventory() {
    public_acquisition(AcquirePoint::UpdateMutation, Stop::Cancel, false);
}
#[test]
fn public_acquisition_registry_error_stays_primary_without_a_stop() {
    for point in [
        AcquirePoint::Install,
        AcquirePoint::UpdatePreflight,
        AcquirePoint::UpdateMutation,
    ] {
        public_acquisition(point, Stop::None, false);
    }
}
