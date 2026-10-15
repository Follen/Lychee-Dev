use super::*;
use crate::blte::{ByteSource, EncodedRead, Ranges};
use lycheedev_core::control::OperationControl;
use std::time::{Duration, Instant};

fn budget(retained: u64) -> Budget {
    Budget::new(
        Cost {
            retained_bytes: retained,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(5),
        None,
    )
}
#[test]
fn owned_span_holds_admission_through_forwarding_and_releases_only_storage() {
    fn forward(span: EncodedSpan<'_>) -> EncodedSpan<'_> {
        span
    }
    let budget = budget(12);
    let span = EncodedSpan::read_owned(8, Some(&budget), |out| {
        out.fill(3);
        budget.charge(Cost {
            decode_work: 1,
            network_bytes: 8,
            network_requests: 1,
            ..Cost::default()
        })
    })
    .unwrap();
    let span = forward(span);
    assert_eq!(&*span, &[3; 8]);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 8);
    assert!(
        EncodedSpan::read_owned(5, Some(&budget), |_| panic!("failed admission cannot fill"))
            .is_err()
    );
    drop(span);
    let used = budget.snapshot().unwrap();
    assert_eq!(
        (
            used.retained_bytes,
            used.decode_work,
            used.network_bytes,
            used.network_requests
        ),
        (0, 1, 8, 1)
    );
    assert_eq!(budget.logical_snapshot().unwrap().retained_bytes, 0);
}
#[test]
fn failed_fill_and_post_read_cancellation_refund_allocation_not_work() {
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let error = EncodedSpan::read_owned(7, Some(&budget), |_| {
        budget.charge(Cost::work(1))?;
        Err(fault("test.read", 5, "read failed"))
    })
    .err()
    .unwrap();
    assert_eq!(error.code, "test.read");
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    let error = EncodedSpan::read_owned(7, Some(&budget), |_| {
        budget.charge(Cost::work(1))?;
        control.tracked_cancel_handle().unwrap().cancel();
        Ok(())
    })
    .err()
    .unwrap();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(budget.snapshot().unwrap().decode_work, 2);
}
#[test]
fn owned_decoded_copy_requires_real_double_live_peak_and_keeps_origin_ledger() {
    let origin = budget(8);
    let alternate = budget(100);
    let raw = EncodedSpan::read_owned(5, Some(&origin), |out| {
        out.copy_from_slice(b"abcde");
        Ok(())
    })
    .unwrap();
    assert_eq!(
        raw.decoded_copy(Some(&alternate), || Ok(()))
            .unwrap_err()
            .code,
        "records.query_resource_budget"
    );
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(alternate.snapshot().unwrap().retained_bytes, 0);
    let origin = budget(10);
    let raw = EncodedSpan::read_owned(5, Some(&origin), |out| {
        out.copy_from_slice(b"abcde");
        Ok(())
    })
    .unwrap();
    let source_ptr = raw.as_ptr();
    let output = raw.decoded_copy(Some(&alternate), || Ok(())).unwrap();
    assert_eq!(output.as_ref(), b"abcde");
    assert_ne!(output.as_ptr(), source_ptr);
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 5);
    assert_eq!(alternate.snapshot().unwrap().retained_bytes, 0);
    drop(output);
    // ByteSource's existing conservative output accounting lasts to query end.
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 5);
}
#[test]
fn borrowed_decoded_span_remains_zero_copy_and_unbudgeted_owned_uses_ranges_ledger() {
    let original = b"borrowed";
    let budget = budget(16);
    let output = EncodedSpan::borrowed(original)
        .decoded_copy(Some(&budget), || Ok(()))
        .unwrap();
    assert_eq!(output.as_ptr(), original.as_ptr());
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    let raw = EncodedSpan::read_owned(8, None, |out| {
        out.copy_from_slice(original);
        Ok(())
    })
    .unwrap();
    assert_eq!(
        raw.decoded_copy(Some(&budget), || Ok(())).unwrap().as_ref(),
        original
    );
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 8);
}
struct Reader<'a> {
    raw: Vec<u8>,
    budget: &'a Budget,
}
impl EncodedRead for Reader<'_> {
    fn check_encoded(&self) -> Result<()> {
        self.budget.check()
    }
    fn encoded_size(&self) -> usize {
        self.raw.len()
    }
    fn read_encoded(&self, at: usize, bytes: usize) -> Result<EncodedSpan<'_>> {
        EncodedSpan::read_owned(bytes, Some(self.budget), |out| {
            out.copy_from_slice(&self.raw[at..at + bytes]);
            Ok(())
        })
    }
}
fn framed(payload: &[u8]) -> Vec<u8> {
    let chunk = [b"N".as_slice(), payload].concat();
    let mut raw = b"BLTE".to_vec();
    raw.extend_from_slice(&36u32.to_be_bytes());
    raw.extend_from_slice(&[15, 0, 0, 1]);
    raw.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    raw.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    raw.extend_from_slice(&Md5::digest(&chunk));
    raw.extend(chunk);
    raw
}
#[test]
fn unbudgeted_ranges_preserve_budgeted_reader_n_output_and_integrity_failures_refund() {
    let budget = budget(1000);
    let reader = Reader {
        raw: framed(b"abcde"),
        budget: &budget,
    };
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader(&reader, Limits::default(), &keys).unwrap();
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    let error = ranges.read_checked_span(0, 5, &[1; 16]).unwrap_err();
    assert_eq!(error.code, "container.integrity_mismatch");
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(
        ranges
            .read_checked_span(0, 5, &Md5::digest(b"abcde"))
            .unwrap()
            .as_ref(),
        b"abcde"
    );
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 5);
}
#[test]
fn independent_ranges_stop_still_blocks_owned_output_on_origin_ledger() {
    let origin = budget(1000);
    let control = OperationControl::default();
    let alternate = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let raw = EncodedSpan::read_owned(5, Some(&origin), |out| {
        out.copy_from_slice(b"abcde");
        Ok(())
    })
    .unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        raw.decoded_copy(Some(&alternate), || Ok(()))
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(alternate.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn ranges_with_different_budget_preserves_reader_output_accounting_and_range_stop() {
    let origin = budget(1000);
    let control = OperationControl::default();
    let alternate = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let reader = Reader {
        raw: framed(b"abcde"),
        budget: &origin,
    };
    let keys = BTreeMap::new();
    let ranges =
        Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &alternate).unwrap();
    let base = alternate.snapshot().unwrap();
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 0);
    let output = ranges
        .read_checked_span(0, 5, &Md5::digest(b"abcde"))
        .unwrap();
    assert_eq!(output.as_ref(), b"abcde");
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 5);
    assert_eq!(alternate.snapshot().unwrap(), base);
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        ranges
            .read_checked_span(0, 5, &Md5::digest(b"abcde"))
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 5);
}
#[test]
fn invalid_directory_releases_all_temporary_encoded_spans() {
    let budget = budget(1000);
    let mut reader = Reader {
        raw: framed(b"abcde"),
        budget: &budget,
    };
    reader.raw[12..16].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(Ranges::open_reader(&reader, Limits::default(), &BTreeMap::new()).is_err());
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn checked_empty_ranges_do_not_read_source_and_bad_expected_precedes_extent_or_io() {
    use std::cell::Cell;
    struct Counting {
        raw: Vec<u8>,
        calls: Cell<usize>,
        fail: Cell<bool>,
    }
    impl EncodedRead for Counting {
        fn encoded_size(&self) -> usize {
            self.raw.len()
        }
        fn read_encoded(&self, at: usize, bytes: usize) -> Result<EncodedSpan<'_>> {
            self.calls.set(self.calls.get() + 1);
            if self.fail.get() {
                return Err(fault("test.source", 5, "must not read"));
            }
            Ok(EncodedSpan::borrowed(&self.raw[at..at + bytes]))
        }
    }
    let reader = Counting {
        raw: framed(b"abcde"),
        calls: Cell::new(0),
        fail: Cell::new(false),
    };
    let keys = BTreeMap::new();
    let ranges = Ranges::open_reader(&reader, Limits::default(), &keys).unwrap();
    let calls = reader.calls.get();
    reader.fail.set(true);
    for offset in [0, 2, 5] {
        assert!(
            ranges
                .read_checked_span(offset, 0, &Md5::digest(b""))
                .unwrap()
                .is_empty()
        );
        assert!(ranges.read_span(offset, 0).unwrap().is_empty());
    }
    for expected in [&[][..], &[0u8; 16][..]] {
        assert_eq!(
            ranges
                .read_checked_span(usize::MAX, 1, expected)
                .unwrap_err()
                .code,
            "container.integrity_mismatch"
        );
        assert_eq!(
            ranges.read_checked_span(0, 1, expected).unwrap_err().code,
            "container.integrity_mismatch"
        );
    }
    assert_eq!(reader.calls.get(), calls);
}
#[test]
fn checked_n_declared_length_mismatch_is_integrity_and_original_stop_stays_first() {
    let mut raw = framed(b"abcde");
    raw[16..20].copy_from_slice(&4u32.to_be_bytes());
    let keys = BTreeMap::new();
    let ranges = Ranges::open(&raw, Limits::default(), &keys).unwrap();
    assert_eq!(
        ranges
            .read_checked_span(0, 4, &Md5::digest(b"abcd"))
            .unwrap_err()
            .code,
        "container.integrity_mismatch"
    );
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let reader = Reader {
        raw: framed(b"abcde"),
        budget: &budget,
    };
    let ranges = Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &budget).unwrap();
    control.tracked_cancel_handle().unwrap().cancel();
    assert_eq!(
        ranges
            .read_checked_span(usize::MAX, 1, &[0; 16])
            .unwrap_err()
            .code,
        "command.cancelled"
    );
}

