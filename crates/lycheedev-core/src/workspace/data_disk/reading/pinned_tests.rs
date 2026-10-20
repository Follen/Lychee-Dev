use super::*;
use crate::{
    control::TrackedCancellation,
    workspace::{self, data_resources::Policy},
};
use lycheedev_windows::filesystem::owned_cleanup::CleanupCandidate;
use std::{
    os::windows::io::AsRawHandle,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-derived-pin-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn publish(&self, body: &[u8]) -> (DerivedObject, String) {
        let control = OperationControl::default();
        let resources = data_resources::acquire(&self.0, Policy::Ordinary, &control).unwrap();
        let owner = resources
            .reserve_derived_cache(AdditionalPeak {
                objects: body.len() as u64,
                staging: body.len() as u64,
                generations: 8,
                ..Default::default()
            })
            .unwrap();
        let mut writer = owner.begin_object(body.len() as u64).unwrap();
        writer.write_bytes(body).unwrap();
        let object = writer.commit().unwrap();
        let key = storage::sha256(b"pin-generation");
        owner.publish_generation(&key, b"manifest").unwrap();
        owner.finish(Ok(())).unwrap();
        resources.close().unwrap();
        (object, key)
    }
    fn path(&self, object: &DerivedObject) -> PathBuf {
        DataCachePaths::new(&self.0).object(&object.sha256).unwrap()
    }
    fn quota(&self) -> ExclusiveLock {
        ExclusiveLock::resource(&self.0.join("locks"), QUOTA_RESOURCE).unwrap()
    }
    fn writable(&self, object: &DerivedObject) {
        storage::close_file(
            fs::OpenOptions::new()
                .write(true)
                .open(self.path(object))
                .unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn failed<T>(result: Result<T>) -> ToolkitError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("expected a failure"),
    }
}
fn check() -> DerivedReadCheck {
    Arc::new(|| Ok(()))
}
fn budget_stop() -> ToolkitError {
    ToolkitError::new("test.pin_budget", "original independent budget stopped", 3)
}
fn cleanup_busy(path: &Path) {
    match CleanupCandidate::open(path, false) {
        Err(error) => assert_eq!(error.raw_os_error(), Some(32)),
        Ok(candidate) => {
            storage::close_file(candidate.into_file()).unwrap();
            panic!("live read pin must deny DELETE access");
        }
    }
}
fn pinned(
    resources: &DataResources,
    object: &DerivedObject,
    check: &DerivedReadCheck,
) -> OwnedDerivedObjectReader {
    resources
        .read_derived_cache_owned_with_check(check)
        .unwrap()
        .open_pinned_object(&object.sha256, object.bytes)
        .unwrap()
        .unwrap()
}

#[test]
fn owned_reader_releases_quota_before_full_sha_and_keeps_original_check_and_handle() {
    let f = Fixture::new();
    let raw = vec![42; 131073];
    let (object, key) = f.publish(&raw);
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let locks = f.0.join("locks");
    let observed = Arc::new(AtomicBool::new(false));
    let hashing_observed = observed.clone();
    HASHED_BYTES.with(|bytes| bytes.set(0));
    let check: DerivedReadCheck = Arc::new(move || {
        if HASHED_BYTES.with(Cell::get) >= 65536 {
            // This is the real full-hash loop, not a timing assertion. Its
            // independent observer can immediately acquire the actual quota.
            ExclusiveLock::resource(&locks, QUOTA_RESOURCE)?.close()?;
            hashing_observed.store(true, Ordering::SeqCst);
        }
        Ok(())
    });
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    assert_eq!(lease.read_generation(&key).unwrap().unwrap(), b"manifest");
    let mut reader = lease
        .open_pinned_object(&object.sha256, object.bytes)
        .unwrap()
        .unwrap();
    assert!(observed.load(Ordering::SeqCst));
    // Owned handoff has a distinct Send + Sync type and retains the exact checks.
    fn assert_owned<T: Send + Sync>(_: &T) {}
    assert_owned(&reader);
    assert_eq!(reader.control.control.deadline(), control.deadline());
    assert!(Arc::ptr_eq(&reader.control.supplemental, &check));
    let handle = reader.file.as_ref().unwrap().as_raw_handle();
    assert_eq!(reader.seek_checked(SeekFrom::Current(0)).unwrap(), 0);
    let mut bytes = vec![0; raw.len()];
    reader.read_exact_checked(&mut bytes).unwrap();
    assert_eq!(bytes, raw);
    assert_eq!(reader.file.as_ref().unwrap().as_raw_handle(), handle);
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(f.path(&object))
            .is_err()
    );
    reader.close().unwrap();
    f.writable(&object);
    resources.close().unwrap();
}

