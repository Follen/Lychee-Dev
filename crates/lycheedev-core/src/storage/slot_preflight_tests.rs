use super::*;
use crate::control::OperationControl;
use lycheedev_windows::filesystem::{close_file, sharing_probe::open_delete_sharing_probe};
use serde_json::{Value, json};
use std::{cell::RefCell, os::windows::fs::OpenOptionsExt};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-slot-preflight-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("Lychee Dev Slot 01.toc"), b"original").unwrap();
        fs::write(root.join("Loader.lua"), b"loader").unwrap();
        fs::write(root.join("Payload.lua"), b"payload").unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("Lychee Dev Slot 01.toc")
    }
    fn members(&self) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(&self.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
    fn three(&self, bytes: &[u8]) {
        assert_eq!(
            self.members(),
            ["Loader.lua", "Lychee Dev Slot 01.toc", "Payload.lua"]
        );
        assert_eq!(fs::read(self.path()).unwrap(), bytes);
        assert_eq!(fs::read(self.0.join("Loader.lua")).unwrap(), b"loader");
        assert_eq!(fs::read(self.0.join("Payload.lua")).unwrap(), b"payload");
    }
    fn held(&self) -> File {
        OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(self.path())
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn encoded(error: &ToolkitError) -> Value {
    serde_json::to_value(error).unwrap()
}
fn probe(path: &Path) -> std::result::Result<(), Failure> {
    check_with(path, open_delete_sharing_probe, close_file)
}
fn no_discard(_: owned_cleanup::OwnedTemporary) -> (File, std::io::Result<()>) {
    panic!("no original-temp discard is allowed on this path")
}
fn no_replace(_: &Path, _: &Path) -> lycheedev_windows::Result<()> {
    panic!("rename must not be attempted")
}
fn assert_open_error(error: &ToolkitError) {
    assert_eq!(error.code, "external.io");
    assert_eq!(error.exit_code, 5);
    assert_eq!(
        error.details["schema"],
        "lycheedev.slot-sharing-preflight.v1"
    );
    assert_eq!(error.details["stage"], "open");
    assert_eq!(error.details["rawOsError"], 32);
    assert_eq!(error.details["renameAttempted"], false);
    assert_eq!(error.details["published"], false);
}

#[test]
fn held_toc_raw32_discards_original_temp_without_rename_and_retry_keeps_three_members() {
    let f = Fixture::new();
    let held = f.held();
    let events = RefCell::new(Vec::new());
    let error = atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || Ok(()),
        |file| {
            events.borrow_mut().push("temp-close");
            close_file(file)
        },
        |path| {
            assert!(events.borrow().is_empty(), "original temp is still open");
            let staged = f
                .members()
                .into_iter()
                .find(|s| s.ends_with(".tmp"))
                .unwrap();
            assert_eq!(fs::read(f.0.join(staged)).unwrap(), b"replacement");
            events.borrow_mut().push("probe");
            probe(path)
        },
        |owned| {
            events.borrow_mut().push("discard");
            owned.discard()
        },
        no_replace,
    )
    .unwrap_err();
    assert_open_error(&error);
    assert_eq!(*events.borrow(), ["probe", "discard", "temp-close"]);
    f.three(b"original");
    close_file(held).unwrap();
    slot_atomic_write_controlled(&f.path(), b"replacement", &OperationControl::default()).unwrap();
    f.three(b"replacement");
}

#[test]
fn successful_probe_closes_once_before_original_temp_close_and_rename() {
    let f = Fixture::new();
    let events = RefCell::new(Vec::new());
    atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || Ok(()),
        |file| {
            events.borrow_mut().push("temp-close");
            close_file(file)
        },
        |path| {
            check_with(
                path,
                |path| {
                    events.borrow_mut().push("open");
                    open_delete_sharing_probe(path)
                },
                |file| {
                    events.borrow_mut().push("probe-close");
                    close_file(file)
                },
            )
        },
        no_discard,
        |from, to| {
            events.borrow_mut().push("rename");
            lycheedev_windows::durable_replace(from, to)
        },
    )
    .unwrap();
    assert_eq!(
        *events.borrow(),
        ["open", "probe-close", "temp-close", "rename"]
    );
    f.three(b"replacement");
}