#[test]
fn logical_header_admission_precedes_header_flag_decode() {
    let origin = budget(1000);
    let logical = budget(35);
    let mut raw = framed(b"abcde");
    raw[8] = 0; // Header size36 is valid, flag is not.
    let reader = Reader {
        raw,
        budget: &origin,
    };
    let error =
        Ranges::open_reader_budgeted(&reader, Limits::default(), &BTreeMap::new(), &logical)
            .err()
            .unwrap();
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(origin.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(logical.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn original_reader_stop_precedes_empty_span_and_bad_digest_in_both_mixed_entries() {
    for separate in [false, true] {
        let control = OperationControl::default();
        let origin = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let alternate = budget(1000);
        let reader = Reader {
            raw: framed(b"abcde"),
            budget: &origin,
        };
        let keys = BTreeMap::new();
        let ranges = if separate {
            Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &alternate).unwrap()
        } else {
            Ranges::open_reader(&reader, Limits::default(), &keys).unwrap()
        };
        control.tracked_cancel_handle().unwrap().cancel();
        assert_eq!(
            ranges.read_span(0, 0).unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(
            ranges
                .read_checked_span(0, 0, &Md5::digest(b""))
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        assert_eq!(
            ranges
                .read_checked_span(usize::MAX, 1, &[0; 16])
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        assert_eq!(origin.snapshot().unwrap().retained_bytes, 0);
        assert!(alternate.check().is_ok());
    }
}

#[test]
fn original_reader_stop_during_n_copy_and_before_commit_refunds_both_allocations() {
    use std::cell::Cell;
    struct CopyStop<'a> {
        raw: Vec<u8>,
        budget: &'a Budget,
        control: OperationControl,
        payload_bytes: usize,
        copy_ready: Cell<bool>,
        copy_checks: Cell<usize>,
        stop_at: usize,
    }
    impl EncodedRead for CopyStop<'_> {
        fn check_encoded(&self) -> Result<()> {
            if self.copy_ready.get() {
                let next = self.copy_checks.get() + 1;
                self.copy_checks.set(next);
                if next == self.stop_at {
                    self.control.tracked_cancel_handle().unwrap().cancel();
                }
            }
            self.control.check()?;
            self.budget.check()
        }
        fn encoded_size(&self) -> usize {
            self.raw.len()
        }
        fn read_encoded(&self, at: usize, bytes: usize) -> Result<EncodedSpan<'_>> {
            let span = EncodedSpan::read_owned(bytes, Some(self.budget), |out| {
                out.copy_from_slice(&self.raw[at..at + bytes]);
                Ok(())
            })?;
            if bytes == self.payload_bytes {
                self.copy_ready.set(true);
            }
            Ok(span)
        }
    }
    let payload = vec![42; 131072];
    // Entry, first chunk, second chunk, pre-commit: stop during the copy or
    // after all bytes were copied. The original reader control is independent
    // of both allocation ledgers, and no extra source read triggers its stop.
    for (stop_at, mixed) in [(3, false), (4, false), (3, true), (4, true)] {
        let origin = budget(2 * payload.len() as u64);
        let alternate = budget(4 * payload.len() as u64);
        let reader = CopyStop {
            raw: framed(&payload),
            budget: &origin,
            control: OperationControl::default(),
            payload_bytes: payload.len(),
            copy_ready: Cell::new(false),
            copy_checks: Cell::new(0),
            stop_at,
        };
        let keys = BTreeMap::new();
        let ranges = if mixed {
            Ranges::open_reader_budgeted(&reader, Limits::default(), &keys, &alternate)
        } else {
            Ranges::open_reader(&reader, Limits::default(), &keys)
        }
        .unwrap();
        let alternate_before = alternate.snapshot().unwrap();
        let error = ranges
            .read_checked_span(0, payload.len(), &Md5::digest(&payload))
            .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        assert_eq!(reader.copy_checks.get(), stop_at);
        assert_eq!(origin.snapshot().unwrap().retained_bytes, 0);
        assert_eq!(alternate.snapshot().unwrap(), alternate_before);
    }
}
