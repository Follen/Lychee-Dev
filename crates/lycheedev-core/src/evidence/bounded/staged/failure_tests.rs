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
