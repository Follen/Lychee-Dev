use super::*;
use crate::control::OperationControl;
use serde_json::{Value, json};
use std::{
    io::{Read, Seek, SeekFrom},
    time::Instant,
};

struct Fixture(PathBuf);
impl Fixture {
    fn new(bytes: &[u8]) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-identity-close-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("identity"), bytes).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("identity")
    }
    fn file(&self) -> File {
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.share_mode(1);
        }
        options.open(self.path()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn close(file: File) -> std::io::Result<()> {
    lycheedev_windows::filesystem::close_file(file)
}
fn injected_close_error(file: File) -> std::io::Result<()> {
    // Real fixture handle is closed, then the close outcome is injected. This
    // does not claim the OS itself returned a failing CloseHandle result.
    close(file)?;
    Err(std::io::Error::other("injected checked-close failure"))
}
fn encoded(error: &ToolkitError) -> Value {
    serde_json::to_value(error).unwrap()
}
fn assert_released(fixture: &Fixture) {
    fs::write(fixture.path(), b"released").unwrap();
    fs::remove_file(fixture.path()).unwrap();
}

#[test]
fn locked_hash_admission_failure_checks_close_and_preserves_both_errors() {
    let fixture = Fixture::new(b"too large");
    let mut closes = 0;
    let error = locked_sha256_with(
        &fixture.path(),
        1,
        || Ok(()),
        |file| {
            closes += 1;
            injected_close_error(file)
        },
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "input.invalid");
    assert_eq!(error.exit_code, 4);
    assert_eq!(error.details["schema"], FILE_CLEANUP_SCHEMA);
    assert_eq!(
        error.details["primary"]["message"],
        "identity file exceeds admission"
    );
    assert_eq!(error.details["cleanup"]["code"], "external.io");
    assert!(error.details["control"].is_null());
    assert_released(&fixture);
}

#[test]
fn locked_hash_cancellation_at_every_post_open_check_closes_once() {
    // One content read, its EOF read and the final identity admission check.
    for cancel_at in 2..=4 {
        let fixture = Fixture::new(b"identity");
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let mut checks = 0;
        let mut closes = 0;
        let error = locked_sha256_with(
            &fixture.path(),
            64,
            || {
                checks += 1;
                if checks == cancel_at {
                    cancel.cancel();
                }
                control.check()
            },
            |file| {
                closes += 1;
                close(file)
            },
        )
        .unwrap_err();
        assert_eq!(closes, 1);
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(error.exit_code, 7);
        assert!(
            error.details.is_null(),
            "the same stop is not a second cause"
        );
        assert_eq!(checks, cancel_at + 1, "final control follows cleanup");
        assert_released(&fixture);
    }
}

#[test]
fn locked_hash_pre_open_rejection_never_calls_close() {
    let fixture = Fixture::new(b"identity");
    let control = OperationControl::uncancelable(Instant::now());
    let mut closes = 0;
    let error = locked_sha256_with(
        &fixture.path(),
        64,
        || control.check(),
        |file| {
            closes += 1;
            close(file)
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert_eq!(closes, 0);
    assert_released(&fixture);
}

#[test]
fn locked_hash_success_transfers_the_original_live_identity_owner() {
    let fixture = Fixture::new(b"sealed identity");
    let mut closes = 0;
    let (mut held, digest) = locked_sha256_with(
        &fixture.path(),
        64,
        || Ok(()),
        |file| {
            closes += 1;
            close(file)
        },
    )
    .unwrap();
    assert_eq!(closes, 0);
    assert_eq!(digest, sha256(b"sealed identity"));
    held.seek(SeekFrom::Start(0)).unwrap();
    let mut actual = Vec::new();
    held.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, b"sealed identity");
    #[cfg(windows)]
    {
        assert!(fs::write(fixture.path(), b"replacement").is_err());
        assert!(fs::remove_file(fixture.path()).is_err());
    }
    finish_file(held, Ok(()), &OperationControl::default()).unwrap();
    assert_released(&fixture);
}

#[test]
fn locked_hash_real_read_error_and_injected_close_failure_both_survive() {
    let fixture = Fixture::new(b"sealed identity");
    // A real write-only File has valid metadata but cannot satisfy Read.
    let file = OpenOptions::new().write(true).open(fixture.path()).unwrap();
    let error = authenticate_locked_file(file, 64, || Ok(()), injected_close_error).unwrap_err();
    assert_eq!(error.code, "external.io");
    assert_eq!(error.details["primary"]["code"], "external.io");
    assert!(
        error.details["primary"]["message"]
            .as_str()
            .is_some_and(|s| !s.is_empty())
    );
    assert_eq!(
        error.details["cleanup"]["message"],
        "injected checked-close failure"
    );
    assert_released(&fixture);
}

#[test]
fn finish_file_keeps_full_primary_cleanup_and_late_control_structure() {
    let fixture = Fixture::new(b"identity");
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let primary = ToolkitError::new("fixture.primary", "primary failure", 3)
        .with_details(json!({"untouched": [1, "value"], "primary": "caller data"}));
    let expected = encoded(&primary);
    let mut closes = 0;
    let error = finish_file_with::<()>(
        fixture.file(),
        Err(primary),
        || control.check(),
        |file| {
            closes += 1;
            cancel.cancel();
            injected_close_error(file)
        },
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "fixture.primary");
    assert_eq!(error.message, "primary failure");
    assert_eq!(error.exit_code, 3);
    assert_eq!(error.details["schema"], FILE_CLEANUP_SCHEMA);
    assert_eq!(error.details["primary"], expected);
    assert_eq!(error.details["cleanup"]["code"], "external.io");
    assert_eq!(error.details["control"]["code"], "command.cancelled");
    assert_eq!(error.details.as_object().unwrap().len(), 4);
    assert_released(&fixture);
}

#[test]
fn finish_file_does_not_let_cancel_or_expiry_skip_cleanup() {
    for cancelled in [true, false] {
        let fixture = Fixture::new(b"identity");
        let control = if cancelled {
            let control = OperationControl::default();
            control.tracked_cancel_handle().unwrap().cancel();
            control
        } else {
            OperationControl::uncancelable(Instant::now())
        };
        let primary = control.check().unwrap_err();
        let expected = encoded(&primary);
        let mut closes = 0;
        let error = finish_file_with::<()>(
            fixture.file(),
            Err(primary),
            || control.check(),
            |file| {
                closes += 1;
                injected_close_error(file)
            },
        )
        .unwrap_err();
        assert_eq!(closes, 1);
        assert_eq!(error.details["primary"], expected);
        assert_eq!(error.details["cleanup"]["code"], "external.io");
        assert!(error.details["control"].is_null());
        assert_released(&fixture);
    }
}

#[test]
fn finish_file_success_never_escapes_close_failure_or_late_cancellation() {
    for failed_close in [true, false] {
        let fixture = Fixture::new(b"identity");
        let control = OperationControl::default();
        let cancel = control.tracked_cancel_handle().unwrap();
        let error = finish_file_with(
            fixture.file(),
            Ok("must not escape"),
            || control.check(),
            |file| {
                cancel.cancel();
                if failed_close {
                    injected_close_error(file)
                } else {
                    close(file)
                }
            },
        )
        .unwrap_err();
        if failed_close {
            assert_eq!(error.code, "external.io");
            assert!(error.details["primary"].is_null());
            assert_eq!(error.details["cleanup"]["code"], "external.io");
            assert_eq!(error.details["control"]["code"], "command.cancelled");
        } else {
            assert_eq!(error.code, "command.cancelled");
            assert!(error.details.is_null());
        }
        assert_released(&fixture);
    }
}

#[test]
fn finish_file_lone_primary_and_lone_cleanup_are_unchanged() {
    let fixture = Fixture::new(b"identity");
    let primary = ToolkitError::invalid("invalid identity").with_details(json!({"value": 7}));
    let expected = serde_json::to_vec(&primary).unwrap();
    let error = finish_file_with::<()>(fixture.file(), Err(primary), || Ok(()), close).unwrap_err();
    assert_eq!(serde_json::to_vec(&error).unwrap(), expected);
    let error =
        finish_file_with(fixture.file(), Ok(()), || Ok(()), injected_close_error).unwrap_err();
    assert_eq!(error.code, "external.io");
    assert_eq!(error.message, "injected checked-close failure");
    assert!(error.details.is_null());
    assert_released(&fixture);
}

#[test]
fn finish_file_consumes_once_and_does_not_retry_uncertain_transferred_owner() {
    let fixture = Fixture::new(b"identity");
    let mut transferred = None;
    let mut closes = 0;
    let error = finish_file_with(
        fixture.file(),
        Ok(()),
        || Ok(()),
        |file| {
            closes += 1;
            // Inject uncertain failure without closing, transferring ownership
            // to the fixture. The production scope must not Drop or retry it.
            transferred = Some(file);
            Err(std::io::Error::other("uncertain close"))
        },
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.message, "uncertain close");
    assert_eq!(transferred.as_ref().unwrap().metadata().unwrap().len(), 8);
    #[cfg(windows)]
    assert!(fs::remove_file(fixture.path()).is_err());
    close(transferred.take().unwrap()).unwrap();
    assert_released(&fixture);
}

#[test]
fn finish_file_success_checks_close_before_final_control_and_returns_value() {
    let fixture = Fixture::new(b"identity");
    let closed = std::cell::Cell::new(false);
    let answer = finish_file_with(
        fixture.file(),
        Ok(42),
        || {
            assert!(closed.get());
            Ok(())
        },
        |file| {
            close(file)?;
            closed.set(true);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(answer, 42);
    assert_released(&fixture);
}

#[test]
fn failed_hash_construction_does_not_retry_uncertain_close_or_drop_transferred_file() {
    let fixture = Fixture::new(b"identity");
    let mut transferred = None;
    let mut closes = 0;
    let error = locked_sha256_with(
        &fixture.path(),
        1,
        || Ok(()),
        |file| {
            closes += 1;
            transferred = Some(file);
            Err(std::io::Error::other("uncertain close"))
        },
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "input.invalid");
    assert_eq!(error.details["cleanup"]["message"], "uncertain close");
    let file = transferred.as_mut().unwrap();
    assert_eq!(file.metadata().unwrap().len(), 8);
    let mut raw = Vec::new();
    file.read_to_end(&mut raw).unwrap();
    assert_eq!(raw, b"identity");
    #[cfg(windows)]
    assert!(fs::remove_file(fixture.path()).is_err());
    close(transferred.take().unwrap()).unwrap();
    assert_released(&fixture);
}

#[test]
fn nested_file_cleanup_preserves_the_complete_prior_error_tree() {
    let first = Fixture::new(b"first");
    let second = Fixture::new(b"second");
    let primary = ToolkitError::invalid("original failure").with_details(json!({"caller": "data"}));
    let inner = finish_file_with::<()>(first.file(), Err(primary), || Ok(()), injected_close_error)
        .unwrap_err();
    let expected = encoded(&inner);
    let outer = finish_file_with::<()>(second.file(), Err(inner), || Ok(()), injected_close_error)
        .unwrap_err();
    assert_eq!(outer.code, "input.invalid");
    assert_eq!(outer.details["primary"], expected);
    assert_eq!(
        outer.details["primary"]["details"]["primary"]["details"],
        json!({"caller":"data"})
    );
    assert_eq!(outer.details["cleanup"]["code"], "external.io");
    assert_released(&first);
    assert_released(&second);
}
