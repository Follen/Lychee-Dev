use super::*;
use crate::control::TrackedCancellation;
use std::{
    cell::Cell,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-blob-supplemental-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn publish(&self, bytes: &[u8]) -> Value {
        publish_blob_bounded(&self.0, bytes, bytes.len(), &alive()).unwrap()
    }
    fn capture_count(&self) -> usize {
        if !self.0.join("state/toolkit.sqlite").exists() {
            return 0;
        }
        let metadata = Metadata::open(&self.0, false).unwrap();
        let count = metadata.list::<Value>("capture/", 100).unwrap().len();
        metadata.close().unwrap();
        count
    }
    fn assert_guard_free(&self, bytes: &[u8]) {
        ExclusiveLock::resource(
            &self.0.join("locks"),
            &format!("blob:{}", storage::sha256(bytes)),
        )
        .unwrap()
        .close()
        .unwrap();
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
fn independent() -> (OperationControl, TrackedCancellation) {
    let signal = TrackedCancellation::new();
    (
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), signal.clone()),
        signal,
    )
}
fn input(bytes: &[u8]) -> CaptureInput<'_> {
    CaptureInput {
        bytes,
        maximum_bytes: bytes.len(),
        media_type: "application/json",
        provenance: serde_json::json!({"kind":"fixture","locator":"independent-budget"}),
        complete: true,
        truncated: false,
    }
}
fn stopped(error: &ToolkitError, control: &OperationControl) {
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    control.check().unwrap();
}
#[test]
fn supplemental_pre_stop_rejects_all_blob_entries_before_filesystem() {
    let f = Fixture::new();
    let absent = f.0.join("must-not-create");
    let invocation = alive();
    let (budget, token) = independent();
    token.cancel();
    let blob = serde_json::json!({"sha256": storage::sha256(b"{}"), "bytes":2});
    stopped(
        &read_blob_bounded_with_check(&absent, &blob, 2, &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    stopped(
        &publish_blob_bounded_with_check(&absent, b"{}", 2, &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    stopped(
        &commit_capture_bounded_with_check(&absent, input(b"{}"), &invocation, || budget.check())
            .unwrap_err(),
        &invocation,
    );
    assert!(!absent.exists());
}
#[test]
fn invocation_stop_precedes_supplemental_without_calling_it() {
    let f = Fixture::new();
    let control = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    let called = Cell::new(false);
    let error = publish_blob_bounded_with_check(&f.0, b"x", 1, &control, || {
        called.set(true);
        Err(ToolkitError::new("budget.stop", "independent stop", 7))
    })
    .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert!(!called.get());
}
#[test]
fn supplemental_hash_stop_does_not_reach_blob_lock_or_publication() {
    let f = Fixture::new();
    let invocation = alive();
    let (budget, token) = independent();
    let bytes = vec![9u8; BLOCK * 3];
    let calls = Cell::new(0);
    let error = digest_bytes_with_check(&bytes, Some(&invocation), &mut || {
        calls.set(calls.get() + 1);
        // Check the real digest block boundary directly, independent of the
        // now-controlled workspace/Directory checks preceding this phase.
        if calls.get() == 2 {
            token.cancel();
        }
        budget.check()
    })
    .unwrap_err();
    stopped(&error, &invocation);
    assert_eq!(calls.get(), 2);
    assert!(fs::read_dir(f.0.join("blobs")).unwrap().next().is_none());
    assert!(fs::read_dir(f.0.join("locks")).unwrap().next().is_none());
}
#[test]
fn supplemental_scan_pre_and_post_read_stop_preserve_real_cursor() {
    let f = Fixture::new();
    let bytes = vec![7; BLOCK * 3];
    let blob = f.publish(&bytes);
    let invocation = alive();
    for stop_at in [1, 2, 3] {
        let (budget, token) = independent();
        let calls = Cell::new(0);
        let accepted = Cell::new(0);
        let mut opened = open_blob(
            &f.0,
            admitted_ref(&blob, bytes.len() as u64).unwrap(),
            Some(&invocation),
        )
        .unwrap();
        let error = scan_open_blob_with_check(
            &mut opened,
            Some(&invocation),
            &mut || {
                calls.set(calls.get() + 1);
                if calls.get() == stop_at {
                    token.cancel();
                }
                budget.check()
            },
            |block| {
                accepted.set(accepted.get() + block.len());
                Ok(())
            },
        )
        .unwrap_err();
        stopped(&error, &invocation);
        assert_eq!(
            opened.file.stream_position().unwrap(),
            if stop_at == 1 { 0 } else { BLOCK as u64 }
        );
        assert_eq!(accepted.get(), if stop_at == 3 { BLOCK } else { 0 });
        lycheedev_windows::filesystem::close_file(opened.file).unwrap();
    }
}
#[test]
fn supplemental_stop_after_blob_lock_open_closes_owner_without_publication() {
    let f = Fixture::new();
    let invocation = alive();
    let (budget, token) = independent();
    let bytes = b"locked";
    let resource = format!("blob:{}", storage::sha256(bytes));
    let lock_path =
        f.0.join("locks")
            .join(format!("{}.lock", storage::sha256(resource.as_bytes())));
    let observed = Cell::new(false);
    let error = publish_blob_bounded_with_check(&f.0, bytes, bytes.len(), &invocation, || {
        if lock_path.exists() {
            observed.set(true);
            token.cancel();
        }
        budget.check()
    })
    .unwrap_err();
    stopped(&error, &invocation);
    assert!(observed.get());
    assert!(!blob_path(&f.0, &storage::sha256(bytes)).unwrap().exists());
    f.assert_guard_free(bytes);
}
#[test]
fn supplemental_publication_stop_discards_actual_stage_and_preserves_typed_error() {
    let f = Fixture::new();
    let invocation = alive();
    let (budget, token) = independent();
    let bytes = vec![3u8; BLOCK * 3];
    let path = blob_path(&f.0, &storage::sha256(&bytes)).unwrap();
    let shard = path.parent().unwrap();
    let staged = Cell::new(false);
    let error = publish_blob_bounded_with_check(&f.0, &bytes, bytes.len(), &invocation, || {
        if let Ok(entries) = fs::read_dir(shard) {
            if entries.filter_map(std::result::Result::ok).any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".lycheedev-export-")
            }) {
                staged.set(true);
                token.cancel();
            }
        }
        budget.check()
    })
    .unwrap_err();
    stopped(&error, &invocation);
    assert!(staged.get());
    assert!(!path.exists());
    assert!(fs::read_dir(shard).unwrap().next().is_none());
    f.assert_guard_free(&bytes);
    assert!(same_error(
        &error,
        &io_error(io::Error::other(error.clone()))
    ));
}
#[test]
fn supplemental_capture_stops_before_blob_after_blob_and_after_manifest() {
    for stop_stage in [0, 1, 2] {
        let f = Fixture::new();
        let invocation = alive();
        let (budget, token) = independent();
        let bytes = b"{}";
        let observed = Cell::new(0);
        let error = commit_internal_observed_with_check(
            &f.0,
            input(bytes),
            Some(&invocation),
            || budget.check(),
            |stage| {
                let index = match stage {
                    CapturePublication::BeforeBlob => 0,
                    CapturePublication::BlobPublished => 1,
                    CapturePublication::ManifestPublished => 2,
                };
                if index == stop_stage {
                    observed.set(observed.get() + 1);
                    token.cancel();
                }
            },
        )
        .unwrap_err();
        stopped(&error, &invocation);
        assert_eq!(observed.get(), 1);
        assert_eq!(
            blob_path(&f.0, &storage::sha256(bytes)).unwrap().exists(),
            stop_stage != 0
        );
        assert_eq!(f.capture_count(), usize::from(stop_stage == 2));
        f.assert_guard_free(bytes);
        stopped(
            &commit_capture_bounded_with_check(&f.0, input(bytes), &invocation, || budget.check())
                .unwrap_err(),
            &invocation,
        );
        assert_eq!(f.capture_count(), usize::from(stop_stage == 2));
    }
}
#[test]
fn supplemental_capture_metadata_wait_cancels_without_manifest_and_releases_blob_guard() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"{\"lock\":true}";
    // Schema admission now precedes publication. Block the actual manifest
    // transaction only after publication, and start the independent budget
    // once that transaction has encountered real SQLite contention.
    let mut blocker = None;
    let busy_before = Cell::new(None);
    let due = Cell::new(None::<Instant>);
    let published = Cell::new(0);
    let manifests = Cell::new(0);
    let error = commit_internal_observed_with_check(
        &f.0,
        input(bytes),
        Some(&invocation),
        || {
            if busy_before
                .get()
                .is_some_and(|before| crate::metadata::sql_busy_observations() > before)
            {
                let deadline = due.get().unwrap_or_else(|| {
                    let deadline = Instant::now() + Duration::from_millis(25);
                    due.set(Some(deadline));
                    deadline
                });
                if Instant::now() >= deadline {
                    return Err(ToolkitError::new(
                        "budget.expired",
                        "independent budget expired",
                        7,
                    ));
                }
            }
            Ok(())
        },
        |stage| match stage {
            CapturePublication::BlobPublished => {
                published.set(published.get() + 1);
                let connection =
                    rusqlite::Connection::open(f.0.join("state/toolkit.sqlite")).unwrap();
                connection.busy_timeout(Duration::ZERO).unwrap();
                connection.execute_batch("BEGIN IMMEDIATE").unwrap();
                blocker = Some(connection);
                busy_before.set(Some(crate::metadata::sql_busy_observations()));
            }
            CapturePublication::ManifestPublished => manifests.set(manifests.get() + 1),
            CapturePublication::BeforeBlob => {}
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "budget.expired");
    invocation.check().unwrap();
    assert_eq!(published.get(), 1);
    assert_eq!(manifests.get(), 0);
    assert!(due.get().is_some());
    let blocker = blocker.unwrap();
    blocker.execute_batch("ROLLBACK").unwrap();
    blocker.close().unwrap();
    assert_eq!(f.capture_count(), 0);
    assert_eq!(
        fs::read(blob_path(&f.0, &storage::sha256(bytes)).unwrap()).unwrap(),
        bytes
    );
    f.assert_guard_free(bytes);
}
#[test]
fn supplemental_publication_and_close_failures_keep_original_causes() {
    let stop = ToolkitError::new("command.cancelled", "budget stopped", 7)
        .with_details(serde_json::json!({"owner":"independent"}));
    let discard = ToolkitError::new("external.io", "discard failed", 5);
    let close = ToolkitError::new("external.io", "lock close failed", 5);
    let result = crate::metadata::owned_write::finish_owned::<()>(
        Err(stop.clone()),
        Err(discard.clone()),
    );
    let error = crate::metadata::owned_write::finish_owned(result, Err(close.clone()))
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["primary"]["details"]["primary"],
        serde_json::to_value(&stop).unwrap()
    );
    assert_eq!(
        error.details["primary"]["details"]["cleanup"],
        serde_json::to_value(discard).unwrap()
    );
    assert_eq!(
        error.details["cleanup"],
        serde_json::to_value(close).unwrap()
    );
    let error = finish_check::<()>(Err(stop.clone()), Err(stop.clone())).unwrap_err();
    assert!(same_error(&stop, &error));
}
#[test]
fn supplemental_noop_preserves_blob_bytes_identity_and_validation_errors() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"same original";
    let legacy = publish_blob_bounded(&f.0, bytes, bytes.len(), &invocation).unwrap();
    let supplemented =
        publish_blob_bounded_with_check(&f.0, bytes, bytes.len(), &invocation, || Ok(())).unwrap();
    assert_eq!(legacy, supplemented);
    assert_eq!(
        read_blob_bounded(&f.0, &legacy, bytes.len(), &invocation).unwrap(),
        read_blob_bounded_with_check(&f.0, &legacy, bytes.len(), &invocation, || Ok(())).unwrap()
    );
    for bad in [
        serde_json::json!({}),
        serde_json::json!({"sha256":"bad","bytes":1}),
        serde_json::json!({"sha256":storage::sha256(bytes),"bytes":999}),
    ] {
        let left = read_blob_bounded(&f.0, &bad, bytes.len(), &invocation).unwrap_err();
        let right = read_blob_bounded_with_check(&f.0, &bad, bytes.len(), &invocation, || Ok(()))
            .unwrap_err();
        assert!(same_error(&left, &right));
    }
}

