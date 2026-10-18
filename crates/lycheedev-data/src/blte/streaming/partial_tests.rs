use super::*;
use lycheedev_core::control::OperationControl;
use std::sync::atomic::Ordering;

// Leaf decoder tests inject fixed limits; real workspace admission is exercised
// by the local/CDN consumer fixtures, not this in-memory reader.

fn framed(corrupt: bool) -> (Vec<u8>, Vec<u8>) {
    let mut logical = b"clear".to_vec();
    logical.extend(2u32.to_le_bytes());
    logical.extend(99u32.to_le_bytes());
    let chunks = [
        b"Nclear".to_vec(),
        vec![
            b'E', 8, 0xef, 0xcd, 0xab, 0x90, 0x78, 0x56, 0x34, 0x12, 4, 0x11, 0x22, 0x33, 0x44,
            b'S', 0xc6, 0xab, 0x63, 0xb7, 0xda, 0xa0, 0x5c, 0xd3, 0x83,
        ],
    ];
    let mut encoded = b"BLTE".to_vec();
    encoded.extend(60u32.to_be_bytes());
    encoded.extend([15, 0, 0, 2]);
    for (chunk, bytes) in chunks.iter().zip([5u32, 8]) {
        encoded.extend((chunk.len() as u32).to_be_bytes());
        encoded.extend(bytes.to_be_bytes());
        encoded.extend(Md5::digest(chunk));
    }
    for chunk in chunks {
        encoded.extend(chunk);
    }
    if corrupt {
        *encoded.last_mut().unwrap() ^= 1;
    }
    (encoded, logical)
}
#[test]
fn table_policy_keeps_exact_missing_span_and_strict_reader_rejects_it() {
    let (encoded, logical) = framed(false);
    for policy in [DecodePolicy::Strict, DecodePolicy::TableAllowMissing] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control);
        let mut input = io::Cursor::new(&encoded);
        let keys = BTreeMap::new();
        let mut reader = Reader::new(
            &mut input,
            encoded.len(),
            logical.len(),
            Md5::digest(&logical).into(),
            &keys,
            &budget,
        )
        .unwrap()
        .with_policy(policy);
        let mut out = Vec::new();
        let result = reader.read_to_end(&mut out);
        if policy == DecodePolicy::Strict {
            assert_eq!(
                stream_io_error(result.unwrap_err()).code,
                "container.key_unavailable"
            );
        } else {
            result.unwrap();
            assert_eq!(&out[..5], b"clear");
            assert_eq!(&out[5..], &[0; 8]);
            assert!(reader.finished);
            let gaps = reader.take_missing();
            assert_eq!(gaps.len(), 1);
            assert_eq!((gaps[0].offset, gaps[0].bytes, gaps[0].chunk), (5, 8, 1));
            assert_eq!(gaps[0].key_id, "1234567890abcdef");
        }
    }
}
#[test]
fn table_policy_still_checks_ciphertext_keys_whole_ckey_eof_and_original_stop() {
    let (valid, logical) = framed(false);
    for mode in ["checksum", "wrong-key", "wrong-ckey", "trailing", "cancel"] {
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let mut encoded = if mode == "checksum" {
            framed(true).0
        } else {
            valid.clone()
        };
        let extent = encoded.len();
        if mode == "trailing" {
            encoded.push(0);
        }
        let mut keys = BTreeMap::new();
        if matches!(mode, "wrong-key" | "wrong-ckey") {
            keys.insert(
                0x1234567890abcdef,
                if mode == "wrong-key" {
                    vec![255; 16]
                } else {
                    (0..16u8).collect()
                },
            );
        }
        let mut expected: [u8; 16] = Md5::digest(&logical).into();
        if mode == "wrong-ckey" {
            expected[0] ^= 1;
        }
        let mut input = io::Cursor::new(encoded);
        let mut reader = Reader::new(&mut input, extent, logical.len(), expected, &keys, &budget)
            .unwrap()
            .with_policy(DecodePolicy::TableAllowMissing);
        let mut clear = [0; 5];
        assert_eq!(reader.read(&mut clear).unwrap(), 5);
        if mode == "cancel" {
            control.cancel_handle().store(true, Ordering::Release);
        }
        let error = stream_io_error(reader.read_to_end(&mut Vec::new()).unwrap_err());
        if mode == "cancel" {
            assert_eq!(error.exit_code, 7);
        }
        if mode == "wrong-ckey" {
            assert_eq!(error.code, "records.content_integrity");
        }
        assert!(!reader.finished, "{mode} became a successful partial body");
    }
}
#[test]
fn table_policy_requires_every_outer_digest_but_complete_content_keeps_ckey_proof() {
    let (valid, logical) = framed(false);
    for zero in [20..36, 44..60] {
        for complete in [false, true] {
            let control = OperationControl::default();
            let budget = Budget::controlled(Cost::defaults(), control.deadline(), control);
            let mut encoded = valid.clone();
            encoded[zero.clone()].fill(0);
            let mut keys = BTreeMap::new();
            if complete {
                keys.insert(0x1234567890abcdef, (0..16u8).collect());
            }
            let mut input = io::Cursor::new(&encoded);
            let mut reader = Reader::new(
                &mut input,
                encoded.len(),
                logical.len(),
                Md5::digest(&logical).into(),
                &keys,
                &budget,
            )
            .unwrap()
            .with_policy(DecodePolicy::TableAllowMissing);
            let mut out = Vec::new();
            let result = reader.read_to_end(&mut out);
            if complete {
                result.unwrap();
                assert_eq!(out, logical);
                assert!(reader.take_missing().is_empty());
            } else {
                assert_eq!(
                    stream_io_error(result.unwrap_err()).code,
                    "container.integrity_mismatch"
                );
                assert!(!reader.finished);
            }
        }
    }
    let mut encoded = valid;
    encoded[20..36].fill(0);
    encoded[61] ^= 1;
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control);
    let keys = BTreeMap::new();
    let mut input = io::Cursor::new(&encoded);
    let mut reader = Reader::new(
        &mut input,
        encoded.len(),
        logical.len(),
        Md5::digest(&logical).into(),
        &keys,
        &budget,
    )
    .unwrap()
    .with_policy(DecodePolicy::TableAllowMissing);
    assert_eq!(
        stream_io_error(reader.read_to_end(&mut Vec::new()).unwrap_err()).code,
        "container.integrity_mismatch"
    );
    assert!(!reader.finished);
}
#[test]
fn nested_missing_key_marks_whole_outer_chunk_unavailable() {
    let (inner, logical) = framed(false);
    let nested = [b"F".as_slice(), inner.as_slice()].concat();
    let mut encoded = b"BLTE".to_vec();
    encoded.extend(36u32.to_be_bytes());
    encoded.extend([15, 0, 0, 1]);
    encoded.extend((nested.len() as u32).to_be_bytes());
    encoded.extend((logical.len() as u32).to_be_bytes());
    encoded.extend(Md5::digest(&nested));
    encoded.extend(nested);
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control);
    let keys = BTreeMap::new();
    let mut input = io::Cursor::new(&encoded);
    let mut reader = Reader::new(
        &mut input,
        encoded.len(),
        logical.len(),
        Md5::digest(&logical).into(),
        &keys,
        &budget,
    )
    .unwrap()
    .with_policy(DecodePolicy::TableAllowMissing);
    let mut out = Vec::new();
    reader.read_to_end(&mut out).unwrap();
    assert_eq!(out, vec![0; logical.len()]);
    let missing = reader.take_missing();
    assert_eq!(missing.len(), 1);
    assert_eq!(
        (missing[0].offset, missing[0].bytes, missing[0].chunk),
        (0, logical.len(), 0)
    );
    assert_eq!(missing[0].key_id, "1234567890abcdef");
}
