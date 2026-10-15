use lycheedev_data::{
    blte::{ByteSource, Limits, Ranges},
    casc,
    metadata_format::Encoding,
};
use md5::{Digest, Md5};
use std::collections::BTreeMap;
#[test]
fn random_reader_rejects_short_reads_and_observes_only_requested_chunks() {
    use std::cell::RefCell;
    struct Reader {
        raw: Vec<u8>,
        reads: RefCell<Vec<(usize, usize)>>,
        short: bool,
    }
    impl lycheedev_data::blte::EncodedRead for Reader {
        fn encoded_size(&self) -> usize {
            self.raw.len()
        }
        fn read_encoded(
            &self,
            offset: usize,
            bytes: usize,
        ) -> lycheedev_core::Result<lycheedev_data::blte::EncodedSpan<'_>> {
            self.reads.borrow_mut().push((offset, bytes));
            let source = &self.raw[offset..offset + bytes - usize::from(self.short)];
            lycheedev_data::blte::EncodedSpan::read_owned(source.len(), None, |out| {
                out.copy_from_slice(source);
                Ok(())
            })
        }
    }
    let raw = framed(&[(b"Nsmall".to_vec(), 5), (vec![b'N'; 4097], 4096)]);
    let keys = BTreeMap::new();
    let reader = Reader {
        raw: raw.clone(),
        reads: RefCell::new(vec![]),
        short: false,
    };
    let ranges = Ranges::open_reader(
        &reader,
        Limits {
            chunk_bytes: 64,
            ..Default::default()
        },
        &keys,
    )
    .unwrap();
    assert_eq!(ranges.read_span(0, 5).unwrap().as_ref(), b"small");
    assert!(
        reader
            .reads
            .borrow()
            .iter()
            .all(|(offset, bytes)| offset + bytes <= 66)
    );
    let reader = Reader {
        raw,
        reads: RefCell::new(vec![]),
        short: true,
    };
    assert!(Ranges::open_reader(&reader, Limits::default(), &keys).is_err());
}
fn framed(parts: &[(Vec<u8>, u32)]) -> Vec<u8> {
    let header = 12 + 24 * parts.len();
    let mut raw = vec![0; header];
    raw[..4].copy_from_slice(b"BLTE");
    raw[4..8].copy_from_slice(&(header as u32).to_be_bytes());
    raw[8] = 15;
    raw[11] = parts.len() as u8;
    for (i, (bytes, decoded)) in parts.iter().enumerate() {
        let at = 12 + 24 * i;
        raw[at..at + 4].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
        raw[at + 4..at + 8].copy_from_slice(&decoded.to_be_bytes());
        raw[at + 8..at + 24].copy_from_slice(&Md5::digest(bytes));
        raw.extend(bytes);
    }
    raw
}
#[test]
fn huge_unvisited_and_missing_key_chunks_do_not_change_requested_range_coverage() {
    let first = [vec![b'N'], b"authenticated".to_vec()].concat();
    let second = vec![b'N'; 4097];
    let encrypted = vec![b'E', 8, 1, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, b'S', 1];
    let raw = framed(&[(first, 13), (second, 4096), (encrypted, 50)]);
    let keys = BTreeMap::new();
    let ranges = Ranges::open(
        &raw,
        Limits {
            chunk_bytes: 64,
            ..Default::default()
        },
        &keys,
    )
    .unwrap();
    assert_eq!(ranges.read_span(0, 13).unwrap().as_ref(), b"authenticated");
    assert_eq!(
        ranges.read_span(13, 1).unwrap_err().code,
        "container.resource_limit"
    );
    assert_eq!(
        ranges.read_span(4109, 1).unwrap_err().code,
        "container.key_unavailable"
    );
    assert_eq!(
        ranges
            .read_checked_span(13, 1, &Md5::digest(b"N"))
            .unwrap()
            .as_ref(),
        b"N"
    );
    assert_eq!(
        ranges.read_checked_span(13, 1, &[0; 16]).unwrap_err().code,
        "container.integrity_mismatch"
    );
    assert!(ranges.read_span(usize::MAX, 2).is_err());
}
#[test]
fn encoding_checked_pages_in_large_n_segment_and_corrupt_unused_page() {
    let mut header = vec![0; 22 + 64];
    header[..5].copy_from_slice(&[b'E', b'N', 1, 16, 16]);
    header[5..7].copy_from_slice(&1u16.to_be_bytes());
    header[9..13].copy_from_slice(&2u32.to_be_bytes());
    let mut pages = vec![0; 2048];
    for (page, key) in [(0, 1u8), (1, 2u8)] {
        let at = page * 1024;
        pages[at] = 1;
        pages[at + 5] = 8;
        pages[at + 6..at + 22].fill(key);
        pages[at + 22..at + 38].fill(key + 8);
        let d = 22 + page * 32;
        header[d..d + 16].fill(key);
        header[d + 16..d + 32].copy_from_slice(&Md5::digest(&pages[at..at + 1024]));
    }
    // N block checksum intentionally does not authenticate the requested page;
    // the independently authenticated Encoding directory page digest does.
    pages[1500] = 17;
    let raw = framed(&[
        ([vec![b'N'], header].concat(), 86),
        ([vec![b'N'], pages].concat(), 2048),
    ]);
    let keys = BTreeMap::new();
    let ranges = Ranges::open(
        &raw,
        Limits {
            chunk_bytes: 1024,
            ..Default::default()
        },
        &keys,
    )
    .unwrap();
    let encoding = Encoding::open(&ranges, casc::Limits::default(), None).unwrap();
    assert_eq!(
        encoding
            .find(&"01".repeat(16), casc::Limits::default(), None)
            .unwrap()
            .decoded_bytes,
        8
    );
    assert_eq!(
        encoding
            .find(&"02".repeat(16), casc::Limits::default(), None)
            .unwrap_err()
            .code,
        "container.integrity_mismatch"
    );
}
#[test]
fn ckey_lookup_does_not_touch_unavailable_physical_ekey_directory() {
    use lycheedev_data::blte::{Content, Missing};
    let mut raw = vec![0; 22 + 32 + 1024 + 32 + 1024];
    raw[..5].copy_from_slice(&[b'E', b'N', 1, 16, 16]);
    raw[5..7].copy_from_slice(&1u16.to_be_bytes());
    raw[7..9].copy_from_slice(&1u16.to_be_bytes());
    raw[9..13].copy_from_slice(&1u32.to_be_bytes());
    raw[13..17].copy_from_slice(&1u32.to_be_bytes());
    raw[22..38].fill(1);
    let at = 54;
    raw[at] = 1;
    raw[at + 5] = 8;
    raw[at + 6..at + 22].fill(1);
    raw[at + 22..at + 38].fill(9);
    let sum = Md5::digest(&raw[at..at + 1024]);
    raw[38..54].copy_from_slice(&sum);
    let content = Content {
        bytes: raw,
        missing: vec![Missing {
            offset: 1078,
            bytes: 1056,
            key_id: "1234".into(),
            chunk: 1,
        }],
    };
    let encoding = Encoding::open(&content, casc::Limits::default(), None).unwrap();
    assert_eq!(
        encoding
            .find(&"01".repeat(16), casc::Limits::default(), None)
            .unwrap()
            .decoded_bytes,
        8
    );
    assert_eq!(
        encoding
            .find_encoding(&"09".repeat(16), casc::Limits::default(), None)
            .unwrap_err()
            .code,
        "container.key_unavailable"
    );
}