#[test]
fn multiple_owned_readers_share_one_physical_object_and_concurrent_cleanup_skips_busy() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let mut first = pinned(&resources, &object, &check);
    let mut second = pinned(&resources, &object, &check);
    assert_ne!(
        first.file.as_ref().unwrap().as_raw_handle(),
        second.file.as_ref().unwrap().as_raw_handle()
    );
    first.seek_checked(SeekFrom::Start(1)).unwrap();
    let mut byte = [0];
    second.read_exact_checked(&mut byte).unwrap();
    assert_eq!(&byte, b"b"); // Independent positions on the same pinned body.
    let home = f.0.clone();
    let path = f.path(&object);
    std::thread::spawn(move || {
        let quota = ExclusiveLock::resource(&home.join("locks"), QUOTA_RESOURCE).unwrap();
        // Exactly one native open; no waiting on a reader while holding quota.
        cleanup_busy(&path);
        quota.close().unwrap();
    })
    .join()
    .unwrap();
    let quota = f.quota();
    first.close().unwrap(); // Close neither reacquires quota nor scans occupancy.
    cleanup_busy(&f.path(&object));
    second.close().unwrap();
    let candidate = storage::authenticate_cleanup_file(
        &f.path(&object),
        object.bytes,
        &object.sha256,
        &control,
    )
    .unwrap();
    // Availability is not deletion authority; this test retains the manifest.
    storage::close_file(candidate.into_file()).unwrap();
    quota.close().unwrap();
    resources.close().unwrap();
    assert_eq!(fs::read(f.path(&object)).unwrap(), b"body");
}

#[test]
fn owned_reader_allows_other_derived_and_shared_data_publication_without_false_growth_error() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let mut reader = pinned(&resources, &object, &check);
    let writer = resources
        .reserve_derived_cache(AdditionalPeak {
            objects: 5,
            staging: 5,
            generations: 5,
            ..Default::default()
        })
        .unwrap();
    let mut output = writer.begin_object(5).unwrap();
    output.write_bytes(b"other").unwrap();
    output.commit().unwrap();
    writer
        .publish_generation(&storage::sha256(b"other-generation"), b"other")
        .unwrap();
    writer.finish(Ok(())).unwrap();
    // The actual registered shared-blob path takes quota -> blob. A live
    // cursor cannot still own quota or this would consume its original deadline.
    let shared =
        publish_shared_blob_with_check(&f.0, b"capture-body", 12, &control, &|| Ok(())).unwrap();
    assert_eq!(shared.sha256, storage::sha256(b"capture-body"));
    let mut bytes = [0; 4];
    reader.read_exact_checked(&mut bytes).unwrap();
    assert_eq!(&bytes, b"body");
    reader.close().unwrap(); // Global occupancy legitimately exceeds its old snapshot.
    resources.close().unwrap();
}

