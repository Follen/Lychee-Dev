use lycheedev_data::{
    blte::{Content, Limits as BlteLimits},
    casc::{self, Index, Limits},
    metadata_format::{Encoding, RootLimits, lookup_root},
};
use md5::{Digest, Md5};
use std::{collections::BTreeMap, path::PathBuf};
fn index(key: &[u8; 16], bytes: u32) -> Vec<u8> {
    let mut raw = vec![0; 58];
    raw[..4].copy_from_slice(&16u32.to_le_bytes());
    raw[8..16].copy_from_slice(&[7, 0, 0, 0, 4, 5, 9, 30]);
    raw[16..24].copy_from_slice(&(1u64 << 30).to_le_bytes());
    let sum = casc::guard_sum(&raw[8..24], 0, 0).0;
    raw[4..8].copy_from_slice(&sum.to_le_bytes());
    raw[32..36].copy_from_slice(&18u32.to_le_bytes());
    raw[40..49].copy_from_slice(&key[..9]);
    raw[50..54].copy_from_slice(&((1u32 << 30) | 64).to_be_bytes());
    raw[54..58].copy_from_slice(&bytes.to_le_bytes());
    let sum = casc::guard_sum(&raw[40..], 0, 0).0;
    raw[36..40].copy_from_slice(&sum.to_le_bytes());
    raw
}
#[test]
fn guarded_index_reference_vectors_and_full_admission() {
    for (s, p, q, c, b) in [
        ("", 0, 0, 0xdeadbeef, 0xdeadbeef),
        ("", 0, 0xdeadbeef, 0xbd5b7dde, 0xdeadbeef),
        (
            "Four score and seven years ago",
            0,
            0,
            0x17770551,
            0xce7226e6,
        ),
        (
            "Four score and seven years ago",
            0,
            1,
            0xe3607cae,
            0xbd371de4,
        ),
        (
            "Four score and seven years ago",
            1,
            0,
            0xcd628161,
            0x6cbea4b3,
        ),
    ] {
        assert_eq!(casc::guard_sum(s.as_bytes(), p, q), (c, b));
    }
    let key = [1; 16];
    let raw = index(&key, 128);
    let idx = Index::open(&raw, 0, Limits::default(), None).unwrap();
    assert_eq!(
        idx.find(&key, Limits::default(), None).unwrap()[0],
        casc::Span {
            archive: 1,
            offset: 64,
            bytes: 128
        }
    );
    assert!(
        idx.find(&[2; 16], Limits::default(), None)
            .unwrap()
            .is_empty()
    );
    for at in [4, 57] {
        let mut raw = raw.clone();
        raw[at] ^= 1;
        assert!(Index::open(&raw, 0, Limits::default(), None).is_err());
    }
    assert!(Index::open(&raw, 1, Limits::default(), None).is_err());
    assert!(Index::open(&raw[..55], 0, Limits::default(), None).is_err());
    let cancel = lycheedev_core::control::TrackedCancellation::new();
    cancel.cancel();
    assert_eq!(
        Index::open(&raw, 0, Limits::default(), Some(&cancel))
            .err()
            .unwrap()
            .exit_code,
        7
    );
}
fn isolated() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/rust-casc-fixture")
        .join(format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
}
#[test]
fn local_archive_reads_newest_generation_and_checks_ekey_ckey_before_decode() {
    let root = isolated();
    std::fs::create_dir_all(root.join("Data/data")).unwrap();
    let payload = b"decoded authenticated content";
    let mut encoded = b"BLTE\0\0\0\0N".to_vec();
    encoded.extend_from_slice(payload);
    let ekey: [u8; 16] = Md5::digest(&encoded).into();
    let ckey = casc::hex(&Md5::digest(payload));
    let raw = index(&ekey, (encoded.len() + 30) as u32);
    std::fs::write(
        root.join("Data/data/0000000001.idx"),
        b"damaged old generation",
    )
    .unwrap();
    std::fs::write(root.join("Data/data/0000000002.idx"), &raw).unwrap();
    let mut archive = vec![0; 94];
    archive.extend_from_slice(&encoded);
    std::fs::write(root.join("Data/data/data.001"), &archive).unwrap();
    let object = casc::open_local(
        &root,
        &casc::hex(&ekey),
        encoded.len(),
        Limits::default(),
        None,
    )
    .unwrap();
    assert_eq!(object.index, "0000000002.idx");
    assert_eq!(
        object
            .decode(&ckey, BlteLimits::default(), &BTreeMap::new())
            .unwrap()
            .bytes,
        payload
    );
    assert_eq!(
        object
            .decode(&"0".repeat(32), BlteLimits::default(), &BTreeMap::new())
            .err()
            .unwrap()
            .code,
        "records.content_integrity"
    );
    // A valid prefix candidate must still fail a forged complete EKey.
    let mut collision = ekey;
    collision[15] ^= 1;
    assert_eq!(
        casc::open_local(
            &root,
            &casc::hex(&collision),
            encoded.len(),
            Limits::default(),
            None
        )
        .err()
        .unwrap()
        .code,
        "archive.index_integrity"
    );
    std::fs::write(
        root.join("Data/data/0000000003.idx"),
        b"damaged newest generation",
    )
    .unwrap();
    assert_eq!(
        casc::open_local(
            &root,
            &casc::hex(&ekey),
            encoded.len(),
            Limits::default(),
            None
        )
        .err()
        .unwrap()
        .code,
        "archive.invalid_index"
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn framed_encoding_identity_covers_directory_then_blte_covers_payload() {
    let mut encoded = b"BLTE\0\0\0\x24\x0f\0\0\x01".to_vec();
    let chunk = b"Nhello";
    encoded.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
    encoded.extend_from_slice(&5u32.to_be_bytes());
    encoded.extend_from_slice(&Md5::digest(chunk));
    encoded.extend_from_slice(chunk);
    let ekey: [u8; 16] = Md5::digest(&encoded[..36]).into();
    let mut archive = vec![0; 30];
    archive.extend_from_slice(&encoded);
    let original = casc::authenticate_span(&archive, &ekey, 1024).unwrap();
    assert_eq!(
        lycheedev_data::blte::decode(original, BlteLimits::default(), &BTreeMap::new()).unwrap(),
        b"hello"
    );
    *archive.last_mut().unwrap() ^= 1;
    let admitted = casc::authenticate_span(&archive, &ekey, 1024).unwrap();
    assert_eq!(
        lycheedev_data::blte::decode(admitted, BlteLimits::default(), &BTreeMap::new())
            .err()
            .unwrap()
            .code,
        "container.integrity_mismatch"
    );
}
fn encoding() -> (Content, String) {
    let ckey = [1; 16];
    let mut page = vec![0; 1024];
    page[0] = 2;
    page[1..6].copy_from_slice(&[0, 0, 0, 0, 7]);
    page[6..22].copy_from_slice(&ckey);
    page[22..38].fill(2);
    page[38..54].fill(3);
    let mut raw = vec![0; 22];
    raw[..5].copy_from_slice(b"EN\x01\x10\x10");
    raw[5..7].copy_from_slice(&1u16.to_be_bytes());
    raw[9..13].copy_from_slice(&1u32.to_be_bytes());
    raw.extend_from_slice(&ckey);
    raw.extend_from_slice(&Md5::digest(&page));
    raw.extend_from_slice(&page);
    (
        Content {
            bytes: raw,
            missing: vec![],
        },
        casc::hex(&ckey),
    )
}
#[test]
fn encoding_returns_all_alternatives_only_after_full_page_check() {
    let (mut content, key) = encoding();
    let e = Encoding::open(&content, Limits::default(), None).unwrap();
    let record = e.find(&key, Limits::default(), None).unwrap();
    assert_eq!(record.decoded_bytes, 7);
    assert_eq!(record.encoding_keys, vec!["02".repeat(16), "03".repeat(16)]);
    assert!(e.find(&"00".repeat(16), Limits::default(), None).is_err());
    *content.bytes.last_mut().unwrap() = 1;
    assert_eq!(
        Encoding::open(&content, Limits::default(), None)
            .unwrap()
            .find(&key, Limits::default(), None)
            .err()
            .unwrap()
            .code,
        "container.integrity_mismatch"
    );
    let digest = Md5::digest(&content.bytes[54..]);
    content.bytes[38..54].copy_from_slice(&digest);
    assert_eq!(
        Encoding::open(&content, Limits::default(), None)
            .unwrap()
            .find(&key, Limits::default(), None)
            .err()
            .unwrap()
            .code,
        "records.metadata_invalid"
    );
}
fn root(version: i32, nameless: bool) -> Content {
    let mut raw = vec![];
    fn word(raw: &mut Vec<u8>, value: u32) {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    if version >= 0 {
        raw.extend_from_slice(b"TSFM");
        if version == 0 {
            word(&mut raw, 2);
            word(&mut raw, if nameless { 0 } else { 2 });
        } else {
            for value in [24, version as u32, 2, if nameless { 0 } else { 2 }, 0] {
                word(&mut raw, value);
            }
        }
    }
    word(&mut raw, 2);
    let flags = 0x80 | if nameless { 0x10000000 } else { 0 };
    if version == 2 {
        for value in [0x202, flags, 0x20] {
            word(&mut raw, value);
        }
        raw.push(1);
    } else {
        word(&mut raw, flags);
        word(&mut raw, 0x202);
    }
    word(&mut raw, 11);
    word(&mut raw, 7);
    for b in [1, 2] {
        raw.extend_from_slice(&[b; 16]);
        if version < 0 {
            raw.extend_from_slice(&(b as u64 + 100).to_le_bytes());
        }
    }
    if version >= 0 && !nameless {
        for value in [101u64, 102] {
            raw.extend_from_slice(&value.to_le_bytes());
        }
    }
    Content {
        bytes: raw,
        missing: vec![],
    }
}
#[test]
fn root_classic_retail_v1_v2_name_and_locale_variants_and_bad_tail() {
    for version in [-1, 0, 1, 2] {
        for nameless in [false, true] {
            if version < 0 && nameless {
                continue;
            }
            let content = root(version, nameless);
            let rows = lookup_root(&content, &[19, 11, 19], RootLimits::default(), None).unwrap();
            assert_eq!(rows.len(), 2);
            assert_eq!((rows[0].file_data_id, rows[1].file_data_id), (11, 19));
            for (i, row) in rows.iter().enumerate() {
                assert_eq!(row.locale_mask, 0x202);
                assert_eq!(row.has_name_hash, !nameless);
                if !nameless {
                    assert_eq!(row.name_hash, 101 + i as u64);
                }
                if version == 2 {
                    assert_eq!(row.content_flags & 0x20020, 0x20020);
                }
            }
            let mut content = content;
            content.bytes.push(0);
            assert!(lookup_root(&content, &[11], RootLimits::default(), None).is_err());
        }
    }
}
#[test]
fn metadata_does_not_interpret_missing_encrypted_regions_as_empty() {
    let (mut content, key) = encoding();
    content.missing.push(lycheedev_data::blte::Missing {
        offset: 54,
        bytes: 1024,
        key_id: "key".into(),
        chunk: 0,
    });
    let e = Encoding::open(&content, Limits::default(), None).unwrap();
    assert_eq!(
        e.find(&key, Limits::default(), None).err().unwrap().code,
        "container.key_unavailable"
    );
    let mut content = root(2, false);
    content.missing.push(lycheedev_data::blte::Missing {
        offset: 49,
        bytes: 16,
        key_id: "key".into(),
        chunk: 0,
    });
    assert_eq!(
        lookup_root(&content, &[11], RootLimits::default(), None)
            .err()
            .unwrap()
            .code,
        "container.key_unavailable"
    );
}
