use super::*;
use crate::test_support::TestDir;
use std::{
    io::Write,
    time::{Duration, Instant},
};
pub(in crate::modus) fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn var(mut value: u64) -> Vec<u8> {
    let mut out = Vec::new();
    while value >= 128 {
        out.push((value as u8) | 128);
        value >>= 7;
    }
    out.push(value as u8);
    out
}
fn field(bytes: &[u8]) -> Vec<u8> {
    let mut v = var(bytes.len() as u64);
    v.extend_from_slice(bytes);
    v
}
fn crc(bytes: &[u8]) -> u32 {
    // Separate fixture writer, checked against the standard CRC32C check value.
    let mut crc = u32::MAX;
    for b in bytes {
        crc ^= u32::from(*b);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0x82f63b78 & mask);
        }
    }
    !crc
}
fn masked(bytes: &[u8]) -> u32 {
    crc(bytes).rotate_right(15).wrapping_add(0xa282ead8)
}
fn physical(kind: u8, bytes: &[u8]) -> Vec<u8> {
    let mut input = vec![kind];
    input.extend_from_slice(bytes);
    let mut out = masked(&input).to_le_bytes().to_vec();
    out.extend_from_slice(&(bytes.len() as u16).to_le_bytes());
    out.push(kind);
    out.extend_from_slice(bytes);
    out
}
fn key(name: &str) -> Vec<u8> {
    let mut out = b"_http://tongji.modus.cool\0\x01".to_vec();
    out.extend_from_slice(name.as_bytes());
    out
}
fn value(text: &str) -> Vec<u8> {
    let mut out = vec![0];
    for unit in text.encode_utf16() {
        out.extend_from_slice(&unit.to_le_bytes());
    }
    out
}
fn batch(sequence: u64, values: &[(Vec<u8>, Option<Vec<u8>>)]) -> Vec<u8> {
    let mut out = sequence.to_le_bytes().to_vec();
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());
    for (key, value) in values {
        out.push(u8::from(value.is_some()));
        out.extend(field(key));
        if let Some(value) = value {
            out.extend(field(value));
        }
    }
    out
}
pub(in crate::modus) fn fixture(
    root: &Path,
    user: Option<&str>,
    device: Option<&str>,
    build: Option<u8>,
) {
    fs::create_dir_all(root).unwrap();
    fs::write(root.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
    fs::write(
        root.join("MANIFEST-000001"),
        physical(1, b"\x02\x04\x09\x00"),
    )
    .unwrap();
    let values = vec![
        (key("user"), user.map(value)),
        (key("app"), device.map(value)),
        (
            key("wow"),
            build.map(|n| value(&json!({"currentGameWow":{"id":n}}).to_string())),
        ),
    ];
    fs::write(root.join("000004.log"), physical(1, &batch(20, &values))).unwrap();
}
fn fixture_block(rows: &[(Vec<u8>, Vec<u8>)], compressed: bool) -> (Vec<u8>, usize) {
    let mut raw = Vec::new();
    for (key, value) in rows {
        raw.push(0);
        raw.extend(var(key.len() as u64));
        raw.extend(var(value.len() as u64));
        raw.extend(key);
        raw.extend(value);
    }
    raw.extend(0u32.to_le_bytes());
    raw.extend(1u32.to_le_bytes());
    let mut out = if compressed {
        let mut enc = var(raw.len() as u64);
        enc.push(60 << 2);
        enc.push((raw.len() - 1) as u8);
        enc.extend(raw);
        enc
    } else {
        raw
    };
    let size = out.len();
    out.push(u8::from(compressed));
    out.extend(masked(&out).to_le_bytes());
    (out, size)
}
fn fixture_table(rows: Vec<(u64, Vec<u8>, Vec<u8>)>, compressed: bool) -> Vec<u8> {
    let rows = rows
        .into_iter()
        .map(|(seq, mut key, value)| {
            key.extend(((seq << 8) | 1).to_le_bytes());
            (key, value)
        })
        .collect::<Vec<_>>();
    let (mut out, size) = fixture_block(&rows, compressed);
    let mut handle = var(0);
    handle.extend(var(size as u64));
    let index_at = out.len();
    let (index, index_size) = fixture_block(&[(b"last".to_vec(), handle)], false);
    out.extend(index);
    let mut footer = vec![0, 0];
    footer.extend(var(index_at as u64));
    footer.extend(var(index_size as u64));
    footer.resize(40, 0);
    out.extend(footer);
    out.extend([0x57, 0xfb, 0x80, 0x8b, 0x24, 0x75, 0x47, 0xdb]);
    out
}
#[test]
fn current_tables_wal_sequences_and_tombstones_select_only_the_modus_origin() {
    assert_eq!(crc(b"123456789"), 0xe3069283);
    assert_eq!(
        snappy(&[12, 8, b'a', b'b', b'c', 34, 3, 0]).unwrap(),
        b"abcabcabcabc"
    );
    for compressed in [false, true] {
        let d = TestDir::new();
        fixture(&d.0, None, None, None);
        let table = fixture_table(
            vec![
                (5, key("user"), value("{\"token\":\"table-old\"}")),
                (30, key("wow"), value("{\"currentGameWow\":{\"id\":3}}")),
            ],
            compressed,
        );
        fs::write(d.0.join("000003.ldb"), &table).unwrap();
        let mut edit = vec![7, 0, 3];
        edit.extend(var(table.len() as u64));
        edit.extend([0, 0]);
        fs::OpenOptions::new()
            .append(true)
            .open(d.0.join("MANIFEST-000001"))
            .unwrap()
            .write_all(&physical(1, &edit))
            .unwrap();
        fs::write(
            d.0.join("000002.log"),
            b"obsolete corrupt file is never scanned",
        )
        .unwrap();
        fs::write(
            d.0.join("000009.ldb"),
            b"orphan corrupt table is never scanned",
        )
        .unwrap();
        let values = read(&d.0, &control()).unwrap();
        assert!(!values.contains_key("user"));
        assert!(values["wow"].contains('3'));
        let mut foreign = b"_https://unrelated.invalid\0\x01user".to_vec();
        fs::write(
            d.0.join("000004.log"),
            physical(
                1,
                &batch(
                    31,
                    &[
                        (
                            key("user"),
                            Some(value("{\"token\":\"current\",\"name\":\"作者\"}")),
                        ),
                        (key("wow"), None),
                        (
                            std::mem::take(&mut foreign),
                            Some(value("{\"token\":\"foreign\"}")),
                        ),
                    ],
                ),
            ),
        )
        .unwrap();
        // Filename order must not override the original sequence numbers.
        fs::write(
            d.0.join("000005.log"),
            physical(
                1,
                &batch(
                    1,
                    &[(key("user"), Some(value("{\"token\":\"older-wal\"}")))],
                ),
            ),
        )
        .unwrap();
        let values = read(&d.0, &control()).unwrap();
        assert_eq!(values.len(), 1);
        assert!(values["user"].contains("current"));
        assert!(values["user"].contains("作者"));
        let mut bytes = fs::read(d.0.join("000004.log")).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(d.0.join("000004.log"), bytes).unwrap();
        let e = read(&d.0, &control()).unwrap_err();
        assert!(!e.to_string().contains("table-old"));
    }
}
#[test]
fn fragmented_committed_wal_ignores_partial_tail_and_manifest_removes_old_tables() {
    let d = TestDir::new();
    fixture(&d.0, None, None, None);
    let raw = batch(
        40,
        &[(
            key("user"),
            Some(value(
                &json!({"token":"current","padding":"x".repeat(22000)}).to_string(),
            )),
        )],
    );
    let mut bytes = physical(2, &raw[..32761]);
    bytes.extend(physical(4, &raw[32761..]));
    let tail = physical(1, &batch(50, &[(key("user"), None)]));
    bytes.extend_from_slice(&tail[..tail.len() - 3]);
    fs::write(d.0.join("000004.log"), bytes).unwrap();
    assert!(read(&d.0, &control()).unwrap()["user"].contains("current"));
    fs::OpenOptions::new()
        .append(true)
        .open(d.0.join("MANIFEST-000001"))
        .unwrap()
        .write_all(&physical(1, &[7, 0, 3, 1, 0, 0, 6, 0, 3]))
        .unwrap();
    fs::write(d.0.join("000003.ldb"), b"deleted table").unwrap();
    assert!(read(&d.0, &control()).is_ok());
    fs::write(d.0.join("CURRENT"), b"../MANIFEST-000001").unwrap();
    assert!(read(&d.0, &control()).is_err());
}