#[test]
fn owned_reader_keeps_object_and_all_ancestors_fixed_after_manifest_retirement() {
    let f = Fixture::new();
    let (object, key) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let manifest = lease.read_generation(&key).unwrap().unwrap();
    let mut reader = lease
        .open_pinned_object(&object.sha256, object.bytes)
        .unwrap()
        .unwrap();
    let quota = f.quota();
    let path = f.path(&object);
    assert!(fs::write(&path, b"evil").is_err());
    assert!(fs::remove_file(&path).is_err());
    assert!(fs::rename(&path, path.with_file_name("substituted")).is_err());
    for directory in [
        path.parent().unwrap().to_path_buf(),
        DataCachePaths::new(&f.0).objects,
        f.0.clone(),
    ] {
        assert!(fs::rename(&directory, directory.with_file_name("substituted-parent")).is_err());
    }
    // The caller already authenticated these manifest bytes. Removing only that
    // reference cannot revoke another reader's independent physical body pin.
    fs::remove_file(
        DataCachePaths::new(&f.0)
            .generations
            .join(format!("{key}.json")),
    )
    .unwrap();
    assert_eq!(manifest, b"manifest");
    let mut bytes = [0; 4];
    reader.read_exact_checked(&mut bytes).unwrap();
    assert_eq!(&bytes, b"body");
    reader.close().unwrap();
    let candidate =
        storage::authenticate_cleanup_file(&path, object.bytes, &object.sha256, &control).unwrap();
    let (file, deleted) = candidate.discard();
    deleted.unwrap();
    storage::close_file(file).unwrap();
    quota.close().unwrap();
    assert!(!path.exists());
    resources.close().unwrap();
}

#[test]
fn owned_read_preserves_reduced_quota_report_without_granting_new_write_capacity() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let mut config = workspace::default_config();
    config["dataCache"]["maxBytes"] = serde_json::json!(1);
    storage::atomic_json(&f.0.join("config.json"), &config).unwrap();
    let check = check();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    assert_eq!(lease.report().configured_max_bytes, 1);
    assert_eq!(lease.report().existing.total().unwrap(), 12);
    assert!(lease.report().over_budget);
    assert!(!lease.report().coverage.total_data_coverage_complete);
    let reader = lease
        .open_pinned_object(&object.sha256, object.bytes)
        .unwrap()
        .unwrap();
    assert_eq!(
        failed(resources.reserve_derived_cache(AdditionalPeak::default())).code,
        "vault.data_cache_budget_exceeded"
    );
    reader.close().unwrap();
    resources.close().unwrap();
    assert_eq!(fs::read(f.path(&object)).unwrap(), b"body");
}

#[test]
fn optional_owned_read_misses_only_an_original_absent_shard_or_body_and_legacy_stays_strict() {
    for shard_missing in [false, true] {
        let f = Fixture::new();
        let (object, _) = f.publish(b"body");
        let path = f.path(&object);
        fs::remove_file(&path).unwrap();
        if shard_missing {
            fs::remove_dir(path.parent().unwrap()).unwrap();
        }
        let control = OperationControl::default();
        let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
        let check = check();
        let lease = resources
            .read_derived_cache_owned_with_check(&check)
            .unwrap();
        assert!(
            lease
                .open_pinned_object(&object.sha256, object.bytes)
                .unwrap()
                .is_none()
        );
        f.quota().close().unwrap();
        let legacy_check = || Ok(());
        let legacy = resources
            .read_derived_cache_with_check(&legacy_check)
            .unwrap();
        let error = failed(legacy.open_object(&object.sha256, object.bytes));
        assert_eq!(error.code, "vault.data_cache_integrity");
        assert_eq!(
            error.message,
            if shard_missing {
                "Referenced Data object shard is missing"
            } else {
                "Referenced Data object is missing"
            }
        );
        legacy.finish(Ok(())).unwrap();
        resources.close().unwrap();
        assert!(!path.exists());
    }
}