#[test]
fn other_open_os_errors_and_error_kind_lookalikes_continue_original_publication() {
    for raw in [None, Some(2), Some(3), Some(5), Some(33), Some(87)] {
        let f = Fixture::new();
        let events = RefCell::new(Vec::new());
        atomic_write_with_hooks(
            &f.path(),
            b"replacement",
            || Ok(()),
            |file| {
                events.borrow_mut().push("temp-close");
                close_file(file)
            },
            |path| {
                check_with(
                    path,
                    |_| {
                        events.borrow_mut().push("open-error");
                        Err(raw
                            .map(std::io::Error::from_raw_os_error)
                            .unwrap_or_else(|| {
                                std::io::Error::new(
                                    std::io::ErrorKind::PermissionDenied,
                                    "sharing violation 32 is only text",
                                )
                            }))
                    },
                    |_| panic!("an unsuccessful open owns no handle"),
                )
            },
            no_discard,
            |from, to| {
                events.borrow_mut().push("rename");
                lycheedev_windows::durable_replace(from, to)
            },
        )
        .unwrap();
        assert_eq!(*events.borrow(), ["open-error", "temp-close", "rename"]);
        f.three(b"replacement");
    }
}

#[test]
fn uncertain_probe_close_stops_and_preserves_probe_temp_close_and_control_causes() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut transferred = None;
    let mut probe_closes = 0;
    let mut temp_closes = 0;
    let error = atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || control.check(),
        |file| {
            temp_closes += 1;
            close_file(file)?;
            Err(std::io::Error::from_raw_os_error(6))
        },
        |path| {
            check_with(path, open_delete_sharing_probe, |file| {
                probe_closes += 1;
                // Inject uncertainty by transferring the real owner to the fixture;
                // this is not a fabricated kernel CloseHandle failure.
                transferred = Some(file);
                cancel.cancel();
                Err(std::io::Error::from_raw_os_error(5))
            })
        },
        no_discard,
        no_replace,
    )
    .unwrap_err();
    assert_eq!(probe_closes, 1);
    assert_eq!(temp_closes, 1);
    assert_eq!(error.code, "external.io");
    assert_eq!(error.exit_code, 5);
    assert_eq!(
        error.details["schema"],
        "lycheedev.atomic-temporary-retained.v1"
    );
    let cause = &error.details["cause"]["details"];
    assert_eq!(cause["schema"], FILE_CLEANUP_SCHEMA);
    assert_eq!(cause["primary"]["details"]["stage"], "close");
    assert_eq!(cause["primary"]["details"]["rawOsError"], 5);
    assert_eq!(cause["primary"]["details"]["renameAttempted"], false);
    assert_eq!(cause["primary"]["details"]["published"], false);
    assert_eq!(
        cause["cleanup"]["message"],
        std::io::Error::from_raw_os_error(6).to_string()
    );
    assert_eq!(cause["control"]["code"], "command.cancelled");
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
    let temporary = PathBuf::from(error.details["temporary"].as_str().unwrap());
    assert_eq!(fs::read(temporary).unwrap(), b"replacement");
    assert_eq!(f.members().len(), 4);
    assert_eq!(transferred.as_ref().unwrap().metadata().unwrap().len(), 8);
    close_file(transferred.take().unwrap()).unwrap();
}

#[test]
fn raw32_keeps_discard_close_and_cancellation_errors_nested_under_original_cause() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let primary = error(&f.path(), "open", std::io::Error::from_raw_os_error(32));
    let mut discards = 0;
    let mut closes = 0;
    let error = atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || control.check(),
        |file| {
            closes += 1;
            close_file(file)?;
            cancel.cancel();
            Err(std::io::Error::other("injected temp close failure"))
        },
        |path| {
            check_with(
                path,
                |_| Err(std::io::Error::from_raw_os_error(32)),
                |_| panic!("no probe handle"),
            )
        },
        |owned| {
            discards += 1;
            (
                owned.into_file(),
                Err(std::io::Error::other("injected discard failure")),
            )
        },
        no_replace,
    )
    .unwrap_err();
    assert_eq!((discards, closes), (1, 1));
    assert_eq!((error.code.as_str(), error.exit_code), ("external.io", 5));
    assert_eq!(error.message, primary.message);
    let causes = &error.details["cause"]["details"];
    assert_eq!(causes["schema"], FILE_CLEANUP_SCHEMA);
    assert_eq!(causes["primary"]["details"]["primary"], encoded(&primary));
    assert_eq!(
        causes["primary"]["details"]["cleanup"]["message"],
        "injected discard failure"
    );
    assert_eq!(causes["cleanup"]["message"], "injected temp close failure");
    assert_eq!(causes["control"]["code"], "command.cancelled");
    assert_eq!(f.members().len(), 4);
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
}

