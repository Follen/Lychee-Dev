use super::*;
use std::{
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-blob-attempt-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn failure() -> ToolkitError {
    ToolkitError::new("fixture.independent_stop", "independent stop", 7)
}
fn stages(home: &Path) -> usize {
    fs::read_dir(home.join("blobs"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                fs::read_dir(entry.path())
                    .unwrap()
                    .map(|child| {
                        child
                            .unwrap()
                            .file_name()
                            .to_string_lossy()
                            .starts_with(".lycheedev-")
                    })
                    .filter(|yes| *yes)
                    .count()
            } else {
                0
            }
        })
        .sum()
}
#[test]
fn cancelled_before_stage_returns_original_failure_and_no_growth_witness() {
    let f = Fixture::new();
    let control = OperationControl::default();
    control.cancel_handle().store(true, Ordering::Release);
    let attempt = publish_blob_attempt_with_check(&f.0, b"body", 4, &control, || Ok(()));
    assert_eq!(attempt.result.unwrap_err().code, "command.cancelled");
    assert!(!attempt.settlement.stage_created);
    assert!(attempt.settlement.may_release_unpublished_intent());
    assert_eq!(stages(&f.0), 0);
}
#[test]
fn actual_created_stage_is_discarded_before_clean_not_published_receipt() {
    let f = Fixture::new();
    let observed = Cell::new(false);
    let attempt =
        publish_blob_attempt_with_check(&f.0, b"body", 4, &OperationControl::default(), || {
            if stages(&f.0) != 0 {
                observed.set(true);
                return Err(failure());
            }
            Ok(())
        });
    assert!(observed.get());
    assert_eq!(attempt.result.unwrap_err().code, "fixture.independent_stop");
    assert!(attempt.settlement.stage_created);
    assert!(attempt.settlement.stage_cleanup_complete);
    assert!(attempt.settlement.may_release_unpublished_intent());
    assert_eq!(stages(&f.0), 0);
    assert!(!blob_path(&f.0, &storage::sha256(b"body")).unwrap().exists());
}
#[test]
fn real_published_body_and_checked_close_survive_late_failure_receipt() {
    let f = Fixture::new();
    let path = blob_path(&f.0, &storage::sha256(b"body")).unwrap();
    let attempt =
        publish_blob_attempt_with_check(&f.0, b"body", 4, &OperationControl::default(), || {
            if path.exists() {
                Err(failure())
            } else {
                Ok(())
            }
        });
    assert_eq!(attempt.result.unwrap_err().code, "fixture.independent_stop");
    assert_eq!(attempt.settlement.publication, StagedPublication::Published);
    assert!(attempt.settlement.stage_cleanup_complete);
    assert!(!attempt.settlement.may_release_unpublished_intent());
    assert_eq!(fs::read(path).unwrap(), b"body");
    assert_eq!(stages(&f.0), 0);
}
#[test]
fn successful_existing_body_reuse_keeps_native_not_published_separate() {
    let f = Fixture::new();
    let control = OperationControl::default();
    publish_blob_bounded(&f.0, b"body", 4, &control).unwrap();
    let attempt = publish_blob_attempt_with_check(&f.0, b"body", 4, &control, || Ok(()));
    assert_eq!(attempt.result.unwrap()["bytes"], 4);
    assert!(attempt.settlement.reused);
    assert!(!attempt.settlement.stage_created);
    assert_eq!(
        attempt.settlement.publication,
        StagedPublication::NotPublished
    );
    assert!(!attempt.settlement.may_release_unpublished_intent());
}
#[test]
fn unknown_or_incomplete_cleanup_never_authorizes_intent_release() {
    let mut receipt = BlobPublicationSettlement::new();
    receipt.stage_acquired();
    assert!(!receipt.may_release_unpublished_intent());
    receipt.stage_closed(false);
    assert!(!receipt.may_release_unpublished_intent());
    receipt.stage_closed(true);
    assert!(receipt.may_release_unpublished_intent());
    receipt.publication = StagedPublication::Unknown;
    assert!(!receipt.may_release_unpublished_intent());
    receipt.publication = StagedPublication::Published;
    assert!(!receipt.may_release_unpublished_intent());
}