#[test]
fn optional_owned_read_never_masks_invalid_digest_kind_length_content_or_sharing_failure() {
    for fault in [
        "digest",
        "length",
        "content",
        "directory",
        "shard-file",
        "sharing",
    ] {
        let f = Fixture::new();
        let (object, _) = f.publish(b"body");
        let control = OperationControl::default();
        let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
        let check = check();
        let lease = resources
            .read_derived_cache_owned_with_check(&check)
            .unwrap();
        let path = f.path(&object);
        let mut writer = None;
        match fault {
            "length" => fs::write(&path, b"bad").unwrap(),
            "content" => fs::write(&path, b"evil").unwrap(),
            "directory" => {
                fs::remove_file(&path).unwrap();
                fs::create_dir(&path).unwrap();
            }
            "shard-file" => {
                fs::remove_file(&path).unwrap();
                fs::remove_dir(path.parent().unwrap()).unwrap();
                fs::write(path.parent().unwrap(), b"not-a-directory").unwrap();
            }
            "sharing" => writer = Some(fs::OpenOptions::new().write(true).open(&path).unwrap()),
            _ => (),
        }
        let digest = if fault == "digest" {
            object.sha256.to_ascii_uppercase()
        } else {
            object.sha256.clone()
        };
        let error = failed(lease.open_pinned_object(&digest, object.bytes));
        assert_eq!(
            error.code,
            match fault {
                "digest" => "input.invalid",
                "sharing" => "external.io",
                _ => "vault.data_cache_integrity",
            },
            "{fault}: {error}"
        );
        if let Some(writer) = writer {
            storage::close_file(writer).unwrap();
        }
        f.quota().close().unwrap();
        resources.close().unwrap();
    }
}

#[test]
fn owned_length_validation_precedes_quota_transfer_and_hashing() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    HASHED_BYTES.with(|bytes| bytes.set(0));
    let error = failed(lease.open_pinned_object_with(
        &object.sha256,
        object.bytes + 1,
        |_| panic!("wrong-length handle reached quota transfer"),
        storage::close_file,
        close_directory,
    ));
    assert_eq!(error.code, "vault.data_cache_integrity");
    assert_eq!(HASHED_BYTES.with(Cell::get), 0);
    f.writable(&object);
    f.quota().close().unwrap();
    resources.close().unwrap();
}

#[test]
fn owned_budget_is_observed_inside_actual_quota_wait_without_replacing_its_arc() {
    let f = Fixture::new();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let held = f.quota();
    let waited = Arc::new(AtomicBool::new(false));
    let observe = waited.clone();
    ExclusiveLock::set_blob_wait_observer(move || observe.store(true, Ordering::SeqCst));
    let stopped = waited.clone();
    let check: DerivedReadCheck = Arc::new(move || {
        if stopped.load(Ordering::SeqCst) {
            Err(budget_stop())
        } else {
            Ok(())
        }
    });
    assert_eq!(
        failed(resources.read_derived_cache_owned_with_check(&check)).code,
        "test.pin_budget"
    );
    assert!(waited.load(Ordering::SeqCst));
    assert_eq!(
        failed(ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)).code,
        "workspace.busy"
    );
    held.close().unwrap();
    resources.close().unwrap();
}

#[test]
fn owned_hash_failure_checked_closes_pin_and_preserves_both_cleanup_events() {
    let f = Fixture::new();
    let (object, _) = f.publish(&vec![42; 131073]);
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    HASHED_BYTES.with(|bytes| bytes.set(0));
    let armed = Arc::new(AtomicBool::new(false));
    let stopped = armed.clone();
    let check: DerivedReadCheck = Arc::new(move || {
        if stopped.load(Ordering::SeqCst) && HASHED_BYTES.with(Cell::get) >= 65536 {
            Err(budget_stop())
        } else {
            Ok(())
        }
    });
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    armed.store(true, Ordering::SeqCst);
    let files = Cell::new(0);
    let directories = Cell::new(0);
    let error = failed(lease.open_pinned_object_with(
        &object.sha256,
        object.bytes,
        |owner| owner.finish(Ok(())),
        |file| {
            files.set(files.get() + 1);
            storage::close_file(file)?;
            Err(io::Error::other("injected pin file close"))
        },
        |directory| {
            directories.set(directories.get() + 1);
            let mut errors = close_directory(directory);
            errors.push(ToolkitError::new(
                "test.pin_directory_close",
                "injected pin directory close",
                5,
            ));
            errors
        },
    ));
    assert_eq!(error.code, "test.pin_budget");
    assert_eq!(HASHED_BYTES.with(Cell::get), 65536);
    assert_eq!(files.get(), 1);
    assert_eq!(directories.get(), 1);
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 3);
    assert_eq!(causes[1]["code"], "external.io");
    assert_eq!(causes[2]["code"], "test.pin_directory_close");
    armed.store(false, Ordering::SeqCst);
    f.writable(&object);
    f.quota().close().unwrap();
    resources.close().unwrap();
}