#[test]
fn supplemental_blob_contention_waits_for_original_stop_and_preserves_existing_owner() {
    let f = Fixture::new();
    let (invocation, cancellation) = independent();
    let bytes = b"busy";
    let key = format!("blob:{}", storage::sha256(bytes));
    let held = ExclusiveLock::resource(&f.0.join("locks"), &key).unwrap();
    // Go AcquireLease waits for contention; stop only after the real lock
    // attempt, without waiting for the fixture's thirty-second deadline.
    ExclusiveLock::set_blob_wait_observer(move || cancellation.cancel());
    let checked = Cell::new(0);
    let error = publish_blob_bounded_with_check(&f.0, bytes, bytes.len(), &invocation, || {
        checked.set(checked.get() + 1);
        Ok(())
    })
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert!(error.details.is_null());
    assert!(checked.get() > 0);
    assert_eq!(
        ExclusiveLock::resource(&f.0.join("locks"), &key)
            .err()
            .unwrap()
            .code,
        "workspace.busy"
    );
    assert!(!blob_path(&f.0, &storage::sha256(bytes)).unwrap().exists());
    held.close().unwrap();
    f.assert_guard_free(bytes);
}
#[test]
fn supplemental_publication_does_not_deduplicate_identical_distinct_discard_failure() {
    let same = ToolkitError::new("external.io", "same failure text", 5);
    let error = crate::metadata::owned_write::finish_owned::<()>(
        Err(same.clone()),
        Err(same.clone()),
    )
    .unwrap_err();
    assert_eq!(
        error.details["primary"],
        serde_json::to_value(&same).unwrap()
    );
    assert_eq!(
        error.details["cleanup"],
        serde_json::to_value(same).unwrap()
    );
}