#[test]
fn lock_acquired_after_successful_probe_still_retains_failed_rename_temp() {
    let f = Fixture::new();
    let mut held = None;
    let mut renames = 0;
    let error = atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || Ok(()),
        close_file,
        |path| {
            probe(path)?;
            held = Some(f.held());
            Ok(())
        },
        no_discard,
        |from, to| {
            renames += 1;
            lycheedev_windows::durable_replace(from, to)
        },
    )
    .unwrap_err();
    assert_eq!(renames, 1);
    assert_eq!(error.code, "vault.publish");
    assert_eq!(
        error.details["schema"],
        "lycheedev.atomic-temporary-retained.v1"
    );
    assert_eq!(
        error.details["detail"],
        "publication failed after original handle closed"
    );
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
    assert_eq!(
        fs::read(error.details["temporary"].as_str().unwrap()).unwrap(),
        b"replacement"
    );
    assert_eq!(f.members().len(), 4);
    close_file(held.take().unwrap()).unwrap();
}

#[test]
fn ordinary_atomic_write_has_no_probe_and_preserves_its_existing_rename_failure() {
    let f = Fixture::new();
    let held = f.held();
    let mut checks = 0;
    let mut closes = 0;
    let error = atomic_write_with_check_and_close(
        &f.path(),
        b"replacement",
        || {
            checks += 1;
            Ok(())
        },
        |file| {
            closes += 1;
            close_file(file)
        },
    )
    .unwrap_err();
    assert_eq!((checks, closes), (6, 1));
    assert_eq!(error.code, "vault.publish");
    assert_eq!(
        error.details["detail"],
        "publication failed after original handle closed"
    );
    assert_eq!(f.members().len(), 4);
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
    close_file(held).unwrap();
}

#[test]
fn cancellation_after_sync_never_opens_probe_and_discards_original_temp() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut checks = 0;
    let error = atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || {
            checks += 1;
            if checks == 4 {
                cancel.cancel();
            }
            control.check()
        },
        close_file,
        |_| panic!("cancelled staging must skip the probe"),
        owned_cleanup::OwnedTemporary::discard,
        no_replace,
    )
    .unwrap_err();
    assert_eq!(checks, 5);
    assert_eq!(error.code, "command.cancelled");
    assert!(error.details.is_null());
    f.three(b"original");
}

#[test]
fn cancellation_during_successful_probe_close_is_rechecked_and_retains_temp() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let cancel = control.tracked_cancel_handle().unwrap();
    let mut closes = 0;
    let error = atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || control.check(),
        |file| {
            closes += 1;
            close_file(file)
        },
        |path| {
            check_with(path, open_delete_sharing_probe, |file| {
                close_file(file)?;
                cancel.cancel();
                Ok(())
            })
        },
        no_discard,
        no_replace,
    )
    .unwrap_err();
    assert_eq!(closes, 1);
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["schema"],
        "lycheedev.atomic-temporary-retained.v1"
    );
    assert_eq!(f.members().len(), 4);
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
}

#[test]
fn all_sixty_four_unknown_temporaries_remain_unadopted_and_probe_is_never_opened() {
    let f = Fixture::new();
    let key = sha256(
        f.path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_lowercase()
            .as_bytes(),
    );
    for index in 0..64 {
        fs::write(
            f.0.join(format!(".lycheedev-{key}-{index}.tmp")),
            index.to_string(),
        )
        .unwrap();
    }
    let before = f.members();
    let error = atomic_write_with_hooks(
        &f.path(),
        b"replacement",
        || Ok(()),
        |_| panic!("no temporary owner was acquired"),
        |_| panic!("no temporary owner was acquired"),
        no_discard,
        no_replace,
    )
    .unwrap_err();
    assert_eq!(error.code, "vault.temporary_collision");
    assert_eq!(error.details["maximumSlots"], json!(64));
    assert_eq!(f.members(), before);
    for index in 0..64 {
        assert_eq!(
            fs::read(f.0.join(format!(".lycheedev-{key}-{index}.tmp"))).unwrap(),
            index.to_string().as_bytes()
        );
    }
    assert_eq!(fs::read(f.path()).unwrap(), b"original");
}
