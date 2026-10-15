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
            "lychee-stream-check-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn publish(&self, bytes: &[u8]) -> Value {
        publish_blob_bounded(&self.0, bytes, bytes.len(), &alive()).unwrap()
    }
    fn held(&self, bytes: &[u8]) -> VerifiedBlob {
        open_verified_blob_bounded(
            &self.0,
            &self.publish(bytes),
            bytes.len() as u64,
            &alive(),
            |_| Ok(()),
        )
        .unwrap()
    }
    fn no_publication(&self, bytes: &[u8]) {
        let path = blob_path(&self.0, &storage::sha256(bytes)).unwrap();
        assert!(!path.exists());
        if path.parent().unwrap().exists() {
            assert!(
                fs::read_dir(path.parent().unwrap())
                    .unwrap()
                    .next()
                    .is_none()
            );
        }
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
    let token = TrackedCancellation::new();
    (
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone()),
        token,
    )
}
fn reference(bytes: &[u8]) -> BlobRef {
    BlobRef {
        sha256: storage::sha256(bytes),
        bytes: bytes.len() as u64,
    }
}
fn failed(result: Result<VerifiedBlob>) -> ToolkitError {
    match result {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("expected stream failure")
        }
    }
}
fn stopped(error: &ToolkitError, invocation: &OperationControl) {
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    invocation.check().unwrap();
}
struct PanicReader;
impl Read for PanicReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("input must not be consumed")
    }
}

