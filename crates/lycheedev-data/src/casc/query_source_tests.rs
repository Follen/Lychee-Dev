use super::*;
use crate::blte::EncodedRead;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn root() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/sql-index-session-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(root.join("Data/data")).unwrap();
    root
}
fn budget(control: &OperationControl) -> Arc<Budget> {
    Arc::new(Budget::query_controlled(
        Instant::now() + Duration::from_secs(30),
        control.clone(),
    ))
}
fn index(bucket: u8, key: &[u8; 16], offset: u32, bytes: u32) -> Vec<u8> {
    let mut raw = vec![0u8; 58];
    raw[..4].copy_from_slice(&16u32.to_le_bytes());
    raw[8..16].copy_from_slice(&[7, 0, bucket, 0, 4, 5, 9, 30]);
    raw[16..24].copy_from_slice(&(1u64 << 30).to_le_bytes());
    let guard = guard_sum(&raw[8..24], 0, 0).0;
    raw[4..8].copy_from_slice(&guard.to_le_bytes());
    raw[32..36].copy_from_slice(&18u32.to_le_bytes());
    raw[40..49].copy_from_slice(&key[..9]);
    raw[50..54].copy_from_slice(&offset.to_be_bytes());
    raw[54..58].copy_from_slice(&bytes.to_le_bytes());
    let guard = guard_sum(&raw[40..], 0, 0).0;
    raw[36..40].copy_from_slice(&guard.to_le_bytes());
    raw
}
fn fixture() -> (PathBuf, [u8; 16], Vec<u8>) {
    let root = root();
    let key = [0x11; 16];
    let raw = index(0, &key, 0, 100);
    fs::write(root.join("Data/data/0000000001.idx"), &raw).unwrap();
    (root, key, raw)
}
#[test]
fn query_index_cache_reuses_validated_bytes_and_charges_go_logical_size_once() {
    let (root, key, raw) = fixture();
    let control = OperationControl::default();
    let budget = budget(&control);
    let source = QuerySource::open(&root, Limits::default(), &budget, &control).unwrap();
    let before = budget.logical_snapshot().unwrap();
    let first = source.find(0, "0000000001.idx", &key).unwrap();
    let loaded = budget.logical_snapshot().unwrap();
    assert_eq!(
        loaded.metadata_bytes - before.metadata_bytes,
        raw.len() as u64
    );
    assert_eq!(
        loaded.retained_bytes - before.retained_bytes,
        raw.len() as u64
    );
    assert_eq!(loaded.decode_work - before.decode_work, 1);
    assert_eq!(source.find(0, "0000000001.idx", &key).unwrap(), first);
    assert_eq!(budget.logical_snapshot().unwrap(), loaded);
    assert_eq!(source.cache.borrow().indexes.len(), 1);
    source.verify().unwrap();
    assert_eq!(
        budget.logical_snapshot().unwrap(),
        loaded,
        "final rehash is a safety-only verification cost"
    );
    source.close_after(Ok(())).unwrap();
}
#[test]
fn query_index_final_rehash_reads_path_not_cached_bytes_or_size() {
    let (root, key, mut raw) = fixture();
    let control = OperationControl::default();
    let budget = budget(&control);
    let source = QuerySource::open(&root, Limits::default(), &budget, &control).unwrap();
    let original = source.find(0, "0000000001.idx", &key).unwrap();
    raw[40] ^= 1;
    fs::write(root.join("Data/data/0000000001.idx"), raw).unwrap();
    assert_eq!(
        source.find(0, "0000000001.idx", &key).unwrap(),
        original,
        "validated query snapshot remains immutable"
    );
    assert_eq!(
        source.verify().unwrap_err().code,
        "records.pinned_build_changed"
    );
    source.close_after(Ok(())).unwrap();
}
#[test]
fn query_index_generation_change_is_checked_before_capture() {
    let (root, _, raw) = fixture();
    let control = OperationControl::default();
    let budget = budget(&control);
    let source = QuerySource::open(&root, Limits::default(), &budget, &control).unwrap();
    fs::write(root.join("Data/data/0000000002.idx"), raw).unwrap();
    assert_eq!(
        source.check().unwrap_err().code,
        "records.pinned_build_changed"
    );
    source.close_after(Ok(())).unwrap();
}
#[test]
fn query_index_reload_after_eviction_rejects_changed_valid_generation() {
    let (root, key, _) = fixture();
    let control = OperationControl::default();
    let budget = budget(&control);
    let source = QuerySource::open(&root, Limits::default(), &budget, &control).unwrap();
    source.find(0, "0000000001.idx", &key).unwrap();
    {
        let mut cache = source.cache.borrow_mut();
        cache.indexes.clear();
        cache.retained = 0;
    }
    fs::write(
        root.join("Data/data/0000000001.idx"),
        index(0, &key, 128, 100),
    )
    .unwrap();
    assert_eq!(
        source.find(0, "0000000001.idx", &key).unwrap_err().code,
        "records.pinned_build_changed"
    );
    source.close_after(Ok(())).unwrap();
}
#[test]
fn query_source_prefers_real_prefix_bucket_before_unneeded_corrupt_index() {
    let root = root();
    let (encoded, key, bucket) = (1u8..=255)
        .find_map(|byte| {
            let payload = [b'N', byte];
            let mut raw = b"BLTE".to_vec();
            raw.extend(36u32.to_be_bytes());
            raw.extend([15, 0, 0, 1]);
            raw.extend(2u32.to_be_bytes());
            raw.extend(1u32.to_be_bytes());
            raw.extend(Md5::digest(payload));
            let key: [u8; 16] = Md5::digest(&raw).into();
            let folded = key[..9].iter().fold(0u8, |n, b| n ^ b);
            let bucket = (folded >> 4) ^ (folded & 15);
            raw.extend(payload);
            (bucket != 0).then_some((raw, key, bucket))
        })
        .unwrap();
    let mut archive = vec![0; 30];
    archive.extend(&encoded);
    fs::write(root.join("Data/data/data.000"), archive).unwrap();
    fs::write(
        root.join(format!("Data/data/{bucket:02x}00000001.idx")),
        index(bucket, &key, 0, encoded.len() as u32 + 30),
    )
    .unwrap();
    fs::write(
        root.join("Data/data/0000000001.idx"),
        b"corrupt unrequested index",
    )
    .unwrap();
    let control = OperationControl::default();
    let budget = budget(&control);
    let source = QuerySource::open(&root, Limits::default(), &budget, &control).unwrap();
    let reader = open_local_reader_from_source(&source, &hex(&key), encoded.len()).unwrap();
    assert_eq!(reader.encoded_size(), encoded.len());
    assert_eq!(source.cache.borrow().indexes.len(), 1);
    reader.close_after(Ok(())).unwrap();
    source.verify().unwrap();
    source.close_after(Ok(())).unwrap();
}
#[test]
fn query_index_verification_keeps_independent_budget_stop_and_control_identity() {
    let (root, key, _) = fixture();
    let control = OperationControl::default();
    let cancel = Arc::new(AtomicBool::new(false));
    let budget = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(cancel.clone()),
    ));
    let source = QuerySource::open(&root, Limits::default(), &budget, &control).unwrap();
    source.find(0, "0000000001.idx", &key).unwrap();
    assert!(source.check_consumer(&budget, &control).is_ok());
    assert_eq!(
        source
            .check_consumer(&budget, &OperationControl::default())
            .unwrap_err()
            .code,
        "records.file_query"
    );
    cancel.store(true, Ordering::Release);
    let error = source.verify().unwrap_err();
    assert_eq!(error.code, "operation.cancelled");
    assert!(control.stop_cause().unwrap().is_none());
    let closed = source.close_after::<()>(Err(error)).unwrap_err();
    assert_eq!(closed.code, "operation.cancelled");
}
