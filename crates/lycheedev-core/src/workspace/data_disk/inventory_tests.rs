//! Executed-boundary observations: successful Directory constructions and actual
//! checked closes, not estimates of native handle/syscall counts or elapsed time.
use super::*;
use crate::{control::TrackedCancellation, evidence, workspace};
use std::{
    collections::BTreeMap,
    io::{self, Cursor, Read},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Default)]
struct Events {
    opened: Vec<PathBuf>,
    closed: Vec<PathBuf>,
    borrowed: Vec<PathBuf>,
    file_closes: usize,
    inventories: usize,
    quotas: usize,
}
#[derive(Default)]
struct Observation {
    events: Events,
    fail_closes: Vec<PathBuf>,
    stop_after_open: Option<(PathBuf, TrackedCancellation)>,
    stop_after_borrow: Option<(PathBuf, TrackedCancellation)>,
}
thread_local! {
    static OBSERVATION: RefCell<Option<Rc<RefCell<Observation>>>> = const { RefCell::new(None) };
}
fn observed(run: impl FnOnce(&mut Observation)) {
    OBSERVATION.with(|slot| {
        if let Some(observation) = slot.borrow().as_ref() {
            run(&mut observation.borrow_mut());
        }
    });
}
pub(super) fn directory_opened(path: &Path) {
    observed(|state| {
        state.events.opened.push(path.to_owned());
        if let Some((target, stop)) = &state.stop_after_open {
            if path == target {
                stop.cancel();
            }
        }
    });
}
pub(super) fn directory_borrowed(path: &Path) {
    observed(|state| {
        state.events.borrowed.push(path.to_owned());
        if let Some((target, stop)) = &state.stop_after_borrow {
            if path == target {
                stop.cancel();
            }
        }
    });
}
pub(super) fn directory_closed(path: PathBuf, mut errors: Vec<ToolkitError>) -> Vec<ToolkitError> {
    observed(|state| {
        state.events.closed.push(path.clone());
        if state.fail_closes.contains(&path) {
            // Inject only after the real checked close has consumed this owner.
            errors.push(ToolkitError::new("test.close", "same close event", 5));
        }
    });
    errors
}
pub(super) fn file_closed() {
    observed(|state| state.events.file_closes += 1);
}
pub(super) fn quota_acquired() {
    observed(|state| state.events.quotas += 1);
}
pub(super) fn inventory_entered() {
    observed(|state| state.events.inventories += 1);
}
struct Monitor(Rc<RefCell<Observation>>);
impl Monitor {
    fn new() -> Self {
        let state = Rc::new(RefCell::new(Observation::default()));
        OBSERVATION.with(|slot| assert!(slot.borrow_mut().replace(state.clone()).is_none()));
        Self(state)
    }
    fn events(&self) -> Events {
        self.0.borrow().events.clone()
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        OBSERVATION.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new(limit: u64) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-inventory-owner-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        let fixture = Self(home);
        fixture.limit(limit);
        fixture
    }
    fn limit(&self, limit: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(limit);
        storage::atomic_json(&self.0.join("config.json"), &config).unwrap();
    }
    fn reserve<'a>(&self, control: &'a OperationControl, peak: u64) -> DerivedReservation<'a> {
        DerivedReservation::acquire_inner(
            None,
            &self.0,
            0,
            AdditionalPeak {
                objects: peak,
                ..Default::default()
            },
            control,
            None,
            false,
        )
        .unwrap()
    }
    fn publish(&self, bytes: &[u8]) -> evidence::BlobRef {
        publish_shared_blob_with_check(
            &self.0,
            bytes,
            bytes.len(),
            &OperationControl::default(),
            &|| Ok(()),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn frequencies(paths: impl IntoIterator<Item = PathBuf>) -> BTreeMap<PathBuf, usize> {
    let mut counts = BTreeMap::new();
    for path in paths {
        *counts.entry(path).or_insert(0) += 1;
    }
    counts
}
fn run_inventory(owner: &DerivedReservation<'_>, reuse: bool) -> Result<DerivedUsage> {
    inventory_recovering(
        &owner.home,
        &owner.paths,
        &owner.control,
        owner.recovery.as_ref(),
        if reuse { &owner.directories } else { &[] },
    )
}
fn same_failure(owner: &DerivedReservation<'_>, code: &str) {
    let before = inventory(&owner.home, &owner.paths, &owner.control).unwrap_err();
    let after = run_inventory(owner, true).unwrap_err();
    assert_eq!(before.code, code);
    assert_eq!(after.code, before.code);
    assert_eq!(after.message, before.message);
    assert_eq!(after.details, before.details);
}

#[test]
fn prepare_root_existing_and_missing_preserve_identity_and_release_checked_owners() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let checks = Checks {
        control: &control,
        supplemental: None,
    };
    let parent = f.0.join("cache");
    for existing in [false, true] {
        for mode in 0..3 {
            let root = parent.join(format!("prepared-{existing}-{mode}"));
            if existing {
                fs::create_dir(&root).unwrap();
            }
            let held = if mode == 1 {
                vec![Directory::open_controlled(&parent, &mut Controlled(&checks)).unwrap()]
            } else {
                Vec::new()
            };
            let directory = if mode == 2 {
                let home =
                    Directory::open_shared_controlled(&f.0, &mut Controlled(&checks)).unwrap();
                let directory = prepare_root_from_home(&root, &home, &held, &checks).unwrap();
                assert!(close_directory(home).is_empty());
                // A child's duplicated ancestry keeps the identity after both
                // temporary shared owners close, without denying parent writes.
                use std::os::windows::fs::OpenOptionsExt;
                for path in [&f.0, &parent] {
                    let writer = fs::OpenOptions::new()
                        .access_mode(0x100) // FILE_WRITE_ATTRIBUTES
                        .share_mode(7)
                        .custom_flags(0x02000000) // FILE_FLAG_BACKUP_SEMANTICS
                        .open(path)
                        .unwrap();
                    storage::close_file(writer).unwrap();
                }
                directory
            } else {
                prepare_root(&root, &held, &checks).unwrap()
            };
            directory.verify().unwrap();
            fs::write(root.join("body"), b"preserved").unwrap();
            let moved = root.with_extension("moved");
            assert!(fs::rename(&root, &moved).is_err());
            assert!(close_directory(directory).is_empty());
            for parent in held {
                parent.verify().unwrap();
                assert!(close_directory(parent).is_empty());
            }
            fs::rename(&root, &moved).unwrap();
            assert_eq!(fs::read(moved.join("body")).unwrap(), b"preserved");
        }
    }
}

#[test]
fn prepare_root_rejects_existing_files_and_preserves_cancellation_without_creation() {
    let f = Fixture::new(1000);
    for mode in 0..3 {
        let control = OperationControl::default();
        let checks = Checks {
            control: &control,
            supplemental: None,
        };
        let parent = f.0.join("cache");
        let root = parent.join(format!("foreign-{mode}"));
        fs::write(&root, b"foreign").unwrap();
        let held = if mode == 1 {
            vec![Directory::open_controlled(&parent, &mut Controlled(&checks)).unwrap()]
        } else {
            Vec::new()
        };
        let home = (mode == 2)
            .then(|| Directory::open_shared_controlled(&f.0, &mut Controlled(&checks)).unwrap());
        let prepare = |path: &Path| match &home {
            Some(home) => prepare_root_from_home(path, home, &held, &checks),
            None => prepare_root(path, &held, &checks),
        };
        assert!(prepare(&root).is_err());
        assert_eq!(fs::read(&root).unwrap(), b"foreign");
        control.tracked_cancel_handle().unwrap().cancel();
        let missing = parent.join(format!("missing-{mode}"));
        for path in [&root, &missing] {
            let error = prepare(path).err().expect("cancelled preparation");
            assert_eq!(error.code, "command.cancelled");
        }
        assert!(!missing.exists());
        if let Some(home) = home {
            home.verify().unwrap();
            assert!(close_directory(home).is_empty());
        }
        for parent in held {
            parent.verify().unwrap();
            assert!(close_directory(parent).is_empty());
        }
        fs::remove_file(&root).unwrap();
    }
}

#[test]
fn initialization_shared_ancestor_close_failures_keep_both_causes_and_release_quota() {
    let f = Fixture::new(1000);
    let paths = DataCachePaths::new(&f.0);
    for root in paths.roots() {
        fs::create_dir_all(root).unwrap();
    }
    let control = OperationControl::default();
    let monitor = Monitor::new();
    monitor.0.borrow_mut().fail_closes = vec![f.0.join("cache"), f.0.clone()];
    let error = DerivedReservation::acquire_inner(
        None,
        &f.0,
        1000,
        AdditionalPeak::default(),
        &control,
        None,
        false,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "test.close");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 2);
    assert!(causes.iter().all(|cause| cause["code"] == "test.close"));
    assert_eq!(
        monitor.events().closed,
        [
            f.0.join("cache"),
            paths.roots()[0].to_owned(),
            f.0.clone(),
            f.0.join("locks"),
        ]
    );
    assert_eq!(monitor.events().quotas, 1);
    drop(monitor);
    ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
        .unwrap()
        .close()
        .unwrap();
    f.reserve(&control, 0).finish(Ok(())).unwrap();
}

