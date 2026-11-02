use super::*;
use crate::control::TrackedCancellation;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lychee-source-cache-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn derived_publisher_matches_existing_blob_identity_and_roundtrips() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let raw = vec![b'x'; 131073];
    let old = publish(&f.0, &raw, raw.len(), None).unwrap();
    let current = publish_derived_controlled(&f.0, &raw, raw.len(), &control).unwrap();
    assert_eq!(old, current);
    assert_eq!(
        read_object(&f.0, &current, raw.len(), &control).unwrap(),
        raw
    );
    let empty = publish_derived_controlled(&f.0, b"", 0, &control).unwrap();
    assert_eq!(read_object(&f.0, &empty, 0, &control).unwrap(), b"");
}
#[test]
fn derived_publisher_rejects_corruption_without_overwrite() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let raw = b"original";
    let blob = publish_derived_controlled(&f.0, raw, 100, &control).unwrap();
    let hash = blob["sha256"].as_str().unwrap();
    let path = f.0.join("cache/objects").join(&hash[..2]).join(&hash[2..]);
    fs::write(&path, b"changed").unwrap();
    let error = publish_derived_controlled(&f.0, raw, 100, &control).unwrap_err();
    assert_eq!(error.code, "vault.cache_integrity");
    assert_eq!(fs::read(path).unwrap(), b"changed");
}
#[test]
fn full_cache_never_prunes_to_make_room() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let mut config = workspace::default_config();
    config["cache"]["maxBytes"] = json!(4);
    storage::atomic_json(&f.0.join("config.json"), &config).unwrap();
    let first = publish_derived_controlled(&f.0, b"four", 4, &control).unwrap();
    assert_eq!(
        publish_derived_controlled(&f.0, b"x", 4, &control)
            .unwrap_err()
            .code,
        "vault.cache_budget_exceeded"
    );
    assert_eq!(read_object(&f.0, &first, 4, &control).unwrap(), b"four");
    assert_eq!(
        publish_derived_controlled(&f.0, b"four", 4, &control).unwrap(),
        first
    );
}
#[test]
fn original_cancel_precedes_missing_workspace_or_cache_reads() {
    let f = Fixture::new();
    let token = TrackedCancellation::new();
    let control = OperationControl::tracked(Instant::now() + Duration::from_secs(5), token.clone());
    token.cancel();
    assert_eq!(
        publish_derived_controlled(&f.0.join("absent"), b"x", 1, &control)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert!(!f.0.join("absent").exists());
    assert_eq!(
        read_object(&f.0, &json!({}), 1, &control).unwrap_err().code,
        "command.cancelled"
    );
}
#[test]
fn quota_lock_contention_remains_immediate_and_does_not_publish() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let lock = ExclusiveLock::resource(&f.0.join("locks"), "cache:quota").unwrap();
    assert_eq!(
        publish_derived_controlled(&f.0, b"x", 1, &control)
            .unwrap_err()
            .code,
        "workspace.busy"
    );
    lock.close().unwrap();
    assert_eq!(object_bytes(&f.0, &control).unwrap(), 0);
}
#[test]
fn malformed_configuration_remains_first_failure_before_quota_scan() {
    let f = Fixture::new();
    let control = OperationControl::default();
    storage::atomic_json(&f.0.join("config.json"), &json!({"schema":"newer"})).unwrap();
    assert_eq!(
        publish_derived_controlled(&f.0, b"x", 1, &control)
            .unwrap_err()
            .code,
        "vault.config_schema_newer"
    );
    assert_eq!(object_bytes(&f.0, &control).unwrap(), 0);
}
