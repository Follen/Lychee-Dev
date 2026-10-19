//! Same BLTE pull -> remote Store -> Core quota owners used by CDN decoding.
//! The encoded source is a deterministic reader, not an HTTP integration fixture.
use super::*;
use crate::resource::Cost;
use lycheedev_core::{control::OperationControl, storage, workspace};
use std::{
    collections::BTreeMap,
    fs,
    io::{self, Cursor, Read},
    path::PathBuf,
    time::{Duration, Instant},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-decoded-disk-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
struct Encoded<'a> {
    store: Store<'a>,
    body: Cursor<&'static [u8]>,
    published: bool,
}
impl Read for Encoded<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if !self.published && self.body.position() >= 8 {
            let paths = workspace::data_disk::DataCachePaths::new(self.store.home);
            let spools: Vec<_> = fs::read_dir(paths.staging).unwrap().collect();
            assert_eq!(spools.len(), 1);
            assert_eq!(spools[0].as_ref().unwrap().metadata().unwrap().len(), 4);
            self.store
                .publish_blob(b"range", 5)
                .map_err(io::Error::other)?;
            self.published = true;
        }
        self.body.read(out)
    }
}
fn exercise(correct_key: bool) {
    let f = Fixture::new();
    let control = OperationControl::uncancelable(Instant::now() + Duration::from_secs(10));
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        control.deadline(),
        control.clone(),
    ));
    let mut encoded = Encoded {
        store: Store {
            home: &f.0,
            control: &control,
            budget: Some(&budget),
        },
        body: Cursor::new(b"BLTE\0\0\0\0Nbody"),
        published: false,
    };
    let expected: [u8; 16] = if correct_key {
        Md5::digest(b"body").into()
    } else {
        Md5::digest(b"fake").into()
    };
    let keys = BTreeMap::new();
    let check = || budget.check();
    let mut reader = crate::blte::StreamingReader::new_incremental_with_check(
        &mut encoded,
        13,
        4,
        expected,
        &keys,
        &budget,
        Some(&check),
    )
    .unwrap();
    let owned = budget.clone();
    let result = workspace::data_disk::publish_shared_blob_stream_with_check(
        &f.0,
        &mut reader,
        4,
        1 << 20,
        &control,
        move || owned.check(),
        &budget.safety_only(),
    );
    if correct_key {
        let mut blob = result.unwrap();
        let mut bytes = Vec::new();
        blob.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"body");
        blob.close().unwrap();
    } else {
        match result {
            Err(error) => assert_eq!(error.code, "records.content_integrity"),
            Ok(blob) => {
                blob.close().unwrap();
                panic!("CKey mismatch must fail before CAS")
            }
        }
        assert!(
            !evidence::blob_path(&f.0, &storage::sha256(b"body"))
                .unwrap()
                .exists()
        );
    }
    drop(reader);
    assert!(encoded.published);
    assert_eq!(
        fs::read(evidence::blob_path(&f.0, &storage::sha256(b"range")).unwrap()).unwrap(),
        b"range"
    );
    let paths = workspace::data_disk::DataCachePaths::new(&f.0);
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
    let completed = fs::read_dir(paths.shared_blobs())
        .unwrap()
        .filter(|entry| {
            entry
                .as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".complete")
        })
        .count();
    assert_eq!(completed, if correct_key { 2 } else { 1 });
}
#[test]
fn real_blte_pull_can_publish_store_range_while_decoded_spool_is_capacity_owned() {
    exercise(true);
}
#[test]
fn real_blte_ckey_failure_keeps_range_receipt_but_publishes_no_decoded_body() {
    exercise(false);
}

#[test]
fn rebuild_business_eof_rechecks_alias_before_core_final_publication() {
    let f = Fixture::new();
    let control = OperationControl::default();
    let expected = workspace::data_disk::publish_shared_blob_with_check(
        &f.0,
        b"body",
        4,
        &control,
        &|| Ok(()),
    )
    .unwrap();
    fs::remove_file(evidence::blob_path(&f.0, &expected.sha256).unwrap()).unwrap();
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        control.deadline(),
        control.clone(),
    ));
    let mut raw = Cursor::new(b"BLTE\0\0\0\0Nbody");
    let keys = BTreeMap::new();
    let ckey = Md5::digest(b"body").into();
    let mut reader =
        StreamingReader::new_incremental_with_check(&mut raw, 13, 4, ckey, &keys, &budget, None)
            .unwrap();
    let observed = std::cell::Cell::new(false);
    let mut checked = CheckedEof {
        reader: &mut reader,
        check: || {
            observed.set(true);
            Err(ToolkitError::new(
                "fixture.alias_changed",
                "original alias changed",
                4,
            ))
        },
    };
    let owned = budget.clone();
    let result = workspace::data_disk::rebuild_shared_blob_stream_with_check(
        &f.0,
        &mut checked,
        &expected,
        4,
        &control,
        move || owned.check(),
        &budget.safety_only(),
    );
    let error = match result {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("changed alias published")
        }
    };
    assert_eq!(error.code, "fixture.alias_changed");
    assert!(observed.get());
    assert!(
        !evidence::blob_path(&f.0, &expected.sha256)
            .unwrap()
            .exists()
    );
    assert_eq!(
        fs::read_dir(workspace::data_disk::DataCachePaths::new(&f.0).staging)
            .unwrap()
            .count(),
        0
    );
}
