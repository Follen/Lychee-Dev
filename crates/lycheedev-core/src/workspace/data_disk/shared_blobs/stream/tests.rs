use super::*;
use crate::{evidence::BlobStreamPhase, workspace};
use std::{
    cell::Cell,
    io::{self, Cursor},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct Fixture(PathBuf);
impl Fixture {
    fn new(limit: u64) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-data-stream-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        let f = Self(home);
        f.limit(limit);
        f
    }
    fn limit(&self, limit: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(limit);
        storage::atomic_json(&self.0.join("config.json"), &config).unwrap();
    }
    fn paths(&self) -> DataCachePaths {
        DataCachePaths::new(&self.0)
    }
    fn usage(&self) -> DerivedUsage {
        let control = OperationControl::default();
        let owner = DerivedReservation::acquire_inner(
            None,
            &self.0,
            0,
            AdditionalPeak::default(),
            &control,
            None,
            true,
        )
        .unwrap();
        let value = owner.report().existing;
        owner.finish(Ok(())).unwrap();
        value
    }
    fn publish(&self, bytes: &[u8]) -> Result<VerifiedBlob> {
        publish_shared_blob_stream_with_check(
            &self.0,
            &mut Cursor::new(bytes),
            bytes.len() as u64,
            1 << 30,
            &OperationControl::default(),
            || Ok(()),
            &Meter,
        )
    }
    fn assert_no_spool(&self) {
        assert_eq!(fs::read_dir(self.paths().staging).unwrap().count(), 0);
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
struct Meter;
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
}
fn failed<T>(result: Result<T>) -> ToolkitError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("expected failure"),
    }
}

