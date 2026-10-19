use super::*;
use std::{cell::Cell, fs, os::windows::io::AsRawHandle};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "lychee-quota-intent-{}",
            crate::random::token().unwrap()
        ));
        fs::create_dir_all(root.join("indexes/data-derived-v1/shared-blobs-v1")).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("indexes/data-derived-v1/shared-blobs-v1")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn record() -> [u8; 48] {
    let mut record = [0; 48];
    record[..8].copy_from_slice(b"LDSB0001");
    record[8..16].copy_from_slice(&4u64.to_le_bytes());
    record[16..].fill(0xab);
    record
}
fn name() -> String {
    format!("{}.intent", "ab".repeat(32))
}
struct StopControl<'a>(&'a Cell<bool>);
impl FileControl for StopControl<'_> {
    type Error = io::Error;
    fn charge(&mut self, _: FileCost) -> io::Result<()> {
        self.check()
    }
    fn check(&mut self) -> io::Result<()> {
        if self.0.get() {
            Err(io::Error::other("cancelled"))
        } else {
            Ok(())
        }
    }
}
#[test]
fn same_native_handle_survives_publish_and_cancellation_withdrawal() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let stopped = Cell::new(false);
    let mut control = StopControl(&stopped);
    let mut intent = QuotaIntent::create(&directory, &mut control).unwrap();
    let original = intent.stage.stage.file.as_ref().unwrap().as_raw_handle();
    assert_eq!(
        intent.publish_record(&record(), &mut control).unwrap(),
        StagedPublication::Published
    );
    assert_eq!(
        intent.stage.stage.file.as_ref().unwrap().as_raw_handle(),
        original
    );
    stopped.set(true);
    assert!(control.check().is_err());
    // No further control/native acquisition is needed for required cleanup.
    fs::write(f.path().join("foreign"), b"protected").unwrap();
    assert!(intent.withdraw().is_empty());
    assert!(!f.path().join(name()).exists());
    assert_eq!(fs::read(f.path().join("foreign")).unwrap(), b"protected");
    directory.close().unwrap();
}
#[test]
fn stopped_or_failed_seal_attempt_cannot_append_a_second_record() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let mut intent = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    let stage = f.path().join(intent.name());
    assert!(
        intent
            .publish_record_using(
                &record(),
                &mut Uncontrolled,
                |_| Err(io::Error::other("seal failure")),
                |stage, name| stage.rename(name, false)
            )
            .is_err()
    );
    assert_eq!(
        intent
            .stage
            .stage
            .file
            .as_ref()
            .unwrap()
            .metadata()
            .unwrap()
            .len(),
        48
    );
    assert!(intent.publish_record(&record(), &mut Uncontrolled).is_err());
    assert_eq!(
        intent
            .stage
            .stage
            .file
            .as_ref()
            .unwrap()
            .metadata()
            .unwrap()
            .len(),
        48
    );
    assert!(intent.close().is_empty());
    assert!(!stage.exists());
    directory.close().unwrap();
}
#[test]
fn collision_never_transfers_winner_cleanup_authority() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    fs::write(f.path().join(name()), record()).unwrap();
    let mut intent = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    let stage = f.path().join(intent.name());
    assert_eq!(
        intent.publish_record(&record(), &mut Uncontrolled).unwrap(),
        StagedPublication::NotPublished
    );
    assert!(intent.withdraw().is_empty());
    assert!(!stage.exists());
    assert_eq!(fs::read(f.path().join(name())).unwrap(), record());
    directory.close().unwrap();
}
#[test]
fn unknown_after_real_rename_refuses_withdraw_and_preserves_record() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let mut intent = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    assert!(
        intent
            .publish_record_using(
                &record(),
                &mut Uncontrolled,
                File::sync_all,
                |stage, name| {
                    stage.rename(name, false)?;
                    Err(io::Error::other("native completion lost"))
                }
            )
            .is_err()
    );
    assert_eq!(intent.publication(), StagedPublication::Unknown);
    assert_eq!(intent.withdraw().len(), 1);
    assert_eq!(fs::read(f.path().join(name())).unwrap(), record());
    directory.close().unwrap();
}
#[test]
fn withdrawal_reports_real_close_failure_once_without_retry() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let mut intent = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    intent.publish_record(&record(), &mut Uncontrolled).unwrap();
    let closes = Cell::new(0);
    let errors = intent.withdraw_with(|file| {
        closes.set(closes.get() + 1);
        super::super::super::super::close_file(file)?;
        Err(io::Error::other("close completion lost"))
    });
    assert_eq!(closes.get(), 1);
    assert_eq!(errors.len(), 1);
    assert!(!f.path().join(name()).exists());
    directory.close().unwrap();
}
#[test]
fn evidence_or_arbitrary_namespace_cannot_create_quota_capability() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("blobs")).unwrap();
    let directory = Directory::open(&f.0.join("blobs")).unwrap();
    assert!(QuotaIntent::create(&directory, &mut Uncontrolled).is_err());
    assert_eq!(fs::read_dir(f.0.join("blobs")).unwrap().count(), 0);
    directory.close().unwrap();
}

