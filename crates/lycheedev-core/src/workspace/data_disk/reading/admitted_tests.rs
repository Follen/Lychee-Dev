use super::*;
use crate::{
    control::TrackedCancellation,
    workspace::{self, data_resources::Policy},
};
use std::time::{Duration, Instant};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-generation-admit-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn publish(&self, bytes: &[u8]) -> String {
        let control = OperationControl::default();
        let resources = data_resources::acquire(&self.0, Policy::Ordinary, &control).unwrap();
        let owner = resources
            .reserve_derived_cache(AdditionalPeak {
                generations: bytes.len() as u64,
                ..Default::default()
            })
            .unwrap();
        let key = storage::sha256(b"admitted-manifest");
        owner.publish_generation(&key, bytes).unwrap();
        owner.finish(Ok(())).unwrap();
        resources.close().unwrap();
        key
    }
    fn path(&self, key: &str) -> PathBuf {
        DataCachePaths::new(&self.0)
            .generations
            .join(format!("{key}.json"))
    }
    fn writable(&self, key: &str) {
        storage::close_file(
            fs::OpenOptions::new()
                .write(true)
                .open(self.path(key))
                .unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn capacity_error() -> ToolkitError {
    ToolkitError::new(
        "test.retained_budget",
        "no remaining manifest allocation capacity",
        3,
    )
}

#[test]
fn absent_generation_never_invokes_admission_even_when_no_allocation_capacity_remains() {
    let f = Fixture::new();
    // Nonempty unrelated generation proves this is not an empty-namespace shortcut.
    f.publish(b"other generation");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check: DerivedReadCheck = Arc::new(|| Ok(()));
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let calls = Cell::new(0);
    let raw = lease
        .read_generation_with_admission(&storage::sha256(b"missing"), |_| {
            calls.set(calls.get() + 1);
            Err(capacity_error())
        })
        .unwrap();
    assert!(raw.is_none());
    assert_eq!(calls.get(), 0);
    lease.finish(Ok(())).unwrap();
    resources.close().unwrap();
}

#[test]
fn original_generation_handle_admits_exact_length_once_before_returning_all_bytes() {
    for length in [0, 8, 65537, MAX_GENERATION_BYTES] {
        let f = Fixture::new();
        let expected = vec![42; length];
        let key = f.publish(&expected);
        let control = OperationControl::default();
        let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
        let check: DerivedReadCheck = Arc::new(|| Ok(()));
        let lease = resources
            .read_derived_cache_owned_with_check(&check)
            .unwrap();
        let admitted = Cell::new(0);
        let calls = Cell::new(0);
        let raw = lease
            .read_generation_with_admission(&key, |bytes| {
                calls.set(calls.get() + 1);
                assert_eq!(bytes, length);
                // At callback time the original immutable Reader is already held.
                let write = fs::OpenOptions::new().write(true).open(f.path(&key));
                assert_eq!(write.unwrap_err().raw_os_error(), Some(32));
                assert!(fs::remove_file(f.path(&key)).is_err());
                admitted.set(bytes);
                Ok(())
            })
            .unwrap()
            .unwrap();
        assert_eq!(calls.get(), 1);
        assert_eq!(admitted.get(), length);
        assert_eq!(raw, expected);
        lease.finish(Ok(())).unwrap();
        f.writable(&key);
        resources.close().unwrap();
    }
}

#[test]
fn invalid_generation_identity_kind_and_overlarge_length_never_invoke_admission() {
    for fault in ["identity", "directory", "length"] {
        let f = Fixture::new();
        let key = f.publish(b"manifest");
        let control = OperationControl::default();
        let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
        let check: DerivedReadCheck = Arc::new(|| Ok(()));
        let lease = resources
            .read_derived_cache_owned_with_check(&check)
            .unwrap();
        match fault {
            "directory" => {
                fs::remove_file(f.path(&key)).unwrap();
                fs::create_dir(f.path(&key)).unwrap();
            }
            "length" => fs::write(f.path(&key), vec![0; MAX_GENERATION_BYTES + 1]).unwrap(),
            _ => (),
        }
        let requested = if fault == "identity" {
            key.to_ascii_uppercase()
        } else {
            key.clone()
        };
        let calls = Cell::new(0);
        let error = lease
            .read_generation_with_admission(&requested, |_| {
                calls.set(calls.get() + 1);
                Ok(())
            })
            .unwrap_err();
        assert_eq!(calls.get(), 0);
        assert_eq!(
            error.code,
            if fault == "identity" {
                "input.invalid"
            } else {
                "vault.data_cache_integrity"
            }
        );
        assert_eq!(
            lease.finish::<()>(Err(error.clone())).unwrap_err().code,
            error.code
        );
        resources.close().unwrap();
    }
}

#[test]
fn rejected_admission_checked_closes_original_unread_manifest_and_keeps_close_error() {
    let f = Fixture::new();
    let key = f.publish(b"manifest");
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check: DerivedReadCheck = Arc::new(|| Ok(()));
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let calls = Cell::new(0);
    let closes = Cell::new(0);
    let error = lease
        .lease
        .owner
        .read_generation_admitted_with_close(
            &key,
            |length| {
                calls.set(calls.get() + 1);
                assert_eq!(length, 8);
                Err(capacity_error())
            },
            |mut file| {
                closes.set(closes.get() + 1);
                assert_eq!(file.metadata()?.len(), 8);
                assert_eq!(file.stream_position()?, 0);
                assert!(
                    fs::OpenOptions::new()
                        .write(true)
                        .open(f.path(&key))
                        .is_err()
                );
                storage::close_file(file)?;
                Err(io::Error::other("injected original manifest close"))
            },
        )
        .unwrap_err();
    assert_eq!(calls.get(), 1);
    assert_eq!(closes.get(), 1);
    assert_eq!(error.code, "test.retained_budget");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 2);
    assert_eq!(causes[0]["code"], "test.retained_budget");
    assert_eq!(causes[1]["code"], "external.io");
    f.writable(&key);
    assert_eq!(
        lease.finish::<()>(Err(error)).unwrap_err().code,
        "test.retained_budget"
    );
    resources.close().unwrap();
}

#[test]
fn successful_admission_that_cancels_is_observed_before_read_and_preserves_close_failure() {
    let f = Fixture::new();
    let key = f.publish(b"manifest");
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check: DerivedReadCheck = Arc::new(|| Ok(()));
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let closes = Cell::new(0);
    let error = lease
        .lease
        .owner
        .read_generation_admitted_with_close(
            &key,
            |length| {
                assert_eq!(length, 8);
                token.cancel();
                Ok(())
            },
            |mut file| {
                closes.set(closes.get() + 1);
                assert_eq!(file.stream_position()?, 0);
                storage::close_file(file)?;
                Err(io::Error::other("injected cancelled manifest close"))
            },
        )
        .unwrap_err();
    assert_eq!(closes.get(), 1);
    assert_eq!(error.code, "command.cancelled");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 2);
    assert_eq!(causes[0]["code"], "command.cancelled");
    assert_eq!(causes[1]["code"], "external.io");
    f.writable(&key);
    assert_eq!(
        lease.finish::<()>(Err(error)).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}

#[test]
fn caller_owned_admission_guard_can_live_through_bytes_and_parse_without_self_borrow() {
    struct Retained<'a>(&'a Cell<usize>);
    impl Drop for Retained<'_> {
        fn drop(&mut self) {
            self.0.set(0);
        }
    }
    let f = Fixture::new();
    let key = f.publish(br#"{"answer":42}"#);
    let control = OperationControl::default();
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let check: DerivedReadCheck = Arc::new(|| Ok(()));
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let retained = Cell::new(0);
    let mut guard = None;
    let raw = lease
        .read_generation_with_admission(&key, |length| {
            retained.set(length);
            guard = Some(Retained(&retained));
            Ok(())
        })
        .unwrap()
        .unwrap();
    assert_eq!(retained.get(), raw.len());
    let parsed: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(parsed["answer"], 42);
    assert_eq!(retained.get(), raw.len());
    drop(parsed);
    drop(raw);
    drop(guard);
    assert_eq!(retained.get(), 0);
    lease.finish(Ok(())).unwrap();
    resources.close().unwrap();
}

