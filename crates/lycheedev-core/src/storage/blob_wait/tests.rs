use super::*;
use std::{
    cell::{Cell, RefCell},
    os::windows::io::AsRawHandle,
};
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
fn prepared_parent_wait_preserves_stop_close_and_late_stop_causes() {
    let f = Fixture::new();
    let phase = Cell::new(0);
    let preparations = Cell::new(0);
    let closes = Cell::new(0);
    let attempts = Cell::new(0);
    let error = failed(ExclusiveLock::acquire_prepared_with(
        &f.0.join("lock"),
        || match phase.get() {
            0 => Ok(()),
            1 => Err(ToolkitError::new("fixture.budget", "budget stop", 7)),
            _ => Err(ToolkitError::new(
                "fixture.invocation",
                "late invocation",
                6,
            )),
        },
        || {
            preparations.set(preparations.get() + 1);
            Ok(())
        },
        |file, check| {
            ExclusiveLock::wait_on_file(
                file,
                check,
                None,
                |_| {
                    attempts.set(attempts.get() + 1);
                    Err(fs2::lock_contended_error())
                },
                |_| phase.set(1),
            )
        },
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)?;
            phase.set(2);
            Err(std::io::Error::other("close failed"))
        },
    ));
    assert_eq!(
        (preparations.get(), attempts.get(), closes.get()),
        (1, 1, 1)
    );
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

#[test]
fn lock_observation_disabled_never_reads_its_clock_and_keeps_the_result() {
    let original: Result<()> = Err(ToolkitError::new("fixture.original", "unchanged", 7));
    let result = measured_lock_with_clock(
        LockPhase::Open,
        None,
        || panic!("disabled observation must not read a clock"),
        || original,
    );
    assert_eq!(result.unwrap_err().code, "fixture.original");
}

#[test]
fn source_lock_profile_counts_real_attempts_and_backoffs_on_one_original_handle() {
    let f = Fixture::new();
    let path = f.0.join("lock");
    let mut first = Some(ExclusiveLock::acquire(&path).unwrap());
    let mut original = None;
    let mut attempts = 0;
    let phases = RefCell::new(Vec::new());
    let observe = |phase, _| phases.borrow_mut().push(phase);
    let observer: LockObserver<'_> = Some(&observe);
    let acquired = ExclusiveLock::acquire_prepared_observed_with(
        &path,
        || Ok(()),
        || Ok(()),
        |file, check| {
            ExclusiveLock::wait_on_file_observed(
                file,
                check,
                None,
                |file| {
                    attempts += 1;
                    if let Some(original) = original {
                        assert_eq!(file.as_raw_handle(), original);
                    } else {
                        original = Some(file.as_raw_handle());
                    }
                    if attempts == 3 {
                        first.take().unwrap().close().unwrap();
                    }
                    fs2::FileExt::try_lock_exclusive(file)
                },
                |delay| assert_eq!(delay, Duration::from_millis(25)),
                observer,
            )
        },
        lycheedev_windows::filesystem::close_file,
        observer,
    )
    .unwrap();
    use LockPhase::*;
    assert_eq!(
        *phases.borrow(),
        [
            Verify,
            Open,
            KernelAttempt,
            ContentionBackoff,
            KernelAttempt,
            ContentionBackoff,
            KernelAttempt
        ]
    );
    assert_eq!(attempts, 3);
    assert!(matches!(ExclusiveLock::acquire(&path), Err(e) if e.code == "workspace.busy"));
    acquired.close().unwrap();
    ExclusiveLock::acquire(&path).unwrap().close().unwrap();
}

#[test]
fn source_lock_profile_records_failed_verify_or_open_without_inventing_a_close() {
    let f = Fixture::new();
    for fail_verify in [true, false] {
        let phases = RefCell::new(Vec::new());
        let observe = |phase, _| phases.borrow_mut().push(phase);
        // Opening a directory for read/write must fail before acquiring a File.
        let error = failed(ExclusiveLock::acquire_prepared_observed_with(
            &f.0,
            || Ok(()),
            || {
                if fail_verify {
                    Err(ToolkitError::new("fixture.verify", "verify failed", 5))
                } else {
                    Ok(())
                }
            },
            |_, _| panic!("failed preparation/open cannot attempt a kernel lock"),
            |_| panic!("no file owner was acquired"),
            Some(&observe),
        ));
        if fail_verify {
            assert_eq!(error.code, "fixture.verify");
            assert_eq!(*phases.borrow(), [LockPhase::Verify]);
        } else {
            assert_eq!(error.code, "external.io");
            assert_eq!(*phases.borrow(), [LockPhase::Verify, LockPhase::Open]);
        }
    }
}

