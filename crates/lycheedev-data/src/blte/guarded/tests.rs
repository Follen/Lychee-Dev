use super::*;
use lycheedev_core::control::OperationControl;
use std::{
    cell::Cell,
    io::Write,
    time::{Duration, Instant},
};

fn budget(retained: u64) -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost {
            retained_bytes: retained,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    ))
}
fn frame(chunks: &[(Vec<u8>, usize)]) -> Vec<u8> {
    let header = 12 + chunks.len() * 24;
    let mut raw = b"BLTE".to_vec();
    raw.extend_from_slice(&(header as u32).to_be_bytes());
    raw.extend_from_slice(&[15, 0, 0, chunks.len() as u8]);
    for (encoded, decoded) in chunks {
        raw.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
        raw.extend_from_slice(&(*decoded as u32).to_be_bytes());
        raw.extend_from_slice(&Md5::digest(encoded));
    }
    for (encoded, _) in chunks {
        raw.extend_from_slice(encoded);
    }
    raw
}
fn n_frame(bytes: &[u8]) -> Vec<u8> {
    frame(&[([b"N".as_slice(), bytes].concat(), bytes.len())])
}
struct Reader<'a> {
    raw: Vec<u8>,
    budget: &'a Budget,
    calls: Cell<usize>,
    last_pointer: Cell<*const u8>,
    control: OperationControl,
    cancel_body: Cell<bool>,
}
impl<'a> Reader<'a> {
    fn new(raw: Vec<u8>, budget: &'a Budget) -> Self {
        Self {
            raw,
            budget,
            calls: Cell::new(0),
            last_pointer: Cell::new(std::ptr::null()),
            control: OperationControl::default(),
            cancel_body: Cell::new(false),
        }
    }
}
impl EncodedRead for Reader<'_> {
    fn check_encoded(&self) -> Result<()> {
        self.control.check()?;
        self.budget.check()
    }
    fn encoded_size(&self) -> usize {
        self.raw.len()
    }
    fn read_encoded(&self, offset: usize, bytes: usize) -> Result<EncodedSpan<'_>> {
        self.calls.set(self.calls.get() + 1);
        let span = EncodedSpan::read_owned(bytes, Some(self.budget), |out| {
            out.copy_from_slice(&self.raw[offset..offset + bytes]);
            Ok(())
        })?;
        self.last_pointer.set(span.as_ptr());
        if self.cancel_body.get() && offset > 36 {
            self.control.tracked_cancel_handle().unwrap().cancel();
        }
        Ok(span)
    }
}
#[test]
fn owned_n_transfer_preserves_allocation_and_exact_ledger_until_final_drop() {
    let owner = budget(32);
    owner
        .charge_split(Cost::metadata(9), Cost::default())
        .unwrap();
    let raw = EncodedSpan::read_owned(12, Some(&owner), |out| {
        out.fill(3);
        owner.charge(Cost {
            decode_work: 2,
            network_bytes: 12,
            network_requests: 1,
            ..Cost::default()
        })
    })
    .unwrap();
    let pointer = raw.as_ptr();
    let owned = raw.into_decoded_owned(&owner, || Ok(())).unwrap();
    assert_eq!(owned.as_ptr(), pointer);
    assert_eq!(owned.as_ref(), &[3; 12]);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, 12);
    let logical = owner.logical_snapshot().unwrap();
    drop(owned);
    let used = owner.snapshot().unwrap();
    assert_eq!(
        (
            used.retained_bytes,
            used.decode_work,
            used.network_bytes,
            used.network_requests
        ),
        (0, 2, 12, 1)
    );
    assert_eq!(owner.logical_snapshot().unwrap(), logical);
    assert_eq!((logical.retained_bytes, logical.metadata_bytes), (9, 9));
}
#[test]
fn owned_span_keeps_budget_alive_after_original_arc_is_dropped() {
    let owner = budget(8);
    let weak = Arc::downgrade(&owner);
    let raw = EncodedSpan::read_owned(8, Some(&owner), |out| {
        out.fill(7);
        Ok(())
    })
    .unwrap();
    let span = raw.into_decoded_owned(&owner, || Ok(())).unwrap();
    drop(owner);
    assert_eq!(
        weak.upgrade().unwrap().snapshot().unwrap().retained_bytes,
        8
    );
    assert_eq!(span.as_ref(), &[7; 8]);
    drop(span);
    assert!(weak.upgrade().is_none());
}
#[test]
fn different_arc_and_same_state_view_cannot_take_an_existing_refund() {
    let owner = budget(16);
    for other in [budget(16), Arc::new(owner.safety_only())] {
        let raw = EncodedSpan::read_owned(8, Some(&owner), |_| Ok(())).unwrap();
        assert_eq!(
            raw.into_decoded_owned(&other, || Ok(()))
                .err()
                .unwrap()
                .code,
            "records.internal"
        );
        assert_eq!(owner.snapshot().unwrap().retained_bytes, 0);
        assert_eq!(other.snapshot().unwrap().retained_bytes, 0);
    }
}
#[test]
fn borrowed_copy_is_admitted_and_checks_origin_between_blocks() {
    let owner = budget(3 << 16);
    let bytes = vec![8; 2 << 16];
    let span = EncodedSpan::borrowed(&bytes)
        .into_decoded_owned(&owner, || Ok(()))
        .unwrap();
    assert_ne!(span.as_ptr(), bytes.as_ptr());
    assert_eq!(span.as_ref(), bytes.as_slice());
    assert_eq!(owner.snapshot().unwrap().retained_bytes, bytes.len() as u64);
    drop(span);
    let control = OperationControl::default();
    let mut checks = 0;
    let error = EncodedSpan::borrowed(&bytes)
        .into_decoded_owned(&owner, || {
            checks += 1;
            if checks == 3 {
                control.tracked_cancel_handle().unwrap().cancel();
            }
            control.check()
        })
        .err()
        .unwrap();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(checks, 3);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, 0);
    let too_small = budget(4);
    assert_eq!(
        EncodedSpan::borrowed(b"12345")
            .into_decoded_owned(&too_small, || Ok(()))
            .err()
            .unwrap()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(too_small.snapshot().unwrap().retained_bytes, 0);
}
#[test]
fn actual_range_n_moves_the_reader_buffer_without_a_second_live_copy() {
    let payload = vec![9; 128 << 10];
    let owner = budget((payload.len() + 1024) as u64);
    let reader = Reader::new(n_frame(&payload), &owner);
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &owner).unwrap();
    let base = owner.snapshot().unwrap().retained_bytes;
    let span = ranges
        .read_checked_owned_span(0, payload.len(), &Md5::digest(&payload), &owner)
        .unwrap();
    assert_eq!(span.as_ptr(), reader.last_pointer.get());
    assert_eq!(span.as_ref(), payload.as_slice());
    assert_eq!(
        owner.snapshot().unwrap().retained_bytes,
        base + payload.len() as u64
    );
    drop(span);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base);
    // Legacy public Cow still needs its independent output copy and must not
    // inherit this private transfer as an unguarded allocation escape.
    assert_eq!(
        ranges
            .read_checked_span(0, payload.len(), &Md5::digest(&payload))
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base);
}
#[test]
fn slice_backed_n_copies_once_and_does_not_escape_the_borrowed_input() {
    let owner = budget(1024);
    let raw = n_frame(b"abcde");
    let keys = BTreeMap::new();
    let ranges = Ranges::from_input(
        RangeInput::Slice(&raw),
        Limits::default(),
        &keys,
        Some(&owner),
    )
    .unwrap();
    let base = owner.snapshot().unwrap().retained_bytes;
    let span = ranges
        .read_checked_owned_span(1, 3, &Md5::digest(b"bcd"), &owner)
        .unwrap();
    assert_eq!(span.as_ref(), b"bcd");
    assert_ne!(span.as_ptr(), raw[38..].as_ptr());
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base + 3);
    drop(span);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base);
}
#[test]
fn zlib_uses_shared_decoder_and_only_final_output_storage_is_refunded() {
    let owner = budget(1 << 20);
    let payload = b"abcdefghijklmnopqrstuvwxyz";
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(payload).unwrap();
    let mut encoded = vec![b'Z'];
    encoded.extend(z.finish().unwrap());
    let reader = Reader::new(frame(&[(encoded, payload.len())]), &owner);
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &owner).unwrap();
    let base = owner.snapshot().unwrap().retained_bytes;
    let span = ranges
        .read_checked_owned_span(3, 9, &Md5::digest(&payload[3..12]), &owner)
        .unwrap();
    assert_eq!(span.as_ref(), &payload[3..12]);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base + 52 + 9);
    let logical = owner.logical_snapshot().unwrap();
    let work = owner.snapshot().unwrap().decode_work;
    assert!(work > 0);
    drop(span);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base + 52);
    assert_eq!(owner.logical_snapshot().unwrap(), logical);
    assert_eq!(owner.snapshot().unwrap().decode_work, work);
    assert_eq!(
        ranges
            .read_checked_span(3, 9, &Md5::digest(&payload[3..12]))
            .unwrap()
            .as_ref(),
        &payload[3..12]
    );
}
#[test]
fn cross_chunk_result_and_corrupt_digest_never_commit_a_failed_page() {
    let owner = budget(1 << 20);
    let reader = Reader::new(
        frame(&[(b"Nabc".to_vec(), 3), (b"Ndef".to_vec(), 3)]),
        &owner,
    );
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &owner).unwrap();
    let base = owner.snapshot().unwrap().retained_bytes;
    let span = ranges
        .read_checked_owned_span(1, 4, &Md5::digest(b"bcde"), &owner)
        .unwrap();
    assert_eq!(span.as_ref(), b"bcde");
    drop(span);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base + 12);
    assert_eq!(
        ranges
            .read_checked_owned_span(1, 4, &[1; 16], &owner)
            .err()
            .unwrap()
            .code,
        "container.integrity_mismatch"
    );
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base + 24);
}
#[test]
fn zero_length_and_invalid_digest_keep_scope_first_without_body_io() {
    let owner = budget(1024);
    let reader = Reader::new(n_frame(b"abc"), &owner);
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &owner).unwrap();
    let calls = reader.calls.get();
    let base = owner.snapshot().unwrap().retained_bytes;
    let span = ranges
        .read_checked_owned_span(0, 0, &Md5::digest([]), &owner)
        .unwrap();
    assert!(span.is_empty());
    drop(span);
    assert_eq!(
        ranges
            .read_checked_owned_span(usize::MAX, 1, &[0; 16], &owner)
            .err()
            .unwrap()
            .code,
        "container.integrity_mismatch"
    );
    assert_eq!(reader.calls.get(), calls);
    reader.control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        ranges
            .read_checked_owned_span(usize::MAX, 1, &[0; 16], &owner)
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    assert_eq!(
        ranges
            .read_checked_owned_span(0, 0, &Md5::digest([]), &owner)
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    assert_eq!(reader.calls.get(), calls);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base);
}
#[test]
fn post_read_origin_stop_releases_owned_n_before_it_can_enter_cache() {
    let owner = budget(1024);
    let reader = Reader::new(n_frame(b"abcde"), &owner);
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &owner).unwrap();
    let base = owner.snapshot().unwrap().retained_bytes;
    reader.cancel_body.set(true);
    assert_eq!(
        ranges
            .read_checked_owned_span(0, 5, &Md5::digest(b"abcde"), &owner)
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    assert!(owner.check().is_ok());
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base);
}
#[test]
fn private_range_rejects_a_different_owner_without_io_or_ledger_transfer() {
    let owner = budget(1024);
    let reader = Reader::new(n_frame(b"abc"), &owner);
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &owner).unwrap();
    let base = owner.snapshot().unwrap();
    let calls = reader.calls.get();
    for other in [budget(1024), Arc::new(owner.safety_only())] {
        assert_eq!(
            ranges
                .read_checked_owned_span(0, 3, &Md5::digest(b"abc"), &other)
                .err()
                .unwrap()
                .code,
            "records.internal"
        );
        assert_eq!(reader.calls.get(), calls);
        assert_eq!(owner.snapshot().unwrap(), base);
    }
}
#[test]
fn missing_key_retains_work_but_refunds_the_unreturned_page() {
    let owner = budget(1 << 20);
    let mut encoded = vec![b'E', 8];
    encoded.extend_from_slice(&42u64.to_le_bytes());
    encoded.extend_from_slice(&[4, 0, 0, 0, 0, b'S', 1, 2, 3, 4]);
    let reader = Reader::new(frame(&[(encoded, 3)]), &owner);
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &owner).unwrap();
    let base = owner.snapshot().unwrap().retained_bytes;
    let error = ranges
        .read_checked_owned_span(0, 3, &Md5::digest(b"abc"), &owner)
        .err()
        .unwrap();
    assert_eq!(error.code, "container.key_unavailable");
    assert_eq!(error.details["missingKey"], true);
    assert_eq!(owner.snapshot().unwrap().retained_bytes, base + 6);
    assert!(owner.snapshot().unwrap().decode_work > 0);
}

