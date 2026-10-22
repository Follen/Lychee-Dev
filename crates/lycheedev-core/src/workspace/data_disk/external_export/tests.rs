use super::*;
use crate::{control::TrackedCancellation, workspace};
use std::{
    cell::Cell,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

fn no_stop() -> Result<()> {
    Ok(())
}

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new(limit: u64) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "lychee-external-quota-{}-{}-{}",
            storage::utc_now().replace([':', '.'], "_"),
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir_all(&root).unwrap();
        let home = root.join("home");
        workspace::initialize(&home).unwrap();
        let fixture = Self { root, home };
        fixture.limit(limit);
        fixture
    }
    fn limit(&self, limit: u64) {
        let mut config = workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(limit);
        storage::atomic_json(&self.home.join("config.json"), &config).unwrap();
    }
    fn records(&self) -> Vec<PathBuf> {
        fs::read_dir(DataCachePaths::new(&self.home).shared_blobs())
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect()
    }
    fn usage(&self) -> DerivedUsage {
        let control = OperationControl::default();
        let owner = reserve_export_temporary_with_check(&self.home, 0, &control, &no_stop).unwrap();
        let usage = owner.report().existing;
        owner.finish(Ok(())).unwrap();
        usage
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn admission_includes_record_before_writer_and_clean_settlement_releases_each_peak() {
    let f = Fixture::new(132);
    let control = OperationControl::default();
    let count = Cell::new(0);
    assert_eq!(
        reserve_export_temporary_with_check(&f.home, 5, &control, &no_stop)
            .err()
            .unwrap()
            .code,
        "vault.data_cache_budget_exceeded"
    );
    assert!(f.records().is_empty());
    for publication in [
        StagedPublication::Published,
        StagedPublication::NotPublished,
    ] {
        let owner = reserve_export_temporary_with_check(&f.home, 4, &control, &no_stop).unwrap();
        assert_eq!(owner.report().admitted_peak_bytes, 132);
        owner
            .with_external_export(|| {
                count.set(count.get() + 1);
                let files = f.records();
                assert_eq!(files.len(), 1);
                let raw = fs::read(&files[0]).unwrap();
                assert_eq!(raw, record(0, 4));
                (Ok(()), publication, true)
            })
            .unwrap();
        assert!(f.records().is_empty());
    }
    assert_eq!(count.get(), 2);
}

#[test]
fn unknown_and_cleanup_failure_keep_peak_after_owner_is_gone_without_touching_output() {
    let f = Fixture::new(1024);
    let control = OperationControl::default();
    let user_file = f.root.join("user-output");
    fs::write(&user_file, b"user-original").unwrap();
    for (state, clean) in [
        (StagedPublication::Unknown, true),
        (StagedPublication::NotPublished, false),
        (StagedPublication::Published, false),
    ] {
        let owner = reserve_export_temporary_with_check(&f.home, 4, &control, &no_stop).unwrap();
        owner
            .with_external_export(|| (Ok(()), state, clean))
            .unwrap();
    }
    assert_eq!(f.records().len(), 3);
    let usage = f.usage();
    assert_eq!(usage.pending_external_export_bytes, 12);
    assert_eq!(usage.generation_namespace_bytes, 3 * 128);
    assert_eq!(usage.total().unwrap(), 12 + 3 * 128);
    assert_eq!(fs::read(user_file).unwrap(), b"user-original");
    f.limit(usage.total().unwrap() + 128 + 3);
    assert_eq!(
        reserve_export_temporary_with_check(&f.home, 4, &control, &no_stop)
            .err()
            .unwrap()
            .code,
        "vault.data_cache_budget_exceeded"
    );
}

#[test]
fn late_cancellation_still_withdraws_clean_closed_publication_and_keeps_original_error() {
    let f = Fixture::new(1024);
    let stop = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(60), stop.clone());
    let owner = reserve_export_temporary_with_check(&f.home, 4, &control, &no_stop).unwrap();
    let error = owner
        .with_external_export::<()>(|| {
            stop.cancel();
            (
                Err(control.check().unwrap_err()),
                StagedPublication::Published,
                true,
            )
        })
        .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(f.records().is_empty());
    assert_eq!(f.usage().pending_external_export_bytes, 0);
}