#[test]
fn source_lock_profile_non_contention_is_one_attempt_and_one_failure_close() {
    let f = Fixture::new();
    let path = f.0.join("lock");
    let phases = RefCell::new(Vec::new());
    let observe = |phase, _| phases.borrow_mut().push(phase);
    let observer: LockObserver<'_> = Some(&observe);
    let closes = Cell::new(0);
    let error = failed(ExclusiveLock::acquire_prepared_observed_with(
        &path,
        || Ok(()),
        || Ok(()),
        |file, check| {
            ExclusiveLock::wait_on_file_observed(
                file,
                check,
                None,
                |_| Err(std::io::Error::from_raw_os_error(5)),
                |_| panic!("non-contention failure must not sleep"),
                observer,
            )
        },
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)
        },
        observer,
    ));
    assert_eq!(error.code, "external.io");
    assert!(error.details.is_null());
    assert_eq!(closes.get(), 1);
    assert_eq!(
        *phases.borrow(),
        [
            LockPhase::Verify,
            LockPhase::Open,
            LockPhase::KernelAttempt,
            LockPhase::FailureClose
        ]
    );
    ExclusiveLock::acquire(&path).unwrap().close().unwrap();
}

#[test]
fn source_lock_profile_keeps_stop_close_and_late_control_causes() {
    let f = Fixture::new();
    for enabled in [false, true] {
        let state = Cell::new(0);
        let closes = Cell::new(0);
        let phases = RefCell::new(Vec::new());
        let observe = |phase, _| phases.borrow_mut().push(phase);
        let observer: LockObserver<'_> = enabled.then_some(&observe);
        let error = failed(ExclusiveLock::acquire_prepared_observed_with(
            &f.0.join("lock"),
            || match state.get() {
                0 => Ok(()),
                1 => Err(ToolkitError::new("fixture.budget", "budget stop", 7)),
                _ => Err(ToolkitError::new(
                    "fixture.invocation",
                    "late invocation",
                    6,
                )),
            },
            || Ok(()),
            |file, check| {
                ExclusiveLock::wait_on_file_observed(
                    file,
                    check,
                    None,
                    |_| Err(fs2::lock_contended_error()),
                    |_| state.set(1),
                    observer,
                )
            },
            |file| {
                closes.set(closes.get() + 1);
                lycheedev_windows::filesystem::close_file(file)?;
                state.set(2);
                Err(std::io::Error::other("close failed"))
            },
            observer,
        ));
        assert_eq!(closes.get(), 1);
        assert_eq!(error.code, "fixture.budget");
        assert_eq!(error.details["primary"]["code"], "fixture.budget");
        assert_eq!(error.details["cleanup"]["code"], "external.io");
        assert_eq!(error.details["control"]["code"], "fixture.invocation");
        use LockPhase::*;
        if enabled {
            assert_eq!(
                *phases.borrow(),
                [Verify, Open, KernelAttempt, ContentionBackoff, FailureClose]
            );
        } else {
            assert!(phases.borrow().is_empty());
        }
    }
}

#[test]
fn source_lock_profile_stops_after_open_or_contended_attempt_without_a_backoff() {
    for stop_after in [LockPhase::Open, LockPhase::KernelAttempt] {
        let f = Fixture::new();
        let path = f.0.join("lock");
        let first = ExclusiveLock::acquire(&path).unwrap();
        let stopped = Cell::new(false);
        let closes = Cell::new(0);
        let phases = RefCell::new(Vec::new());
        let observe = |phase, _| {
            phases.borrow_mut().push(phase);
            if phase == stop_after {
                stopped.set(true);
            }
        };
        let observer: LockObserver<'_> = Some(&observe);
        let error = failed(ExclusiveLock::acquire_prepared_observed_with(
            &path,
            || {
                if stopped.get() {
                    Err(ToolkitError::new("fixture.invocation", "original stop", 7))
                } else {
                    Ok(())
                }
            },
            || Ok(()),
            |file, check| {
                ExclusiveLock::wait_on_file_observed(
                    file,
                    check,
                    None,
                    fs2::FileExt::try_lock_exclusive,
                    |_| panic!("stop must precede backoff"),
                    observer,
                )
            },
            |file| {
                closes.set(closes.get() + 1);
                lycheedev_windows::filesystem::close_file(file)
            },
            observer,
        ));
        assert_eq!(error.code, "fixture.invocation");
        assert_eq!(closes.get(), 1);
        use LockPhase::*;
        if stop_after == Open {
            assert_eq!(*phases.borrow(), [Verify, Open, FailureClose]);
        } else {
            assert_eq!(
                *phases.borrow(),
                [Verify, Open, KernelAttempt, FailureClose]
            );
        }
        // Failed acquisition cannot release the unrelated original publisher.
        assert!(matches!(ExclusiveLock::acquire(&path), Err(e) if e.code == "workspace.busy"));
        first.close().unwrap();
        ExclusiveLock::acquire(&path).unwrap().close().unwrap();
    }
}