#[test]
fn owned_quota_close_failure_prevents_hash_and_checked_closes_every_transferred_owner_once() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let quota_directories = lease.lease.owner.directories.len();
    let seen_directories = Cell::new(0);
    let quota_closes = Cell::new(0);
    let pin_closes = Cell::new(0);
    let pin_directories = Cell::new(0);
    HASHED_BYTES.with(|bytes| bytes.set(0));
    let error = failed(lease.open_pinned_object_with(
        &object.sha256,
        object.bytes,
        |mut owner| {
            let result = owner.verify_usage().map(|_| ());
            let cleanup = owner.cleanup_with(
                |directory| {
                    seen_directories.set(seen_directories.get() + 1);
                    close_directory(directory)
                },
                |quota| {
                    quota_closes.set(quota_closes.get() + 1);
                    quota.close()?;
                    Err(ToolkitError::new(
                        "test.quota_close",
                        "injected consumed quota close",
                        5,
                    ))
                },
            );
            join(result, cleanup, &owner.control)
        },
        |file| {
            pin_closes.set(pin_closes.get() + 1);
            storage::close_file(file)?;
            Err(io::Error::other("injected consumed pin close"))
        },
        |directory| {
            pin_directories.set(pin_directories.get() + 1);
            let mut errors = close_directory(directory);
            errors.push(ToolkitError::new(
                "test.pin_directory_close",
                "injected consumed pin directory close",
                5,
            ));
            errors
        },
    ));
    assert_eq!(seen_directories.get(), quota_directories);
    assert_eq!(quota_closes.get(), 1);
    assert_eq!(pin_closes.get(), 1);
    assert_eq!(pin_directories.get(), 1);
    assert_eq!(HASHED_BYTES.with(Cell::get), 0);
    assert_eq!(error.code, "test.quota_close");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 3);
    assert_eq!(causes[1]["code"], "external.io");
    assert_eq!(causes[2]["code"], "test.pin_directory_close");
    f.writable(&object);
    f.quota().close().unwrap();
    resources.close().unwrap();
}

#[test]
fn optional_owned_miss_still_reports_failed_quota_close() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    fs::remove_file(f.path(&object)).unwrap();
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let error = failed(lease.open_pinned_object_with(
        &object.sha256,
        object.bytes,
        |owner| {
            owner.finish(Ok(()))?;
            Err(ToolkitError::new(
                "test.quota_close",
                "injected consumed quota close",
                5,
            ))
        },
        |_| panic!("absent object acquired a reader"),
        |_| panic!("absent object transferred a directory"),
    ));
    assert_eq!(error.code, "test.quota_close");
    f.quota().close().unwrap();
    resources.close().unwrap();
}

