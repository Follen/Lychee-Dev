use super::*;
use std::{cell::Cell, os::windows::io::AsRawHandle};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "lychee-blob-wait-{}-{}-{}",
            std::process::id(),
            utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn failed(result: Result<ExclusiveLock>) -> ToolkitError {
    match result {
        Err(e) => e,
        Ok(lock) => {
            lock.close().unwrap();
            panic!("expected failure")
        }
    }
}
#[test]
fn blob_wait_retries_real_contention_on_exactly_the_same_open_file() {
    let f = Fixture::new();
    let path = f.0.join("lock");
    let mut first = Some(ExclusiveLock::acquire(&path).unwrap());
    let mut handle = None;
    let mut attempts = 0;
    let sleeps = Cell::new(0);
    let acquired = ExclusiveLock::acquire_wait_with(
        &path,
        None,
        || Ok(()),
        |file| {
            attempts += 1;
            let actual = file.as_raw_handle();
            if let Some(original) = handle {
                assert_eq!(actual, original);
            } else {
                handle = Some(actual);
            }
            if attempts == 3 {
                first.take().unwrap().close().unwrap();
            }
            fs2::FileExt::try_lock_exclusive(file)
        },
        |delay| {
            assert_eq!(delay, Duration::from_millis(25));
            sleeps.set(sleeps.get() + 1);
        },
        lycheedev_windows::filesystem::close_file,
    )
    .unwrap();
    assert_eq!(attempts, 3);
    assert_eq!(sleeps.get(), 2);
    assert!(matches!(ExclusiveLock::acquire(&path), Err(e) if e.code == "workspace.busy"));
    acquired.close().unwrap();
    assert!(path.is_file());
    ExclusiveLock::acquire(&path).unwrap().close().unwrap();
}
#[test]
fn blob_wait_non_contention_error_is_not_retried_and_closes_once() {
    let f = Fixture::new();
    let tries = Cell::new(0);
    let closes = Cell::new(0);
    let error = failed(ExclusiveLock::acquire_wait_with(
        &f.0.join("lock"),
        None,
        || Ok(()),
        |_| {
            tries.set(tries.get() + 1);
            Err(std::io::Error::from_raw_os_error(5))
        },
        |_| panic!("permission failure must not sleep"),
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)
        },
    ));
    assert_eq!(error.code, "external.io");
    assert_eq!(tries.get(), 1);
    assert_eq!(closes.get(), 1);
    assert!(error.details.is_null());
}
#[test]
fn blob_wait_success_then_stop_consumes_lock_before_returning_typed_stop() {
    let f = Fixture::new();
    let path = f.0.join("lock");
    let acquired = Cell::new(false);
    let closes = Cell::new(0);
    let stop = ToolkitError::new(
        "fixture.original_invocation",
        "original invocation stopped",
        7,
    )
    .with_details(serde_json::json!({"owner":7}));
    let error = failed(ExclusiveLock::acquire_wait_with(
        &path,
        None,
        || {
            if acquired.get() {
                Err(stop.clone())
            } else {
                Ok(())
            }
        },
        |file| {
            fs2::FileExt::try_lock_exclusive(file)?;
            acquired.set(true);
            Ok(())
        },
        |_| panic!("no contention"),
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)
        },
    ));
    assert_eq!(error.code, stop.code);
    assert_eq!(error.details, stop.details);
    assert_eq!(closes.get(), 1);
    ExclusiveLock::acquire(&path).unwrap().close().unwrap();
}
#[test]
fn blob_wait_stop_close_and_late_stop_keep_all_distinct_causes() {
    let f = Fixture::new();
    let phase = Cell::new(0);
    let closes = Cell::new(0);
    let attempts = Cell::new(0);
    let error = failed(ExclusiveLock::acquire_wait_with(
        &f.0.join("lock"),
        None,
        || match phase.get() {
            0 => Ok(()),
            1 => Err(ToolkitError::new("fixture.budget", "budget stop", 7)),
            _ => Err(ToolkitError::new(
                "fixture.invocation",
                "late invocation",
                6,
            )),
        },
        |_| {
            attempts.set(attempts.get() + 1);
            Err(fs2::lock_contended_error())
        },
        |_| phase.set(1),
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)?;
            phase.set(2);
            Err(std::io::Error::other("close failed"))
        },
    ));
    assert_eq!(attempts.get(), 1);
    assert_eq!(closes.get(), 1);
    assert_eq!(error.code, "fixture.budget");
    assert_eq!(error.details["primary"]["code"], "fixture.budget");
    assert_eq!(error.details["cleanup"]["code"], "external.io");
    assert_eq!(error.details["control"]["code"], "fixture.invocation");
}
#[test]
fn blob_wait_pause_is_capped_by_original_deadline_without_replacing_error() {
    let f = Fixture::new();
    let tries = Cell::new(0);
    let stop = Cell::new(false);
    let original = Instant::now() + Duration::from_millis(10);
    let error = failed(ExclusiveLock::acquire_wait_with(
        &f.0.join("lock"),
        Some(original),
        || {
            if stop.get() {
                Err(ToolkitError::new(
                    "fixture.original_deadline",
                    "caller deadline",
                    6,
                ))
            } else {
                Ok(())
            }
        },
        |_| {
            tries.set(tries.get() + 1);
            Err(fs2::lock_contended_error())
        },
        |delay| {
            assert!(delay <= Duration::from_millis(10));
            stop.set(true);
        },
        lycheedev_windows::filesystem::close_file,
    ));
    assert_eq!(error.code, "fixture.original_deadline");
    assert_eq!(tries.get(), 1);
}