#[test]
fn full_inventory_reuses_managed_owners_and_keeps_identical_file_work() {
    let f = Fixture::new(10000);
    let body = f.publish(b"shared-body");
    let control = OperationControl::default();
    let owner = f.reserve(&control, 1000);
    let unknown = owner.paths.objects.join("unknown");
    let nested = unknown.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(nested.join("charged"), b"abc").unwrap();
    fs::write(owner.paths.spill.join("spill"), b"de").unwrap();
    fs::write(owner.paths.metadata.join("unknown"), b"fghi").unwrap();
    let managed: Vec<_> = owner.directories[1..]
        .iter()
        .map(|directory| directory.path().to_owned())
        .collect();
    assert_eq!(managed.len(), 11);
    let fallback = Monitor::new();
    let before = run_inventory(&owner, false).unwrap();
    let fallback_events = fallback.events();
    drop(fallback);
    let reused = Monitor::new();
    let after = run_inventory(&owner, true).unwrap();
    let reused_events = reused.events();
    drop(reused);
    assert_eq!(after, before);
    assert_eq!(after.shared_blob_bytes, body.bytes);
    assert_eq!(after.total().unwrap(), body.bytes + 80 + 9);
    assert_eq!(after.files, 5);
    assert_eq!(fallback_events.inventories, 1);
    assert_eq!(reused_events.inventories, 1);
    assert_eq!(fallback_events.file_closes, 9);
    assert_eq!(reused_events.file_closes, fallback_events.file_closes);
    // Eleven managed directories plus durable spool declarations, external
    // export allowances, and semantic shared-body attribution.
    assert_eq!(fallback_events.opened.len(), 17); // 14 managed traversals + 2 unknown + blobs.
    assert_eq!(reused_events.opened.len(), 3);
    assert_eq!(fallback_events.closed.len(), 18);
    assert_eq!(
        fallback_events.closed.len() - reused_events.closed.len(),
        14
    );
    assert!(fallback_events.borrowed.is_empty());
    let mut expected_managed = frequencies(managed.clone());
    *expected_managed
        .get_mut(&owner.paths.shared_blobs())
        .unwrap() += 3;
    assert_eq!(
        frequencies(reused_events.borrowed.clone()),
        expected_managed
    );
    for events in [&fallback_events, &reused_events] {
        for path in [&unknown, &nested] {
            assert_eq!(
                events.opened.iter().filter(|value| *value == path).count(),
                1
            );
            assert_eq!(
                events.closed.iter().filter(|value| *value == path).count(),
                1
            );
        }
    }
    assert_eq!(
        frequencies(
            fallback_events
                .closed
                .into_iter()
                .filter(|path| managed.contains(path))
        ),
        expected_managed,
    );
    assert!(
        reused_events
            .closed
            .iter()
            .all(|path| !managed.contains(path))
    );
    // Semantic body checks still own and close blobs root + shard in both cases.
    assert_eq!(reused_events.closed.len(), 4);
    assert_eq!(owner.directories.len(), 12);
    owner.finish(Ok(())).unwrap();
}

