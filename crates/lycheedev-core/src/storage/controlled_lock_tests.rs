use super::*;
use std::cell::Cell;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "repair-lock-{}-{}-{}",
            std::process::id(),
            utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
#[test]
fn controlled_lock_rejects_cancel_before_creating_namespace() {
    let f = Fixture::new();
    let c = crate::control::OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    let path = f.0.join("absent/lock");
    assert!(
        matches!(ExclusiveLock::acquire_controlled(&path,&c), Err(e) if e.code == "command.cancelled")
    );
    assert!(!path.parent().unwrap().exists());
}
#[test]
fn controlled_lock_contention_preserves_the_owner() {
    let f = Fixture::new();
    let path = f.0.join("lock");
    let c = crate::control::OperationControl::default();
    let first = ExclusiveLock::acquire_controlled(&path, &c).unwrap();
    assert!(
        matches!(ExclusiveLock::acquire_controlled(&path,&c),Err(e) if e.code=="workspace.busy")
    );
    first.close().unwrap();
    ExclusiveLock::acquire_controlled(&path, &c)
        .unwrap()
        .close()
        .unwrap();
}
#[test]
fn cancelled_after_acquisition_closes_once_and_releases() {
    let f = Fixture::new();
    let path = f.0.join("lock");
    let c = crate::control::OperationControl::default();
    let closes = Cell::new(0);
    let error = ExclusiveLock::acquire_with(
        &path,
        || c.check(),
        |file| {
            ExclusiveLock::lock_file(file)?;
            c.tracked_cancel_handle()?.cancel();
            Ok(())
        },
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)
        },
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(closes.get(), 1);
    ExclusiveLock::acquire(&path).unwrap().close().unwrap();
}
#[test]
fn acquisition_failure_retains_checked_close_failure() {
    let f = Fixture::new();
    let closes = Cell::new(0);
    let error = ExclusiveLock::acquire_with(
        &f.0.join("lock"),
        || Ok(()),
        |_| Err(ToolkitError::new("workspace.busy", "busy", 6)),
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)?;
            Err(std::io::Error::other("injected close failure"))
        },
    )
    .err()
    .unwrap();
    assert_eq!(closes.get(), 1);
    assert_eq!(error.code, "workspace.busy");
    assert_eq!(error.details["schema"], FILE_CLEANUP_SCHEMA);
    assert_eq!(error.details["cleanup"]["code"], "external.io");
}

#[test]
fn atomic_publication_cancel_during_write_still_closes_once_and_keeps_original() {
    let f = Fixture::new();
    let path = f.0.join("output");
    fs::write(&path, b"original").unwrap();
    let checks = Cell::new(0);
    let closes = Cell::new(0);
    let error = atomic_write_with_check_and_close(
        &path,
        &vec![b'x'; 131072],
        || {
            checks.set(checks.get() + 1);
            if checks.get() >= 4 {
                Err(ToolkitError::new("command.cancelled", "cancelled", 7))
            } else {
                Ok(())
            }
        },
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(closes.get(), 1);
    assert_eq!(fs::read(&path).unwrap(), b"original");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 1);
}
#[test]
fn atomic_publication_write_stop_and_close_failure_retain_both_causes() {
    let f = Fixture::new();
    let path = f.0.join("output");
    let checks = Cell::new(0);
    let closes = Cell::new(0);
    let error = atomic_write_with_check_and_close(
        &path,
        b"new",
        || {
            checks.set(checks.get() + 1);
            if checks.get() >= 3 {
                Err(ToolkitError::new("command.deadline", "deadline", 6))
            } else {
                Ok(())
            }
        },
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)?;
            Err(std::io::Error::other("injected close fault"))
        },
    )
    .unwrap_err();
    assert_eq!(closes.get(), 1);
    assert!(!path.exists());
    assert_eq!(error.code, "command.deadline");
    assert_eq!(
        error.details["schema"],
        "lycheedev.atomic-temporary-retained.v1"
    );
    let cause = &error.details["cause"];
    assert_eq!(cause["code"], "command.deadline");
    assert_eq!(cause["details"]["schema"], FILE_CLEANUP_SCHEMA);
    assert_eq!(cause["details"]["primary"]["code"], "command.deadline");
    assert_eq!(cause["details"]["cleanup"]["code"], "external.io");
}
#[test]
fn atomic_publication_retains_closed_temporary_without_path_cleanup() {
    let f = Fixture::new();
    let path = f.0.join("output");
    let closes = Cell::new(0);
    let error = atomic_write_with_check_and_close(
        &path,
        b"new",
        || Ok(()),
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)?;
            // Make only this fixture's exact temporary path a directory after its
            // file is closed. Cleanup must preserve this substituted object.
            let temporary = fs::read_dir(&f.0)?.next().unwrap()?.path();
            fs::remove_file(&temporary)?;
            fs::create_dir(&temporary)?;
            Err(std::io::Error::other("injected close fault"))
        },
    )
    .unwrap_err();
    assert_eq!(closes.get(), 1);
    assert!(!path.exists());
    assert_eq!(
        error.details["schema"],
        "lycheedev.atomic-temporary-retained.v1"
    );
    assert_eq!(error.details["cause"]["code"], "external.io");
    assert!(Path::new(error.details["temporary"].as_str().unwrap()).is_dir());
}

