use super::*;
use lycheedev_core::control::OperationControl;
use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

struct Source {
    raw: Vec<u8>,
    budget: Arc<Budget>,
    control: OperationControl,
    reads: Arc<Mutex<Vec<(usize, usize)>>>,
    extent: Arc<AtomicUsize>,
    drops: Arc<AtomicUsize>,
}
impl EncodedRead for Source {
    fn check_encoded(&self) -> Result<()> {
        self.control.check()?;
        self.budget.check()
    }
    fn encoded_size(&self) -> usize {
        self.extent.load(Ordering::SeqCst)
    }
    fn read_encoded(&self, at: usize, bytes: usize) -> Result<EncodedSpan<'_>> {
        self.check_encoded()?;
        self.reads.lock().unwrap().push((at, bytes));
        EncodedSpan::read_owned(bytes, Some(&self.budget), |out| {
            out.copy_from_slice(&self.raw[at..at + bytes]);
            Ok(())
        })
    }
}
impl Drop for Source {
    fn drop(&mut self) {
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
fn budget() -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    ))
}
fn frame(chunk: &[u8], decoded: usize) -> Vec<u8> {
    let mut raw = b"BLTE".to_vec();
    raw.extend_from_slice(&36u32.to_be_bytes());
    raw.extend_from_slice(&[15, 0, 0, 1]);
    raw.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    raw.extend_from_slice(&(decoded as u32).to_be_bytes());
    raw.extend_from_slice(&Md5::digest(chunk));
    raw.extend_from_slice(chunk);
    raw
}
fn source(raw: Vec<u8>, budget: &Arc<Budget>) -> Source {
    Source {
        extent: Arc::new(AtomicUsize::new(raw.len())),
        raw,
        budget: budget.clone(),
        control: OperationControl::default(),
        reads: Arc::new(Mutex::new(Vec::new())),
        drops: Arc::new(AtomicUsize::new(0)),
    }
}
#[test]
fn repeated_short_views_share_one_directory_without_reparse_or_clone() {
    let budget = budget();
    let source = source(frame(b"Nhello", 5), &budget);
    let reads = source.reads.clone();
    let drops = source.drops.clone();
    let prepared = PreparedRanges::open(source, Limits::default(), budget.clone()).unwrap();
    assert_eq!(prepared.size(), 5);
    let initial = reads.lock().unwrap().clone();
    assert_eq!(initial, [(0, 8), (8, 28)]);
    let ledger = budget.snapshot().unwrap();
    let logical = budget.logical_snapshot().unwrap();
    let pointer = prepared.directory.parts.as_ptr();
    let keys = BTreeMap::new();
    for _ in 0..10 {
        let view = prepared.view(&keys).unwrap();
        assert_eq!(view.parts.as_ptr(), pointer);
        assert!(matches!(&view.parts, RangeParts::Borrowed(_)));
        assert_eq!(view.size(), 5);
    }
    assert_eq!(*reads.lock().unwrap(), initial);
    assert_eq!(budget.snapshot().unwrap(), ledger);
    assert_eq!(budget.logical_snapshot().unwrap(), logical);
    drop(prepared);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
}
#[test]
fn prepared_page_outlives_source_without_refunding_its_live_output() {
    let budget = budget();
    let source = source(frame(b"Nhello", 5), &budget);
    let drops = source.drops.clone();
    let prepared = PreparedRanges::open(source, Limits::default(), budget.clone()).unwrap();
    let base = budget.snapshot().unwrap().retained_bytes;
    let keys = BTreeMap::new();
    let page = prepared
        .view(&keys)
        .unwrap()
        .read_checked_owned_span(0, 5, &Md5::digest(b"hello"), &budget)
        .unwrap();
    assert_eq!(budget.snapshot().unwrap().retained_bytes, base + 5);
    drop(prepared);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(page.as_ref(), b"hello");
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 5);
    drop(page);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
}
#[test]
fn plain_owned_span_requires_intersecting_chunk_digest_not_only_selected_bytes() {
    let budget = budget();
    let mut raw = frame(b"Nhello", 5);
    raw[20] ^= 1;
    let prepared =
        PreparedRanges::open(source(raw, &budget), Limits::default(), budget.clone()).unwrap();
    let keys = BTreeMap::new();
    let view = prepared.view(&keys).unwrap();
    assert_eq!(
        view.read_owned_span(0, 5, &budget).err().unwrap().code,
        "container.integrity_mismatch"
    );
    // Independent expected MD5 permits N's existing bounded direct path; that
    // success is page authentication and never a claim about the bad chunk MD5.
    assert_eq!(
        view.read_checked_owned_span(0, 5, &Md5::digest(b"hello"), &budget)
            .unwrap()
            .as_ref(),
        b"hello"
    );
}
#[test]
fn plain_owned_span_checks_bounds_limits_and_empty_scope_without_io() {
    let budget = budget();
    let source = source(frame(b"Nhello", 5), &budget);
    let reads = source.reads.clone();
    let stop = source.control.clone();
    let prepared = PreparedRanges::open(
        source,
        Limits {
            chunk_bytes: 4,
            ..Limits::default()
        },
        budget.clone(),
    )
    .unwrap();
    let initial = reads.lock().unwrap().len();
    let keys = BTreeMap::new();
    let view = prepared.view(&keys).unwrap();
    assert!(view.read_owned_span(0, 0, &budget).unwrap().is_empty());
    assert_eq!(
        view.read_owned_span(4, 2, &budget).err().unwrap().code,
        "container.invalid_format"
    );
    assert_eq!(
        view.read_owned_span(0, 5, &budget).err().unwrap().code,
        "container.resource_limit"
    );
    // A small requested range still cannot silently skip the full intersecting
    // chunk's limit merely because no independent page digest is available.
    assert_eq!(
        view.read_owned_span(0, 1, &budget).err().unwrap().code,
        "container.resource_limit"
    );
    assert_eq!(reads.lock().unwrap().len(), initial);
    stop.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        view.read_owned_span(0, 0, &budget).err().unwrap().code,
        "command.cancelled"
    );
    assert_eq!(reads.lock().unwrap().len(), initial);
}
#[test]
fn cached_geometry_checks_original_scope_and_exact_extent() {
    let budget = budget();
    let source = source(frame(b"Nhello", 5), &budget);
    let extent = source.extent.clone();
    let reads = source.reads.clone();
    let stop = source.control.clone();
    let prepared = PreparedRanges::open(source, Limits::default(), budget).unwrap();
    let initial = reads.lock().unwrap().len();
    let keys = BTreeMap::new();
    extent.fetch_add(1, Ordering::SeqCst);
    assert_eq!(
        prepared.view(&keys).err().unwrap().code,
        "container.invalid_format"
    );
    extent.fetch_sub(1, Ordering::SeqCst);
    stop.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        prepared.view(&keys).err().unwrap().code,
        "command.cancelled"
    );
    assert_eq!(reads.lock().unwrap().len(), initial);
}
#[test]
fn failed_prepare_drops_source_and_keeps_prior_admitted_work() {
    let budget = budget();
    let mut raw = frame(b"Nhello", 5);
    raw.push(0);
    let source = source(raw, &budget);
    let drops = source.drops.clone();
    budget.charge(Cost::work(3)).unwrap();
    assert_eq!(
        PreparedRanges::open(source, Limits::default(), budget.clone())
            .err()
            .unwrap()
            .code,
        "container.invalid_format"
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert_eq!(budget.snapshot().unwrap().decode_work, 4);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(budget.logical_snapshot().unwrap().retained_bytes, 64);
    assert_eq!(budget.logical_snapshot().unwrap().metadata_bytes, 64);
}
#[test]
fn next_key_snapshot_can_decode_same_framing_after_missing_key_without_reparse() {
    let budget = budget();
    let key = vec![0; 16];
    let id = 42u64;
    let mut encoded = vec![b'E', 8];
    encoded.extend_from_slice(&id.to_le_bytes());
    encoded.extend_from_slice(&[4, 0, 0, 0, 0, b'S']);
    encoded.extend_from_slice(&salsa(b"Nhello", &key, [0; 8], None).unwrap());
    let source = source(frame(&encoded, 5), &budget);
    let reads = source.reads.clone();
    let prepared = PreparedRanges::open(source, Limits::default(), budget.clone()).unwrap();
    let empty = BTreeMap::new();
    assert_eq!(
        prepared
            .view(&empty)
            .unwrap()
            .read_checked_owned_span(0, 5, &Md5::digest(b"hello"), &budget)
            .err()
            .unwrap()
            .code,
        "container.key_unavailable"
    );
    let keys = BTreeMap::from([(id, key)]);
    let page = prepared
        .view(&keys)
        .unwrap()
        .read_checked_owned_span(0, 5, &Md5::digest(b"hello"), &budget)
        .unwrap();
    assert_eq!(page.as_ref(), b"hello");
    assert_eq!(
        reads
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, _)| *at == 0)
            .count(),
        1
    );
}
#[test]
fn real_encoding_parser_borrows_prepared_view_and_plain_owned_header_directory() {
    let budget = budget();
    let ckey = [1; 16];
    let ekey = [2; 16];
    let mut page = vec![0; 1024];
    page[0] = 1;
    page[2..6].copy_from_slice(&128u32.to_be_bytes());
    page[6..22].copy_from_slice(&ckey);
    page[22..38].copy_from_slice(&ekey);
    let mut logical = vec![0; 22];
    logical[..5].copy_from_slice(b"EN\x01\x10\x10");
    logical[5..7].copy_from_slice(&1u16.to_be_bytes());
    logical[9..13].copy_from_slice(&1u32.to_be_bytes());
    logical.extend_from_slice(&ckey);
    logical.extend_from_slice(&Md5::digest(&page));
    logical.extend_from_slice(&page);
    let encoded = [b"N".as_slice(), logical.as_slice()].concat();
    let source = source(frame(&encoded, logical.len()), &budget);
    let reads = source.reads.clone();
    let prepared = PreparedRanges::open(source, Limits::default(), budget.clone()).unwrap();
    let keys = BTreeMap::new();
    let view = prepared.view(&keys).unwrap();
    let header = view.read_owned_span(0, 22, &budget).unwrap();
    assert_eq!(header.as_ref(), &logical[..22]);
    let directory = view.read_owned_span(22, 32, &budget).unwrap();
    assert_eq!(directory.as_ref(), &logical[22..54]);
    drop(directory);
    drop(header);
    drop(view);
    let mut previous_reads = reads.lock().unwrap().len();
    for _ in 0..2 {
        let view = prepared.view(&keys).unwrap();
        let limits = crate::casc::Limits::default();
        let encoding =
            crate::metadata_format::Encoding::open_with_check(&view, limits, None, || {
                prepared.check()
            })
            .unwrap();
        let record = encoding
            .find_with_check(&crate::casc::hex(&ckey), limits, None, || prepared.check())
            .unwrap();
        assert_eq!(record.decoded_bytes, 128);
        assert_eq!(record.encoding_keys, [crate::casc::hex(&ekey)]);
        let current = reads.lock().unwrap().len();
        assert!(current > previous_reads);
        previous_reads = current;
    }
    // Public Encoding remains uncached. Only the BLTE directory was prepared;
    // The private PreparedEncoding consumer separately caches verified pages.
    assert_eq!(
        reads
            .lock()
            .unwrap()
            .iter()
            .filter(|(at, _)| *at == 0)
            .count(),
        1
    );
}

#[test]
fn prepared_view_rejects_independent_budget_stop_without_touching_live_source() {
    let independent = OperationControl::default();
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        independent.deadline(),
        independent.clone(),
    ));
    let source = source(frame(b"Nhello", 5), &budget);
    let original = source.control.clone();
    let reads = source.reads.clone();
    let prepared = PreparedRanges::open(source, Limits::default(), budget).unwrap();
    let count = reads.lock().unwrap().len();
    independent.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        prepared.view(&BTreeMap::new()).err().unwrap().code,
        "command.cancelled"
    );
    assert!(original.check().is_ok());
    assert_eq!(reads.lock().unwrap().len(), count);
}