#[test]
fn admission_rejection_preserves_primary_close_and_both_original_stops() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let f = Fixture::new();
    let key = f.publish(b"manifest");
    let token = TrackedCancellation::new();
    let control =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let resources = data_resources::acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let stopped = Arc::new(AtomicBool::new(false));
    let budget = stopped.clone();
    let check: DerivedReadCheck = Arc::new(move || {
        if budget.load(Ordering::SeqCst) {
            Err(ToolkitError::new(
                "test.independent_budget",
                "original independent budget stop",
                3,
            ))
        } else {
            Ok(())
        }
    });
    let lease = resources
        .read_derived_cache_owned_with_check(&check)
        .unwrap();
    let error = lease
        .lease
        .owner
        .read_generation_admitted_with_close(
            &key,
            |_| {
                token.cancel();
                stopped.store(true, Ordering::SeqCst);
                Err(capacity_error())
            },
            |mut file| {
                assert_eq!(file.stream_position()?, 0);
                storage::close_file(file)?;
                Err(io::Error::other("injected rejection cleanup"))
            },
        )
        .unwrap_err();
    assert_eq!(error.code, "test.retained_budget");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 4);
    assert_eq!(causes[0]["code"], "test.retained_budget");
    assert_eq!(causes[1]["code"], "external.io");
    assert_eq!(causes[2]["code"], "command.cancelled");
    assert_eq!(causes[3]["code"], "test.independent_budget");
    f.writable(&key);
    assert_eq!(
        lease.finish::<()>(Err(error)).unwrap_err().code,
        "test.retained_budget"
    );
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}
