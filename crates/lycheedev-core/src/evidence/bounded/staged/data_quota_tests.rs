//! Real Data publication with failures at the existing native-owner boundaries.
use super::*;
use std::io::Cursor;
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-stream-settlement-{}",
            lycheedev_windows::random::token().unwrap()
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn paths(&self) -> crate::workspace::data_disk::DataCachePaths {
        crate::workspace::data_disk::DataCachePaths::new(&self.0)
    }
    fn receipt(&self, suffix: &str) -> PathBuf {
        self.paths()
            .shared_blobs()
            .join(format!("{}.{suffix}", storage::sha256(b"body")))
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
struct FailShard {
    input: Cursor<&'static [u8]>,
    fail_close: bool,
}
impl Read for FailShard {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.input.read(out)?;
        if n == 0 {
            test_hooks::set_before_stage_write(|| {
                Err(ToolkitError::new(
                    "fixture.final_write",
                    "original shard write failed",
                    5,
                ))
            });
            if self.fail_close {
                test_hooks::set_after_stage_close(|| {
                    Err(ToolkitError::new(
                        "fixture.checked_close",
                        "checked stage close completion failed",
                        5,
                    ))
                });
            }
        }
        Ok(n)
    }
}
fn publish(f: &Fixture, fail_close: bool) -> ToolkitError {
    let mut reader = FailShard {
        input: Cursor::new(b"body"),
        fail_close,
    };
    match crate::workspace::data_disk::publish_shared_blob_stream_with_check(
        &f.0,
        &mut reader,
        4,
        4,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    ) {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("fault did not execute")
        }
    }
}
#[test]
fn real_clean_unpublished_shard_and_spool_cleanup_withdraw_intent_and_allow_retry() {
    let f = Fixture::new();
    let error = publish(&f, false);
    assert_eq!(error.code, "fixture.final_write");
    assert!(!f.receipt("intent").exists());
    assert!(!f.receipt("complete").exists());
    assert_eq!(fs::read_dir(f.paths().staging).unwrap().count(), 0);
    crate::workspace::data_disk::publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"body"),
        4,
        4,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    )
    .unwrap()
    .close()
    .unwrap();
    assert!(f.receipt("intent").exists());
    assert!(f.receipt("complete").exists());
}
#[test]
fn real_unpublished_shard_close_failure_keeps_original_error_and_retained_intent() {
    let f = Fixture::new();
    let error = publish(&f, true);
    assert_eq!(error.code, "fixture.final_write");
    assert!(
        serde_json::to_string(&error.details)
            .unwrap()
            .contains("fixture.checked_close")
    );
    assert!(f.receipt("intent").exists());
    assert!(!f.receipt("complete").exists());
    // The injected diagnostic follows the real successful native cleanup. An
    // absent path still does not grant quota permission to guess that outcome.
    assert!(!blob_path(&f.0, &storage::sha256(b"body")).unwrap().exists());
    assert_eq!(fs::read_dir(f.paths().staging).unwrap().count(), 0);
    let retried = crate::workspace::data_disk::publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"body"),
        4,
        4,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    );
    match retried {
        Err(error) => assert_eq!(error.code, "vault.data_shared_blob_recovery_required"),
        Ok(blob) => {
            blob.close().unwrap();
            panic!("uncertain cleanup must not replay")
        }
    }
}
#[test]
fn unknown_or_reused_or_incomplete_stream_settlement_never_grants_withdrawal() {
    for state in [
        StagedPublication::NotPublished,
        StagedPublication::Published,
        StagedPublication::Unknown,
    ] {
        for reused in [false, true] {
            for spool in [false, true] {
                for shard in [false, true] {
                    let settled = StreamPublicationSettlement {
                        publication: state,
                        reused,
                        spool_cleanup_complete: spool,
                        shard_cleanup_complete: shard,
                    };
                    assert_eq!(
                        settled.may_release_unpublished_intent(),
                        state == StagedPublication::NotPublished && !reused && spool && shard
                    );
                }
            }
        }
    }
}

#[test]
fn unauthenticated_existing_winner_does_not_turn_clean_not_published_into_retained_intent() {
    let f = Fixture::new();
    let path = blob_path(&f.0, &storage::sha256(b"body")).unwrap();
    fs::create_dir(path.parent().unwrap()).unwrap();
    fs::write(&path, b"fake").unwrap();
    let result = crate::workspace::data_disk::publish_shared_blob_stream_with_check(
        &f.0,
        &mut Cursor::new(b"body"),
        4,
        4,
        &OperationControl::default(),
        || Ok(()),
        &Meter,
    );
    match result {
        Err(error) => assert_eq!(error.code, "vault.blob_integrity"),
        Ok(blob) => {
            blob.close().unwrap();
            panic!("wrong body must fail SHA authentication")
        }
    }
    assert_eq!(fs::read(&path).unwrap(), b"fake");
    assert!(!f.receipt("intent").exists());
    assert!(!f.receipt("complete").exists());
    assert_eq!(fs::read_dir(f.paths().staging).unwrap().count(), 0);
}
