//! Real staged ownership/cleanup with deterministic private-write fault injection.
//! Codec validation itself is covered by Data's eager/incremental comparisons.
use super::*;
use std::{
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-stream-first-error-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        workspace::initialize(&path).unwrap();
        Self(path)
    }
    fn no_files(&self) {
        fn count(path: &Path) -> usize {
            fs::read_dir(path)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_dir() {
                        count(&entry.path())
                    } else {
                        1
                    }
                })
                .sum()
        }
        assert_eq!(
            count(&self.0.join("blobs")),
            0,
            "neither private stage nor published digest may remain"
        );
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
#[derive(Default)]
struct Meter(Cell<usize>);
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        panic!("caller owns buffer")
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}
struct DeferredSource {
    calls: usize,
    late_error: bool,
    early_error: bool,
}
impl Read for DeferredSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        if self.early_error || self.late_error && self.calls > 1 {
            return Err(io::Error::other(ToolkitError::new(
                "fixture.source",
                "deferred source validation",
                4,
            )));
        }
        out.fill(7);
        Ok(out.len())
    }
}
fn failed(result: Result<VerifiedBlob>) -> ToolkitError {
    match result {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("expected failure")
        }
    }
}

#[test]
fn staged_first_private_write_failure_keeps_sink_and_cleanup_without_reading_deferred_error() {
    for late_error in [false, true] {
        for cancel in [false, true] {
            let fixture = Fixture::new();
            let control = OperationControl::default();
            let token = control.tracked_cancel_handle().unwrap();
            let mut source = DeferredSource {
                calls: 0,
                late_error,
                early_error: false,
            };
            let meter = Meter::default();
            test_hooks::set_before_stage_write(move || {
                if cancel {
                    token.cancel();
                }
                Err(ToolkitError::new(
                    "fixture.sink",
                    "private stage write failed",
                    5,
                ))
            });
            test_hooks::set_after_stage_close(|| {
                Err(ToolkitError::new(
                    "fixture.close",
                    "checked stage close failed",
                    5,
                ))
            });
            let error = failed(publish_blob_stream_bounded_with_check(
                &fixture.0,
                &mut source,
                (2 * BLOCK) as u64,
                (2 * BLOCK) as u64,
                &control,
                || Ok(()),
                &meter,
            ));
            assert_eq!(error.code, "fixture.sink");
            assert_eq!(source.calls, 1);
            assert_eq!(meter.0.get(), 1);
            let all = serde_json::to_string(&error).unwrap();
            assert!(all.contains("fixture.sink") && all.contains("fixture.close"));
            assert_eq!(all.contains("command.cancelled"), cancel);
            assert!(
                !all.contains("fixture.source"),
                "unobserved late input is not fabricated or drained"
            );
            assert_eq!(error.details["publication"]["state"], "not_published");
            fixture.no_files();
        }
    }
}

#[test]
fn staged_producer_error_never_reenters_reader_or_attempts_private_write() {
    let fixture = Fixture::new();
    let mut source = DeferredSource {
        calls: 0,
        late_error: false,
        early_error: true,
    };
    let writes = std::rc::Rc::new(Cell::new(0));
    let observed = writes.clone();
    test_hooks::set_before_stage_write(move || {
        observed.set(observed.get() + 1);
        Ok(())
    });
    test_hooks::set_after_stage_close(|| {
        Err(ToolkitError::new(
            "fixture.close",
            "checked stage close failed",
            5,
        ))
    });
    let error = failed(publish_blob_stream_bounded_with_check(
        &fixture.0,
        &mut source,
        BLOCK as u64,
        BLOCK as u64,
        &OperationControl::default(),
        || Ok(()),
        &Meter::default(),
    ));
    assert_eq!(error.code, "fixture.source");
    assert_eq!(source.calls, 1);
    assert_eq!(writes.get(), 0);
    assert!(
        serde_json::to_string(&error)
            .unwrap()
            .contains("fixture.close")
    );
    // Remove this deliberately unused thread-local hook before another test.
    test_hooks::before_stage_write().unwrap().unwrap();
    assert_eq!(writes.get(), 1);
    fixture.no_files();
}

struct CountedSource {
    body: io::Cursor<Vec<u8>>,
    calls: usize,
}
impl CountedSource {
    fn new() -> Self { Self { body: io::Cursor::new(b"verified body".to_vec()), calls: 0 } }
}
impl Read for CountedSource {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.calls += 1;
        self.body.read(out)
    }
}
fn private_test_error(code: &str) -> ToolkitError {
    ToolkitError::new(code, "injected real-owner boundary failure", 5)
}

