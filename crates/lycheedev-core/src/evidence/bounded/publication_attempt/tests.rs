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
    fn predeclared(
        &self,
        reference: &BlobRef,
    ) -> lycheedev_windows::filesystem::destination::ManagedStageName {
        use lycheedev_windows::filesystem::destination::{
            ManagedStageDeclaration, ManagedStageKind,
        };
        let records = self.0.join("indexes/data-derived-v1/shared-blobs-v1");
        fs::create_dir_all(&records).unwrap();
        let control = OperationControl::default();
        let mut check = || Ok(());
        let mut files = PathControl {
            control: Some(&control),
            supplemental: &mut check,
        };
        let directory = Directory::open_controlled(&records, &mut files).unwrap();
        let mut raw = [0; 128];
        raw[..8].copy_from_slice(b"LDST0002");
        raw[8] = 2; // Shard, slot zero.
        raw[16..24].copy_from_slice(&reference.bytes.to_le_bytes());
        hex::decode_to_slice(&reference.sha256, &mut raw[56..88]).unwrap();
        let checksum = Sha256::digest(&raw[..96]);
        raw[96..].copy_from_slice(&checksum);
        let mut declaration =
            ManagedStageDeclaration::create(&directory, ManagedStageKind::Shard, 0, &mut files)
                .unwrap();
        let name = declaration.persist(&raw, &mut files).unwrap();
        assert!(declaration.close().is_empty());
        directory.close().unwrap();
        name
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
    let reference = BlobRef { sha256: storage::sha256(b"body"), bytes: 4 };
    let name = f.predeclared(&reference);
    let control = OperationControl::default();
    control.tracked_cancel_handle().unwrap().cancel();
    let attempt = publish_blob_attempt_predeclared_with_check(&f.0, b"body", &reference, 4, &control, || Ok(()), &name);
    assert_eq!(attempt.result.unwrap_err().code, "command.cancelled");
    assert!(!attempt.settlement.stage_created);
    assert!(attempt.settlement.may_release_unpublished_intent());
    assert_eq!(stages(&f.0), 0);
}
#[test]
fn actual_created_stage_is_discarded_before_clean_not_published_receipt() {
    let f = Fixture::new();
    let reference = BlobRef { sha256: storage::sha256(b"body"), bytes: 4 };
    let name = f.predeclared(&reference);
    let observed = Cell::new(false);
    let attempt =
        publish_blob_attempt_predeclared_with_check(&f.0, b"body", &reference, 4, &OperationControl::default(), || {
            if stages(&f.0) != 0 {
                observed.set(true);
                return Err(failure());
            }
            Ok(())
        }, &name);
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
    let reference = BlobRef { sha256: storage::sha256(b"body"), bytes: 4 };
    let name = f.predeclared(&reference);
    let path = blob_path(&f.0, &storage::sha256(b"body")).unwrap();
    let attempt =
        publish_blob_attempt_predeclared_with_check(&f.0, b"body", &reference, 4, &OperationControl::default(), || {
            if path.exists() {
                Err(failure())
            } else {
                Ok(())
            }
        }, &name);
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
    let reference = BlobRef { sha256: storage::sha256(b"body"), bytes: 4 };
    let name = f.predeclared(&reference);
    let control = OperationControl::default();
    publish_blob_bounded(&f.0, b"body", 4, &control).unwrap();
    let attempt = publish_blob_attempt_predeclared_with_check(&f.0, b"body", &reference, 4, &control, || Ok(()), &name);
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
fn predeclared_authenticated_body_preserves_publication_reuse_and_winner_validation() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let reference = BlobRef {
        sha256: storage::sha256(b"body"),
        bytes: 4,
    };
    let name = f.predeclared(&reference);
    let attempt = publish_blob_attempt_predeclared_with_check(
        &f.0,
        b"body",
        &reference,
        4,
        &control,
        || Ok(()),
        &name,
    );
    assert_eq!(
        attempt.result.unwrap(),
        serde_json::to_value(&reference).unwrap()
    );
    assert_eq!(attempt.settlement.publication, StagedPublication::Published);
    assert!(attempt.settlement.stage_cleanup_complete);
    assert_eq!(stages(&f.0), 0);

    let reused = publish_blob_attempt_predeclared_with_check(
        &f.0,
        b"body",
        &reference,
        4,
        &control,
        || Ok(()),
        &name,
    );
    assert!(reused.result.is_ok());
    assert!(reused.settlement.reused);
    assert!(!reused.settlement.stage_created);
    let path = blob_path(&f.0, &reference.sha256).unwrap();
    fs::write(&path, b"evil").unwrap();
    let corrupt = publish_blob_attempt_predeclared_with_check(
        &f.0,
        b"body",
        &reference,
        4,
        &control,
        || Ok(()),
        &name,
    );
    assert_eq!(corrupt.result.unwrap_err().code, "vault.blob_integrity");
    assert_eq!(fs::read(path).unwrap(), b"evil");
    assert_eq!(stages(&f.0), 0);
}

#[test]
fn predeclared_authenticated_body_keeps_length_and_maximum_admission() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let reference = BlobRef {
        sha256: storage::sha256(b"body"),
        bytes: 4,
    };
    let name = f.predeclared(&reference);
    let limit = publish_blob_attempt_predeclared_with_check(
        &f.0,
        b"body",
        &reference,
        3,
        &control,
        || Ok(()),
        &name,
    );
    assert_eq!(limit.result.unwrap_err().code, "vault.blob_limit");
    assert!(!limit.settlement.stage_created);
    let mismatched = BlobRef {
        bytes: 5,
        ..reference.clone()
    };
    let length = publish_blob_attempt_predeclared_with_check(
        &f.0,
        b"body",
        &mismatched,
        4,
        &control,
        || Ok(()),
        &name,
    );
    assert_eq!(length.result.unwrap_err().code, "vault.blob_integrity");
    assert!(!length.settlement.stage_created);
    assert!(!blob_path(&f.0, &reference.sha256).unwrap().exists());
    assert_eq!(stages(&f.0), 0);
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
