use super::*;
use crate::workspace::{
    self,
    data_resources::{Policy, acquire},
};
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-data-object-{}-{}-{}",
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
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn peak(bytes: u64) -> AdditionalPeak {
    AdditionalPeak {
        objects: bytes,
        staging: bytes,
        generations: 1024,
        ..AdditionalPeak::default()
    }
}
#[test]
fn streamed_objects_are_content_addressed_deduplicated_and_checked_before_manifest() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources.reserve_derived_cache(peak(131073)).unwrap();
    let raw = vec![42; 131073];
    let mut writer = owner.begin_object(raw.len() as u64).unwrap();
    for chunk in raw.chunks(8191) {
        writer.write_all(chunk).unwrap();
    }
    let object = writer.commit().unwrap();
    assert_eq!(object.sha256, storage::sha256(&raw));
    assert_eq!(object.bytes, raw.len() as u64);
    assert_eq!(
        fs::read(owner.paths.object(&object.sha256).unwrap()).unwrap(),
        raw
    );
    assert_eq!(fs::read_dir(&owner.paths.staging).unwrap().count(), 0);
    assert_eq!(owner.charged.get().objects, object.bytes);
    assert_eq!(owner.charged.get().staging, 0);
    let mut again = owner.begin_object(raw.len() as u64).unwrap();
    again.write_all(&raw).unwrap();
    assert_eq!(again.commit().unwrap(), object);
    assert_eq!(owner.charged.get().objects, object.bytes); // No second physical copy.
    let manifest = serde_json::to_vec(&object).unwrap();
    let key = storage::sha256(b"generation identity");
    owner.publish_generation(&key, &manifest).unwrap();
    owner.publish_generation(&key, &manifest).unwrap();
    assert_eq!(owner.charged.get().generations, manifest.len() as u64);
    assert_eq!(
        owner.verify_usage().unwrap().total().unwrap(),
        object.bytes + manifest.len() as u64
    );
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn corrupted_existing_object_or_manifest_is_never_replaced() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources.reserve_derived_cache(peak(10)).unwrap();
    let key = storage::sha256(b"good");
    let path = owner.paths.object(&key).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"evil").unwrap();
    let mut writer = owner.begin_object(4).unwrap();
    writer.write_bytes(b"good").unwrap();
    assert_eq!(
        writer.commit().unwrap_err().code,
        "vault.data_cache_integrity"
    );
    assert_eq!(fs::read(&path).unwrap(), b"evil");
    let key = storage::sha256(b"generation");
    owner.publish_generation(&key, b"original").unwrap();
    assert_eq!(
        owner
            .publish_generation(&key, b"different")
            .unwrap_err()
            .code,
        "vault.data_cache_integrity"
    );
    assert_eq!(
        fs::read(owner.paths.generations.join(format!("{key}.json"))).unwrap(),
        b"original"
    );
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn size_charge_precedes_write_and_error_survives_codec_adapter() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources.reserve_derived_cache(peak(3)).unwrap();
    assert!(owner.begin_object(4).is_err());
    let mut writer = owner.begin_object(3).unwrap();
    writer.write_all(b"one").unwrap();
    assert!(writer.write_all(b"x").is_err());
    assert_eq!(
        writer.error().unwrap().code,
        "vault.data_cache_reservation_exceeded"
    );
    assert_eq!(writer.len(), 3);
    assert_eq!(
        writer.commit().unwrap_err().code,
        "vault.data_cache_reservation_exceeded"
    );
    assert_eq!(owner.charged.get().staging, 0);
    assert_eq!(owner.verify_usage().unwrap().total().unwrap(), 0);
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn concurrent_private_writers_share_one_reserved_staging_allowance() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources.reserve_derived_cache(peak(5)).unwrap();
    let mut first = owner.begin_object(5).unwrap();
    let mut second = owner.begin_object(5).unwrap();
    first.write_bytes(b"four").unwrap();
    assert_eq!(
        second.write_bytes(b"no").unwrap_err().code,
        "vault.data_cache_reservation_exceeded"
    );
    assert_eq!(
        second.abort(Ok(())).unwrap_err().code,
        "vault.data_cache_reservation_exceeded"
    );
    first.abort(Ok(())).unwrap();
    assert_eq!(owner.charged.get().staging, 0);
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn cancellation_discards_only_its_private_spool_and_preserves_unknown_neighbor() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources.reserve_derived_cache(peak(16)).unwrap();
    let foreign = owner.paths.staging.join("unowned-neighbor");
    fs::write(&foreign, b"protected").unwrap();
    let mut writer = owner.begin_object(16).unwrap();
    writer.write_bytes(b"partial").unwrap();
    control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(writer.commit().unwrap_err().code, "command.cancelled");
    assert_eq!(fs::read_dir(&owner.paths.staging).unwrap().count(), 1);
    assert_eq!(fs::read(foreign).unwrap(), b"protected");
    assert_eq!(owner.finish(Ok(())).unwrap_err().code, "command.cancelled");
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}
#[test]
fn late_failure_retains_published_object_identity_without_publishing_generation() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources.reserve_derived_cache(peak(4)).unwrap();
    let mut writer = owner.begin_object(4).unwrap();
    writer.write_bytes(b"body").unwrap();
    let object = writer.commit().unwrap();
    let path = owner.paths.object(&object.sha256).unwrap();
    control.cancel_handle().store(true, Ordering::Release);
    assert_eq!(
        owner
            .publish_generation(&storage::sha256(b"generation"), b"manifest")
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    let error = owner.finish(Ok(())).unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(
        error.details["publicationReceipts"][0]["sha256"],
        object.sha256
    );
    assert_eq!(
        error.details["publicationReceipts"][0]["state"],
        "Published"
    );
    assert_eq!(fs::read(path).unwrap(), b"body");
    assert_eq!(
        fs::read_dir(DataCachePaths::new(&f.0).generations)
            .unwrap()
            .count(),
        0
    );
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}
#[test]
fn zero_byte_object_and_generation_bounds_preserve_identity() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources
        .reserve_derived_cache(AdditionalPeak::default())
        .unwrap();
    let object = owner.begin_object(0).unwrap().commit().unwrap();
    assert_eq!(
        object,
        DerivedObject {
            sha256: storage::sha256(b""),
            bytes: 0
        }
    );
    assert_eq!(
        owner
            .publish_generation(&storage::sha256(b"id"), b"a")
            .unwrap_err()
            .code,
        "vault.data_cache_reservation_exceeded"
    );
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}

