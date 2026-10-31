//! Public Delivery consumers stop at the entry to the real Core acquisition.
//! Core's own coordination tests cover cancellation after maintenance and during
//! maintenance acquisition, including checked-close faults. These hooks do not claim to
//! inject inside those Core phases, replace acquisition, or refresh its budget.
use lycheedev_core::{
    VERSION, control::OperationControl, request::RequestContext,
    coordination::InstallationMaintenanceLease, storage,
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
    fn request(&self, point: AcquirePoint, plan: bool) -> (RequestContext, crate::Request) {
        let ctx = RequestContext { home: self.root.join("home"), cwd: self.root.clone(), control: OperationControl::default() };
        let request = if point == AcquirePoint::Install {
            crate::Request::AddonInstall { installation: self.client.clone(), mode: crate::InstallMode::Release { root: self.next.clone(), archive: Some(self.archive.clone()) } }
        } else {
            let mut targets = Vec::new();
            if point == AcquirePoint::UpdateMutation {
                targets.push(crate::update::UpdateTarget { component: "skill".into(), path: self.skill.clone() });
            }
            targets.push(crate::update::UpdateTarget { component: "addon".into(), path: self.client.clone() });
            crate::Request::Update(crate::update::Request {
                selection: crate::update::TargetSelection::Explicit(targets),
                action: crate::update::Action::Managed { release: self.next.clone(), plan },
            })
        };
        (ctx, request)
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
}
fn public_acquisition(point: AcquirePoint, stop: Stop, plan: bool) {
    let f = Fixture::new();
    if point == AcquirePoint::UpdateMutation {
        let old = f.root.join("old");
        candidate(&old, b"old skill", 'a');
        crate::deploy::fresh_controlled(&old, &f.skill, "skill", VERSION, &Default::default()).unwrap();
    }
    let (mut ctx, request) = f.request(point, plan);
    if matches!(stop, Stop::Deadline) {
        ctx.control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    }
    let original = ctx.control.clone();
    let cancel = original.tracked_cancel_handle().unwrap();
    let observed = Rc::new(RefCell::new(Vec::new()));
    let expected = Rc::new(RefCell::new(None));
    let seen = observed.clone();
    let saved = expected.clone();
    let root = f.root.clone();
    let observer = Observation::install(move |at, control| {
        assert!(control.same_scope(&original));
        assert_eq!(control.deadline(), original.deadline());
        control.check().unwrap();
        seen.borrow_mut().push(at);
        if at != point {
            return;
        }
        *saved.borrow_mut() = Some(snapshot(&root));
        match stop {
            Stop::Cancel => cancel.cancel(),
            Stop::Deadline => {
                std::thread::sleep(control.remaining().unwrap() + Duration::from_millis(1));
            }
        }
    });
    let error = crate::dispatch(&ctx, &request).unwrap_err();
    drop(observer);
    assert_eq!(
        observed.borrow().as_slice(),
        if point == AcquirePoint::UpdateMutation {
            &[AcquirePoint::UpdatePreflight, AcquirePoint::UpdateMutation][..]
        } else {
            std::slice::from_ref(&point)
        },
    );
    let expected_error = ctx.control.check().unwrap_err();
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
        let release = crate::release::inspect_with_control(&f.next, VERSION, &Default::default()).unwrap();
        assert_eq!(
            crate::installation::inspect_main_controlled(&f.skill, "skill", &Default::default()).unwrap()["receipt"],
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
        assert!(!f.target.parent().unwrap().join(".lycheedev-locks").exists());
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