#[test]
fn stream_pre_stopped_open_and_publish_do_not_touch_missing_home_or_source() {
    let f = Fixture::new();
    let missing = f.0.join("absent");
    let invocation = alive();
    let (budget, token) = independent();
    token.cancel();
    let error = failed(open_verified_blob_bounded_with_check(
        &missing,
        &serde_json::json!({}),
        1,
        &invocation,
        |_| panic!("observer must not run"),
        move || budget.check(),
    ));
    stopped(&error, &invocation);
    let (budget, token) = independent();
    token.cancel();
    let error = failed(publish_blob_reader_bounded_with_check(
        &missing,
        &mut PanicReader,
        &reference(b"x"),
        1,
        &invocation,
        move || budget.check(),
    ));
    stopped(&error, &invocation);
    assert!(!missing.exists());
}
#[test]
fn stream_owned_check_survives_return_and_is_released_only_after_close() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"owned";
    let marker = Arc::new(());
    let weak = Arc::downgrade(&marker);
    let owner = marker.clone();
    let (budget, token) = independent();
    let mut blob = open_verified_blob_bounded_with_check(
        &f.0,
        &f.publish(bytes),
        bytes.len() as u64,
        &invocation,
        |_| Ok(()),
        move || {
            let _ = &owner;
            budget.check()
        },
    )
    .unwrap();
    drop(marker);
    assert!(weak.upgrade().is_some());
    token.cancel();
    let error = blob.read_checked(&mut [0; 8]).unwrap_err();
    stopped(&error, &invocation);
    assert_eq!(blob.opened.file.stream_position().unwrap(), 0);
    stopped(
        &blob.close_after::<()>(Err(error)).unwrap_err(),
        &invocation,
    );
    assert!(weak.upgrade().is_none());
}
#[test]
fn stream_post_read_stop_exposes_typed_error_and_does_not_undo_actual_cursor() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = vec![8u8; BLOCK * 2];
    let armed = Arc::new(AtomicBool::new(false));
    let count = Arc::new(AtomicUsize::new(0));
    let active = armed.clone();
    let checks = count.clone();
    let (budget, token) = independent();
    let mut blob = open_verified_blob_bounded_with_check(
        &f.0,
        &f.publish(&bytes),
        bytes.len() as u64,
        &invocation,
        |_| Ok(()),
        move || {
            if active.load(Ordering::Acquire) && checks.fetch_add(1, Ordering::AcqRel) == 1 {
                token.cancel();
            }
            budget.check()
        },
    )
    .unwrap();
    armed.store(true, Ordering::Release);
    let mut out = vec![0; BLOCK * 2];
    let error = blob.read_checked(&mut out).unwrap_err();
    stopped(&error, &invocation);
    assert_eq!(blob.opened.file.stream_position().unwrap(), BLOCK as u64);
    assert_eq!(&out[..BLOCK], &bytes[..BLOCK]);
    assert_eq!(count.load(Ordering::Acquire), 2);
    stopped(
        &blob.close_after::<()>(Err(error)).unwrap_err(),
        &invocation,
    );
}
#[test]
fn stream_rewind_checks_the_same_owner_before_and_after_seek() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"rewind";
    let armed = Arc::new(AtomicBool::new(false));
    let checks = Arc::new(AtomicUsize::new(0));
    let active = armed.clone();
    let count = checks.clone();
    let (budget, token) = independent();
    let mut blob = open_verified_blob_bounded_with_check(
        &f.0,
        &f.publish(bytes),
        bytes.len() as u64,
        &invocation,
        |_| Ok(()),
        move || {
            if active.load(Ordering::Acquire) && count.fetch_add(1, Ordering::AcqRel) == 1 {
                token.cancel();
            }
            budget.check()
        },
    )
    .unwrap();
    assert_eq!(blob.read_checked(&mut [0; 3]).unwrap(), 3);
    armed.store(true, Ordering::Release);
    let error = blob.rewind().unwrap_err();
    stopped(&error, &invocation);
    assert_eq!(blob.opened.file.stream_position().unwrap(), 0);
    stopped(
        &blob.close_after::<()>(Err(error)).unwrap_err(),
        &invocation,
    );
}
#[test]
fn stream_std_read_retains_full_toolkit_error_inner() {
    let f = Fixture::new();
    let invocation = alive();
    let fail = Arc::new(AtomicBool::new(false));
    let flag = fail.clone();
    let expected = ToolkitError::new("budget.expired", "independent deadline", 7)
        .with_details(serde_json::json!({"ledger":"original","used":9}));
    let error = expected.clone();
    let mut blob = open_verified_blob_bounded_with_check(
        &f.0,
        &f.publish(b"data"),
        4,
        &invocation,
        |_| Ok(()),
        move || {
            if flag.load(Ordering::Acquire) {
                Err(error.clone())
            } else {
                Ok(())
            }
        },
    )
    .unwrap();
    fail.store(true, Ordering::Release);
    let bridge = blob.read(&mut [0; 4]).unwrap_err();
    let inner = bridge
        .get_ref()
        .unwrap()
        .downcast_ref::<ToolkitError>()
        .unwrap();
    assert!(same_error(inner, &expected));
    assert!(same_error(&io_error(bridge), &expected));
    assert!(same_error(&blob.close().unwrap_err(), &expected));
}
#[test]
fn stream_invocation_stop_precedes_supplemental_during_consumption() {
    let f = Fixture::new();
    let (invocation, stop) = independent();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut blob = open_verified_blob_bounded_with_check(
        &f.0,
        &f.publish(b"x"),
        1,
        &invocation,
        |_| Ok(()),
        move || {
            count.fetch_add(1, Ordering::AcqRel);
            Ok(())
        },
    )
    .unwrap();
    calls.store(0, Ordering::Release);
    stop.cancel();
    assert_eq!(
        blob.read_checked(&mut [0; 1]).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert_eq!(blob.close().unwrap_err().code, "command.cancelled");
    assert_eq!(calls.load(Ordering::Acquire), 0);
}
#[test]
fn stream_initial_observer_order_stops_next_block_and_closes_opened_file() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = vec![5; BLOCK * 3];
    let value = f.publish(&bytes);
    let (budget, token) = independent();
    let signal = token.clone();
    let mut observed = 0;
    let error = failed(open_verified_blob_bounded_with_check(
        &f.0,
        &value,
        bytes.len() as u64,
        &invocation,
        |block| {
            observed += block.len();
            signal.cancel();
            Ok(())
        },
        move || budget.check(),
    ));
    stopped(&error, &invocation);
    assert_eq!(observed, BLOCK);
    // Deny-write ownership from failed verified-open has actually ended.
    fs::write(
        blob_path(&f.0, &storage::sha256(&bytes)).unwrap(),
        b"closed",
    )
    .unwrap();
}
struct ObservedReader {
    source: io::Cursor<Vec<u8>>,
    observed: Arc<AtomicUsize>,
    stop: Option<TrackedCancellation>,
    terminal: Option<ToolkitError>,
}
impl Read for ObservedReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.source.read(out)?;
        // This is the source owner's already-consumed ledger. Record before
        // cancellation or a terminal failure is exposed; Core must not refund it.
        self.observed.fetch_add(n, Ordering::AcqRel);
        if n > 0 {
            if let Some(token) = self.stop.take() {
                token.cancel();
            }
        }
        if n == 0 {
            if let Some(error) = &self.terminal {
                return Err(io::Error::other(error.clone()));
            }
        }
        Ok(n)
    }
}
#[test]
fn stream_publish_keeps_source_observed_bytes_when_post_read_budget_stops() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = vec![4; BLOCK * 2];
    let observed = Arc::new(AtomicUsize::new(0));
    let (budget, token) = independent();
    let mut source = ObservedReader {
        source: io::Cursor::new(bytes.clone()),
        observed: observed.clone(),
        stop: Some(token),
        terminal: None,
    };
    let error = failed(publish_blob_reader_bounded_with_check(
        &f.0,
        &mut source,
        &reference(&bytes),
        bytes.len() as u64,
        &invocation,
        move || budget.check(),
    ));
    stopped(&error, &invocation);
    assert_eq!(observed.load(Ordering::Acquire), BLOCK);
    f.no_publication(&bytes);
}
#[test]
fn stream_publish_retains_terminal_source_failure_and_does_not_publish() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"terminal bytes";
    let observed = Arc::new(AtomicUsize::new(0));
    let source_error = ToolkitError::new("records.source_failure", "terminal source failed", 5)
        .with_details(serde_json::json!({"actual":bytes.len()}));
    let mut source = ObservedReader {
        source: io::Cursor::new(bytes.to_vec()),
        observed: observed.clone(),
        stop: None,
        terminal: Some(source_error.clone()),
    };
    let error = failed(publish_blob_reader_bounded_with_check(
        &f.0,
        &mut source,
        &reference(bytes),
        bytes.len() as u64,
        &invocation,
        || Ok(()),
    ));
    assert!(same_error(&error, &source_error));
    assert_eq!(observed.load(Ordering::Acquire), bytes.len());
    f.no_publication(bytes);
}
#[test]
fn stream_publish_returned_owner_and_existing_hit_keep_consumption_check() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"published";
    for existing in [false, true] {
        let (budget, token) = independent();
        let mut cursor = io::Cursor::new(bytes);
        let mut must_not_read = PanicReader;
        let mut blob = if existing {
            publish_blob_reader_bounded_with_check(
                &f.0,
                &mut must_not_read,
                &reference(bytes),
                bytes.len() as u64,
                &invocation,
                move || budget.check(),
            )
            .unwrap()
        } else {
            publish_blob_reader_bounded_with_check(
                &f.0,
                &mut cursor,
                &reference(bytes),
                bytes.len() as u64,
                &invocation,
                move || budget.check(),
            )
            .unwrap()
        };
        assert_eq!(blob.reference().sha256, storage::sha256(bytes));
        token.cancel();
        let error = blob.read_checked(&mut [0; 1]).unwrap_err();
        stopped(&error, &invocation);
        stopped(
            &blob.close_after::<()>(Err(error)).unwrap_err(),
            &invocation,
        );
    }
}
#[test]
fn stream_source_discard_and_file_close_failures_retain_all_three_causes() {
    let f = Fixture::new();
    let blob = f.held(b"held");
    let source = ToolkitError::new("records.source_failure", "source failed", 5)
        .with_details(serde_json::json!({"source":"original"}));
    let discard = ToolkitError::new("external.io", "discard failed", 5);
    let close = ToolkitError::new("external.io", "close failed", 5);
    let result = finish_publication(Some(source.clone()), Err(discard.clone()), false);
    let error = blob
        .close_after_with(result, |file| {
            lycheedev_windows::filesystem::close_file(file)?;
            Err(io::Error::other("close failed"))
        })
        .unwrap_err();
    assert_eq!(error.code, "records.source_failure");
    assert_eq!(
        error.details["primary"]["details"]["primary"],
        serde_json::to_value(source).unwrap()
    );
    assert_eq!(
        error.details["primary"]["details"]["cleanup"],
        serde_json::to_value(discard).unwrap()
    );
    assert_eq!(
        error.details["cleanup"],
        serde_json::to_value(close).unwrap()
    );
}
#[test]
fn stream_close_after_work_error_preserves_cancellation_and_consumes_callback_owner() {
    let f = Fixture::new();
    let invocation = alive();
    let marker = Arc::new(());
    let weak = Arc::downgrade(&marker);
    let owner = marker.clone();
    let (budget, token) = independent();
    let blob = open_verified_blob_bounded_with_check(
        &f.0,
        &f.publish(b"work"),
        4,
        &invocation,
        |_| Ok(()),
        move || {
            let _ = &owner;
            budget.check()
        },
    )
    .unwrap();
    drop(marker);
    token.cancel();
    let original = ToolkitError::new("records.consumer_failure", "consumer failed", 5);
    let error = blob.close_after::<()>(Err(original.clone())).unwrap_err();
    assert_eq!(error.code, original.code);
    assert_eq!(
        error.details["primary"],
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(error.details["cleanup"]["code"], "command.cancelled");
    assert!(weak.upgrade().is_none());
}
#[test]
fn stream_noop_and_original_entries_preserve_body_identity_observer_and_rewind() {
    fn send<T: Send>() {}
    send::<VerifiedBlob>();
    let f = Fixture::new();
    let invocation = alive();
    let bytes = vec![2u8; BLOCK + 13];
    let value = f.publish(&bytes);
    let mut old_observed = 0;
    let mut new_observed = 0;
    let mut old =
        open_verified_blob_bounded(&f.0, &value, bytes.len() as u64, &invocation, |block| {
            old_observed += block.len();
            Ok(())
        })
        .unwrap();
    let mut new = open_verified_blob_bounded_with_check(
        &f.0,
        &value,
        bytes.len() as u64,
        &invocation,
        |block| {
            new_observed += block.len();
            Ok(())
        },
        || Ok(()),
    )
    .unwrap();
    assert_eq!(old_observed, bytes.len());
    assert_eq!(new_observed, bytes.len());
    let mut first = Vec::new();
    let mut second = Vec::new();
    old.read_to_end(&mut first).unwrap();
    new.read_to_end(&mut second).unwrap();
    assert_eq!(first, bytes);
    assert_eq!(second, bytes);
    old.rewind().unwrap();
    new.rewind().unwrap();
    assert_eq!(old.read_checked(&mut [0; 1]).unwrap(), 1);
    assert_eq!(new.read_checked(&mut [0; 1]).unwrap(), 1);
    old.close().unwrap();
    new.close().unwrap();
    let old = publish_blob_reader_bounded(
        &f.0,
        &mut PanicReader,
        &reference(&bytes),
        bytes.len() as u64,
        &invocation,
    )
    .unwrap();
    let new = publish_blob_reader_bounded_with_check(
        &f.0,
        &mut PanicReader,
        &reference(&bytes),
        bytes.len() as u64,
        &invocation,
        || Ok(()),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(old.reference()).unwrap(),
        serde_json::to_value(new.reference()).unwrap()
    );
    old.close().unwrap();
    new.close().unwrap();
}
#[test]
fn stream_publish_lock_boundary_stop_releases_guard_without_reading_source() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"lock-boundary";
    let (budget, token) = independent();
    let key = format!("blob:{}", storage::sha256(bytes));
    let lock =
        f.0.join("locks")
            .join(format!("{}.lock", storage::sha256(key.as_bytes())));
    let observed = Arc::new(AtomicBool::new(false));
    let seen = observed.clone();
    let error = failed(publish_blob_reader_bounded_with_check(
        &f.0,
        &mut PanicReader,
        &reference(bytes),
        bytes.len() as u64,
        &invocation,
        move || {
            if lock.exists() {
                seen.store(true, Ordering::Release);
                token.cancel();
            }
            budget.check()
        },
    ));
    stopped(&error, &invocation);
    assert!(observed.load(Ordering::Acquire));
    f.no_publication(bytes);
}
#[test]
fn stream_publish_digest_failure_discards_stage_without_returning_verified_owner() {
    let f = Fixture::new();
    let invocation = alive();
    let expected = b"expected";
    let wrong = b"incorrect";
    let error = failed(publish_blob_reader_bounded_with_check(
        &f.0,
        &mut io::Cursor::new(wrong),
        &BlobRef {
            sha256: storage::sha256(expected),
            bytes: wrong.len() as u64,
        },
        wrong.len() as u64,
        &invocation,
        || Ok(()),
    ));
    assert_eq!(error.code, "vault.blob_integrity");
    assert_eq!(error.message, "published stream digest differs");
    f.no_publication(expected);
}