#[test]
fn shared_body_inventory_reuses_one_root_and_preserves_both_completed_charges() {
    let f = Fixture::new(10000);
    let first = f.publish(b"first shared body");
    let second = f.publish(b"second shared body");
    let control = OperationControl::default();
    let owner = f.reserve(&control, 0);
    let blobs = f.0.join("blobs");
    let monitor = Monitor::new();
    let usage = run_inventory(&owner, true).unwrap();
    assert_eq!(usage.shared_blob_bytes, first.bytes + second.bytes);
    assert_eq!(usage.retained_shared_peak_bytes, 0);
    assert_eq!(usage.total().unwrap(), first.bytes + second.bytes + 160);
    let events = monitor.events();
    assert_eq!(
        events.opened.iter().filter(|path| **path == blobs).count(),
        1
    );
    assert_eq!(
        events.closed.iter().filter(|path| **path == blobs).count(),
        1
    );
    for (reference, body) in [
        (&first, b"first shared body".as_slice()),
        (&second, b"second shared body".as_slice()),
    ] {
        assert_eq!(
            fs::read(evidence::blob_path(&f.0, &reference.sha256).unwrap()).unwrap(),
            body
        );
    }
    drop(monitor);
    owner.finish(Ok(())).unwrap();
    let moved = f.0.join("blobs-moved");
    fs::rename(&blobs, &moved).unwrap();
    fs::rename(&moved, &blobs).unwrap();
    assert_eq!(f.publish(b"first shared body").sha256, first.sha256);
}

