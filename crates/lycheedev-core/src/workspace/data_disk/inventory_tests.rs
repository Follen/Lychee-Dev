//! Executed-boundary observations: successful Directory constructions and actual
//! checked closes, not estimates of native handle/syscall counts or elapsed time.
use super::*;
use crate::{evidence, workspace};
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
    stop_after_open: Option<(PathBuf, Arc<AtomicBool>)>,
    stop_after_borrow: Option<(PathBuf, Arc<AtomicBool>)>,
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
                stop.store(true, Ordering::Release);
            }
        }
    });
}
pub(super) fn directory_borrowed(path: &Path) {
    observed(|state| {
        state.events.borrowed.push(path.to_owned());
        if let Some((target, stop)) = &state.stop_after_borrow {
            if path == target {
                stop.store(true, Ordering::Release);
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
fn full_inventory_reuses_ten_owners_and_second_attribution_walk_with_identical_file_work() {
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
    assert_eq!(managed.len(), 10);
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
    assert_eq!(fallback_events.opened.len(), 13); // 11 managed traversals + 2 unknown.
    assert_eq!(reused_events.opened.len(), 2);
    assert_eq!(fallback_events.closed.len(), 15);
    assert_eq!(
        fallback_events.closed.len() - reused_events.closed.len(),
        11
    );
    assert!(fallback_events.borrowed.is_empty());
    let mut expected_managed = frequencies(managed.clone());
    *expected_managed
        .get_mut(&owner.paths.shared_blobs())
        .unwrap() += 1;
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
    assert_eq!(owner.directories.len(), 11);
    owner.finish(Ok(())).unwrap();
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
    assert_eq!(monitor.events().borrowed.len(), 33);
    owner.finish(Ok(())).unwrap();
    let events = monitor.events();
    assert_eq!(events.inventories, 4);
    assert_eq!(events.borrowed.len(), 44);
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
    assert_eq!(
        run_inventory(&owner, true).unwrap().object_namespace_bytes,
        1
    );
    // No unknown-child owner escapes the inventory. A later replacement is new work.
    fs::rename(&unknown, owner.paths.objects.join("old")).unwrap();
    fs::create_dir(&unknown).unwrap();
    fs::write(unknown.join("body"), b"changed").unwrap();
    assert_eq!(
        run_inventory(&owner, true).unwrap().object_namespace_bytes,
        8
    );
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
    monitor.0.borrow_mut().stop_after_open = Some((unknown.clone(), control.cancel_handle()));
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
    assert_eq!(events.closed.len(), 12); // One fallback plus all eleven owner directories.
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
            control.cancel_handle().store(true, Ordering::Release);
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
    let stop = Arc::new(AtomicBool::new(false));
    let check = || {
        if stop.load(Ordering::Acquire) {
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
    let error = scan(
        &unknown,
        &owner.paths.staging,
        &owner.paths.metadata,
        0,
        &mut entries,
        &mut files,
        &owner.control,
        &owner.directories,
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
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.data_cache_scan_limit");
    assert_eq!(monitor.events().opened.len(), 1); // Depth refusal precedes open.
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
    assert_eq!(events.borrowed.len(), 44);
    assert!(events.opened.is_empty());
    let paths = DataCachePaths::new(&std::path::absolute(&f.0).unwrap());
    assert_eq!(
        events
            .borrowed
            .iter()
            .filter(|path| **path == paths.shared_blobs())
            .count(),
        8
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
    let f = Fixture::new(100);
    let mut producer = Nested {
        home: &f.0,
        data: Cursor::new(b"body"),
        entered: false,
    };
    let monitor = Monitor::new();
    let error = publish_shared_blob_stream_with_check(
        &f.0,
        &mut producer,
        4,
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
        !evidence::blob_path(&f.0, &storage::sha256(b"body"))
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
fn metadata_only_inventory_keeps_its_original_fallback_scope() {
    let f = Fixture::new(1000);
    let control = OperationControl::default();
    let owner = f.reserve(&control, 100);
    fs::write(owner.paths.metadata.join("private"), b"meta").unwrap();
    fs::write(owner.paths.objects.join("not-metadata"), b"object").unwrap();
    let monitor = Monitor::new();
    let usage = metadata_inventory(&owner.paths, &owner.control).unwrap();
    assert_eq!(usage.private_metadata_namespace_bytes, 4);
    assert_eq!(usage.total().unwrap(), 4);
    assert_eq!(monitor.events().opened, [owner.paths.metadata.clone()]);
    assert_eq!(monitor.events().closed, [owner.paths.metadata.clone()]);
    assert!(monitor.events().borrowed.is_empty());
    drop(monitor);
    owner.finish(Ok(())).unwrap();
}