#[test]
fn independent_budget_stop_blocks_private_transfer_without_cancelling_origin() {
    let original = OperationControl::default();
    let independent = OperationControl::default();
    let owner = Arc::new(Budget::controlled(
        Cost::defaults(),
        independent.deadline(),
        independent.clone(),
    ));
    let raw = EncodedSpan::read_owned(8, Some(&owner), |_| Ok(())).unwrap();
    independent.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        raw.into_decoded_owned(&owner, || original.check())
            .err()
            .unwrap()
            .code,
        "command.cancelled"
    );
    assert!(original.check().is_ok());
    assert_eq!(owner.snapshot().unwrap().retained_bytes, 0);
    let expired = Arc::new(Budget::new(Cost::defaults(), Instant::now(), None));
    assert_eq!(
        EncodedSpan::borrowed(b"abc")
            .into_decoded_owned(&expired, || original.check())
            .err()
            .unwrap()
            .code,
        "records.query_resource_budget"
    );
    assert!(original.check().is_ok());
    assert_eq!(expired.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn unbudgeted_owned_input_transfers_without_copy_or_failed_owner_charge() {
    let owner = budget(8);
    // The pre-existing allocation was made without a ledger. Only subsequent
    // ownership admission is under test; its earlier peak is not reclassified.
    let raw = EncodedSpan::read_owned(8, None, |out| {
        out.copy_from_slice(b"existing");
        Ok(())
    })
    .unwrap();
    let pointer = raw.as_ptr();
    assert_eq!(owner.snapshot().unwrap().retained_bytes, 0);
    let span = raw.into_decoded_owned(&owner, || Ok(())).unwrap();
    assert_eq!(span.as_ptr(), pointer);
    assert_eq!(span.as_ref(), b"existing");
    assert_eq!(owner.snapshot().unwrap().retained_bytes, 8);
    assert_eq!(owner.logical_snapshot().unwrap(), Cost::default());
    drop(span);
    assert_eq!(owner.snapshot().unwrap(), Cost::default());
    let too_small = budget(7);
    let raw = EncodedSpan::read_owned(8, None, |_| Ok(())).unwrap();
    assert_eq!(
        raw.into_decoded_owned(&too_small, || Ok(()))
            .err()
            .unwrap()
            .code,
        "records.query_resource_budget"
    );
    // The consuming call has no escaped buffer; its owned Vec drops on this
    // failure. Admission is atomic and cannot debit/refund another owner.
    assert_eq!(too_small.snapshot().unwrap(), Cost::default());
    assert_eq!(too_small.logical_snapshot().unwrap(), Cost::default());
}