#[test]
fn shared_body_inventory_closes_reused_root_after_cancel_and_preserves_close_error() {
    let f = Fixture::new(10000);
    let body = b"shared cancellation body";
    let reference = f.publish(body);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 0);
    let blobs = f.0.join("blobs");
    let monitor = Monitor::new();
    monitor.0.borrow_mut().fail_closes = vec![blobs.clone()];
    monitor.0.borrow_mut().stop_after_open =
        Some((blobs.clone(), control.tracked_cancel_handle().unwrap()));
    let check = || {
        if control.check().is_err() {
            Err(ToolkitError::new(
                "test.budget",
                "original independent budget",
                3,
            ))
        } else {
            Ok(())
        }
    };
    let checks = Checks {
        control: &control,
        supplemental: Some(&check),
    };
    let error =
        shared_blobs::inventory_recovering(&f.0, &owner.paths, &checks, None, &owner.directories)
            .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    let detail = serde_json::to_string(&error).unwrap();
    assert!(detail.contains("\"code\":\"test.close\""));
    assert!(detail.contains("\"code\":\"test.budget\""));
    let events = monitor.events();
    assert_eq!(
        events.opened.iter().filter(|path| **path == blobs).count(),
        1
    );
    assert_eq!(
        events.closed.iter().filter(|path| **path == blobs).count(),
        1
    );
    assert_eq!(
        fs::read(evidence::blob_path(&f.0, &reference.sha256).unwrap()).unwrap(),
        body
    );
    drop(monitor);
    assert_eq!(
        owner.finish::<()>(Err(error)).unwrap_err().code,
        "command.cancelled"
    );
    let moved = f.0.join("blobs-moved");
    fs::rename(&blobs, &moved).unwrap();
    fs::rename(&moved, &blobs).unwrap();
    assert_eq!(f.publish(body).sha256, reference.sha256);
}