#[test]
fn exact_stream_uses_declared_length_not_maximum_and_shared_body_is_charged_once() {
    let f = Fixture::new(3 * 4 + 80);
    let mut blob = f.publish(b"body").unwrap();
    let mut bytes = Vec::new();
    blob.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"body");
    blob.close().unwrap();
    let usage = f.usage();
    assert_eq!(usage.shared_blob_bytes, 4);
    assert_eq!(usage.total().unwrap(), 84);
    f.assert_no_spool();
    // A second stream needs its real 4-byte spool, not the 1GiB request ceiling.
    f.limit(88);
    f.publish(b"body").unwrap().close().unwrap();
    assert_eq!(f.usage().total().unwrap(), 84);
}
#[test]
fn pre_spool_admission_refuses_without_reading_and_zero_tail_never_publishes() {
    struct Count<'a>(&'a Cell<usize>);
    impl Read for Count<'_> {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            self.0.set(self.0.get() + 1);
            Ok(0)
        }
    }
    let f = Fixture::new(3);
    let reads = Cell::new(0);
    let error = failed(publish_shared_blob_stream_with_check(
        &f.0,
        &mut Count(&reads),
        4,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    ));
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    assert_eq!(reads.get(), 0);
    f.limit(1000);
    let error = failed(publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"ab"),
        4,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    ));
    assert_eq!(error.code, "vault.blob_integrity");
    assert!(
        !evidence::blob_path(&f.0, &storage::sha256(b"ab\0\0"))
            .unwrap()
            .exists()
    );
    f.assert_no_spool();
    assert_eq!(f.usage().total().unwrap(), 0);
}
#[test]
fn producer_eof_error_and_extra_byte_are_never_optimized_away_for_existing_body() {
    let f = Fixture::new(1000);
    f.publish(b"body").unwrap().close().unwrap();
    let error = failed(publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"body!"),
        4,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    ));
    assert_eq!(error.code, "vault.blob_integrity");
    f.assert_no_spool();
    struct BadEof(Cursor<&'static [u8]>);
    impl Read for BadEof {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            let n = self.0.read(b)?;
            if n == 0 {
                Err(io::Error::other("business EOF failed"))
            } else {
                Ok(n)
            }
        }
    }
    let error = failed(publish_shared_blob_stream_with_check(
        &f.0,
        &mut BadEof(Cursor::new(b"body")),
        4,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    ));
    assert!(error.message.contains("business EOF failed"));
    f.assert_no_spool();
    assert_eq!(f.usage().total().unwrap(), 84);
}
#[test]
fn nested_shared_publication_observes_presized_spool_without_reentering_its_quota_lock() {
    struct Nested<'a> {
        f: &'a Fixture,
        data: Cursor<&'static [u8]>,
        entered: bool,
    }
    impl Read for Nested<'_> {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            if !self.entered {
                self.entered = true;
                let usage = self.f.usage();
                assert_eq!(usage.temporary_namespace_bytes, 4);
                publish_shared_blob_with_check(
                    &self.f.0,
                    b"range",
                    5,
                    &OperationControl::default(),
                    &|| Ok(()),
                )
                .map_err(io::Error::other)?;
                assert_eq!(self.f.usage().temporary_namespace_bytes, 4);
            }
            self.data.read(b)
        }
    }
    let f = Fixture::new(1000);
    let mut producer = Nested {
        f: &f,
        data: Cursor::new(b"body"),
        entered: false,
    };
    publish_shared_blob_stream_with_check(
        &f.0,
        &mut producer,
        4,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    )
    .unwrap()
    .close()
    .unwrap();
    assert!(producer.entered);
    let usage = f.usage();
    assert_eq!(usage.shared_blob_bytes, 9);
    assert_eq!(usage.total().unwrap(), 169);
    f.assert_no_spool();
}
#[test]
fn final_peak_refusal_publishes_no_body_and_discards_only_original_spool() {
    let f = Fixture::new(91); // Cold 4-byte final needs 4 + 8 + 80 = 92.
    let error = failed(f.publish(b"body"));
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    f.assert_no_spool();
    assert_eq!(f.usage().total().unwrap(), 0);
    assert!(
        !evidence::blob_path(&f.0, &storage::sha256(b"body"))
            .unwrap()
            .exists()
    );
}
#[test]
fn independent_stop_during_producer_keeps_first_cause_and_cleans_owned_spool() {
    struct Stop {
        data: Cursor<&'static [u8]>,
        stop: Arc<AtomicBool>,
    }
    impl Read for Stop {
        fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
            let n = self.data.read(bytes)?;
            self.stop.store(true, Ordering::Release);
            Ok(n)
        }
    }
    let f = Fixture::new(1000);
    let stop = Arc::new(AtomicBool::new(false));
    let checked = stop.clone();
    let error = failed(publish_shared_blob_stream_with_check(
        &f.0,
        &mut Stop {
            data: Cursor::new(b"body"),
            stop,
        },
        4,
        1000,
        &OperationControl::default(),
        move || {
            if checked.load(Ordering::Acquire) {
                Err(ToolkitError::new(
                    "fixture.original_budget",
                    "same Budget stopped",
                    7,
                ))
            } else {
                Ok(())
            }
        },
        &Meter,
    ));
    assert_eq!(error.code, "fixture.original_budget");
    let reported = PathBuf::from(
        error.details["publication"]["sourceStagePath"]
            .as_str()
            .unwrap(),
    );
    assert_eq!(reported.parent(), Some(f.paths().staging.as_path()));
    f.assert_no_spool();
    assert_eq!(f.usage().total().unwrap(), 0);
}
#[test]
fn known_owned_leftover_spool_counts_after_restart_without_adoption_or_deletion() {
    let f = Fixture::new(1000);
    f.publish(b"body").unwrap().close().unwrap();
    let orphan = f.paths().staging.join(".lycheedev-export-crash-fixture");
    fs::write(&orphan, [0; 100]).unwrap(); // Explicit synthetic crash residue, not a real kill test.
    f.limit(184);
    let error = failed(f.publish(b"next"));
    assert_eq!(error.code, "vault.data_cache_budget_exceeded");
    assert_eq!(fs::metadata(&orphan).unwrap().len(), 100);
    assert_eq!(f.usage().total().unwrap(), 184);
}
#[test]
fn published_final_late_stop_retains_pending_attribution_and_never_deletes_shared_evidence() {
    struct StopOnVerify(Arc<AtomicBool>);
    impl CaptureAccounting for StopOnVerify {
        fn check(&self) -> Result<()> {
            Ok(())
        }
        fn admit_allocation(&self, _: usize) -> Result<()> {
            Ok(())
        }
        fn admit_work(&self, _: usize) -> Result<()> {
            Ok(())
        }
        fn blob_stream_phase(&self, phase: BlobStreamPhase) -> impl Drop + '_ {
            if matches!(phase, BlobStreamPhase::FinalVerify) {
                self.0.store(true, Ordering::Release);
            }
            struct Span;
            impl Drop for Span {
                fn drop(&mut self) {}
            }
            Span
        }
    }
    let f = Fixture::new(1000);
    let stop = Arc::new(AtomicBool::new(false));
    let checked = stop.clone();
    let error = failed(publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"body"),
        4,
        1000,
        &OperationControl::default(),
        move || {
            if checked.load(Ordering::Acquire) {
                Err(ToolkitError::new(
                    "fixture.late_budget",
                    "published stop",
                    7,
                ))
            } else {
                Ok(())
            }
        },
        &StopOnVerify(stop),
    ));
    assert_eq!(error.code, "fixture.late_budget");
    assert_eq!(error.details["publication"]["state"], "published");
    assert_eq!(
        fs::read(evidence::blob_path(&f.0, &storage::sha256(b"body")).unwrap()).unwrap(),
        b"body"
    );
    f.assert_no_spool();
    let usage = f.usage();
    assert_eq!(usage.retained_shared_peak_bytes, 8);
    assert_eq!(usage.total().unwrap(), 56);
    assert!(
        !f.paths()
            .shared_blobs()
            .join(format!("{}.complete", storage::sha256(b"body")))
            .exists()
    );
}

#[test]
fn registered_body_removed_before_digest_lock_is_not_republished_by_stream_reuse() {
    struct RemoveAtLock(PathBuf);
    impl CaptureAccounting for RemoveAtLock {
        fn check(&self) -> Result<()> {
            Ok(())
        }
        fn admit_allocation(&self, _: usize) -> Result<()> {
            Ok(())
        }
        fn admit_work(&self, _: usize) -> Result<()> {
            Ok(())
        }
        fn blob_stream_phase(&self, phase: BlobStreamPhase) -> impl Drop + '_ {
            if matches!(phase, BlobStreamPhase::DigestLock) {
                fs::remove_file(&self.0).unwrap();
            }
            struct Span;
            impl Drop for Span {
                fn drop(&mut self) {}
            }
            Span
        }
    }
    let f = Fixture::new(1000);
    f.publish(b"body").unwrap().close().unwrap();
    let path = evidence::blob_path(&f.0, &storage::sha256(b"body")).unwrap();
    let error = failed(publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"body"),
        4,
        1000,
        &OperationControl::default(),
        || Ok(()),
        &RemoveAtLock(path.clone()),
    ));
    assert_eq!(error.code, "vault.blob_integrity");
    assert!(!path.exists());
    f.assert_no_spool();
    assert!(
        f.paths()
            .shared_blobs()
            .join(format!("{}.intent", storage::sha256(b"body")))
            .exists()
    );
    assert!(
        f.paths()
            .shared_blobs()
            .join(format!("{}.complete", storage::sha256(b"body")))
            .exists()
    );
}
