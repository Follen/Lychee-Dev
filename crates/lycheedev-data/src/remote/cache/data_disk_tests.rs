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
fn real_configuration_download_obeys_quota_and_closes_publisher_before_private_alias() {
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
    // A private SQLite schema and rollback-journal peak now join the Data
    // budget. The tiny first limit proves blob-stage rejection, not room for
    // metadata. Use an explicit sufficient budget for real publication.
    f.limit(64 << 20);
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let store = Store {
        home: &f.home,
        control: &control,
        budget: None,
    };
    // This real download publishes the shared blob, then publishes the alias.
    // Both stages use the same quota resource, so retaining either owner
    // across the next stage would exhaust this original deadline.
    let (parsed, reference) = store.configuration(&route, &digest, false, &fetch).unwrap();
    assert_eq!(parsed.raw, raw);
    f.limit(1);
    let (cached, reused) = f
        .store()
        .configuration(&route, &digest, true, &fetch)
        .unwrap();
    assert_eq!(cached.raw, raw);
    assert_eq!(reused.sha256, reference.sha256);
    assert_eq!(fetch.calls.get(), 2);
    assert!(f.home.join("state/data/cache-v1.sqlite").is_file());
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.store()
        .lease("data-derived-v1:quota")
        .unwrap()
        .close()
        .unwrap();
    assert_eq!(fs::read_dir(f.receipts()).unwrap().count(), 2);
    assert_eq!(
        fs::read(evidence::blob_path(&f.home, &reference.sha256).unwrap()).unwrap(),
        raw
    );
}
#[test]
fn existing_store_body_is_deduplicated_and_still_readable_after_budget_shrink() {
    // Intent + completion + durable shard declaration + two possible bodies.
    let f = Fixture::new(48 + 32 + 128 + 2 * 4);
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

#[test]
fn private_store_does_not_import_core_aliases_or_wait_on_core_metadata_owners() {
    use lycheedev_core::metadata::Metadata;
    let f = Fixture::new(64 << 20);
    let key = "remote-config/isolated";
    let mut core = Metadata::open(&f.home, true).unwrap();
    core.create(key, &false).unwrap();
    core.create("core-test/held-owner", &true).unwrap();
    // A legacy Core alias cannot become a private cache hit or supply its CAS
    // generation. The immutable shared blob publisher remains separate.
    assert!(f.store().read::<bool>(key).unwrap().is_none());
    assert_eq!(f.store().generation(key).unwrap(), None);
    assert!(!f.home.join("state/data/cache-v1.sqlite").exists());
    let schema = f.store().lease("metadata-schema").unwrap();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let store = Store {
        home: &f.home,
        control: &control,
        budget: None,
    };
    // The callback owns a real Core BEGIN IMMEDIATE transaction while the
    // schema lease is also held. Neither may serialize the private database.
    core.consume::<bool, ()>("core-test/held-owner", |held| {
        assert!(held);
        store.put(key, &true, None)?;
        assert!(store.read::<bool>(key)?.unwrap().0);
        Ok(())
    })
    .unwrap();
    schema.close().unwrap();
    assert!(!core.read::<bool>(key).unwrap());
    assert!(core.list::<Value>("capture/", 10).unwrap().is_empty());
    core.close().unwrap();
    assert!(f.home.join("state/data/cache-v1.sqlite").is_file());
    assert!(f.home.join("state/toolkit.sqlite").is_file());
    f.store()
        .lease("data-derived-v1:quota")
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn core_capture_shares_catalog_serialization_but_not_private_data_capacity() {
    use lycheedev_core::metadata::Metadata;
    let f = Fixture::new(1);
    let quota = f.store().lease("data-derived-v1:quota").unwrap();
    let bytes = b"core capture remains independent";
    let home = f.home.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
        ready_tx.send(()).unwrap();
        let result = evidence::commit_capture_bounded(
            &home,
            evidence::CaptureInput {
                bytes,
                maximum_bytes: bytes.len(),
                media_type: "text/plain",
                provenance: serde_json::json!({"kind":"fixture", "locator":"core-cap-isolation"}),
                complete: true,
                truncated: false,
            },
            &control,
        );
        done_tx.send(result).unwrap();
    });
    ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    // All evidence domains acquire quota before the common catalog gate, even
    // when their own body and metadata are not charged to Data capacity.
    assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    quota.close().unwrap();
    let capture = done_rx
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
        .unwrap();
    worker.join().unwrap();
    let id = capture["id"].as_str().unwrap();
    let core = Metadata::open(&f.home, false).unwrap();
    let persisted: Value = core.read(&format!("capture/{id}")).unwrap();
    assert_eq!(persisted, capture);
    assert!(core.list::<Value>("remote-config/", 10).unwrap().is_empty());
    core.close().unwrap();
    assert!(!f.home.join("state/data/cache-v1.sqlite").exists());
    assert!(!f.home.join("state/data/evidence-v2.sqlite").exists());
    assert_eq!(
        f.store()
            .put("remote-config/no-room", &true, None)
            .unwrap_err()
            .code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(evidence::read_capture(&f.home, id).unwrap(), capture);
    assert_eq!(
        f.store().read_blob(&capture["blob"], bytes.len()).unwrap(),
        bytes
    );
}

#[test]
fn private_store_reopens_below_reduced_capacity_but_rejects_new_growth() {
    let f = Fixture::new(64 << 20);
    let key = "remote-config/retained";
    let reference = f.store().publish_blob(b"body", 4).unwrap();
    f.store().put(key, &reference, None).unwrap();
    let generation = f.store().generation(key).unwrap().unwrap();
    let private_db = f.home.join("state/data/cache-v1.sqlite");
    let bytes_before = fs::metadata(&private_db).unwrap().len();
    assert!(bytes_before > 1);
    f.limit(1);
    // Each Store entry opens and consumes its own private database owner.
    // Existing authenticated documents and bodies remain usable below quota.
    let (retained, observed_generation) = f.store().read::<BlobRef>(key).unwrap().unwrap();
    assert_eq!(observed_generation, generation);
    assert_eq!(retained.sha256, reference.sha256);
    assert_eq!(f.store().read_blob(&retained, 4).unwrap(), b"body");
    assert_eq!(
        f.store()
            .put("remote-config/new-growth", &"new", None)
            .unwrap_err()
            .code,
        "vault.data_cache_budget_exceeded"
    );
    assert_eq!(f.store().generation(key).unwrap(), Some(generation));
    assert!(
        f.store()
            .read::<String>("remote-config/new-growth")
            .unwrap()
            .is_none()
    );
    assert_eq!(fs::metadata(&private_db).unwrap().len(), bytes_before);
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.store()
        .lease("data-derived-v1:quota")
        .unwrap()
        .close()
        .unwrap();
    // Rejected growth must not poison the next writable owner.
    f.limit(64 << 20);
    f.store()
        .put("remote-config/new-growth", &"new", None)
        .unwrap();
    assert_eq!(
        f.store()
            .read::<String>("remote-config/new-growth")
            .unwrap()
            .unwrap()
            .0,
        "new"
    );
}

#[test]
fn private_store_keeps_strict_absent_and_observed_generation_cas() {
    let f = Fixture::new(64 << 20);
    let key = "remote-config/cas";
    f.store().put(key, &"first", None).unwrap();
    let initial = f.store().generation(key).unwrap().unwrap();
    assert_eq!(
        f.store().put(key, &"unobserved", None).unwrap_err().code,
        "vault.generation_conflict"
    );
    f.store().put(key, &"second", Some(initial)).unwrap();
    let (value, current) = f.store().read::<String>(key).unwrap().unwrap();
    assert_eq!(value, "second");
    assert!(current > initial);
    assert_eq!(
        f.store()
            .put(key, &"stale", Some(initial))
            .unwrap_err()
            .code,
        "vault.generation_conflict"
    );
    assert_eq!(f.store().read::<String>(key).unwrap().unwrap().0, "second");
    f.store()
        .lease("data-derived-v1:quota")
        .unwrap()
        .close()
        .unwrap();
}

#[test]
fn corrupt_private_metadata_is_terminal_before_configuration_network_fallback() {
    let f = Fixture::new(64 << 20);
    let raw = b"archives = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n".to_vec();
    let digest = crate::casc::hex(&Md5::digest(&raw));
    let route = Distribution {
        path: "tpr/wow".into(),
        hosts: vec!["cdn.example.test".into()],
    };
    let reference = f.store().publish_blob(&raw, raw.len()).unwrap();
    f.store()
        .put(&format!("remote-config/{digest}"), &reference, None)
        .unwrap();
    fs::write(
        f.home.join("state/data/cache-v1.sqlite"),
        b"not a SQLite database",
    )
    .unwrap();
    let fetch = Fetch {
        bytes: raw,
        calls: Cell::new(0),
    };
    let error = f
        .store()
        .configuration(&route, &digest, false, &fetch)
        .unwrap_err();
    assert!(!pure_expected(&error, "vault.record_missing"));
    assert_eq!(fetch.calls.get(), 0);
    assert!(!f.home.join("state/toolkit.sqlite").exists());
    f.store()
        .lease("data-derived-v1:quota")
        .unwrap()
        .close()
        .unwrap();
}
