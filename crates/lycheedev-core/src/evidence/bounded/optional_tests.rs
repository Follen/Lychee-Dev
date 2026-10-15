use super::*;
use crate::control::TrackedCancellation;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-optional-blob-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn publish(&self, bytes: &[u8]) -> Value {
        publish_blob_bounded(&self.0, bytes, bytes.len(), &alive()).unwrap()
    }
    fn read(&self, blob: &Value) -> Result<Option<Vec<u8>>> {
        read_blob_bounded_optional_with_check(&self.0, blob, BLOCK * 3, &alive(), || Ok(()))
    }
    fn open(&self, blob: &Value) -> Result<Option<VerifiedBlob>> {
        open_verified_blob_bounded_optional_with_check(
            &self.0,
            blob,
            (BLOCK * 3) as u64,
            &alive(),
            |_| Ok(()),
            || Ok(()),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let result = fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn alive() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn reference(bytes: &[u8]) -> Value {
    serde_json::json!({"sha256":storage::sha256(bytes),"bytes":bytes.len()})
}
fn budget_stop() -> ToolkitError {
    ToolkitError::new("fixture.budget_stop", "independent stop", 7)
}
fn failed<T>(result: Result<Option<T>>) -> ToolkitError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("expected error, never a cache miss"),
    }
}

#[test]
fn optional_missing_shard_and_missing_entry_are_none_in_both_apis() {
    let f = Fixture::new();
    let blob = reference(b"missing");
    let path = blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap();
    assert!(!path.parent().unwrap().exists());
    assert!(f.read(&blob).unwrap().is_none());
    assert!(f.open(&blob).unwrap().is_none());
    fs::create_dir(path.parent().unwrap()).unwrap();
    assert!(f.read(&blob).unwrap().is_none());
    assert!(f.open(&blob).unwrap().is_none());
}

#[test]
fn optional_required_workspace_and_blobs_parent_are_not_cache_misses() {
    let f = Fixture::new();
    let blob = reference(b"absent");
    let error = failed(read_blob_bounded_optional_with_check(
        &f.0.join("missing"),
        &blob,
        100,
        &alive(),
        || Ok(()),
    ));
    assert_ne!(error.code, "fixture.budget_stop");
    fs::remove_dir(f.0.join("blobs")).unwrap();
    assert_eq!(failed(f.read(&blob)).code, "external.io");
    assert_eq!(failed(f.open(&blob)).code, "external.io");
}