#[test]
fn owned_reader_preserves_original_cancellation_and_budget_through_explicit_close() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let budget = stopped.clone();
    let check: DerivedReadCheck = Arc::new(move || {
        if budget.load(Ordering::SeqCst) {
            Err(budget_stop())
        } else {
            Ok(())
        }
    });
    let mut reader = pinned(&resources, &object, &check);
    stopped.store(true, Ordering::SeqCst);
    assert_eq!(
        reader.read_checked(&mut [0]).unwrap_err().code,
        "test.pin_budget"
    );
    stopped.store(false, Ordering::SeqCst);
    assert_eq!(
        reader.seek_checked(SeekFrom::Start(0)).unwrap_err().code,
        "test.pin_budget"
    );
    assert_eq!(reader.error().unwrap().code, "test.pin_budget");
    token.cancel();
    stopped.store(true, Ordering::SeqCst);
    let files = Cell::new(0);
    let directories = Cell::new(0);
    let handle = reader.file.as_ref().unwrap().as_raw_handle();
    let error = reader
        .close_after_with::<()>(
            Err(ToolkitError::new("test.consumer", "consumer failure", 4)),
            |file| {
                assert_eq!(file.as_raw_handle(), handle);
                files.set(files.get() + 1);
                storage::close_file(file)?;
                Err(io::Error::other("injected pin close"))
            },
            |directory| {
                directories.set(directories.get() + 1);
                let mut errors = close_directory(directory);
                errors.push(ToolkitError::new(
                    "test.pin_directory_close",
                    "injected directory close",
                    5,
                ));
                errors
            },
        )
        .unwrap_err();
    assert_eq!(files.get(), 1);
    assert_eq!(directories.get(), 1);
    assert_eq!(error.code, "test.consumer");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 5);
    assert_eq!(causes[1]["code"], "external.io");
    assert_eq!(causes[2]["code"], "test.pin_directory_close");
    assert_eq!(causes[3]["code"], "command.cancelled");
    assert_eq!(causes[4]["code"], "test.pin_budget");
    f.writable(&object);
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}

#[test]
fn cancellation_after_quota_release_never_returns_an_unhashed_reader() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    HASHED_BYTES.with(|bytes| bytes.set(0));
    let error = failed(lease.open_pinned_object_with(
        &object.sha256,
        object.bytes,
        |owner| {
            owner.finish(Ok(()))?;
            token.cancel();
            Ok(())
        },
        storage::close_file,
        close_directory,
    ));
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(HASHED_BYTES.with(Cell::get), 0);
    f.writable(&object);
    f.quota().close().unwrap();
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}

#[test]
fn owned_reader_drop_is_only_fallback_release_and_preserves_body_and_manifest() {
    let f = Fixture::new();
    let (object, key) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let checker_calls = Arc::new(AtomicUsize::new(0));
    let observed = checker_calls.clone();
    let check: DerivedReadCheck = Arc::new(move || {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let reader = pinned(&resources, &object, &check);
    drop(check); // The owned reader retains the same independent checker.
    cleanup_busy(&f.path(&object));
    let calls = checker_calls.load(Ordering::SeqCst);
    drop(reader);
    assert_eq!(checker_calls.load(Ordering::SeqCst), calls); // Drop never asks for admission.
    f.writable(&object);
    f.quota().close().unwrap();
    resources.close().unwrap();
    assert_eq!(fs::read(f.path(&object)).unwrap(), b"body");
    assert_eq!(
        fs::read(
            DataCachePaths::new(&f.0)
                .generations
                .join(format!("{key}.json"))
        )
        .unwrap(),
        b"manifest"
    );
}

#[test]
fn owned_manifest_early_return_finishes_quota_without_ever_opening_a_body() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = check();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    assert!(
        lease
            .read_generation(&storage::sha256(b"absent-generation"))
            .unwrap()
            .is_none()
    );
    lease.finish(Ok(())).unwrap();
    f.quota().close().unwrap();
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let error = lease
        .finish::<()>(Err(ToolkitError::new(
            "test.manifest",
            "semantic manifest rejection",
            4,
        )))
        .unwrap_err();
    assert_eq!(error.code, "test.manifest");
    f.writable(&object);
    f.quota().close().unwrap();
    resources.close().unwrap();
}