#[test]
fn private_cleanup_failure_reports_exact_candidate_and_does_not_refund_uncertain_bytes() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let owner = resources.reserve_derived_cache(peak(4)).unwrap();
    let mut writer = owner.begin_object(4).unwrap();
    writer.write_bytes(b"body").unwrap();
    let expected = owner
        .paths
        .staging
        .join(writer.stage.as_ref().unwrap().name());
    let mut calls = 0;
    let error = writer
        .abort_with::<()>(
            Err(ToolkitError::new("test.operation", "first error", 4)),
            |stage| {
                calls += 1;
                assert!(stage.close().is_empty());
                vec![std::io::Error::other("injected post-close uncertainty")]
            },
        )
        .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.code, "test.operation");
    assert_eq!(error.details["temporary"], serde_json::json!(expected));
    assert_eq!(error.details["cleanupComplete"], false);
    assert_eq!(owner.charged.get().staging, 4);
    let error = owner.finish::<()>(Err(error)).unwrap_err();
    assert_eq!(error.details["temporary"], serde_json::json!(expected));
    resources.close().unwrap();
}
#[test]
fn uncommitted_stage_cleanup_error_keeps_path_and_exact_publication_state() {
    let path = PathBuf::from("managed/objects/shard/body");
    let error = publication_result::<()>(
        Err(ToolkitError::new("test.close", "close failed", 5)),
        StagedPublication::NotPublished,
        path.clone(),
        "owned-private-stage".into(),
        true,
    )
    .unwrap_err();
    assert_eq!(error.code, "test.close");
    assert_eq!(error.details["path"], serde_json::json!(path));
    let temporary: PathBuf = serde_json::from_value(error.details["temporary"].clone()).unwrap();
    assert_eq!(
        temporary,
        PathBuf::from("managed/objects/shard/owned-private-stage")
    );
    assert_eq!(error.details["state"], "NotPublished");
    assert_eq!(error.details["cleanupComplete"], false);
}