#[test]
fn atomic_postclose_regular_file_substitution_is_retained_never_deleted() {
    let f = Fixture::new();
    let path = f.0.join("output");
    let mut replacement = None;
    let closes = Cell::new(0);
    let error = atomic_write_with_check_and_close(
        &path,
        b"owned",
        || Ok(()),
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)?;
            let temporary = fs::read_dir(&f.0)?.next().unwrap()?.path();
            fs::remove_file(&temporary)?;
            fs::write(&temporary, b"foreign regular replacement")?;
            replacement = Some(temporary);
            Err(std::io::Error::other("injected post-close failure"))
        },
    )
    .unwrap_err();
    assert_eq!(closes.get(), 1);
    assert!(!path.exists());
    let replacement = replacement.unwrap();
    assert_eq!(
        fs::read(&replacement).unwrap(),
        b"foreign regular replacement"
    );
    assert_eq!(
        error.details["temporary"],
        serde_json::to_value(replacement).unwrap()
    );
}
#[test]
fn atomic_uncertain_residues_are_bounded_per_target_not_directory() {
    let f = Fixture::new();
    let path = f.0.join("blocked-target");
    for _ in 0..64 {
        assert!(
            atomic_write_with_check_and_close(
                &path,
                b"owned",
                || Ok(()),
                |file| {
                    lycheedev_windows::filesystem::close_file(file)?;
                    Err(std::io::Error::other("injected uncertain close"))
                }
            )
            .is_err()
        );
    }
    let error = atomic_write_with_check(&path, b"new", || Ok(())).unwrap_err();
    assert_eq!(error.code, "vault.temporary_collision");
    assert_eq!(error.details["maximumSlots"], 64);
    let other = f.0.join("unrelated-target");
    atomic_write_with_check(&other, b"valid parallel namespace", || Ok(())).unwrap();
    assert_eq!(fs::read(other).unwrap(), b"valid parallel namespace");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 65);
}

#[test]
fn only_kernel_lock_contention_is_retryable() {
    let busy = ExclusiveLock::lock_error(fs2::lock_contended_error());
    assert_eq!((busy.code.as_str(), busy.exit_code), ("workspace.busy", 6));
    for raw in [5, 6, 87] {
        let error = std::io::Error::from_raw_os_error(raw);
        let expected = error.to_string();
        let actual = ExclusiveLock::lock_error(error);
        assert_eq!((actual.code.as_str(), actual.exit_code), ("external.io", 5));
        assert_eq!(actual.message, expected);
    }
    assert_eq!(
        ExclusiveLock::lock_error(std::io::Error::other("synthetic I/O")).code,
        "external.io"
    );
}
#[test]
fn non_contention_lock_failure_closes_once_without_retries() {
    let f = Fixture::new();
    let attempts = Cell::new(0);
    let closes = Cell::new(0);
    let error = match ExclusiveLock::acquire_with(
        &f.0.join("io-lock"),
        || Ok(()),
        |_| {
            attempts.set(attempts.get() + 1);
            Err(ExclusiveLock::lock_error(
                std::io::Error::from_raw_os_error(5),
            ))
        },
        |file| {
            closes.set(closes.get() + 1);
            lycheedev_windows::filesystem::close_file(file)
        },
    ) {
        Ok(_) => panic!("injected lock I/O failure must not acquire"),
        Err(error) => error,
    };
    assert_eq!(error.code, "external.io");
    assert_eq!(attempts.get(), 1);
    assert_eq!(closes.get(), 1);
    assert_eq!(error.details, serde_json::Value::Null);
}
