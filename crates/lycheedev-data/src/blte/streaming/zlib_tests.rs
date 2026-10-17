use super::*;
use lycheedev_core::control::OperationControl;
use std::{cell::Cell, io::Write};

fn compressed(bytes: &[u8]) -> Vec<u8> {
    let mut codec = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    codec.write_all(bytes).unwrap();
    [b"Z".as_slice(), &codec.finish().unwrap()].concat()
}
fn frame(parts: &[(Vec<u8>, usize)]) -> Vec<u8> {
    let header = 12 + 24 * parts.len();
    let mut bytes = b"BLTE".to_vec();
    bytes.extend_from_slice(&(header as u32).to_be_bytes());
    bytes.extend_from_slice(&[15, 0, 0, parts.len() as u8]);
    for (encoded, decoded) in parts {
        bytes.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&(*decoded as u32).to_be_bytes());
        bytes.extend_from_slice(&Md5::digest(encoded));
    }
    for (encoded, _) in parts {
        bytes.extend_from_slice(encoded);
    }
    bytes
}
fn body(size: usize) -> Vec<u8> {
    (0..size)
        .map(|i| ((i * 17 + i / 251) % 253) as u8)
        .collect()
}
fn budget(control: &OperationControl, work: u64) -> Budget {
    Budget::controlled(
        Cost {
            decode_work: work,
            ..Cost::defaults()
        },
        control.deadline(),
        control.clone(),
    )
}
fn reader<'a>(
    source: &'a mut dyn Read,
    encoded: usize,
    decoded: usize,
    key: [u8; 16],
    keys: &'a BTreeMap<u64, Vec<u8>>,
    budget: &'a Budget,
    incremental: bool,
) -> Reader<'a> {
    if incremental {
        Reader::new_incremental_with_check(source, encoded, decoded, key, keys, budget, None)
    } else {
        Reader::new(source, encoded, decoded, key, keys, budget)
    }
    .unwrap()
}
fn consume(reader: &mut Reader<'_>, size: usize) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    let mut out = vec![0; size];
    loop {
        let n = reader.read(&mut out).map_err(stream_io_error)?;
        if n == 0 {
            return Ok(result);
        }
        result.extend_from_slice(&out[..n]);
    }
}
fn error_for(
    raw: &[u8],
    decoded: usize,
    key: [u8; 16],
    incremental: bool,
) -> lycheedev_core::ToolkitError {
    let control = OperationControl::default();
    let budget = budget(&control, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let mut source = io::Cursor::new(raw);
    let mut reader = reader(
        &mut source,
        raw.len(),
        decoded,
        key,
        &keys,
        &budget,
        incremental,
    );
    consume(&mut reader, 65536).unwrap_err()
}

#[test]
fn incremental_z_matches_eager_bytes_and_exact_work_across_read_and_chunk_boundaries() {
    let first = body(2 * 65536 + 17);
    let second = body(65536 + 3);
    let expected = [&first[..], &second[..], b"tail"].concat();
    let raw = frame(&[
        (compressed(&[]), 0),
        (compressed(&first), first.len()),
        (compressed(&second), second.len()),
        (b"Ntail".to_vec(), 4),
        (compressed(&[]), 0),
    ]);
    for size in [1, 7, 65535, 65536, 100000] {
        let mut observed = Vec::new();
        for incremental in [false, true] {
            let control = OperationControl::default();
            let budget = budget(&control, Cost::defaults().decode_work);
            let keys = BTreeMap::new();
            let mut source = io::Cursor::new(&raw);
            let mut reader = reader(
                &mut source,
                raw.len(),
                expected.len(),
                Md5::digest(&expected).into(),
                &keys,
                &budget,
                incremental,
            );
            assert_eq!(consume(&mut reader, size).unwrap(), expected);
            assert_eq!(reader.read(&mut [0]).unwrap(), 0);
            drop(reader);
            observed.push((
                budget.snapshot().unwrap(),
                budget.logical_snapshot().unwrap(),
            ));
        }
        assert_eq!(observed[0], observed[1], "buffer size {size}");
    }
}

#[test]
fn incremental_z_three_large_blocks_keep_bounded_buffers_and_original_admission() {
    let expected = body(17 << 20);
    let parts: Vec<_> = expected
        .chunks(8 << 20)
        .map(|part| (compressed(part), part.len()))
        .collect();
    assert_eq!(parts.len(), 3);
    let raw = frame(&parts);
    let control = OperationControl::default();
    let budget = budget(&control, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let mut source = io::Cursor::new(&raw);
    let mut reader = reader(
        &mut source,
        raw.len(),
        expected.len(),
        Md5::digest(&expected).into(),
        &keys,
        &budget,
        true,
    );
    let mut buffer = [0; 65536];
    let mut offset = 0;
    let mut peak_buffers = 0;
    let mut peak_admitted = 0;
    loop {
        let n = reader.read(&mut buffer).unwrap();
        peak_buffers = peak_buffers.max(reader.zlib.as_ref().map_or(0, |z| z.buffer_capacity()));
        peak_admitted = peak_admitted.max(budget.snapshot().unwrap().retained_bytes);
        if n == 0 {
            break;
        }
        assert_eq!(&buffer[..n], &expected[offset..offset + n]);
        offset += n;
    }
    assert_eq!(offset, expected.len());
    let maximum_encoded = parts.iter().map(|(raw, _)| raw.len()).max().unwrap();
    assert!(peak_buffers <= maximum_encoded + 65536);
    assert!(
        peak_buffers < 1 << 20,
        "fixture's owned buffers must not retain decoded-sized storage"
    );
    assert!(
        peak_admitted >= 2 * (8 << 20),
        "original conservative guard remains; no ledger peak reduction claim"
    );
    drop(reader);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn incremental_z_matches_eager_single_faults_and_keeps_public_decoder_eager() {
    let bytes = body(3 * 65536 + 9);
    let encoded = compressed(&bytes);
    let mut corrupt = encoded.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    let mut trailing = encoded.clone();
    trailing.push(0);
    for (part, declared) in [
        (corrupt, bytes.len()),
        (trailing, bytes.len()),
        (encoded.clone(), bytes.len() - 1),
        (encoded.clone(), bytes.len() + 1),
    ] {
        let raw = frame(&[(part, declared)]);
        let eager = error_for(&raw, declared, Md5::digest(&bytes).into(), false);
        let streamed = error_for(&raw, declared, Md5::digest(&bytes).into(), true);
        assert_eq!(
            (streamed.code, streamed.exit_code, streamed.message),
            (eager.code, eager.exit_code, eager.message)
        );
    }
    // The public decoder remains the eager oracle, including its owned output.
    let raw = frame(&[(encoded, bytes.len())]);
    assert_eq!(
        super::super::decode(&raw, Limits::default(), &BTreeMap::new()).unwrap(),
        bytes
    );
}

#[test]
fn incremental_z_encoded_digest_failure_precedes_any_output() {
    let bytes = body(3 * 65536);
    let mut raw = frame(&[(compressed(&bytes), bytes.len())]);
    raw[20] ^= 1;
    let control = OperationControl::default();
    let budget = budget(&control, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let mut source = io::Cursor::new(&raw);
    let mut reader = reader(
        &mut source,
        raw.len(),
        bytes.len(),
        Md5::digest(&bytes).into(),
        &keys,
        &budget,
        true,
    );
    let mut out = [0x5a; 65536];
    assert_eq!(
        stream_io_error(reader.read(&mut out).unwrap_err()).code,
        "container.integrity_mismatch"
    );
    assert_eq!(out, [0x5a; 65536]);
    assert_eq!(reader.emitted, 0);
}

#[test]
fn incremental_z_terminal_probe_still_checks_whole_key_and_source_eof() {
    let bytes = body(65536 + 5);
    let raw = frame(&[(compressed(&bytes), bytes.len())]);
    for trailing in [false, true] {
        let mut input = raw.clone();
        if trailing {
            input.push(0);
        }
        let control = OperationControl::default();
        let budget = budget(&control, Cost::defaults().decode_work);
        let keys = BTreeMap::new();
        let mut source = io::Cursor::new(input);
        let key = if trailing {
            Md5::digest(&bytes).into()
        } else {
            [0; 16]
        };
        let mut reader = reader(
            &mut source,
            raw.len(),
            bytes.len(),
            key,
            &keys,
            &budget,
            true,
        );
        let mut out = [0; 65536];
        assert_eq!(reader.read(&mut out).unwrap(), 65536);
        assert_eq!(reader.read(&mut out).unwrap(), 5);
        assert_eq!(
            stream_io_error(reader.read(&mut [0]).unwrap_err()).code,
            if trailing {
                "container.invalid_format"
            } else {
                "records.content_integrity"
            }
        );
    }
}

#[test]
fn incremental_z_cancel_and_work_exhaustion_are_terminal_without_more_decode_or_next_chunk() {
    let bytes = body(4 * 65536);
    let first = compressed(&bytes);
    let raw = frame(&[(first.clone(), bytes.len()), (b"Nnext".to_vec(), 4)]);
    for cancel in [false, true] {
        let control = OperationControl::default();
        // MD5 + one inflate + one emission, then fail the second inflate charge.
        let work = if cancel {
            1000
        } else {
            first.len().div_ceil(65536) as u64 + 2
        };
        let budget = budget(&control, work);
        let keys = BTreeMap::new();
        let mut source = io::Cursor::new(&raw);
        let mut reader = reader(
            &mut source,
            raw.len(),
            bytes.len() + 4,
            [0; 16],
            &keys,
            &budget,
            true,
        );
        let mut out = [0; 65536];
        assert_eq!(reader.read(&mut out).unwrap(), 65536);
        if cancel {
            control.tracked_cancel_handle().unwrap().cancel();
        }
        let error = stream_io_error(reader.read(&mut out).unwrap_err());
        assert_eq!(
            error.code,
            if cancel {
                "command.cancelled"
            } else {
                "records.query_resource_budget"
            }
        );
        let used = budget.snapshot().unwrap().decode_work;
        assert_eq!(
            stream_io_error(reader.read(&mut out).unwrap_err()).code,
            error.code
        );
        assert_eq!(budget.snapshot().unwrap().decode_work, used);
        assert_eq!(reader.next, 1);
        drop(reader);
        assert_eq!(source.position(), (12 + 48 + first.len()) as u64);
        assert_eq!(budget.refundable_retained_bytes(), 0);
    }
}

#[test]
fn incremental_z_private_sink_failure_stops_before_unobserved_corruption_or_next_chunk() {
    let bytes = body(4 * 65536);
    let mut encoded = compressed(&bytes);
    *encoded.last_mut().unwrap() ^= 1;
    let raw = frame(&[(encoded.clone(), bytes.len()), (b"Nnext".to_vec(), 4)]);
    let control = OperationControl::default();
    let budget = budget(&control, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let mut source = io::Cursor::new(&raw);
    let mut reader = reader(
        &mut source,
        raw.len(),
        bytes.len() + 4,
        [0; 16],
        &keys,
        &budget,
        true,
    );
    struct FailSink(usize);
    impl Write for FailSink {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            self.0 += 1;
            Err(io::Error::other("private sink failed"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut sink = FailSink(0);
    let mut out = [0; 65536];
    let n = reader.read(&mut out).unwrap();
    let error = sink.write_all(&out[..n]).unwrap_err();
    assert_eq!(error.to_string(), "private sink failed");
    assert_eq!(sink.0, 1);
    assert_eq!(reader.next, 1);
    assert_eq!(reader.zlib.as_ref().unwrap().produced(), 65536);
    drop(reader);
    assert_eq!(source.position(), (12 + 48 + encoded.len()) as u64);
    assert_eq!(budget.refundable_retained_bytes(), 0);
    assert_eq!(
        error_for(&raw, bytes.len() + 4, [0; 16], false).code,
        "container.invalid_format"
    );
}

#[test]
fn incremental_z_original_scope_error_is_terminal_before_more_inflate() {
    let bytes = body(4 * 65536);
    let raw = frame(&[(compressed(&bytes), bytes.len())]);
    let control = OperationControl::default();
    let budget = budget(&control, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let stopped = Cell::new(false);
    let check = || {
        if stopped.get() {
            Err(fault("test.original_scope", 7, "original scope stopped"))
        } else {
            Ok(())
        }
    };
    let mut source = io::Cursor::new(&raw);
    let mut reader = Reader::new_incremental_with_check(
        &mut source,
        raw.len(),
        bytes.len(),
        Md5::digest(&bytes).into(),
        &keys,
        &budget,
        Some(&check),
    )
    .unwrap();
    assert_eq!(reader.read(&mut [0; 65536]).unwrap(), 65536);
    let produced = reader.zlib.as_ref().unwrap().produced();
    stopped.set(true);
    assert_eq!(
        stream_io_error(reader.read(&mut [0; 65536]).unwrap_err()).code,
        "test.original_scope"
    );
    assert_eq!(reader.zlib.as_ref().unwrap().produced(), produced);
}

#[test]
fn incremental_z_preserves_pinned_bufread_decoder_truncation_semantics() {
    let bytes = body(2 * 65536 + 9);
    let encoded = compressed(&bytes);
    for cut in 1..=8 {
        let raw = frame(&[(encoded[..encoded.len() - cut].to_vec(), bytes.len())]);
        let mut results = Vec::new();
        for incremental in [false, true] {
            let control = OperationControl::default();
            let budget = budget(&control, Cost::defaults().decode_work);
            let keys = BTreeMap::new();
            let mut source = io::Cursor::new(&raw);
            let mut reader = reader(
                &mut source,
                raw.len(),
                bytes.len(),
                Md5::digest(&bytes).into(),
                &keys,
                &budget,
                incremental,
            );
            results.push(consume(&mut reader, 65536).map_err(|e| (e.code, e.exit_code, e.message)));
        }
        assert_eq!(results[0], results[1], "truncated {cut} bytes");
    }
}

#[test]
fn incremental_z_tight_work_limit_records_streaming_order_without_exceeding_original_cap() {
    let bytes = body(4 * 65536);
    let mut encoded = compressed(&bytes);
    *encoded.last_mut().unwrap() ^= 1;
    let raw = frame(&[(encoded.clone(), bytes.len())]);
    let mut observed_difference = false;
    for limit in 0..32 {
        let mut errors = Vec::new();
        for incremental in [false, true] {
            let control = OperationControl::default();
            let budget = budget(&control, limit);
            let keys = BTreeMap::new();
            let mut source = io::Cursor::new(&raw);
            let mut reader = reader(
                &mut source,
                raw.len(),
                bytes.len(),
                [0; 16],
                &keys,
                &budget,
                incremental,
            );
            errors.push(consume(&mut reader, 65536).unwrap_err().code);
            assert!(budget.snapshot().unwrap().decode_work <= limit);
            drop(reader);
            assert_eq!(budget.refundable_retained_bytes(), 0);
        }
        observed_difference |=
            errors == ["container.invalid_format", "records.query_resource_budget"];
    }
    // Private streaming observes the interleaved output-work limit first.
    // Neither execution may add budget, refund completed work or publish bytes.
    assert!(observed_difference);
}

struct Home(std::path::PathBuf);
impl Home {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-incremental-z-{}-{}-{}",
            std::process::id(),
            lycheedev_core::storage::utc_now().replace(':', "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        lycheedev_core::workspace::initialize(&path).unwrap();
        Self(path)
    }
    fn no_blob_files(&self) {
        fn count(path: &std::path::Path) -> usize {
            std::fs::read_dir(path)
                .unwrap()
                .map(|entry| {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_dir() {
                        count(&entry.path())
                    } else {
                        1
                    }
                })
                .sum()
        }
        assert_eq!(count(&self.0.join("blobs")), 0);
    }
}
impl Drop for Home {
    fn drop(&mut self) {
        let result = std::fs::remove_dir_all(&self.0);
        if !std::thread::panicking() {
            result.unwrap();
        }
    }
}
fn publish_failure(raw: &[u8], declared: usize, key: [u8; 16]) -> lycheedev_core::ToolkitError {
    let home = Home::new();
    let control = OperationControl::default();
    let budget = budget(&control, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let mut source = io::Cursor::new(raw);
    let mut reader = reader(&mut source, raw.len(), declared, key, &keys, &budget, true);
    let owned = budget.safety_only();
    let result = lycheedev_core::evidence::publish_blob_stream_bounded_with_check(
        &home.0,
        &mut reader,
        declared as u64,
        declared as u64,
        &control,
        move || owned.check(),
        &budget.safety_only(),
    );
    let error = match result {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("expected rejection")
        }
    };
    assert_eq!(error.details["publication"]["state"], "not_published");
    drop(reader);
    home.no_blob_files();
    assert_eq!(budget.refundable_retained_bytes(), 0);
    error
}

#[test]
fn incremental_z_real_publisher_rejects_late_codec_size_key_and_eof_before_publication() {
    let bytes = body(4 * 65536);
    let key = Md5::digest(&bytes).into();
    let encoded = compressed(&bytes);
    let mut corrupt = encoded.clone();
    *corrupt.last_mut().unwrap() ^= 1;
    assert_eq!(
        publish_failure(&frame(&[(corrupt, bytes.len())]), bytes.len(), key).code,
        "container.invalid_format"
    );
    assert_eq!(
        publish_failure(
            &frame(&[(encoded.clone(), bytes.len() + 1)]),
            bytes.len() + 1,
            key
        )
        .code,
        "container.integrity_mismatch"
    );
    assert_eq!(
        publish_failure(
            &frame(&[(encoded.clone(), bytes.len())]),
            bytes.len(),
            [0; 16]
        )
        .code,
        "records.content_integrity"
    );
    let mut trailing = encoded;
    trailing.push(0);
    assert_eq!(
        publish_failure(&frame(&[(trailing, bytes.len())]), bytes.len(), key).code,
        "container.invalid_format"
    );
}

#[test]
fn incremental_z_real_publisher_consumer_work_failure_does_not_drain_corrupt_tail() {
    let home = Home::new();
    let bytes = body(4 * 65536);
    let mut encoded = compressed(&bytes);
    *encoded.last_mut().unwrap() ^= 1;
    let raw = frame(&[(encoded.clone(), bytes.len()), (b"Nnext".to_vec(), 4)]);
    let control = OperationControl::default();
    let budget = budget(&control, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let mut source = io::Cursor::new(&raw);
    let mut reader = reader(
        &mut source,
        raw.len(),
        bytes.len() + 4,
        [0; 16],
        &keys,
        &budget,
        true,
    );
    struct StopMeter(Cell<usize>);
    impl lycheedev_core::evidence::CaptureAccounting for StopMeter {
        fn check(&self) -> Result<()> {
            Ok(())
        }
        fn admit_allocation(&self, _: usize) -> Result<()> {
            panic!("caller owns buffer")
        }
        fn admit_work(&self, _: usize) -> Result<()> {
            self.0.set(self.0.get() + 1);
            if self.0.get() == 2 {
                Err(fault("test.consumer_budget", 3, "consumer work exhausted"))
            } else {
                Ok(())
            }
        }
    }
    let meter = StopMeter(Cell::new(0));
    let result = lycheedev_core::evidence::publish_blob_stream_bounded_with_check(
        &home.0,
        &mut reader,
        (bytes.len() + 4) as u64,
        (bytes.len() + 4) as u64,
        &control,
        || Ok(()),
        &meter,
    );
    let error = match result {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("expected rejection")
        }
    };
    assert_eq!(error.code, "test.consumer_budget");
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert_eq!(reader.emitted, 65536);
    assert_eq!(reader.zlib.as_ref().unwrap().produced(), 65536);
    assert_eq!(reader.next, 1);
    drop(reader);
    assert_eq!(source.position(), (12 + 48 + encoded.len()) as u64);
    assert_eq!(budget.refundable_retained_bytes(), 0);
    home.no_blob_files();
}

#[test]
fn incremental_z_real_publisher_matches_eager_blob_and_complete_work_totals() {
    let first = body(2 * 65536 + 17);
    let second = body(65536 + 3);
    let bytes = [&first[..], &second[..]].concat();
    let raw = frame(&[
        (compressed(&first), first.len()),
        (compressed(&second), second.len()),
    ]);
    let mut observations = Vec::new();
    for incremental in [false, true] {
        let home = Home::new();
        let control = OperationControl::default();
        let budget = budget(&control, Cost::defaults().decode_work);
        let keys = BTreeMap::new();
        let mut source = io::Cursor::new(&raw);
        let mut reader = reader(
            &mut source,
            raw.len(),
            bytes.len(),
            Md5::digest(&bytes).into(),
            &keys,
            &budget,
            incremental,
        );
        let owned = budget.safety_only();
        let mut blob = lycheedev_core::evidence::publish_blob_stream_bounded_with_check(
            &home.0,
            &mut reader,
            bytes.len() as u64,
            bytes.len() as u64,
            &control,
            move || owned.check(),
            &budget.safety_only(),
        )
        .unwrap();
        let reference = blob.reference().clone();
        let mut actual = Vec::new();
        blob.read_to_end(&mut actual).unwrap();
        assert_eq!(actual, bytes);
        blob.close().unwrap();
        drop(reader);
        assert_eq!(budget.refundable_retained_bytes(), 0);
        observations.push((
            reference.sha256,
            reference.bytes,
            budget.snapshot().unwrap(),
            budget.logical_snapshot().unwrap(),
        ));
    }
    assert_eq!(observations[0], observations[1]);
}

#[test]
fn incremental_z_preserves_typed_deadline_failure_before_encoded_body_read() {
    let bytes = body(2 * 65536);
    let raw = frame(&[(compressed(&bytes), bytes.len())]);
    let outer = OperationControl::default();
    let budget = budget(&outer, Cost::defaults().decode_work);
    let keys = BTreeMap::new();
    let expired = OperationControl::uncancelable(std::time::Instant::now());
    let stop = Cell::new(false);
    let check = || {
        if stop.get() {
            expired.check()
        } else {
            outer.check()
        }
    };
    let mut source = io::Cursor::new(&raw);
    let mut reader = Reader::new_incremental_with_check(
        &mut source,
        raw.len(),
        bytes.len(),
        [0; 16],
        &keys,
        &budget,
        Some(&check),
    )
    .unwrap();
    stop.set(true);
    assert_eq!(
        stream_io_error(reader.read(&mut [0; 65536]).unwrap_err()).code,
        "command.deadline"
    );
    assert_eq!(reader.next, 0);
    drop(reader);
    assert_eq!(source.position(), 36);
    assert_eq!(budget.refundable_retained_bytes(), 0);
}

#[test]
fn incremental_z_headerless_and_nested_fallback_match_eager_output_and_work() {
    let bytes = body(65536 + 3);
    let z = compressed(&bytes);
    let headerless = [b"BLTE\0\0\0\0".as_slice(), &z].concat();
    let nested = frame(&[([b"F".as_slice(), &headerless].concat(), bytes.len())]);
    for raw in [headerless, nested] {
        let mut observations = Vec::new();
        for incremental in [false, true] {
            let control = OperationControl::default();
            let budget = budget(&control, Cost::defaults().decode_work);
            let keys = BTreeMap::new();
            let mut source = io::Cursor::new(&raw);
            let mut reader = reader(
                &mut source,
                raw.len(),
                bytes.len(),
                Md5::digest(&bytes).into(),
                &keys,
                &budget,
                incremental,
            );
            assert_eq!(consume(&mut reader, 7).unwrap(), bytes);
            drop(reader);
            observations.push((
                budget.snapshot().unwrap(),
                budget.logical_snapshot().unwrap(),
            ));
        }
        assert_eq!(observations[0], observations[1]);
    }
}

#[test]
fn incremental_z_keeps_original_pre_read_memory_admission_for_every_mode() {
    for mode in [b'N', b'F', b'E', b'Z'] {
        let encoded = vec![mode, 0, 1, 2];
        let decoded = 65536;
        let mut raw = frame(&[(encoded.clone(), decoded)]);
        raw[20] ^= 1; // Also malformed/integrity-invalid, but memory stops first.
        for incremental in [false, true] {
            let control = OperationControl::default();
            let budget = Budget::controlled(
                Cost {
                    retained_bytes: (std::mem::size_of::<Part>()
                        + 2 * (encoded.len() + decoded)
                        + 65536
                        - 1) as u64,
                    ..Cost::defaults()
                },
                control.deadline(),
                control.clone(),
            );
            let keys = BTreeMap::new();
            let mut source = io::Cursor::new(&raw);
            let mut reader = reader(
                &mut source,
                raw.len(),
                decoded,
                [0; 16],
                &keys,
                &budget,
                incremental,
            );
            assert_eq!(
                stream_io_error(reader.read(&mut [0; 65536]).unwrap_err()).code,
                "records.query_resource_budget"
            );
            drop(reader);
            assert_eq!(source.position(), 36);
            assert_eq!(budget.refundable_retained_bytes(), 0);
        }
    }
}
