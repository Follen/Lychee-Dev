use super::*;
use crate::blte::{self, EncodedSpan};
use lycheedev_core::control::OperationControl;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

struct Source {
    raw: Vec<u8>,
    budget: Arc<Budget>,
    control: OperationControl,
    reads: Arc<Mutex<Vec<(usize, usize)>>>,
    fail: Arc<AtomicBool>,
}
impl EncodedRead for Source {
    fn encoded_size(&self) -> usize {
        self.raw.len()
    }
    fn check_encoded(&self) -> Result<()> {
        self.control.check()?;
        self.budget.check()
    }
    fn read_encoded(&self, at: usize, bytes: usize) -> Result<EncodedSpan<'_>> {
        self.check_encoded()?;
        self.reads.lock().unwrap().push((at, bytes));
        if self.fail.swap(false, Ordering::SeqCst) {
            return Err(fault("test.read", 5, "injected source read failure"));
        }
        EncodedSpan::read_owned(bytes, Some(&self.budget), |out| {
            out.copy_from_slice(&self.raw[at..at + bytes]);
            Ok(())
        })
    }
}
fn payload() -> (Vec<u8>, String, String) {
    let ckey = "11".repeat(16);
    let ekey = "22".repeat(16);
    let mut c = vec![0; 1024];
    c[0] = 1;
    c[2..6].copy_from_slice(&5u32.to_be_bytes());
    c[6..22].copy_from_slice(&casc::key(&ckey).unwrap());
    c[22..38].copy_from_slice(&casc::key(&ekey).unwrap());
    let mut e = vec![0; 1024];
    e[..16].copy_from_slice(&casc::key(&ekey).unwrap());
    e[21..25].copy_from_slice(&42u32.to_be_bytes());
    let mut raw = vec![0; 22];
    raw[..5].copy_from_slice(&[b'E', b'N', 1, 16, 16]);
    raw[5..7].copy_from_slice(&1u16.to_be_bytes());
    raw[7..9].copy_from_slice(&1u16.to_be_bytes());
    raw[9..13].copy_from_slice(&1u32.to_be_bytes());
    raw[13..17].copy_from_slice(&1u32.to_be_bytes());
    raw.extend_from_slice(&casc::key(&ckey).unwrap());
    raw.extend_from_slice(&Md5::digest(&c));
    raw.extend(c);
    raw.extend_from_slice(&casc::key(&ekey).unwrap());
    raw.extend_from_slice(&Md5::digest(&e));
    raw.extend(e);
    (raw, ckey, ekey)
}
fn open(
    raw: &[u8],
) -> (
    PreparedEncoding<Source>,
    Arc<Mutex<Vec<(usize, usize)>>>,
    Arc<AtomicBool>,
    OperationControl,
    Arc<Budget>,
) {
    let mut limits = Cost::defaults();
    limits.decode_work = 1_000_000_000;
    let budget = Arc::new(Budget::new(
        limits,
        Instant::now() + Duration::from_secs(120),
        None,
    ));
    let chunk = [b"N".as_slice(), raw].concat();
    let mut encoded = b"BLTE".to_vec();
    encoded.extend_from_slice(&36u32.to_be_bytes());
    encoded.extend_from_slice(&[15, 0, 0, 1]);
    encoded.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&(raw.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&Md5::digest(&chunk));
    encoded.extend_from_slice(&chunk);
    let reads = Arc::new(Mutex::new(Vec::new()));
    let fail = Arc::new(AtomicBool::new(false));
    let control = OperationControl::default();
    let source = Source {
        raw: encoded,
        budget: budget.clone(),
        control: control.clone(),
        reads: reads.clone(),
        fail: fail.clone(),
    };
    let ranges = PreparedRanges::open(source, blte::Limits::default(), budget.clone()).unwrap();
    let index = PreparedEncoding::open(ranges, &BTreeMap::new(), Limits::default(), budget.clone())
        .unwrap();
    (index, reads, fail, control, budget)
}
#[test]
fn repeated_ckey_and_ekey_lookups_reuse_verified_pages_and_physical_directory() {
    let (raw, ckey, ekey) = payload();
    let (index, reads, _, _, budget) = open(&raw);
    let keys = BTreeMap::new();
    let c = index
        .find(&keys, &ckey, Limits::default(), || Ok(()))
        .unwrap();
    assert_eq!(c.decoded_bytes, 5);
    let e = index
        .physical(&keys, &ekey, Limits::default(), &mut || Ok(()))
        .unwrap();
    assert_eq!(e.encoded_bytes, 42);
    let before_reads = reads.lock().unwrap().clone();
    let logical = budget.logical_snapshot().unwrap();
    let safety = budget.snapshot().unwrap();
    for _ in 0..5 {
        assert_eq!(
            index
                .find(&keys, &ckey, Limits::default(), || Ok(()))
                .unwrap()
                .encoding_keys,
            vec![ekey.clone()]
        );
        assert_eq!(
            index
                .physical(&keys, &ekey, Limits::default(), &mut || Ok(()))
                .unwrap()
                .encoded_bytes,
            42
        );
    }
    assert_eq!(*reads.lock().unwrap(), before_reads);
    assert_eq!(budget.logical_snapshot().unwrap(), logical);
    assert_eq!(budget.snapshot().unwrap(), safety);
    assert_eq!(index.cache.borrow().pages.len(), 2);
    assert!(index.cache.borrow().physical.is_some());
    drop(index);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(budget.logical_snapshot().unwrap(), logical);
}
#[test]
fn failed_page_and_directory_attempts_keep_logical_cost_but_never_install_or_leak() {
    let (raw, _, _) = payload();
    let (index, _, fail, _, budget) = open(&raw);
    let keys = BTreeMap::new();
    let base = budget.snapshot().unwrap().retained_bytes;
    let initial = budget.logical_snapshot().unwrap();
    let error = index.page(&keys, 54, 1024, &[7; 16]).err().unwrap();
    assert_eq!(error.code, "container.integrity_mismatch");
    assert!(index.cache.borrow().pages.is_empty());
    assert_eq!(budget.snapshot().unwrap().retained_bytes, base);
    assert_eq!(
        budget.logical_snapshot().unwrap().retained_bytes,
        initial.retained_bytes + 1152
    );
    fail.store(true, Ordering::SeqCst);
    assert_eq!(
        index.physical_directory(&keys).err().unwrap().code,
        "test.read"
    );
    assert!(index.cache.borrow().physical.is_none());
    assert_eq!(budget.snapshot().unwrap().retained_bytes, base);
    let before = budget.logical_snapshot().unwrap();
    assert_eq!(index.physical_directory(&keys).unwrap().len(), 32);
    assert_eq!(
        budget.logical_snapshot().unwrap().retained_bytes,
        before.retained_bytes + 32
    );
    assert_eq!(
        budget.logical_snapshot().unwrap().metadata_bytes,
        before.metadata_bytes + 32
    );
}
#[test]
fn cache_exact_capacity_oversized_read_and_eviction_preserve_live_page_owners() {
    let (mut raw, _, _) = payload();
    let at = raw.len();
    raw.resize(at + CAPACITY + 2, b'x');
    let (index, _, _, _, budget) = open(&raw);
    let keys = BTreeMap::new();
    let base = budget.snapshot().unwrap().retained_bytes;
    let first = index
        .page(&keys, at, CAPACITY, &Md5::digest(&raw[at..at + CAPACITY]))
        .unwrap();
    assert_eq!(index.cache.borrow().bytes, CAPACITY);
    let oversize = index
        .page(&keys, at + 1, CAPACITY + 1, &Md5::digest(&raw[at + 1..]))
        .unwrap();
    assert_eq!(index.cache.borrow().pages.len(), 1);
    assert!(index.cache.borrow().pages.contains_key(&at));
    drop(oversize);
    let replacement = index
        .page(&keys, at + 1, 1, &Md5::digest(&raw[at + 1..at + 2]))
        .unwrap();
    assert_eq!(index.cache.borrow().bytes, 1);
    assert!(!index.cache.borrow().pages.contains_key(&at));
    assert_eq!(first.as_ref().len(), CAPACITY);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        base + CAPACITY as u64 + ENTRY_ALLOCATION as u64 + 1 + ENTRY_ALLOCATION as u64
    );
    let logical = budget.logical_snapshot().unwrap();
    drop(first);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        base + 1 + ENTRY_ALLOCATION as u64
    );
    drop(index);
    assert_eq!(replacement.as_ref(), b"x");
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        1 + ENTRY_ALLOCATION as u64
    );
    drop(replacement);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(budget.logical_snapshot().unwrap(), logical);
}
#[test]
fn cached_hit_checks_original_scope_without_new_source_io() {
    let (raw, ckey, _) = payload();
    let (index, reads, _, control, _) = open(&raw);
    let keys = BTreeMap::new();
    index
        .find(&keys, &ckey, Limits::default(), || Ok(()))
        .unwrap();
    let count = reads.lock().unwrap().len();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        index
            .find(&keys, &ckey, Limits::default(), || Ok(()))
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    assert_eq!(reads.lock().unwrap().len(), count);
}

#[test]
#[cfg(target_pointer_width = "64")]
fn cached_page_safety_allowance_covers_pinned_node_and_arc_layout() {
    // Source-derived from pinned alloc::collections::btree::node B=6,
    // LeafNode keys/vals[11], InternalNode edges[12], and repr(C) ArcInner.
    let word = size_of::<usize>();
    let leaf =
        (word + 2 * size_of::<u16>() + 11 * (word + size_of::<Page>())).next_multiple_of(word);
    let internal = leaf + 12 * word;
    let arc = 2 * word + size_of::<CachedPage>();
    assert_eq!(leaf, 192);
    assert_eq!(internal, 288);
    assert_eq!(arc, 72);
    assert!(ENTRY_ALLOCATION >= internal + arc);
}
