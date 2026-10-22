//! Actual Files::open -> owned section; no unbudgeted replacement backend.
use super::*;
use crate::blte::EncodedRead;

#[test]
fn owned_open_keeps_same_authenticated_files_and_block_when_moved() {
    let f = Fixture::new("archives =\n");
    let (key, raw) = framed(b"prepared metadata");
    let size = raw.len();
    f.object(&key, raw.clone());
    let budget = Budget::controlled(Cost::defaults(), f.control.deadline(), f.control.clone());
    let files = Arc::new(f.files(false, Some(&budget), 1));
    let weak = Arc::downgrade(&files);
    let owned = files.open_owned(&key, size as i64).unwrap();
    let calls = f.backend.calls.lock().unwrap().len();
    let hits = files.counters().unwrap().block_cache_hits;
    let retained = budget.snapshot().unwrap().retained_bytes;
    fn moved<'a>(value: OwnedEncoded<'a>) -> OwnedEncoded<'a> {
        value
    }
    let mut owned = moved(owned);
    drop(files);
    assert_eq!(owned.encoded_size(), size);
    let span = owned.read_encoded(0, 8).unwrap();
    assert_eq!(span.as_ref(), &raw[..8]);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, retained + 8);
    drop(span);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, retained);
    let mut out = vec![0; size];
    assert_eq!(owned.read(&mut out).unwrap(), size);
    assert_eq!(out, raw);
    owned.rewind().unwrap();
    let mut prefix = [0; 4];
    owned.read_exact(&mut prefix).unwrap();
    assert_eq!(&prefix, b"BLTE");
    assert_eq!(f.backend.calls.lock().unwrap().len(), calls);
    assert!(weak.upgrade().unwrap().counters().unwrap().block_cache_hits >= hits + 3);
    let work = budget.snapshot().unwrap();
    drop(owned);
    assert!(weak.upgrade().is_none());
    let after = budget.snapshot().unwrap();
    assert_eq!(
        (
            after.network_requests,
            after.network_bytes,
            after.decode_work
        ),
        (work.network_requests, work.network_bytes, work.decode_work)
    );
    std::fs::remove_dir_all(&f.home).unwrap();
}
#[test]
fn prepared_owned_source_supplies_exact_admitted_budget_to_guarded_n_page() {
    let f = Fixture::new("archives =\n");
    let body = b"authenticated";
    let (key, raw) = framed(body);
    let size = raw.len();
    f.object(&key, raw);
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        f.control.deadline(),
        f.control.clone(),
    ));
    let files = Arc::new(f.files(false, Some(&budget), 1));
    let before = budget.snapshot().unwrap().retained_bytes;
    let owned = files.open_owned(&key, size as i64).unwrap();
    let keys = BTreeMap::new();
    let ranges =
        blte::Ranges::open_reader_budgeted(&owned, blte::Limits::default(), &keys, &budget)
            .unwrap();
    let base = budget.snapshot().unwrap().retained_bytes;
    assert!(base >= before + size as u64);
    let page = ranges
        .read_checked_owned_span(0, body.len(), &Md5::digest(body), &budget)
        .unwrap();
    assert_eq!(page.as_ref(), body);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        base + body.len() as u64
    );
    drop(ranges);
    drop(owned);
    // Files remains alive, but the section's one loaded block has been freed.
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        base - size as u64 + body.len() as u64
    );
    assert_eq!(page.as_ref(), body);
    drop(page);
    assert_eq!(
        budget.snapshot().unwrap().retained_bytes,
        base - size as u64
    );
    drop(files);
    std::fs::remove_dir_all(&f.home).unwrap();
}
#[test]
fn owned_open_cannot_bypass_framing_or_ekey_authentication() {
    for corrupt_header in [false, true] {
        let f = Fixture::new("archives =\n");
        let (key, mut raw) = framed(b"body");
        let size = raw.len();
        if corrupt_header {
            raw[12] ^= 1;
        } else {
            raw[0] ^= 1;
        }
        f.object(&key, raw);
        let budget = Budget::controlled(Cost::defaults(), f.control.deadline(), f.control.clone());
        let files = Arc::new(f.files(false, Some(&budget), 1));
        let before = budget.snapshot().unwrap().retained_bytes;
        let error = files.open_owned(&key, size as i64).err().unwrap();
        assert_eq!(
            error.code,
            if corrupt_header {
                "container.integrity_mismatch"
            } else {
                "container.invalid_format"
            }
        );
        assert_eq!(Arc::strong_count(&files), 1);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, before);
        assert!(budget.snapshot().unwrap().network_bytes > 0);
        drop(files);
        std::fs::remove_dir_all(&f.home).unwrap();
    }
}
#[test]
fn owned_sequential_read_preserves_prefix_and_old_block_after_next_block_error() {
    let f = Fixture::new("archives =\n");
    let (key, raw) = framed(&vec![7; BLOCK + 1]);
    let size = raw.len();
    f.object(&key, raw.clone());
    let budget = Budget::controlled(Cost::defaults(), f.control.deadline(), f.control.clone());
    let files = Arc::new(f.files(false, Some(&budget), 1));
    let mut owned = files.open_owned(&key, size as i64).unwrap();
    let base = budget.snapshot().unwrap().retained_bytes;
    let mut out = vec![0; size];
    assert_eq!(owned.read(&mut out).unwrap(), BLOCK);
    assert_eq!(&out[..BLOCK], &raw[..BLOCK]);
    f.backend
        .errors
        .lock()
        .unwrap()
        .insert(key, fault("records.remote_http", 5, "later block fails"));
    let error = blte::stream_io_error(owned.read(&mut out).unwrap_err());
    assert_eq!(error.code, "records.remote_http");
    assert_eq!(owned.position, BLOCK);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, base);
    let calls = f.backend.calls.lock().unwrap().len();
    {
        let prefix = owned.read_encoded(0, 4).unwrap();
        assert_eq!(&*prefix, b"BLTE");
        assert_eq!(f.backend.calls.lock().unwrap().len(), calls);
    }
    drop(owned);
    drop(files);
    std::fs::remove_dir_all(&f.home).unwrap();
}
#[test]
fn owned_cached_reads_and_rewind_keep_original_and_independent_budget_stops() {
    for stop_budget in [false, true] {
        let f = Fixture::new("archives =\n");
        let independent = OperationControl::default();
        let budget = Budget::controlled(
            Cost::defaults(),
            independent.deadline(),
            independent.clone(),
        );
        let (key, raw) = framed(b"body");
        let size = raw.len();
        f.object(&key, raw);
        let files = Arc::new(f.files(false, Some(&budget), 1));
        let mut owned = files.open_owned(&key, size as i64).unwrap();
        let calls = f.backend.calls.lock().unwrap().len();
        if stop_budget {
            independent.tracked_cancel_handle().unwrap().cancel();
        } else {
            f.control.tracked_cancel_handle().unwrap().cancel();
        }
        assert_eq!(owned.check_encoded().unwrap_err().code, "command.cancelled");
        assert_eq!(
            owned.read_encoded(0, 8).err().unwrap().code,
            "command.cancelled"
        );
        assert_eq!(owned.rewind().unwrap_err().code, "command.cancelled");
        assert_eq!(
            blte::stream_io_error(owned.read(&mut [0; 1]).unwrap_err()).code,
            "command.cancelled"
        );
        assert_eq!(f.backend.calls.lock().unwrap().len(), calls);
        if stop_budget {
            assert!(f.control.check().is_ok());
        } else {
            assert!(budget.check().is_ok());
        }
        drop(owned);
        drop(files);
        std::fs::remove_dir_all(&f.home).unwrap();
    }
}