#[test]
fn staged_read_ready_and_final_seal_errors_preserve_stop_cleanup_and_nonpublication() {
    for final_stage in [false, true] {
        for cancel in [false, true] {
            let fixture = Fixture::new();
            let control = OperationControl::default();
            let token = control.tracked_cancel_handle().unwrap();
            let failure = move || {
                if cancel { token.cancel(); }
                Err(private_test_error("fixture.seal"))
            };
            if final_stage { test_hooks::set_after_final_stage_seal(failure); }
            else { test_hooks::set_after_private_read_ready(failure); }
            test_hooks::set_after_stage_close(|| Err(private_test_error("fixture.close")));
            let mut source = CountedSource::new();
            let len = source.body.get_ref().len() as u64;
            let meter = Meter::default();
            let error = failed(publish_blob_stream_bounded_with_check(&fixture.0, &mut source, len, len, &control, || Ok(()), &meter));
            assert_eq!(source.calls, 2, "never replay the authenticated source");
            assert_eq!(meter.0.get(), if final_stage { 4 } else { 2 });
            assert_eq!(error.code, "fixture.seal");
            let details = serde_json::to_string(&error).unwrap();
            assert!(details.contains("fixture.close"));
            assert_eq!(details.contains("command.cancelled"), cancel);
            assert_eq!(error.details["publication"]["state"], "not_published");
            fixture.no_files();
        }
    }
}

#[test]
fn staged_cancel_after_private_read_ready_or_final_sync_never_publishes() {
    for final_stage in [false, true] {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let token = control.tracked_cancel_handle().unwrap();
        let stop = move || { token.cancel(); Ok(()) };
        if final_stage { test_hooks::set_after_final_stage_seal(stop); }
        else { test_hooks::set_after_private_read_ready(stop); }
        let mut source = CountedSource::new();
        let len = source.body.get_ref().len() as u64;
        let meter = Meter::default();
        let error = failed(publish_blob_stream_bounded_with_check(&fixture.0, &mut source, len, len, &control, || Ok(()), &meter));
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(source.calls, 2);
        assert_eq!(meter.0.get(), if final_stage { 4 } else { 2 });
        assert_eq!(error.details["publication"]["state"], "not_published");
        fixture.no_files();
    }
}

#[test]
fn staged_shard_write_error_after_read_ready_is_checked_without_source_replay() {
    let fixture = Fixture::new();
    test_hooks::set_after_private_read_ready(|| {
        test_hooks::set_before_stage_write(|| Err(private_test_error("fixture.disk_write")));
        Ok(())
    });
    test_hooks::set_after_stage_close(|| Err(private_test_error("fixture.close")));
    let mut source = CountedSource::new();
    let len = source.body.get_ref().len() as u64;
    let meter = Meter::default();
    let error = failed(publish_blob_stream_bounded_with_check(&fixture.0, &mut source, len, len, &OperationControl::default(), || Ok(()), &meter));
    assert_eq!(error.code, "fixture.disk_write");
    assert_eq!(source.calls, 2);
    assert_eq!(meter.0.get(), 3);
    assert!(serde_json::to_string(&error).unwrap().contains("fixture.close"));
    assert_eq!(error.details["publication"]["state"], "not_published");
    fixture.no_files();
}

#[test]
fn staged_private_spool_close_failure_preserves_final_publication_and_no_replay() {
    let fixture = Fixture::new();
    // The final durable stage closes first, before the verified winner opens.
    // Inject the following private-spool close only after that real close.
    test_hooks::set_after_stage_close(|| {
        test_hooks::set_after_stage_close(|| Err(private_test_error("fixture.private_close")));
        Ok(())
    });
    let mut source = CountedSource::new();
    let body = source.body.get_ref().clone();
    let len = body.len() as u64;
    let meter = Meter::default();
    let error = failed(publish_blob_stream_bounded_with_check(&fixture.0, &mut source, len, len, &OperationControl::default(), || Ok(()), &meter));
    assert_eq!(source.calls, 2);
    assert_eq!(meter.0.get(), 6);
    assert_eq!(error.code, "fixture.private_close");
    assert_eq!(error.details["publication"]["state"], "published");
    assert_eq!(fs::read(blob_path(&fixture.0, &storage::sha256(&body)).unwrap()).unwrap(), body);
    assert_eq!(super::tests::temporary_count(&fixture.0), 0);
}

#[test]
fn staged_private_spool_read_failure_prevents_final_seal_and_publication() {
    let fixture = Fixture::new();
    test_hooks::set_after_private_read_ready(|| {
        test_hooks::set_before_stream_read(|| Err(private_test_error("fixture.disk_read")));
        Ok(())
    });
    test_hooks::set_after_stage_close(|| Err(private_test_error("fixture.close")));
    let mut source = CountedSource::new();
    let len = source.body.get_ref().len() as u64;
    let meter = Meter::default();
    let error = failed(publish_blob_stream_bounded_with_check(&fixture.0, &mut source, len, len, &OperationControl::default(), || Ok(()), &meter));
    assert_eq!(error.code, "fixture.disk_read");
    assert_eq!(source.calls, 2);
    assert_eq!(meter.0.get(), 3);
    assert!(serde_json::to_string(&error).unwrap().contains("fixture.close"));
    assert_eq!(error.details["publication"]["state"], "not_published");
    fixture.no_files();
}
