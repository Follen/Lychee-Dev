use super::*;
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "lychee-staged-digest-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&p).unwrap();
        Self(p)
    }
    fn path(&self, bytes: &[u8]) -> PathBuf {
        blob_path(&self.0, &storage::sha256(bytes)).unwrap()
    }
    fn no_temps(&self) {
        assert_eq!(temporary_count(&self.0), 0);
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
pub(super) fn temporary_count(home: &Path) -> usize {
    fn walk(path: &Path) -> usize {
        fs::read_dir(path)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".lycheedev-export-")
                {
                    1
                } else if entry.file_type().unwrap().is_dir() {
                    walk(&entry.path())
                } else {
                    0
                }
            })
            .sum()
    }
    walk(&home.join("blobs"))
}
#[derive(Default)]
struct Meter {
    reads: Cell<usize>,
}

#[derive(Default)]
struct ObservedMeter {
    meter: Meter,
    events: std::cell::RefCell<Vec<(BlobStreamPhase, bool)>>,
}
struct ObservedSpan<'a> {
    events: &'a std::cell::RefCell<Vec<(BlobStreamPhase, bool)>>,
    phase: BlobStreamPhase,
}
impl Drop for ObservedSpan<'_> {
    fn drop(&mut self) {
        self.events.borrow_mut().push((self.phase, false));
    }
}
impl CaptureAccounting for ObservedMeter {
    fn check(&self) -> Result<()> { self.meter.check() }
    fn admit_allocation(&self, bytes: usize) -> Result<()> { self.meter.admit_allocation(bytes) }
    fn admit_work(&self, units: usize) -> Result<()> { self.meter.admit_work(units) }
    fn blob_stream_phase(&self, phase: BlobStreamPhase) -> impl Drop + '_ {
        self.events.borrow_mut().push((phase, true));
        ObservedSpan { events: &self.events, phase }
    }
}
impl ObservedMeter {
    fn count(&self, phase: BlobStreamPhase) -> usize {
        let events = self.events.borrow();
        let started = events.iter().filter(|e| **e == (phase, true)).count();
        assert_eq!(started, events.iter().filter(|e| **e == (phase, false)).count());
        started
    }
    fn assert_not_nested(&self) {
        let mut active = None;
        for (phase, start) in self.events.borrow().iter().copied() {
            if start {
                assert_eq!(active.replace(phase), None);
            } else {
                assert_eq!(active.take(), Some(phase));
            }
        }
        assert_eq!(active, None);
    }
}
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        panic!("caller owns the one fixed buffer reservation")
    }
    fn admit_work(&self, n: usize) -> Result<()> {
        assert_eq!(n, 1);
        self.reads.set(self.reads.get() + n);
        Ok(())
    }
}
fn failed(result: Result<VerifiedBlob>) -> ToolkitError {
    match result {
        Err(e) => e,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("expected failure")
        }
    }
}
fn publish(
    f: &Fixture,
    source: &mut impl Read,
    len: u64,
    meter: &impl CaptureAccounting,
) -> Result<VerifiedBlob> {
    publish_blob_stream_bounded_with_check(
        &f.0,
        source,
        len,
        len,
        &OperationControl::default(),
        || Ok(()),
        meter,
    )
}
fn cause() -> ToolkitError {
    ToolkitError::new(
        "container.key_unavailable",
        "business EOF validation failed",
        3,
    )
    .with_details(serde_json::json!({"missingKey":true,"key":"fixture"}))
}
struct EofFailure {
    source: io::Cursor<Vec<u8>>,
    calls: usize,
}
impl Read for EofFailure {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        assert!(!out.is_empty());
        self.calls += 1;
        if self.source.position() == self.source.get_ref().len() as u64 {
            return Err(io::Error::other(cause()));
        }
        self.source.read(out)
    }
}
struct Generated {
    remaining: usize,
    offset: usize,
    calls: usize,
    largest: usize,
}
impl Read for Generated {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        assert!(!out.is_empty());
        self.calls += 1;
        self.largest = self.largest.max(out.len());
        let n = out.len().min(self.remaining);
        for (i, b) in out[..n].iter_mut().enumerate() {
            *b = ((self.offset + i) % 251) as u8;
        }
        self.remaining -= n;
        self.offset += n;
        Ok(n)
    }
}
#[test]
fn staged_generated_body_uses_one_source_traversal_and_one_live_block_per_pass() {
    let f = Fixture::new();
    let size = 8 * BLOCK + 13;
    let mut source = Generated {
        remaining: size,
        offset: 0,
        calls: 0,
        largest: 0,
    };
    let meter = Meter::default();
    let mut blob = publish(&f, &mut source, size as u64, &meter).unwrap();
    assert_eq!(source.offset, size);
    assert_eq!(source.calls, size.div_ceil(BLOCK) + 1);
    assert_eq!(source.largest, BLOCK);
    assert_eq!(meter.reads.get(), 3 * (size.div_ceil(BLOCK) + 1));
    assert_eq!(blob.reference().bytes, size as u64);
    let mut out = [0u8; 8192];
    let mut offset = 0;
    let mut hash = Sha256::new();
    loop {
        let n = blob.read_checked(&mut out).unwrap();
        if n == 0 {
            break;
        }
        for (i, b) in out[..n].iter().enumerate() {
            assert_eq!(*b, ((offset + i) % 251) as u8);
        }
        hash.update(&out[..n]);
        offset += n;
    }
    assert_eq!(offset, size);
    assert_eq!(hex::encode(hash.finalize()), blob.reference().sha256);
    blob.close().unwrap();
    f.no_temps();
}