#[test]
fn borrowed_scans_never_close_original_owners_and_finish_consumes_each_once() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 0);
    let paths: Vec<_> = owner
        .directories
        .iter()
        .map(|d| d.path().to_owned())
        .collect();
    let monitor = Monitor::new();
    for _ in 0..3 {
        assert_eq!(owner.verify_usage().unwrap(), DerivedUsage::default());
    }
    assert!(monitor.events().opened.is_empty());
    assert!(monitor.events().closed.is_empty());
    let mut expected_walk = frequencies(paths[1..].iter().cloned());
    *expected_walk.get_mut(&owner.paths.shared_blobs()).unwrap() += 3;
    assert_eq!(
        frequencies(monitor.events().borrowed),
        expected_walk
            .iter()
            .map(|(path, count)| (path.clone(), count * 3))
            .collect()
    );
    owner.finish(Ok(())).unwrap();
    let events = monitor.events();
    assert_eq!(events.inventories, 4);
    assert_eq!(
        frequencies(events.borrowed),
        expected_walk
            .into_iter()
            .map(|(path, count)| (path, count * 4))
            .collect()
    );
    assert_eq!(
        events.closed,
        paths[1..]
            .iter()
            .rev()
            .chain(&paths[..1])
            .cloned()
            .collect::<Vec<_>>()
    );
    drop(monitor);
    ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn fallback_and_reuse_both_reenumerate_new_unknown_files_and_keep_exact_limits() {
    let f = Fixture::new(10);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 10);
    assert_eq!(run_inventory(&owner, true).unwrap().total().unwrap(), 0);
    fs::write(owner.paths.exports.join("unknown"), [0; 10]).unwrap();
    assert_eq!(
        run_inventory(&owner, false).unwrap(),
        owner.verify_usage().unwrap()
    );
    owner.finish(Ok(())).unwrap();
    let owner = f.reserve(&control, 0);
    assert_eq!(owner.report.existing.total().unwrap(), 10);
    owner.finish(Ok(())).unwrap();
    let error = DerivedReservation::acquire_inner(
        None,
        &f.0,
        0,
        AdditionalPeak {
            objects: 1,
            ..Default::default()
        },
        &control,
        None,
        false,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    f.limit(9);
    let error = DerivedReservation::acquire_inner(
        None,
        &f.0,
        0,
        AdditionalPeak::default(),
        &control,
        None,
        false,
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    assert_eq!(
        fs::metadata(DataCachePaths::new(&f.0).exports.join("unknown"))
            .unwrap()
            .len(),
        10
    );
}

#[test]
fn held_directories_deny_replacement_and_unknown_child_files_are_freshly_authenticated() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 100);
    for directory in &owner.directories[1..] {
        let error = fs::rename(
            directory.path(),
            directory.path().with_extension("replacement"),
        )
        .unwrap_err();
        assert_eq!(error.raw_os_error(), Some(32));
    }
    let unknown = owner.paths.objects.join("unknown");
    fs::create_dir(&unknown).unwrap();
    fs::write(unknown.join("body"), b"a").unwrap();
    let monitor = Monitor::new();
    assert_eq!(
        run_inventory(&owner, true).unwrap().object_namespace_bytes,
        1
    );
    assert_eq!(frequencies(monitor.events().opened).get(&unknown), Some(&1));
    assert_eq!(frequencies(monitor.events().closed).get(&unknown), Some(&1));
    assert_eq!(monitor.events().file_closes, 1);
    drop(monitor);
    // The quota still pins objects itself. Do not assume MoveFileExW can rename
    // a direct child directory under that deny-write/delete parent. Instead,
    // replace a file inside the unknown directory whose scan owner just closed.
    // The same quota's next scan must open that directory and the new file again.
    fs::rename(unknown.join("body"), unknown.join("old")).unwrap();
    fs::write(unknown.join("body"), b"changed").unwrap();
    let monitor = Monitor::new();
    assert_eq!(
        run_inventory(&owner, true).unwrap().object_namespace_bytes,
        8
    );
    assert_eq!(frequencies(monitor.events().opened).get(&unknown), Some(&1));
    assert_eq!(frequencies(monitor.events().closed).get(&unknown), Some(&1));
    assert_eq!(monitor.events().file_closes, 2);
    drop(monitor);
    assert_eq!(
        run_inventory(&owner, false).unwrap(),
        run_inventory(&owner, true).unwrap()
    );
    owner.finish(Ok(())).unwrap();
}

#[test]
fn corrupt_or_orphan_attribution_and_registered_body_faults_remain_hard_failures() {
    // Mutate only after admission, so each assertion actually exercises the scan.
    for fault in [
        "intent",
        "completion",
        "orphan",
        "missing",
        "length",
        "kind",
    ] {
        let f = Fixture::new(1000);
        let reference = f.publish(b"body");
        let control = OperationControl::default();
        let owner = f.reserve(&control, 100);
        let intent = owner
            .paths
            .shared_blobs()
            .join(format!("{}.intent", reference.sha256));
        let completion = owner
            .paths
            .shared_blobs()
            .join(format!("{}.complete", reference.sha256));
        let body = evidence::blob_path(&f.0, &reference.sha256).unwrap();
        match fault {
            "intent" => fs::write(&intent, [0; 48]).unwrap(),
            "completion" => fs::write(&completion, [0; 32]).unwrap(),
            "orphan" => fs::remove_file(&intent).unwrap(),
            "missing" => fs::remove_file(&body).unwrap(),
            "length" => fs::write(&body, b"bad").unwrap(),
            "kind" => {
                fs::remove_file(&body).unwrap();
                fs::create_dir(&body).unwrap();
            }
            _ => unreachable!(),
        }
        same_failure(&owner, "vault.data_shared_blob_integrity");
        owner
            .finish::<()>(Err(ToolkitError::invalid("fixture fault")))
            .unwrap_err();
    }
}

