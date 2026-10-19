use super::*;
use crate::resource::Cost;
use md5::{Digest, Md5};
use std::{
    cell::Cell,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    home: PathBuf,
    control: OperationControl,
}
impl Fixture {
    fn new(limit: u64) -> Self {
        let home = std::env::temp_dir().join(format!(
            "lychee-store-disk-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let f = Self {
            home,
            control: OperationControl::default(),
        };
        f.limit(limit);
        f
    }
    fn limit(&self, limit: u64) {
        let mut config = lycheedev_core::workspace::default_config();
        config["dataCache"]["maxBytes"] = serde_json::json!(limit);
        storage::atomic_json(&self.home.join("config.json"), &config).unwrap();
    }
    fn store(&self) -> Store<'_> {
        Store {
            home: &self.home,
            control: &self.control,
            budget: None,
        }
    }
    fn receipts(&self) -> PathBuf {
        lycheedev_core::workspace::data_disk::DataCachePaths::new(&self.home).shared_blobs()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.home).unwrap();
        }
    }
}
struct Fetch {
    bytes: Vec<u8>,
    calls: Cell<usize>,
}
impl MetadataFetcher for Fetch {
    fn fetch(
        &self,
        _: &str,
        maximum: usize,
        control: &OperationControl,
        budget: Option<&Budget>,
    ) -> Result<Vec<u8>> {
        control.check()?;
        if let Some(budget) = budget {
            budget.check()?;
        }
        assert!(self.bytes.len() <= maximum);
        self.calls.set(self.calls.get() + 1);
        Ok(self.bytes.clone())
    }
}
#[test]
fn real_configuration_download_obeys_shared_and_derived_quota_before_alias_commit() {
    let raw = b"archives = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".to_vec();
    let digest = crate::casc::hex(&Md5::digest(&raw));
    let route = Distribution {
        path: "tpr/wow".into(),
        hosts: vec!["cdn.example.test".into()],
    };
    let peak = raw.len() as u64 * 2 + 80;
    let f = Fixture::new(peak - 1);
    let fetch = Fetch {
        bytes: raw.clone(),
        calls: Cell::new(0),
    };
    assert_eq!(
        f.store()
            .configuration(&route, &digest, false, &fetch)
            .unwrap_err()
            .code,
        "vault.data_cache_budget_exceeded"
    );
    assert!(
        f.store()
            .read::<CachedBlob>(&format!("remote-config/{digest}"))
            .unwrap()
            .is_none()
    );
    f.limit(peak);
    let (parsed, reference) = f
        .store()
        .configuration(&route, &digest, false, &fetch)
        .unwrap();
    assert_eq!(parsed.raw, raw);
    let (cached, reused) = f
        .store()
        .configuration(&route, &digest, true, &fetch)
        .unwrap();
    assert_eq!(cached.raw, raw);
    assert_eq!(reused.sha256, reference.sha256);
    assert_eq!(fetch.calls.get(), 2);
    assert_eq!(fs::read_dir(f.receipts()).unwrap().count(), 2);
    assert_eq!(
        fs::read(evidence::blob_path(&f.home, &reference.sha256).unwrap()).unwrap(),
        raw
    );
}
#[test]
fn existing_store_body_is_deduplicated_and_still_readable_after_budget_shrink() {
    let f = Fixture::new(88);
    let original = f.store().publish_blob(b"body", 4).unwrap();
    f.limit(1);
    let duplicate = f.store().publish_blob(b"body", 4).unwrap();
    assert_eq!(original.sha256, duplicate.sha256);
    assert_eq!(
        f.store().publish_blob(b"next", 4).unwrap_err().code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(f.store().read_blob(&original, 4).unwrap(), b"body");
    assert_eq!(fs::read_dir(f.receipts()).unwrap().count(), 2);
}
#[test]
fn original_independent_budget_stops_store_before_disk_intent() {
    let f = Fixture::new(88);
    let budget = Budget::new(Cost::defaults(), Instant::now(), None);
    let store = Store {
        home: &f.home,
        control: &f.control,
        budget: Some(&budget),
    };
    let error = store.publish_blob(b"body", 4).unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(!f.receipts().exists());
    assert!(f.control.check().is_ok());
}