#[test]
fn optional_noop_matches_required_success_bytes_and_observer_order() {
    let f = Fixture::new();
    let bytes = vec![19; BLOCK + 11];
    let blob = f.publish(&bytes);
    assert_eq!(
        f.read(&blob).unwrap().unwrap(),
        read_blob_bounded(&f.0, &blob, bytes.len(), &alive()).unwrap()
    );
    let mut required_blocks = Vec::new();
    let required = open_verified_blob_bounded(&f.0, &blob, bytes.len() as u64, &alive(), |block| {
        required_blocks.push(block.to_vec());
        Ok(())
    })
    .unwrap();
    required.close().unwrap();
    let mut optional_blocks = Vec::new();
    let mut held = open_verified_blob_bounded_optional_with_check(
        &f.0,
        &blob,
        bytes.len() as u64,
        &alive(),
        |block| {
            optional_blocks.push(block.to_vec());
            Ok(())
        },
        || Ok(()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(optional_blocks, required_blocks);
    let mut read = Vec::new();
    held.read_to_end(&mut read).unwrap();
    assert_eq!(read, bytes);
    held.rewind().unwrap();
    held.close().unwrap();
}

#[test]
fn optional_invalid_reference_limit_and_integrity_match_required_errors() {
    let f = Fixture::new();
    let blob = f.publish(b"right");
    let mut cases = vec![
        serde_json::json!({"sha256":"bad","bytes":5}),
        serde_json::json!({"sha256":blob["sha256"],"bytes":999999}),
    ];
    let path = blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap();
    fs::write(&path, b"wrong").unwrap();
    cases.push(blob);
    for blob in cases {
        let expected =
            read_blob_bounded_with_check(&f.0, &blob, BLOCK * 3, &alive(), || Ok(())).unwrap_err();
        let actual = failed(f.read(&blob));
        assert_eq!(
            (actual.code, actual.message, actual.details),
            (expected.code, expected.message, expected.details)
        );
        assert_ne!(failed(f.open(&blob)).code, "fixture.budget_stop");
    }
}

#[test]
fn optional_entry_disappearing_after_metadata_is_a_native_open_miss() {
    let f = Fixture::new();
    let blob = f.publish(b"race");
    let path = blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap();
    optional::test_hooks::set_before_body(move || fs::remove_file(&path).unwrap());
    assert!(f.read(&blob).unwrap().is_none());
}

#[test]
fn optional_entry_disappearing_before_metadata_is_a_native_inspection_miss() {
    let f = Fixture::new();
    let blob = f.publish(b"race-before");
    let path = blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap();
    optional::test_hooks::set_before_metadata(move || fs::remove_file(&path).unwrap());
    let result = open_verified_blob_bounded_optional_with_check(
        &f.0,
        &blob,
        100,
        &alive(),
        |_| panic!("missing body must not be observed"),
        || Ok(()),
    )
    .unwrap();
    assert!(result.is_none());
}

#[test]
fn optional_post_miss_stop_cannot_be_returned_as_none() {
    let f = Fixture::new();
    let blob = reference(b"missing-with-stop");
    let mut total = 0;
    assert!(
        read_blob_bounded_optional_with_check(&f.0, &blob, 100, &alive(), || {
            total += 1;
            Ok(())
        })
        .unwrap()
        .is_none()
    );
    let mut checks = 0;
    let error = failed(read_blob_bounded_optional_with_check(
        &f.0,
        &blob,
        100,
        &alive(),
        || {
            checks += 1;
            // The last callback follows native miss and checked parent close.
            if checks >= total {
                Err(budget_stop())
            } else {
                Ok(())
            }
        },
    ));
    assert_eq!(error.code, "fixture.budget_stop");
    assert_eq!(checks, total);
}

#[test]
fn optional_pre_stopped_invocation_precedes_budget_and_path_io() {
    let f = Fixture::new();
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    token.cancel();
    let error = failed(read_blob_bounded_optional_with_check(
        &f.0.join("absent"),
        &reference(b"missing"),
        100,
        &control,
        || panic!("invocation must win"),
    ));
    assert_eq!(error.code, "command.cancelled");
    let error = failed(open_verified_blob_bounded_optional_with_check(
        &f.0,
        &reference(b"missing"),
        100,
        &control,
        |_| panic!("no body"),
        || panic!("invocation must win"),
    ));
    assert_eq!(error.code, "command.cancelled");
}

#[test]
fn optional_held_owner_observes_later_stop_and_retains_typed_io_bridge() {
    let f = Fixture::new();
    let blob = f.publish(b"held owner");
    let stopped = Arc::new(AtomicBool::new(false));
    let owned = stopped.clone();
    let mut held = open_verified_blob_bounded_optional_with_check(
        &f.0,
        &blob,
        100,
        &alive(),
        |_| Ok(()),
        move || {
            if owned.load(Ordering::SeqCst) {
                Err(budget_stop())
            } else {
                Ok(())
            }
        },
    )
    .unwrap()
    .unwrap();
    stopped.store(true, Ordering::SeqCst);
    assert_eq!(
        held.read_checked(&mut [0; 8]).unwrap_err().code,
        "fixture.budget_stop"
    );
    let error = held.read(&mut [0; 8]).unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .unwrap()
            .downcast_ref::<ToolkitError>()
            .unwrap()
            .code,
        "fixture.budget_stop"
    );
    assert_eq!(held.rewind().unwrap_err().code, "fixture.budget_stop");
    assert_eq!(held.close().unwrap_err().code, "fixture.budget_stop");
    // Closed on the error path, so the original entry is writable again.
    fs::write(
        blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap(),
        b"different!",
    )
    .unwrap();
}

#[test]
fn optional_existing_writer_sharing_error_is_not_a_cache_miss() {
    let f = Fixture::new();
    let blob = f.publish(b"writer");
    let path = blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap();
    let writer = fs::OpenOptions::new().write(true).open(&path).unwrap();
    assert_eq!(failed(f.read(&blob)).code, "external.io");
    assert_eq!(failed(f.open(&blob)).code, "external.io");
    lycheedev_windows::filesystem::close_file(writer).unwrap();
    assert_eq!(f.read(&blob).unwrap().unwrap(), b"writer");
}

#[test]
fn optional_acquired_body_observer_error_is_not_miss_and_releases_owner() {
    let f = Fixture::new();
    let blob = f.publish(b"observer");
    let expected = ToolkitError::new("fixture.observer", "format observer", 4);
    let error = failed(open_verified_blob_bounded_optional_with_check(
        &f.0,
        &blob,
        100,
        &alive(),
        |_| Err(expected.clone()),
        || Ok(()),
    ));
    assert_eq!(error.code, expected.code);
    assert_eq!(f.read(&blob).unwrap().unwrap(), b"observer");
    fs::remove_file(blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap()).unwrap();
}

#[test]
fn optional_close_keeps_primary_file_cleanup_and_original_stop_priority() {
    for cancel_invocation in [false, true] {
        let f = Fixture::new();
        let blob = f.publish(b"close");
        let token = TrackedCancellation::new();
        let control =
            OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
        let stopped = Arc::new(AtomicBool::new(false));
        let owned = stopped.clone();
        let held = open_verified_blob_bounded_optional_with_check(
            &f.0,
            &blob,
            100,
            &control,
            |_| Ok(()),
            move || {
                if owned.load(Ordering::SeqCst) {
                    Err(budget_stop())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap()
        .unwrap();
        let error = held
            .close_after_with::<()>(
                Err(ToolkitError::new("fixture.primary", "primary read", 4)),
                |file| {
                    lycheedev_windows::filesystem::close_file(file)?;
                    if cancel_invocation {
                        token.cancel();
                    }
                    stopped.store(true, Ordering::SeqCst);
                    Err(io::Error::other("file close failure"))
                },
            )
            .unwrap_err();
        assert_eq!(error.code, "fixture.primary");
        let rendered = serde_json::to_string(&error.details).unwrap();
        for cause in [
            "fixture.primary",
            "file close failure",
            if cancel_invocation {
                "command.cancelled"
            } else {
                "fixture.budget_stop"
            },
        ] {
            assert!(rendered.contains(cause), "missing cause: {cause}");
        }
        // Returned streams preserve R2 invocation-first callback semantics.
        if cancel_invocation {
            assert!(!rendered.contains("fixture.budget_stop"));
        }
    }
}

#[test]
fn optional_nonfile_and_wrong_length_entries_keep_integrity_diagnostics() {
    let f = Fixture::new();
    let blob = reference(b"kind");
    let path = blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap();
    fs::create_dir_all(&path).unwrap();
    for is_directory in [true, false] {
        if !is_directory {
            fs::remove_dir(&path).unwrap();
            fs::write(&path, b"longer-than-declared").unwrap();
        }
        let expected = read_blob_bounded(&f.0, &blob, 100, &alive()).unwrap_err();
        let actual = failed(f.read(&blob));
        assert_eq!(actual.code, "vault.blob_integrity");
        assert_eq!(
            (actual.message, actual.details),
            (expected.message, expected.details)
        );
        assert_eq!(failed(f.open(&blob)).code, "vault.blob_integrity");
    }
}

#[test]
fn optional_native_stop_releases_core_owned_parent_shard_and_temporary_handles() {
    for after_shard in [true, false] {
        let f = Fixture::new();
        let blob = f.publish(b"native-stop");
        let stopped = Arc::new(AtomicBool::new(false));
        let signal = stopped.clone();
        if after_shard {
            optional::test_hooks::set_after_shard(move || signal.store(true, Ordering::SeqCst));
        } else {
            optional::test_hooks::set_before_body(move || signal.store(true, Ordering::SeqCst));
        }
        let error = failed(read_blob_bounded_optional_with_check(
            &f.0,
            &blob,
            100,
            &alive(),
            || {
                if stopped.load(Ordering::SeqCst) {
                    Err(budget_stop())
                } else {
                    Ok(())
                }
            },
        ));
        assert_eq!(error.code, "fixture.budget_stop");
        let moved = f.0.with_extension("moved");
        fs::rename(&f.0, &moved).unwrap();
        fs::rename(&moved, &f.0).unwrap();
        assert_eq!(f.read(&blob).unwrap().unwrap(), b"native-stop");
    }
}

#[test]
fn optional_shard_that_is_a_regular_file_is_not_a_miss() {
    let f = Fixture::new();
    let blob = reference(b"shard-kind");
    let path = blob_path(&f.0, blob["sha256"].as_str().unwrap()).unwrap();
    fs::write(path.parent().unwrap(), b"not a directory").unwrap();
    assert_eq!(failed(f.read(&blob)).code, "external.io");
    assert_eq!(failed(f.open(&blob)).code, "external.io");
    fs::remove_file(path.parent().unwrap()).unwrap();
}