#[test]
fn pending_intent_and_exact_repair_peak_have_identical_fresh_accounting() {
    let f = Fixture::new(1000);
    let reference = f.publish(b"body");
    let control = OperationControl::default();
    let owner = f.reserve(&control, 100);
    let root = owner.paths.shared_blobs();
    let intent = root.join(format!("{}.intent", reference.sha256));
    let complete = root.join(format!("{}.complete", reference.sha256));
    let repair = root.join(format!("{}.repair.intent", reference.sha256));
    let raw = fs::read(&intent).unwrap();
    fs::write(&repair, &raw).unwrap();
    let usage = run_inventory(&owner, true).unwrap();
    assert_eq!(usage, run_inventory(&owner, false).unwrap());
    assert_eq!(usage.shared_blob_bytes, 4);
    assert_eq!(usage.retained_shared_peak_bytes, 4);
    assert_eq!(usage.total().unwrap(), 136);
    fs::remove_file(&repair).unwrap();
    fs::remove_file(&complete).unwrap();
    let usage = run_inventory(&owner, true).unwrap();
    assert_eq!(usage, run_inventory(&owner, false).unwrap());
    assert_eq!(usage.shared_blob_bytes, 0);
    assert_eq!(usage.retained_shared_peak_bytes, 8);
    assert_eq!(usage.total().unwrap(), 56);
    owner.finish(Ok(())).unwrap();
}

#[test]
fn cancellation_after_real_unknown_open_checks_close_and_original_owner_cleanup() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 100);
    let unknown = owner.paths.objects.join("unknown");
    fs::create_dir(&unknown).unwrap();
    let monitor = Monitor::new();
    monitor.0.borrow_mut().stop_after_open =
        Some((unknown.clone(), control.tracked_cancel_handle().unwrap()));
    let error = owner.finish(Ok(())).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    let events = monitor.events();
    assert_eq!(events.opened, [unknown.clone()]);
    assert_eq!(
        events
            .closed
            .iter()
            .filter(|path| **path == unknown)
            .count(),
        1
    );
    assert_eq!(events.closed.len(), 13); // One fallback plus all twelve owner directories.
    drop(monitor);
    ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn fallback_close_failures_preserve_both_equal_events_primary_cancel_and_independent_budget() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 100);
    let outer = owner.paths.objects.join("outer");
    let inner = outer.join("inner");
    fs::create_dir_all(&inner).unwrap();
    let budget = Arc::new(AtomicBool::new(false));
    let check = || {
        if budget.load(Ordering::Acquire) {
            Err(ToolkitError::new(
                "test.budget",
                "original independent budget",
                3,
            ))
        } else {
            Ok(())
        }
    };
    let checks = Checks {
        control: &control,
        supplemental: Some(&check),
    };
    let monitor = Monitor::new();
    monitor.0.borrow_mut().fail_closes = vec![outer.clone(), inner.clone()];
    let error = with_inventory_directory(&outer, &owner.directories, &checks, |_| {
        with_inventory_directory(&inner, &owner.directories, &checks, |_| {
            control.tracked_cancel_handle().unwrap().cancel();
            budget.store(true, Ordering::Release);
            Err::<(), _>(ToolkitError::new("test.primary", "original failure", 4))
        })
    })
    .unwrap_err();
    assert_eq!(error.code, "test.primary");
    let outer_causes = error.details["causes"].as_array().unwrap();
    let inner_causes = outer_causes[0]["details"]["causes"].as_array().unwrap();
    // Each checked-close boundary retains its own aggregation. The original
    // finish contract neither flattens these events nor deduplicates across layers.
    for causes in [inner_causes, outer_causes] {
        assert_eq!(causes.len(), 4);
        assert_eq!(causes[0]["code"], "test.primary");
        assert_eq!(causes[1]["code"], "test.close");
        assert_eq!(causes[1]["message"], "same close event");
        assert_eq!(causes[2]["code"], "command.cancelled");
        assert_eq!(causes[3]["code"], "test.budget");
    }
    assert_eq!(monitor.events().opened, [outer.clone(), inner.clone()]);
    assert_eq!(monitor.events().closed, [inner, outer]);
    drop(monitor);
    assert_eq!(
        owner.finish::<()>(Err(error)).unwrap_err().code,
        "test.primary"
    );
}