#[test]
fn staged_publisher_observer_preserves_bytes_work_and_real_passes_on_new_and_reused_blob() {
    use BlobStreamPhase::*;
    let f = Fixture::new();
    let body = vec![31; 2 * BLOCK + 17];
    let blocks = body.len().div_ceil(BLOCK) + 1;
    for reused in [false, true] {
        let meter = ObservedMeter::default();
        let mut source = io::Cursor::new(&body);
        let mut blob = publish(&f, &mut source, body.len() as u64, &meter).unwrap();
        assert_eq!(source.position(), body.len() as u64);
        assert_eq!(meter.meter.reads.get(), if reused { 2 } else { 3 } * blocks);
        assert_eq!(blob.reference().sha256, storage::sha256(&body));
        let mut readback = Vec::new();
        blob.read_to_end(&mut readback).unwrap();
        assert_eq!(readback, body);
        blob.close().unwrap();
        for phase in [SourceSpool, PrivateSpoolReadReady, FinalVerify, DigestLock, ShardOpen] {
            assert_eq!(meter.count(phase), 1);
        }
        for phase in [ShardCopy, FinalStageSeal, Rename] {
            assert_eq!(meter.count(phase), usize::from(!reused));
        }
        for phase in [StageCreate, WinnerOpen] {
            assert_eq!(meter.count(phase), if reused { 1 } else { 2 });
        }
        assert_eq!(meter.count(CheckedClose), if reused { 3 } else { 4 });
        meter.assert_not_nested();
        f.no_temps();
    }
}

