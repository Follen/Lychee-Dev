use super::*;
use crate::{
    control::TrackedCancellation,
    workspace::{self, data_resources::Policy},
};
use std::{
    os::windows::io::AsRawHandle,
    rc::Rc,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-derived-read-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn publish(&self, body: &[u8], manifest: &[u8]) -> (DerivedObject, String) {
        let control = OperationControl::default();
        let resources = data_resources::acquire(&self.0, Policy::Ordinary, &control).unwrap();
        let owner = resources
            .reserve_derived_cache(AdditionalPeak {
                objects: body.len() as u64,
                staging: body.len() as u64,
                generations: manifest.len() as u64,
                ..Default::default()
            })
            .unwrap();
        let mut writer = owner.begin_object(body.len() as u64).unwrap();
        writer.write_bytes(body).unwrap();
        let object = writer.commit().unwrap();
        let key = storage::sha256(b"fixture-generation");
        owner.publish_generation(&key, manifest).unwrap();
        owner.finish(Ok(())).unwrap();
        resources.close().unwrap();
        (object, key)
    }
    fn limit(&self, limit: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(limit);
        storage::atomic_json(&self.0.join("config.json"), &config).unwrap();
    }
    fn path(&self, object: &DerivedObject) -> PathBuf {
        DataCachePaths::new(&self.0).object(&object.sha256).unwrap()
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
fn budget_stop() -> ToolkitError {
    ToolkitError::new("test.read_budget", "independent read budget stopped", 3)
}

#[test]
fn real_immutable_reader_reuses_one_query_owner_and_one_verified_read_seek_handle() {
    let f = Fixture::new();
    let raw: Vec<_> = (0..131073).map(|i| (i % 251) as u8).collect();
    let (object, key) = f.publish(&raw, b"manifest");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = || Ok(());
    let lease = resources.read_derived_cache_with_check(&check).unwrap();
    assert!(std::ptr::eq(lease.owner._resources.unwrap(), &resources));
    assert_eq!(
        lease.owner.report.additional_peak,
        AdditionalPeak::default()
    );
    assert_eq!(lease.read_generation(&key).unwrap().unwrap(), b"manifest");
    let before = lease.report().existing;
    let mut reader = lease.open_object(&object.sha256, object.bytes).unwrap();
    let handle = reader.file.as_ref().unwrap().as_raw_handle();
    assert_eq!(reader.len(), raw.len() as u64);
    assert!(!reader.is_empty());
    assert_eq!(reader.seek_checked(SeekFrom::Current(0)).unwrap(), 0);
    let mut first = [0; 100000];
    assert_eq!(reader.read_checked(&mut first).unwrap(), 65536);
    assert_eq!(&first[..65536], &raw[..65536]);
    assert_eq!(
        reader.seek(SeekFrom::End(-5)).unwrap(),
        raw.len() as u64 - 5
    );
    let mut tail = [0; 5];
    reader.read_exact_checked(&mut tail).unwrap();
    assert_eq!(tail, raw[raw.len() - 5..]);
    assert_eq!(reader.read(&mut [0]).unwrap(), 0);
    assert_eq!(reader.seek_checked(SeekFrom::Start(0)).unwrap(), 0);
    let mut all = vec![0; raw.len()];
    reader.read_exact_checked(&mut all).unwrap();
    assert_eq!(all, raw);
    assert_eq!(reader.file.as_ref().unwrap().as_raw_handle(), handle);
    assert!(
        fs::OpenOptions::new()
            .write(true)
            .open(f.path(&object))
            .is_err()
    );
    assert!(fs::remove_file(f.path(&object)).is_err());
    assert_eq!(
        failed(ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)).code,
        "workspace.busy"
    );
    reader.close().unwrap();
    assert_eq!(lease.owner.verify_usage().unwrap(), before);
    lease.finish(Ok(())).unwrap();
    resources.close().unwrap();
    assert_eq!(fs::read(f.path(&object)).unwrap(), raw);
}

#[test]
fn reduced_limit_allows_only_zero_growth_read_capability_and_keeps_actual_usage_visible() {
    let f = Fixture::new();
    let (object, key) = f.publish(b"body", b"manifest");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    f.limit(1); // The read owner must reload current policy, not its query snapshot.
    assert_eq!(
        failed(resources.reserve_derived_cache(AdditionalPeak::default())).code,
        "vault.data_cache_budget_exceeded"
    );
    let check = || Ok(());
    let lease = resources.read_derived_cache_with_check(&check).unwrap();
    let report = lease.report();
    assert_eq!(report.configured_max_bytes, 1);
    assert_eq!(report.existing.total().unwrap(), 12);
    assert!(report.over_budget);
    assert!(!report.coverage.total_data_coverage_complete);
    assert_eq!(lease.read_generation(&key).unwrap().unwrap(), b"manifest");
    lease
        .open_object(&object.sha256, object.bytes)
        .unwrap()
        .close()
        .unwrap();
    lease.finish(Ok(())).unwrap();
    assert_eq!(
        failed(resources.reserve_derived_cache(AdditionalPeak {
            objects: 1,
            staging: 1,
            ..Default::default()
        }))
        .code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(fs::read(f.path(&object)).unwrap(), b"body");
    resources.close().unwrap();
}

#[test]
fn ordinary_write_reservation_can_borrow_reads_without_recursive_quota_or_new_charges() {
    let f = Fixture::new();
    let (object, key) = f.publish(b"body", b"manifest");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources
        .reserve_derived_cache(AdditionalPeak::default())
        .unwrap();
    assert_eq!(owner.read_generation(&key).unwrap().unwrap(), b"manifest");
    let mut reader = owner.open_object(&object.sha256, object.bytes).unwrap();
    let mut bytes = [0; 4];
    reader.read_exact_checked(&mut bytes).unwrap();
    assert_eq!(&bytes, b"body");
    reader.close().unwrap();
    assert_eq!(owner.charged.get(), AdditionalPeak::default());
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}

#[test]
fn generation_reads_bound_allocation_and_only_absent_valid_identity_is_a_miss() {
    for length in [0, MAX_GENERATION_BYTES, MAX_GENERATION_BYTES + 1] {
        let f = Fixture::new();
        let raw = vec![b'x'; length];
        let (_, key) = f.publish(b"body", &raw);
        let control = OperationControl::default();
        let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
        let check = || Ok(());
        let lease = resources.read_derived_cache_with_check(&check).unwrap();
        assert!(
            lease
                .read_generation(&storage::sha256(b"missing"))
                .unwrap()
                .is_none()
        );
        for invalid in ["../escape".into(), key.to_ascii_uppercase(), "0".repeat(63)] {
            assert_eq!(
                lease.read_generation(&invalid).unwrap_err().code,
                "input.invalid"
            );
        }
        if length > MAX_GENERATION_BYTES {
            assert_eq!(
                lease.read_generation(&key).unwrap_err().code,
                "vault.data_cache_integrity"
            );
        } else {
            // Raw bytes are not represented as authenticated generation semantics.
            assert_eq!(lease.read_generation(&key).unwrap().unwrap(), raw);
        }
        lease.finish(Ok(())).unwrap();
        resources.close().unwrap();
    }
}

#[test]
fn missing_wrong_length_and_same_length_corrupt_objects_are_errors_never_cache_misses() {
    for fault in ["missing", "shard", "length", "content", "expected-length"] {
        let f = Fixture::new();
        let (object, _) = f.publish(b"body", b"manifest");
        let path = f.path(&object);
        match fault {
            "missing" | "shard" => {
                fs::remove_file(&path).unwrap();
                if fault == "shard" {
                    fs::remove_dir(path.parent().unwrap()).unwrap();
                }
            }
            "length" => fs::write(&path, b"bad").unwrap(),
            "content" => fs::write(&path, b"evil").unwrap(),
            _ => (),
        }
        let control = OperationControl::default();
        let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
        let check = || Ok(());
        let lease = resources.read_derived_cache_with_check(&check).unwrap();
        let expected = if fault == "expected-length" { 5 } else { 4 };
        assert_eq!(
            failed(lease.open_object(&object.sha256, expected)).code,
            "vault.data_cache_integrity",
            "{fault}"
        );
        assert_eq!(
            failed(lease.open_object(&object.sha256.to_ascii_uppercase(), 4)).code,
            "input.invalid"
        );
        lease.finish(Ok(())).unwrap();
        resources.close().unwrap();
        assert_eq!(path.exists(), !matches!(fault, "missing" | "shard"));
    }
}

#[test]
fn independent_budget_stops_inside_actual_sha_scan_and_checked_closes_original_reader() {
    let f = Fixture::new();
    let raw = vec![42; 131073];
    let (object, _) = f.publish(&raw, b"manifest");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    HASHED_BYTES.with(|progress| progress.set(0));
    let armed = Cell::new(false);
    let check = || {
        if armed.get() && HASHED_BYTES.with(Cell::get) >= 65536 {
            Err(budget_stop())
        } else {
            Ok(())
        }
    };
    let lease = resources.read_derived_cache_with_check(&check).unwrap();
    armed.set(true);
    let error = failed(lease.open_object(&object.sha256, object.bytes));
    assert_eq!(error.code, "test.read_budget");
    assert_eq!(HASHED_BYTES.with(Cell::get), 65536);
    armed.set(false);
    // A successful write-capable open proves the failed immutable reader is closed.
    storage::close_file(
        fs::OpenOptions::new()
            .write(true)
            .open(f.path(&object))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(fs::read(f.path(&object)).unwrap(), raw);
    assert_eq!(
        lease.finish::<()>(Err(error)).unwrap_err().code,
        "test.read_budget"
    );
    resources.close().unwrap();
}

#[test]
fn read_seek_codec_adapters_keep_the_original_independent_failure_sticky_through_close() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body", b"manifest");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let stopped = Cell::new(false);
    let check = || {
        if stopped.get() {
            Err(budget_stop())
        } else {
            Ok(())
        }
    };
    let lease = resources.read_derived_cache_with_check(&check).unwrap();
    let mut reader = lease.open_object(&object.sha256, object.bytes).unwrap();
    stopped.set(true);
    let error = reader.read(&mut [0]).unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .unwrap()
            .downcast_ref::<ToolkitError>()
            .unwrap()
            .code,
        "test.read_budget"
    );
    assert_eq!(reader.error().unwrap().code, "test.read_budget");
    stopped.set(false);
    assert_eq!(
        reader.seek_checked(SeekFrom::Start(0)).unwrap_err().code,
        "test.read_budget"
    );
    assert_eq!(
        reader.read_exact_checked(&mut []).unwrap_err().code,
        "test.read_budget"
    );
    assert_eq!(reader.close().unwrap_err().code, "test.read_budget");
    lease.finish(Ok(())).unwrap();
    resources.close().unwrap();
}

#[test]
fn checked_close_keeps_primary_both_native_cleanup_failures_and_both_original_stops() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"body", b"manifest");
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let stopped = Cell::new(false);
    let check = || {
        if stopped.get() {
            Err(budget_stop())
        } else {
            Ok(())
        }
    };
    let lease = resources.read_derived_cache_with_check(&check).unwrap();
    let reader = lease.open_object(&object.sha256, object.bytes).unwrap();
    let handle = reader.file.as_ref().unwrap().as_raw_handle();
    let files = Cell::new(0);
    let directories = Cell::new(0);
    token.cancel();
    stopped.set(true);
    let error = reader
        .close_after_with::<()>(
            Err(ToolkitError::new("test.consumer", "consumer failed", 4)),
            |file| {
                assert_eq!(file.as_raw_handle(), handle);
                files.set(files.get() + 1);
                storage::close_file(file)?;
                Err(io::Error::other("injected post-close failure"))
            },
            |directory| {
                directories.set(directories.get() + 1);
                let mut errors = close_directory(directory);
                errors.push(ToolkitError::new(
                    "test.directory_close",
                    "injected directory close failure",
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
    assert_eq!(causes[2]["code"], "test.directory_close");
    assert_eq!(causes[3]["code"], "command.cancelled");
    assert_eq!(causes[4]["code"], "test.read_budget");
    assert_eq!(
        lease.finish::<()>(Err(error)).unwrap_err().code,
        "test.consumer"
    );
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
    assert_eq!(fs::read(f.path(&object)).unwrap(), b"body");
}

#[test]
fn empty_content_is_authenticated_and_read_finish_rejects_unreserved_growth_without_deletion() {
    let f = Fixture::new();
    let (object, _) = f.publish(b"", b"manifest");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check = || Ok(());
    let lease = resources.read_derived_cache_with_check(&check).unwrap();
    let mut reader = lease.open_object(&object.sha256, 0).unwrap();
    assert!(reader.is_empty());
    assert_eq!(reader.read_checked(&mut [0]).unwrap(), 0);
    assert_eq!(reader.seek_checked(SeekFrom::End(0)).unwrap(), 0);
    reader.close().unwrap();
    let foreign = DataCachePaths::new(&f.0)
        .objects
        .join("unregistered-growth");
    fs::write(&foreign, b"extra").unwrap(); // Fault fixture, not a production write capability.
    assert_eq!(
        lease.finish(Ok(())).unwrap_err().code,
        "vault.data_cache_reservation_exceeded"
    );
    resources.close().unwrap();
    assert_eq!(fs::read(foreign).unwrap(), b"extra");
    assert_eq!(fs::read(f.path(&object)).unwrap(), b"");
}

#[test]
fn read_admission_waits_on_actual_quota_with_the_same_independent_budget() {
    let f = Fixture::new();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let held = ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE).unwrap();
    let stopped = Rc::new(Cell::new(false));
    let observe = stopped.clone();
    // acquire_inner uses the existing same-handle wait primitive, whose hook
    // runs only after real kernel contention (unlike owned_lease retries).
    ExclusiveLock::set_blob_wait_observer(move || observe.set(true));
    let check = || {
        if stopped.get() {
            Err(budget_stop())
        } else {
            Ok(())
        }
    };
    assert_eq!(
        failed(resources.read_derived_cache_with_check(&check)).code,
        "test.read_budget"
    );
    assert!(stopped.get());
    assert_eq!(
        failed(ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)).code,
        "workspace.busy"
    );
    held.close().unwrap();
    resources.close().unwrap();
}

#[test]
fn post_open_cancellation_transfers_original_manifest_handle_and_keeps_checked_close_failure() {
    let f = Fixture::new();
    let (_, key) = f.publish(b"body", b"manifest");
    let path = DataCachePaths::new(&f.0)
        .generations
        .join(format!("{key}.json"));
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let armed = Cell::new(false);
    let observed_original_reader = Cell::new(false);
    let check = || {
        if armed.get() && !observed_original_reader.get() {
            match fs::OpenOptions::new().write(true).open(&path) {
                Ok(file) => storage::close_file(file)?,
                Err(error) => {
                    // Real immutable Reader sharing, not a directory-listing
                    // timestamp/length heuristic, proves the original is open.
                    assert_eq!(error.raw_os_error(), Some(32));
                    observed_original_reader.set(true);
                    token.cancel();
                    return control.check();
                }
            }
        }
        Ok(())
    };
    let lease = resources.read_derived_cache_with_check(&check).unwrap();
    let close_calls = Cell::new(0);
    armed.set(true);
    let error = lease
        .owner
        .read_generation_with_close(&key, |file| {
            assert!(observed_original_reader.get());
            assert_eq!(file.metadata()?.len(), 8);
            assert!(fs::OpenOptions::new().write(true).open(&path).is_err());
            close_calls.set(close_calls.get() + 1);
            storage::close_file(file)?;
            // The real native owner is already closed; inject its reported
            // cleanup failure without retrying or manufacturing another owner.
            Err(io::Error::other("injected original-reader close failure"))
        })
        .unwrap_err();
    armed.set(false);
    assert!(observed_original_reader.get());
    assert_eq!(close_calls.get(), 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.exit_code, 7);
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 2);
    assert_eq!(causes[0]["code"], "command.cancelled");
    assert_eq!(causes[1]["code"], "external.io");
    assert!(
        causes[1]["message"]
            .as_str()
            .unwrap()
            .contains("original-reader close failure")
    );
    storage::close_file(fs::OpenOptions::new().write(true).open(&path).unwrap()).unwrap();
    assert_eq!(
        lease.finish::<()>(Err(error)).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
    assert_eq!(fs::read(path).unwrap(), b"manifest");
}