#[test]
fn borrowed_entry_observes_original_supplemental_stop_without_closing_the_owner() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 0);
    let stop = TrackedCancellation::new();
    let check = || {
        if stop.is_cancelled() {
            Err(ToolkitError::new("test.budget", "original budget", 3))
        } else {
            Ok(())
        }
    };
    let checks = Checks {
        control: &control,
        supplemental: Some(&check),
    };
    let monitor = Monitor::new();
    monitor.0.borrow_mut().stop_after_borrow = Some((owner.paths.objects.clone(), stop.clone()));
    let error = inventory_recovering(&owner.home, &owner.paths, &checks, None, &owner.directories)
        .unwrap_err();
    assert_eq!(error.code, "test.budget");
    assert!(monitor.events().opened.is_empty());
    assert!(monitor.events().closed.is_empty());
    assert!(ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE).is_err());
    drop(monitor);
    owner.finish::<()>(Err(error)).unwrap_err();
    ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn unknown_directory_depth_and_entry_limits_preserve_owned_cleanup() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 0);
    let unknown = owner.paths.objects.join("unknown");
    fs::create_dir(&unknown).unwrap();
    fs::write(unknown.join("entry"), []).unwrap();
    let monitor = Monitor::new();
    let mut entries = MAX_ENTRIES;
    let mut files = 0;
    let mut managed_spools = BTreeMap::from([("unvisited-spool".to_owned(), 123)]);
    let error = scan(
        &unknown,
        &owner.paths.staging,
        &owner.paths.metadata,
        0,
        &mut entries,
        &mut files,
        &owner.control,
        &owner.directories,
        &mut managed_spools,
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.data_cache_scan_limit");
    assert_eq!(monitor.events().opened, [unknown.clone()]);
    assert_eq!(monitor.events().closed, [unknown.clone()]);
    let error = scan(
        &unknown,
        &owner.paths.staging,
        &owner.paths.metadata,
        MAX_DEPTH + 1,
        &mut 0,
        &mut 0,
        &owner.control,
        &owner.directories,
        &mut managed_spools,
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.data_cache_scan_limit");
    assert_eq!(monitor.events().opened.len(), 1); // Depth refusal precedes open.
    assert_eq!(
        managed_spools,
        BTreeMap::from([("unvisited-spool".to_owned(), 123)])
    );
    drop(monitor);
    owner.finish(Ok(())).unwrap();
}

struct Meter;
impl evidence::CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
}

#[test]
fn ordinary_new_body_stream_keeps_two_quota_acquisitions_and_four_full_inventories() {
    let f = Fixture::new(1000);
    let monitor = Monitor::new();
    publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"body"),
        4,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    )
    .unwrap()
    .close()
    .unwrap();
    let events = monitor.events();
    assert_eq!(events.quotas, 2);
    assert_eq!(events.inventories, 4);
    assert_eq!(events.borrowed.len(), 56); // 14 managed traversals per inventory.
    // Only the final inventory has an attributed body and opens its shared root.
    assert_eq!(events.opened, [f.0.join("blobs")]);
    let paths = DataCachePaths::new(&std::path::absolute(&f.0).unwrap());
    assert_eq!(
        events
            .borrowed
            .iter()
            .filter(|path| **path == paths.shared_blobs())
            .count(),
        16 // File inventory, spool declarations, external allowances, shared attribution.
    );
    drop(monitor);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 0);
    assert_eq!(owner.report.existing.total().unwrap(), 84);
    owner.finish(Ok(())).unwrap();
}

#[test]
fn later_quota_inventory_sees_nested_producer_publication_and_refuses_stale_admission() {
    struct Nested<'a> {
        home: &'a Path,
        data: Cursor<&'static [u8]>,
        entered: bool,
    }
    impl Read for Nested<'_> {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if !self.entered {
                self.entered = true;
                publish_shared_blob_with_check(
                    self.home,
                    b"range",
                    5,
                    &OperationControl::default(),
                    &|| Ok(()),
                )
                .map_err(io::Error::other)?;
            }
            self.data.read(out)
        }
    }
    // Keep the nested publisher admissible with its 128-byte durable shard
    // declaration: outer spool64 + nested bodies10 + records80 + declaration128.
    // Once it commits 85 bytes, outer final publication needs 85 + 64 + 128 + 80
    // = 357 bytes and must fail only when its later inventory sees that commit.
    const OUTER: &[u8] = &[b'b'; 64];
    let f = Fixture::new(282);
    let mut producer = Nested {
        home: &f.0,
        data: Cursor::new(OUTER),
        entered: false,
    };
    let monitor = Monitor::new();
    let error = publish_shared_blob_stream_with_check(
        &f.0,
        &mut producer,
        OUTER.len() as u64,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    )
    .err()
    .unwrap();
    assert!(producer.entered);
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    assert_eq!(monitor.events().quotas, 3); // Outer A, nested memory owner, outer B.
    assert_eq!(monitor.events().inventories, 5); // Outer B admission fails; no success finish scan.
    drop(monitor);
    assert!(
        !evidence::blob_path(&f.0, &storage::sha256(OUTER))
            .unwrap()
            .exists()
    );
    let control = OperationControl::default();
    let owner = f.reserve(&control, 0);
    assert_eq!(owner.report.existing.shared_blob_bytes, 5);
    assert_eq!(owner.report.existing.total().unwrap(), 85);
    owner.finish(Ok(())).unwrap();
}