#[test]
fn unwind_retains_synced_declaration_and_only_charges_its_external_allowance_once() {
    let f = Fixture::new(1024);
    let control = OperationControl::default();
    let owner = reserve_export_temporary_with_check(&f.home, 7, &control, &no_stop).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.with_external_export::<()>(|| panic!("after durable record"))
    }));
    assert!(result.is_err());
    let usage = f.usage();
    assert_eq!(usage.pending_external_export_bytes, 7);
    assert_eq!(usage.generation_namespace_bytes, 128);
    assert_eq!(usage.total().unwrap(), 135);
}

#[test]
fn occupied_and_torn_slots_are_retained_and_never_adopted() {
    let f = Fixture::new(1024);
    let control = OperationControl::default();
    let owner = reserve_export_temporary_with_check(&f.home, 4, &control, &no_stop).unwrap();
    let parent = directory(&owner);
    let occupied = parent
        .path()
        .join(ManagedStageDeclaration::record_name(KIND, 0).unwrap());
    fs::write(&occupied, b"foreign").unwrap();
    // Account the already inserted foreign bytes in a fresh owner.
    owner.finish(Ok(())).unwrap();
    let owner = reserve_export_temporary_with_check(&f.home, 4, &control, &no_stop).unwrap();
    owner
        .with_external_export(|| {
            assert_eq!(f.records().len(), 2);
            assert_eq!(fs::read(&occupied).unwrap(), b"foreign");
            (Ok(()), StagedPublication::Published, true)
        })
        .unwrap();
    assert_eq!(f.records(), vec![occupied.clone()]);
    assert_eq!(fs::read(occupied).unwrap(), b"foreign");
    assert_eq!(f.usage().pending_external_export_bytes, 0);
}

#[test]
fn inventory_refreshes_sparse_case_insensitive_slots_and_retains_torn_records() {
    let f = Fixture::new(1024);
    assert_eq!(f.usage().pending_external_export_bytes, 0);
    let records = DataCachePaths::new(&f.home).shared_blobs();
    let high = ManagedStageDeclaration::record_name(KIND, 63)
        .unwrap()
        .to_ascii_uppercase();
    fs::write(records.join(&high), record(63, 9)).unwrap();
    let usage = f.usage();
    assert_eq!(usage.pending_external_export_bytes, 9);
    assert_eq!(usage.generation_namespace_bytes, 128);

    fs::write(
        records.join(ManagedStageDeclaration::record_name(KIND, 31).unwrap()),
        record(31, 5),
    )
    .unwrap();
    let torn = records.join(ManagedStageDeclaration::record_name(KIND, 7).unwrap());
    fs::write(&torn, b"torn").unwrap();
    let usage = f.usage();
    assert_eq!(usage.pending_external_export_bytes, 14);
    assert_eq!(usage.generation_namespace_bytes, 260);
    assert_eq!(fs::read(records.join(high)).unwrap(), record(63, 9));
    assert_eq!(fs::read(torn).unwrap(), b"torn");
}

#[test]
fn failed_withdrawal_reports_error_and_retains_original_record_for_later_inventory() {
    let f = Fixture::new(1024);
    let control = OperationControl::default();
    let owner = reserve_export_temporary_with_check(&f.home, 9, &control, &no_stop).unwrap();
    let error = owner
        .with_external_export(|| {
            let path = f.records().pop().unwrap();
            let mut permissions = fs::metadata(&path).unwrap().permissions();
            permissions.set_readonly(true);
            fs::set_permissions(path, permissions).unwrap();
            (Ok(()), StagedPublication::Published, true)
        })
        .unwrap_err();
    assert_eq!(error.code, "external.io");
    let path = f.records().pop().unwrap();
    assert_eq!(fs::read(&path).unwrap(), record(0, 9));
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
    let usage = f.usage();
    assert_eq!(usage.pending_external_export_bytes, 9);
    assert_eq!(usage.total().unwrap(), 137);
}