#[test]
fn independent_budget_stops_inside_multichunk_private_write_and_keeps_first_error() {
    use std::cell::{Cell, RefCell};
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let armed = Cell::new(false);
    let path = RefCell::new(None::<PathBuf>);
    let check = || {
        if armed.get()
            && path
                .borrow()
                .as_ref()
                .is_some_and(|path| fs::metadata(path).is_ok_and(|m| m.len() >= 65536))
        {
            return Err(ToolkitError::new(
                "test.query_budget",
                "independent query budget stopped",
                3,
            ));
        }
        Ok(())
    };
    let owner = resources
        .reserve_derived_cache_with_check(peak(131073), &check)
        .unwrap();
    let mut writer = owner.begin_object(131073).unwrap();
    *path.borrow_mut() = Some(
        owner
            .paths
            .staging
            .join(writer.stage.as_ref().unwrap().name()),
    );
    armed.set(true);
    let error = writer.write_bytes(&vec![9; 131073]).unwrap_err();
    assert_eq!(error.code, "test.query_budget");
    assert_eq!(
        fs::metadata(path.borrow().as_ref().unwrap()).unwrap().len(),
        65536
    );
    assert_eq!(
        writer.abort::<()>(Err(error)).unwrap_err().code,
        "test.query_budget"
    );
    assert_eq!(owner.verify_usage().unwrap().total().unwrap(), 0);
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn independent_budget_stops_inside_final_object_copy_before_visibility() {
    use std::cell::Cell;
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let armed = Cell::new(false);
    OBJECT_COPIED_BYTES.with(|progress| progress.set(0));
    let check = || {
        // Directory-entry metadata can also observe an already-published final
        // object. Only actual writes to this private final stage trigger here.
        if armed.get() && OBJECT_COPIED_BYTES.with(Cell::get) >= 65536 {
            return Err(ToolkitError::new(
                "test.copy_budget",
                "stop within final copy",
                3,
            ));
        }
        Ok(())
    };
    let owner = resources
        .reserve_derived_cache_with_check(peak(131073), &check)
        .unwrap();
    let mut writer = owner.begin_object(131073).unwrap();
    let raw = vec![7; 131073];
    writer.write_bytes(&raw).unwrap();
    armed.set(true);
    assert_eq!(writer.commit().unwrap_err().code, "test.copy_budget");
    assert_eq!(OBJECT_COPIED_BYTES.with(Cell::get), 65536);
    assert!(owner.publication_receipts.borrow().is_empty());
    assert!(!owner.paths.object(&storage::sha256(&raw)).unwrap().exists());
    assert_eq!(owner.charged.get().objects, 0);
    assert_eq!(owner.charged.get().staging, 0);
    armed.set(false);
    assert_eq!(owner.verify_usage().unwrap().total().unwrap(), 0);
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
#[test]
fn independent_budget_after_final_visibility_preserves_published_body_and_receipt() {
    use std::cell::Cell;
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let raw = vec![7; 131073];
    let digest = storage::sha256(&raw);
    let path = resources.data_cache_paths().object(&digest).unwrap();
    let armed = Cell::new(false);
    let check = || {
        if armed.get() && path.try_exists()? {
            return Err(ToolkitError::new(
                "test.after_visibility",
                "stop after final object is visible",
                3,
            ));
        }
        Ok(())
    };
    let owner = resources
        .reserve_derived_cache_with_check(peak(raw.len() as u64), &check)
        .unwrap();
    let mut writer = owner.begin_object(raw.len() as u64).unwrap();
    writer.write_bytes(&raw).unwrap();
    armed.set(true);
    let error = writer.commit().unwrap_err();
    assert_eq!(error.code, "test.after_visibility");
    assert_eq!(error.details["cause"]["code"], "test.after_visibility");
    assert_eq!(
        error.details["publicationReceipts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(error.details["publicationReceipts"][0]["kind"], "object");
    let published_path: PathBuf =
        serde_json::from_value(error.details["publicationReceipts"][0]["path"].clone()).unwrap();
    assert_eq!(published_path, path);
    assert_eq!(
        error.details["publicationReceipts"][0]["state"],
        "Published"
    );
    assert_eq!(error.details["publicationReceipts"][0]["sha256"], digest);
    assert_eq!(error.details["publicationReceipts"][0]["bytes"], raw.len());
    assert_eq!(fs::read(&path).unwrap(), raw);
    assert_eq!(owner.charged.get().objects, raw.len() as u64);
    assert_eq!(owner.charged.get().staging, 0);
    armed.set(false);
    assert_eq!(
        owner.verify_usage().unwrap().total().unwrap(),
        raw.len() as u64
    );
    let error = owner.finish::<()>(Err(error)).unwrap_err();
    assert_eq!(error.code, "test.after_visibility");
    assert_eq!(
        error.details["publicationReceipts"][0]["state"],
        "Published"
    );
    assert_eq!(fs::read(&path).unwrap(), raw);
    resources.close().unwrap();
}
#[test]
fn both_terminal_scopes_survive_after_consuming_all_disk_handles() {
    use std::cell::Cell;
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let stopped = Cell::new(false);
    let check = || {
        if stopped.get() {
            Err(ToolkitError::new("test.independent", "independent stop", 3))
        } else {
            Ok(())
        }
    };
    let owner = resources
        .reserve_derived_cache_with_check(peak(1), &check)
        .unwrap();
    control.cancel_handle().store(true, Ordering::Release);
    stopped.set(true);
    let error = owner
        .finish::<()>(Err(ToolkitError::new(
            "test.operation",
            "operation failed",
            4,
        )))
        .unwrap_err();
    assert_eq!(error.code, "test.operation");
    let causes = error.details["causes"].as_array().unwrap();
    assert_eq!(causes.len(), 3);
    assert_eq!(causes[1]["code"], "command.cancelled");
    assert_eq!(causes[2]["code"], "test.independent");
    ExclusiveLock::resource(&f.0.join("locks"), QUOTA_RESOURCE)
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(resources.close().unwrap_err().code, "command.cancelled");
}

#[test]
fn independent_budget_interrupts_manifest_hash_before_any_stage_or_charge() {
    use std::cell::Cell;
    let f = Fixture::new();
    let control = OperationControl::default();
    let resources = acquire(&f.0, Policy::Ordinary, &control).unwrap();
    let armed = Cell::new(false);
    MANIFEST_HASHED_BYTES.with(|progress| progress.set(0));
    let check = || {
        if armed.get() && MANIFEST_HASHED_BYTES.with(Cell::get) >= 65536 {
            return Err(ToolkitError::new(
                "test.manifest_budget",
                "stop inside manifest hash",
                3,
            ));
        }
        Ok(())
    };
    let owner = resources
        .reserve_derived_cache_with_check(
            AdditionalPeak {
                generations: 131073,
                ..AdditionalPeak::default()
            },
            &check,
        )
        .unwrap();
    armed.set(true);
    assert_eq!(
        owner
            .publish_generation(&storage::sha256(b"manifest"), &vec![1; 131073])
            .unwrap_err()
            .code,
        "test.manifest_budget"
    );
    assert_eq!(MANIFEST_HASHED_BYTES.with(Cell::get), 65536);
    assert_eq!(fs::read_dir(&owner.paths.generations).unwrap().count(), 0);
    assert_eq!(owner.charged.get().generations, 0);
    armed.set(false);
    owner.finish(Ok(())).unwrap();
    resources.close().unwrap();
}