#[test]
fn metadata_finish_verifies_once_on_success_and_failure_and_releases_quota() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    for evidence in [false, true] {
        for failed in [false, true] {
            let monitor = Monitor::new();
            let result = if failed {
                Err(ToolkitError::new(
                    "test.primary",
                    "metadata write failed",
                    5,
                ))
            } else {
                Ok(())
            };
            let result = if evidence {
                EvidenceQuota::acquire(&f.0, &control, &|| Ok(()))
                    .unwrap()
                    .finish(result)
            } else {
                MetadataQuota::acquire(&f.0, &control, &|| Ok(()))
                    .unwrap()
                    .finish(result)
            };
            assert_eq!(monitor.events().inventories, 2); // Admission and final usage.
            if failed {
                assert_eq!(result.unwrap_err().code, "test.primary");
            } else {
                result.unwrap();
            }
            drop(monitor);
            ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
                .unwrap()
                .close()
                .unwrap();
        }
    }
}

#[test]
fn metadata_only_inventory_keeps_its_original_fallback_scope() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 100);
    fs::write(owner.paths.metadata.join("private"), b"meta").unwrap();
    fs::write(owner.paths.objects.join("not-metadata"), b"object").unwrap();
    let monitor = Monitor::new();
    let usage = metadata_inventory(&owner.paths, &owner.control, &[]).unwrap();
    assert_eq!(usage.private_metadata_namespace_bytes, 4);
    assert_eq!(usage.total().unwrap(), 4);
    assert_eq!(monitor.events().opened, [owner.paths.metadata.clone()]);
    assert_eq!(monitor.events().closed, [owner.paths.metadata.clone()]);
    assert!(monitor.events().borrowed.is_empty());
    drop(monitor);
    owner.finish(Ok(())).unwrap();
}

#[test]
fn metadata_read_owners_reuse_only_private_metadata_and_release_quota() {
    for evidence in [false, true] {
        let f = Fixture::new(1);
        let paths = DataCachePaths::new(&std::path::absolute(&f.0).unwrap());
        fs::create_dir_all(&paths.metadata).unwrap();
        let name = if evidence {
            "evidence-v2.sqlite"
        } else {
            "cache-v1.sqlite"
        };
        fs::write(paths.metadata.join(name), b"meta").unwrap();
        // Unrelated derived data may be unavailable. A metadata-only read must
        // neither open that namespace nor recreate other payload directories.
        fs::write(paths.roots()[0], b"unavailable payload namespace").unwrap();
        let control = OperationControl::default();
        let monitor = Monitor::new();
        if evidence {
            let mut owner = EvidenceQuota::acquire_read(&f.0, &control, &|| Ok(())).unwrap();
            assert_eq!(owner.pin_existing_main().unwrap(), Some(4));
            let report = owner.snapshot().unwrap();
            assert_eq!(report.existing.private_metadata_namespace_bytes, 4);
            assert_eq!(report.existing.total().unwrap(), 4);
            owner.finish(Ok(())).unwrap();
        } else {
            let owner = MetadataReadQuota::acquire(&f.0, &control, &|| Ok(())).unwrap();
            assert_eq!(owner.length(name).unwrap(), Some(4));
            owner.verify_recovery().unwrap();
            owner.finish(Ok(())).unwrap();
        }
        let events = monitor.events();
        assert!(events.opened.is_empty());
        assert!(!events.borrowed.is_empty());
        assert!(events.borrowed.iter().all(|path| *path == paths.metadata));
        // Initialization closes its shared state parent and home. The retained
        // metadata/locks owners keep independent shared ancestry until finish.
        assert_eq!(
            events.closed,
            [
                f.0.join("state"),
                f.0.clone(),
                paths.metadata.clone(),
                f.0.join("locks"),
            ]
        );
        let moved = paths.metadata.with_extension("closed");
        fs::rename(&paths.metadata, &moved).unwrap();
        assert_eq!(fs::read(moved.join(name)).unwrap(), b"meta");
        assert!(!paths.roots()[1].exists());
        assert!(!paths.roots()[2].exists());
        drop(monitor);
        ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
            .unwrap()
            .close()
            .unwrap();
    }
}