#[test]
fn source_relative_lock_preserves_path_lock_identity_and_real_contention() {
    let f = Fixture::new();
    let resource = "source-cache:source-facts-rust:fixture";
    let first = ExclusiveLock::resource(&f.0, resource).unwrap();
    let directory = Directory::open(&f.0).unwrap();
    ExclusiveLock::set_blob_wait_observer(move || first.close().unwrap());
    let phases = RefCell::new(Vec::new());
    let observe = |phase, _| phases.borrow_mut().push(phase);
    let acquired = ExclusiveLock::resource_wait_in_directory(
        &directory,
        resource,
        &crate::control::OperationControl::default(),
        Some(&observe),
    )
    .unwrap();
    use LockPhase::*;
    assert_eq!(
        *phases.borrow(),
        [
            Verify,
            Open,
            KernelAttempt,
            ContentionBackoff,
            KernelAttempt
        ]
    );
    assert!(
        matches!(ExclusiveLock::resource(&f.0, resource), Err(e) if e.code == "workspace.busy")
    );
    acquired.close().unwrap();
    ExclusiveLock::resource(&f.0, resource)
        .unwrap()
        .close()
        .unwrap();
    directory.close().unwrap();
}

#[test]
fn source_relative_open_stop_retains_owner_for_checked_close_and_all_causes() {
    let f = Fixture::new();
    let directory = Directory::open(&f.0).unwrap();
    for enabled in [false, true] {
        let state = Cell::new(0);
        let closes = Cell::new(0);
        let phases = RefCell::new(Vec::new());
        let observe = |phase, _| phases.borrow_mut().push(phase);
        let observer: LockObserver<'_> = enabled.then_some(&observe);
        let error = failed(ExclusiveLock::acquire_prepared_open_observed_with(
            || match state.get() {
                0 => Ok(()),
                1 => Err(ToolkitError::new("fixture.budget", "original stop", 7)),
                _ => Err(ToolkitError::new("fixture.invocation", "late stop", 6)),
            },
            || directory.verify().map_err(ToolkitError::io),
            || {
                let file = directory.open_lock_file_owner("owner.lock".as_ref())?;
                state.set(1);
                Ok(file)
            },
            |file, check| {
                ExclusiveLock::wait_on_file_observed(
                    file,
                    check,
                    None,
                    |_| panic!("post-open stop precedes the first kernel attempt"),
                    |_| panic!("post-open stop cannot sleep"),
                    observer,
                )
            },
            |file| {
                closes.set(closes.get() + 1);
                lycheedev_windows::filesystem::close_file(file)?;
                state.set(2);
                Err(std::io::Error::other("close failed after consuming owner"))
            },
            observer,
        ));
        assert_eq!(closes.get(), 1);
        assert_eq!(error.code, "fixture.budget");
        assert_eq!(error.details["primary"]["code"], "fixture.budget");
        assert_eq!(error.details["cleanup"]["code"], "external.io");
        assert_eq!(error.details["control"]["code"], "fixture.invocation");
        if enabled {
            assert_eq!(
                *phases.borrow(),
                [LockPhase::Verify, LockPhase::Open, LockPhase::FailureClose]
            );
        } else {
            assert!(phases.borrow().is_empty());
        }
        // Failure cleanup has consumed the original owner; no lock survives it.
        ExclusiveLock::acquire(&f.0.join("owner.lock"))
            .unwrap()
            .close()
            .unwrap();
    }
    directory.close().unwrap();
}

#[test]
fn source_relative_open_failure_has_no_kernel_attempt_or_failure_close() {
    let f = Fixture::new();
    let resource = "source-cache:source-facts-rust:blocked";
    let path = f.0.join(format!("{}.lock", sha256(resource.as_bytes())));
    fs::create_dir(&path).unwrap();
    let directory = Directory::open(&f.0).unwrap();
    let phases = RefCell::new(Vec::new());
    let observe = |phase, _| phases.borrow_mut().push(phase);
    let error = failed(ExclusiveLock::resource_wait_in_directory(
        &directory,
        resource,
        &crate::control::OperationControl::default(),
        Some(&observe),
    ));
    assert_eq!(error.code, "external.io");
    assert_eq!(*phases.borrow(), [LockPhase::Verify, LockPhase::Open]);
    assert!(path.is_dir());
    directory.close().unwrap();
}