#[test]
fn repair_reservation_uses_distinct_fixed_name_and_only_withdraws_original_handle() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let original = f.path().join(name());
    fs::write(&original, record()).unwrap();
    let completed = f.path().join(format!("{}.complete", "ab".repeat(32)));
    fs::write(&completed, [7; 32]).unwrap();
    let mut repair = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    let handle = repair.stage.stage.file.as_ref().unwrap().as_raw_handle();
    assert_eq!(
        repair
            .publish_repair_record(&record(), &mut Uncontrolled)
            .unwrap(),
        StagedPublication::Published
    );
    assert_eq!(
        repair.stage.stage.file.as_ref().unwrap().as_raw_handle(),
        handle
    );
    assert!(
        f.path()
            .join(format!("{}.repair.intent", "ab".repeat(32)))
            .exists()
    );
    assert!(repair.withdraw().is_empty());
    assert!(
        !f.path()
            .join(format!("{}.repair.intent", "ab".repeat(32)))
            .exists()
    );
    assert_eq!(fs::read(original).unwrap(), record());
    assert_eq!(fs::read(completed).unwrap(), [7; 32]);
    directory.close().unwrap();
}
#[test]
fn existing_repair_reservation_is_never_replaced_or_withdrawn_by_a_loser() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let path = f.path().join(format!("{}.repair.intent", "ab".repeat(32)));
    fs::write(&path, record()).unwrap();
    let mut repair = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    assert_eq!(
        repair
            .publish_repair_record(&record(), &mut Uncontrolled)
            .unwrap(),
        StagedPublication::NotPublished
    );
    assert!(repair.withdraw().is_empty());
    assert_eq!(fs::read(path).unwrap(), record());
    directory.close().unwrap();
}

#[test]
fn unknown_repair_marker_never_grants_withdrawal_or_retry() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let mut repair = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    let private = f.path().join(repair.name());
    let error = repair
        .publish_record_named_using(
            &record(),
            ".repair.intent",
            &mut Uncontrolled,
            File::sync_all,
            |_, _| Err(io::Error::other("unknown rename completion")),
        )
        .unwrap_err();
    assert!(error.to_string().contains("unknown rename completion"));
    assert_eq!(repair.publication(), StagedPublication::Unknown);
    assert!(!repair.withdraw().is_empty());
    assert_eq!(fs::read(private).unwrap(), record());
    directory.close().unwrap();
}
#[test]
fn repair_withdraw_checked_close_failure_is_reported_after_original_disposition() {
    let f = Fixture::new();
    let directory = Directory::open(&f.path()).unwrap();
    let mut repair = QuotaIntent::create(&directory, &mut Uncontrolled).unwrap();
    repair
        .publish_repair_record(&record(), &mut Uncontrolled)
        .unwrap();
    let errors = repair.withdraw_with(|file| {
        drop(file);
        Err(io::Error::other("repair checked close failed"))
    });
    assert_eq!(errors.len(), 1);
    assert!(
        errors[0]
            .to_string()
            .contains("repair checked close failed")
    );
    assert!(
        !f.path()
            .join(format!("{}.repair.intent", "ab".repeat(32)))
            .exists()
    );
    directory.close().unwrap();
}