#[test]
fn staged_publisher_observer_keeps_source_error_and_only_observes_acquired_cleanup() {
    use BlobStreamPhase::*;
    let f = Fixture::new();
    let meter = ObservedMeter::default();
    let mut source = EofFailure { source: io::Cursor::new(b"abc".to_vec()), calls: 0 };
    let error = failed(publish(&f, &mut source, 3, &meter));
    assert_eq!(error.code, cause().code);
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert_eq!(source.calls, 2);
    assert_eq!(meter.meter.reads.get(), 2);
    assert_eq!(meter.count(SourceSpool), 1);
    assert_eq!(meter.count(StageCreate), 1);
    assert_eq!(meter.count(CheckedClose), 2);
    for phase in [ShardCopy, FinalVerify, PrivateSpoolReadReady, FinalStageSeal, DigestLock, ShardOpen, Rename, WinnerOpen] {
        assert_eq!(meter.count(phase), 0);
    }
    meter.assert_not_nested();
    assert!(!f.path(b"abc").exists());
    f.no_temps();
}
#[test]
fn staged_empty_short_and_excess_sources_require_real_eof() {
    let f = Fixture::new();
    let meter = Meter::default();
    let mut empty = io::Cursor::new(b"");
    let blob = publish(&f, &mut empty, 0, &meter).unwrap();
    assert_eq!(blob.reference().sha256, storage::sha256(b""));
    blob.close().unwrap();
    assert_eq!(meter.reads.get(), 3);
    for bytes in [b"abc".as_slice(), b"abcde".as_slice()] {
        let error = failed(publish(
            &f,
            &mut io::Cursor::new(bytes),
            4,
            &Meter::default(),
        ));
        assert_eq!(error.code, "vault.blob_integrity");
        assert_eq!(error.details["publication"]["state"], "not_published");
        f.no_temps();
    }
    assert!(!f.path(b"abcd").exists());
}
#[test]
fn staged_typed_business_eof_failure_never_publishes_or_erases_missing_key() {
    let f = Fixture::new();
    let mut reader = EofFailure {
        source: io::Cursor::new(b"body".to_vec()),
        calls: 0,
    };
    let error = failed(publish(&f, &mut reader, 4, &Meter::default()));
    assert_eq!(reader.calls, 2);
    assert_eq!(error.code, cause().code);
    assert_eq!(error.message, cause().message);
    assert_eq!(error.details["missingKey"], true);
    assert_eq!(error.details["key"], "fixture");
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert!(!f.path(b"body").exists());
    f.no_temps();
}
#[test]
fn staged_existing_blob_does_not_bypass_this_sources_eof_validation() {
    let f = Fixture::new();
    let body = b"body";
    publish_blob_bounded(&f.0, body, body.len(), &OperationControl::default()).unwrap();
    let mut reader = EofFailure {
        source: io::Cursor::new(body.to_vec()),
        calls: 0,
    };
    let error = failed(publish(&f, &mut reader, 4, &Meter::default()));
    assert_eq!(error.code, cause().code);
    assert_eq!(reader.calls, 2);
    assert_eq!(fs::read(f.path(body)).unwrap(), body);
    let meter = Meter::default();
    let blob = publish(&f, &mut io::Cursor::new(body), 4, &meter).unwrap();
    blob.close().unwrap();
    assert_eq!(meter.reads.get(), 4);
    f.no_temps();
}
#[test]
fn staged_partial_read_records_original_error_and_both_stops_then_cleans() {
    for source_error in [false, true] {
        let f = Fixture::new();
        let inv = OperationControl::default();
        let token = inv.tracked_cancel_handle().unwrap();
        let budget = Arc::new(AtomicBool::new(false));
        let budget_reader = budget.clone();
        struct StopReader {
            token: crate::control::TrackedCancellation,
            budget: Arc<AtomicBool>,
            source_error: bool,
            calls: usize,
        }
        impl Read for StopReader {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                self.calls += 1;
                out[..3].copy_from_slice(b"abc");
                self.token.cancel();
                self.budget.store(true, Ordering::Release);
                if self.source_error {
                    Err(io::Error::other(ToolkitError::new(
                        "fixture.source",
                        "partial source failed",
                        5,
                    )))
                } else {
                    Ok(3)
                }
            }
        }
        let mut reader = StopReader {
            token,
            budget: budget_reader,
            source_error,
            calls: 0,
        };
        let error = failed(publish_blob_stream_bounded_with_check(
            &f.0,
            &mut reader,
            8,
            8,
            &inv,
            move || {
                if budget.load(Ordering::Acquire) {
                    Err(ToolkitError::new(
                        "fixture.budget",
                        "independent budget stopped",
                        7,
                    ))
                } else {
                    Ok(())
                }
            },
            &Meter::default(),
        ));
        assert_eq!(reader.calls, 1);
        assert_eq!(
            error.code,
            if source_error {
                "fixture.source"
            } else {
                "command.cancelled"
            }
        );
        let rendered = serde_json::to_string(&error).unwrap();
        assert!(rendered.contains("command.cancelled"));
        assert!(rendered.contains("fixture.budget"));
        assert_eq!(error.details["publication"]["state"], "not_published");
        f.no_temps();
    }
}
#[test]
fn staged_work_stop_during_disk_copy_owns_both_temps_without_source_replay() {
    let f = Fixture::new();
    let source_calls = Rc::new(Cell::new(0));
    struct Counted {
        source: io::Cursor<Vec<u8>>,
        calls: Rc<Cell<usize>>,
    }
    impl Read for Counted {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            self.calls.set(self.calls.get() + 1);
            self.source.read(out)
        }
    }
    struct StopMeter {
        calls: Cell<usize>,
        source_calls: Rc<Cell<usize>>,
        home: PathBuf,
    }
    impl CaptureAccounting for StopMeter {
        fn check(&self) -> Result<()> {
            Ok(())
        }
        fn admit_allocation(&self, _: usize) -> Result<()> {
            panic!("no permanent buffer allocation")
        }
        fn admit_work(&self, n: usize) -> Result<()> {
            assert_eq!(n, 1);
            self.calls.set(self.calls.get() + 1);
            if self.calls.get() == 3 {
                assert_eq!(self.source_calls.get(), 2);
                assert_eq!(temporary_count(&self.home), 2);
                Err(ToolkitError::new(
                    "fixture.work",
                    "disk copy not admitted",
                    3,
                ))
            } else {
                Ok(())
            }
        }
    }
    let meter = StopMeter {
        calls: Cell::new(0),
        source_calls: source_calls.clone(),
        home: f.0.clone(),
    };
    let mut reader = Counted {
        source: io::Cursor::new(b"data".to_vec()),
        calls: source_calls.clone(),
    };
    let error = failed(publish(&f, &mut reader, 4, &meter));
    assert_eq!(error.code, "fixture.work");
    assert_eq!(source_calls.get(), 2);
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert!(!f.path(b"data").exists());
    f.no_temps();
}
#[test]
fn staged_native_collision_verifies_good_or_corrupt_winner_without_overwrite() {
    for corrupt in [false, true] {
        let f = Fixture::new();
        let body = b"body";
        let path = f.path(body);
        let raced = path.clone();
        test_hooks::set_before_rename(move || {
            fs::write(raced, if corrupt { b"evil" } else { b"body" }).unwrap()
        });
        let result = publish(&f, &mut io::Cursor::new(body), 4, &Meter::default());
        if corrupt {
            let error = failed(result);
            assert_eq!(error.code, "vault.blob_integrity");
            assert_eq!(error.details["publication"]["state"], "not_published");
            assert_eq!(error.details["publication"]["reused"], true);
        } else {
            result.unwrap().close().unwrap();
        }
        assert_eq!(
            fs::read(path).unwrap(),
            if corrupt { b"evil" } else { b"body" }
        );
        f.no_temps();
    }
}
#[test]
fn staged_existing_wrong_length_is_terminal_and_preserves_original_entry() {
    let f = Fixture::new();
    let path = f.path(b"body");
    fs::create_dir(path.parent().unwrap()).unwrap();
    fs::write(&path, b"wrong length").unwrap();
    let error = failed(publish(
        &f,
        &mut io::Cursor::new(b"body"),
        4,
        &Meter::default(),
    ));
    assert_eq!(error.code, "vault.blob_integrity");
    assert_eq!(fs::read(path).unwrap(), b"wrong length");
    f.no_temps();
}
#[test]
fn staged_source_failure_plus_real_stage_close_boundary_preserves_both() {
    let f = Fixture::new();
    test_hooks::set_after_stage_close(|| {
        Err(ToolkitError::new(
            "fixture.close",
            "close reported failure",
            5,
        ))
    });
    let mut reader = EofFailure {
        source: io::Cursor::new(b"body".to_vec()),
        calls: 0,
    };
    let error = failed(publish(&f, &mut reader, 4, &Meter::default()));
    assert_eq!(error.code, cause().code);
    let rendered = serde_json::to_string(&error).unwrap();
    assert!(rendered.contains("fixture.close"));
    assert!(rendered.contains("missingKey"));
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert_eq!(reader.calls, 2);
    f.no_temps();
}
#[test]
fn staged_post_rename_close_failure_retains_published_identity_and_does_not_replay() {
    let f = Fixture::new();
    test_hooks::set_after_stage_close(|| {
        Err(ToolkitError::new(
            "fixture.close",
            "final stage close failed",
            5,
        ))
    });
    let mut reader = Generated {
        remaining: 4,
        offset: 0,
        calls: 0,
        largest: 0,
    };
    let error = failed(publish(&f, &mut reader, 4, &Meter::default()));
    assert_eq!(error.code, "fixture.close");
    assert_eq!(error.details["publication"]["state"], "published");
    let digest = error.details["publication"]["reference"]["sha256"]
        .as_str()
        .unwrap();
    assert_eq!(
        fs::read(blob_path(&f.0, digest).unwrap()).unwrap(),
        [0, 1, 2, 3]
    );
    assert_eq!(reader.calls, 2);
    f.no_temps();
}
#[test]
fn staged_original_parent_and_final_shard_cannot_be_replaced_during_publication() {
    let f = Fixture::new();
    let root = f.0.join("blobs");
    let moved = f.0.join("moved");
    let shard = f.path(b"body").parent().unwrap().to_path_buf();
    let moved_shard = f.0.join("moved-shard");
    struct Checked {
        source: io::Cursor<Vec<u8>>,
        root: PathBuf,
        moved: PathBuf,
    }
    impl Read for Checked {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            assert!(fs::rename(&self.root, &self.moved).is_err());
            self.source.read(out)
        }
    }
    test_hooks::set_before_rename(move || assert!(fs::rename(shard, moved_shard).is_err()));
    let mut reader = Checked {
        source: io::Cursor::new(b"body".to_vec()),
        root,
        moved,
    };
    publish(&f, &mut reader, 4, &Meter::default())
        .unwrap()
        .close()
        .unwrap();
    f.no_temps();
}
#[test]
fn staged_borrowed_meter_ends_at_return_but_owned_stop_lives_until_close() {
    let f = Fixture::new();
    let stopped = Arc::new(AtomicBool::new(false));
    let worker_stop = stopped.clone();
    let marker = Arc::new(());
    let weak = Arc::downgrade(&marker);
    let mut blob = {
        let meter = Meter::default();
        publish_blob_stream_bounded_with_check(
            &f.0,
            &mut io::Cursor::new(b"body"),
            4,
            4,
            &OperationControl::default(),
            move || {
                let _ = &marker;
                if worker_stop.load(Ordering::Acquire) {
                    Err(ToolkitError::new(
                        "fixture.owned_budget",
                        "original budget",
                        7,
                    ))
                } else {
                    Ok(())
                }
            },
            &meter,
        )
        .unwrap()
    };
    assert!(weak.upgrade().is_some());
    stopped.store(true, Ordering::Release);
    assert_eq!(
        blob.read_checked(&mut [0u8; 1]).unwrap_err().code,
        "fixture.owned_budget"
    );
    assert_eq!(blob.rewind().unwrap_err().code, "fixture.owned_budget");
    assert_eq!(blob.close().unwrap_err().code, "fixture.owned_budget");
    assert!(weak.upgrade().is_none());
    f.no_temps();
}
#[test]
fn staged_admission_failure_precedes_workspace_and_source_io() {
    struct NoRead;
    impl Read for NoRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("source must remain untouched")
        }
    }
    let f = Fixture::new();
    let missing = f.0.join("missing");
    let error = failed(publish_blob_stream_bounded_with_check(
        &missing,
        &mut NoRead,
        2,
        1,
        &OperationControl::default(),
        || Ok(()),
        &Meter::default(),
    ));
    assert_eq!(error.code, "vault.blob_limit");
    assert!(!missing.exists());
    assert_eq!(error.details["publication"]["state"], "not_published");
}
#[test]
fn staged_two_same_digest_sources_finish_once_then_coalesce_under_original_lease() {
    let f = Fixture::new();
    let body = b"same";
    let held = ExclusiveLock::resource(
        &f.0.join("locks"),
        &format!("blob:{}", storage::sha256(body)),
    )
    .unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for _ in 0..2 {
            let home = &f.0;
            let tx = tx.clone();
            workers.push(scope.spawn(move || {
                ExclusiveLock::set_blob_wait_observer(move || tx.send(()).unwrap());
                let meter = Meter::default();
                let blob = publish_blob_stream_bounded_with_check(
                    home,
                    &mut io::Cursor::new(body),
                    4,
                    4,
                    &OperationControl::default(),
                    || Ok(()),
                    &meter,
                )?;
                let reference = blob.reference().clone();
                blob.close()?;
                Ok::<_, ToolkitError>(reference)
            }));
        }
        for _ in 0..2 {
            rx.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        assert_eq!(temporary_count(&f.0), 2);
        held.close().unwrap();
        let first = workers.remove(0).join().unwrap().unwrap();
        let second = workers.remove(0).join().unwrap().unwrap();
        assert_eq!(first.sha256, second.sha256);
    });
    f.no_temps();
}

#[test]
fn staged_prior_publication_fact_cannot_be_downgraded_into_a_key_retry() {
    struct Prior;
    impl Read for Prior {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            let mut error = cause();
            error.details["publication"] =
                serde_json::json!({"state":"unknown","reference":{"sha256":"prior-owner"}});
            Err(io::Error::other(error))
        }
    }
    let f = Fixture::new();
    let error = failed(publish(&f, &mut Prior, 4, &Meter::default()));
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!(error.details["missingKey"], true);
    assert_eq!(error.details["publication"]["state"], "unknown");
    assert_eq!(
        error.details["publication"]["attemptState"],
        "not_published"
    );
    assert_eq!(
        error.details["publication"]["prior"]["reference"]["sha256"],
        "prior-owner"
    );
    f.no_temps();
}
