use super::*;
use crate::blte::EncodedRead;

#[test]
fn local_reader_span_drop_refunds_encoded_storage_and_never_read_work() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/local-span-fixtures")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "-"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("data.000");
    let mut archive = vec![0; 30];
    archive.extend_from_slice(b"0123456789");
    fs::write(&path, &archive).unwrap();
    let budget = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(5),
        None,
    );
    let reader = LocalReader {
        file: Mutex::new(fs::File::open(&path).unwrap()),
        root,
        path,
        archive_bytes: 40,
        index: String::new(),
        span: Span {
            offset: 0,
            bytes: 40,
            archive: 0,
        },
        encoding_key: String::new(),
        header_sha256: String::new(),
        limits: Limits::default(),
        io_bytes: AtomicU64::new(0),
        io_reads: AtomicU64::new(0),
        cancel: None,
        control: None,
        budget: Some(&budget),
    };
    let first = reader.read_encoded(1, 5).unwrap();
    let second = reader.read_encoded(5, 5).unwrap();
    assert_eq!(&*first, b"12345");
    assert_eq!(&*second, b"56789");
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 10);
    drop(first);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 5);
    drop(second);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    assert_eq!(budget.snapshot().unwrap().decode_work, 2);
    assert_eq!(reader.io_stats()["bytes"], 10);
    assert!(reader.read_encoded(9, 2).is_err());
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
}
