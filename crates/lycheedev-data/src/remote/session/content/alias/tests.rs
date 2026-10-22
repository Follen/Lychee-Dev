use super::*;
use lycheedev_core::{
    control::OperationControl,
    metadata::{Metadata, OwnedMutation},
    storage,
};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
struct Fixture {
    home: PathBuf,
    control: OperationControl,
    budget: Arc<Budget>,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../.tmp/decoded-alias-fixtures")
            .join(format!(
                "{}-{}-{}",
                std::process::id(),
                storage::utc_now().replace([':', '.'], "-"),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        lycheedev_core::workspace::initialize(&home).unwrap();
        let control = OperationControl::default();
        let budget = Arc::new(Budget::controlled(
            Cost::defaults(),
            control.deadline(),
            control.clone(),
        ));
        Self {
            home,
            control,
            budget,
        }
    }
    fn store(&self) -> Store<'_> {
        Store {
            home: &self.home,
            control: &self.control,
            budget: Some(&self.budget),
        }
    }
    fn keys(&self) -> keys::Context {
        keys::Context::prepare_scoped(
            &self.home,
            None,
            true,
            &self.control,
            self.budget.deadline,
            Some(self.budget.clone()),
        )
        .unwrap()
    }
    fn body(&self, bytes: &[u8]) -> (EncodingRecord, evidence::BlobRef) {
        let record = EncodingRecord {
            content_key: casc::hex(&Md5::digest(bytes)),
            decoded_bytes: bytes.len() as u64,
            encoding_keys: vec!["a".repeat(32)],
            page: 0,
        };
        (
            record,
            self.store()
                .publish_blob(bytes, bytes.len().max(1))
                .unwrap(),
        )
    }
    fn entry(
        &self,
        key: &str,
        record: &EncodingRecord,
        reference: &evidence::BlobRef,
        keys: &keys::Context,
    ) {
        self.store()
            .put(
                key,
                &Entry {
                    content_key: record.content_key.clone(),
                    blob: BlobRef::try_from(reference.clone()).unwrap(),
                    keys: Source::from(&keys.source),
                },
                None,
            )
            .unwrap();
    }
    fn raw(&self, key: &str, raw: &[u8]) {
        // Keep exact duplicate/case/null/deep wire bytes through the existing
        // RawValue serialization seam, now in the private Store database.
        let raw =
            serde_json::value::RawValue::from_string(String::from_utf8(raw.to_vec()).unwrap())
                .unwrap();
        self.store().put(key, &raw, None).unwrap();
    }
}
#[test]
fn verified_body_memo_shares_actual_owner_and_new_request_rechecks_missing_blob() {
    let f = Fixture::new();
    let mut keys = f.keys();
    let (record, blob) = f.body(b"verified body");
    f.entry("decoded-v1/decoded-hit", &record, &blob, &keys);
    let before = f.budget.snapshot().unwrap().retained_bytes;
    let mut stats = Stats::default();
    let mut memo = Memo::default();
    let (_, first) = memo
        .read(
            &f.store(),
            Some("decoded-v1/decoded-hit"),
            &record,
            &mut keys,
            &mut stats,
            &f.budget,
        )
        .unwrap()
        .unwrap();
    let charged = f.budget.snapshot().unwrap().retained_bytes;
    assert!(charged >= before + blob.bytes);
    std::fs::remove_file(evidence::blob_path(&f.home, &blob.sha256).unwrap()).unwrap();
    let (_, second) = memo
        .read(
            &f.store(),
            Some("decoded-v1/decoded-hit"),
            &record,
            &mut keys,
            &mut stats,
            &f.budget,
        )
        .unwrap()
        .unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(stats.blob_reads, 1);
    assert_eq!(stats.blob_read_bytes, blob.bytes);
    let mut other = Memo::default();
    assert!(
        other
            .read(
                &f.store(),
                Some("decoded-v1/decoded-hit"),
                &record,
                &mut keys,
                &mut stats,
                &f.budget
            )
            .unwrap()
            .is_none()
    );
    drop(other);
    drop(memo);
    drop(first);
    drop(second);
    assert_eq!(f.budget.snapshot().unwrap().retained_bytes, before);
    assert_eq!(f.budget.snapshot().unwrap().network_bytes, 0);
    drop(keys);
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn decoded_blob_sha_and_whole_ckey_are_both_mandatory_without_fallback() {
    for corrupt_disk in [true, false] {
        let f = Fixture::new();
        let mut keys = f.keys();
        let (mut record, blob) = f.body(b"first body");
        if corrupt_disk {
            std::fs::write(
                evidence::blob_path(&f.home, &blob.sha256).unwrap(),
                b"other body",
            )
            .unwrap();
        } else {
            record.content_key = casc::hex(&Md5::digest(b"other body"));
        }
        f.entry("decoded-v1/decoded-corrupt", &record, &blob, &keys);
        let before = f.budget.snapshot().unwrap().retained_bytes;
        let mut memo = Memo::default();
        let mut stats = Stats::default();
        let error = memo
            .read(
                &f.store(),
                Some("decoded-v1/decoded-corrupt"),
                &record,
                &mut keys,
                &mut stats,
                &f.budget,
            )
            .err()
            .unwrap();
        assert_eq!(error.code, "vault.blob_integrity");
        assert_eq!(stats.content_decode_calls, 0);
        assert_eq!(stats.blob_reads, if corrupt_disk { 0 } else { 1 });
        drop(memo);
        assert_eq!(f.budget.snapshot().unwrap().retained_bytes, before);
        drop(keys);
        std::fs::remove_dir_all(f.home).unwrap();
    }
}
#[test]
fn loaded_public_alias_reestablishes_availability_before_blob_read() {
    let f = Fixture::new();
    let mut keys = f.keys();
    let (record, blob) = f.body(b"not secret");
    let mut source = Source::from(&keys.source);
    source.state = "loaded".into();
    f.store()
        .put(
            "decoded-v1/decoded-loaded",
            &Entry {
                content_key: record.content_key.clone(),
                blob: BlobRef::try_from(blob).unwrap(),
                keys: source,
            },
            None,
        )
        .unwrap();
    let mut stats = Stats::default();
    let mut memo = Memo::default();
    assert!(
        memo.read(
            &f.store(),
            Some("decoded-v1/decoded-loaded"),
            &record,
            &mut keys,
            &mut stats,
            &f.budget
        )
        .unwrap()
        .is_none()
    );
    assert_eq!(keys.source.state, "unavailable_offline");
    assert_eq!(stats.blob_reads, 0);
    drop(memo);
    drop(keys);
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn save_existing_alias_checks_only_existence_and_conflict_revalidates_pure_error() {
    let f = Fixture::new();
    let mut keys = f.keys();
    let (record, blob) = f.body(b"cached body");
    f.raw(
        "decoded-v1/already-present",
        br#"{"blob":{"bytes":"invalid shape"}}"#,
    );
    let mut memo = Memo::default();
    let mut stats = Stats::default();
    memo.save(
        &f.store(),
        Some("decoded-v1/already-present"),
        &record,
        &blob,
        &mut keys,
        &mut stats,
        &f.budget,
    )
    .unwrap();
    assert_eq!(
        read_entry(&f.store(), "decoded-v1/already-present")
            .err()
            .unwrap()
            .code,
        "vault.blob_integrity"
    );
    f.entry("decoded-v1/raced", &record, &blob, &keys);
    let conflict = fault(
        "vault.generation_conflict",
        4,
        "injected result of concurrent create",
    );
    memo.after_create(
        &f.store(),
        "decoded-v1/raced",
        &record,
        &mut keys,
        &mut stats,
        &f.budget,
        Err(conflict.clone()),
    )
    .unwrap();
    let composite = conflict.with_details(
        serde_json::json!({"cleanup":{"code":"test.close"},"budget":{"code":"command.cancelled"}}),
    );
    let error = memo
        .after_create(
            &f.store(),
            "decoded-v1/raced",
            &record,
            &mut keys,
            &mut stats,
            &f.budget,
            Err(composite.clone()),
        )
        .unwrap_err();
    assert_eq!(error.details, composite.details);
    drop(memo);
    drop(keys);
    std::fs::remove_dir_all(f.home).unwrap();
}

#[test]
fn missing_alias_lookup_keeps_exact_original_reference_for_rebuild() {
    let f = Fixture::new();
    let mut keys = f.keys();
    let (record, blob) = f.body(b"verified body");
    f.entry("decoded-v1/decoded-rebuild", &record, &blob, &keys);
    std::fs::remove_file(evidence::blob_path(&f.home, &blob.sha256).unwrap()).unwrap();
    let found = Memo::default()
        .lookup(
            &f.store(),
            Some("decoded-v1/decoded-rebuild"),
            &record,
            &mut keys,
            &mut Stats::default(),
            &f.budget,
        )
        .unwrap();
    match found {
        Lookup::Missing(original) => {
            assert_eq!(original.sha256, blob.sha256);
            assert_eq!(original.bytes, blob.bytes);
        }
        _ => panic!("exact missing alias was discarded"),
    }
    validate_rebuild(
        &f.store(),
        Some("decoded-v1/decoded-rebuild"),
        &record,
        &keys,
        &blob,
    )
    .unwrap();
    std::fs::remove_dir_all(f.home).unwrap();
}
#[test]
fn rebuild_rechecks_alias_digest_length_ckey_and_key_source_before_publication() {
    for changed in ["digest", "length", "ckey", "keys"] {
        let f = Fixture::new();
        let keys = f.keys();
        let (record, blob) = f.body(b"verified body");
        f.entry("decoded-v1/decoded-rebuild", &record, &blob, &keys);
        let mut entry = Entry {
            content_key: record.content_key.clone(),
            blob: BlobRef::try_from(blob.clone()).unwrap(),
            keys: Source::from(&keys.source),
        };
        match changed {
            "digest" => entry.blob.sha256 = "b".repeat(64),
            "length" => entry.blob.bytes += 1,
            "ckey" => entry.content_key = "c".repeat(32),
            "keys" => entry.keys.sha256 = "d".repeat(64),
            _ => unreachable!(),
        }
        let generation = f
            .store()
            .generation("decoded-v1/decoded-rebuild")
            .unwrap()
            .unwrap();
        f.store()
            .put("decoded-v1/decoded-rebuild", &entry, Some(generation))
            .unwrap();
        std::fs::remove_file(evidence::blob_path(&f.home, &blob.sha256).unwrap()).unwrap();

        assert_eq!(
            validate_rebuild(
                &f.store(),
                Some("decoded-v1/decoded-rebuild"),
                &record,
                &keys,
                &blob
            )
            .unwrap_err()
            .code,
            "vault.blob_integrity",
            "{changed}"
        );
        assert!(!evidence::blob_path(&f.home, &blob.sha256).unwrap().exists());
        std::fs::remove_dir_all(f.home).unwrap();
    }
}
