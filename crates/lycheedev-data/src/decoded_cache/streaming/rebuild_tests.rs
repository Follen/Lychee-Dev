//! Narrow local Metadata -> authenticated BLTE EOF -> exact Core rebuild gates.
//! Public local routing and retained CAP coverage live in content_budget_parity.
use super::*;
use lycheedev_core::workspace::{self, data_disk};
use std::{
    fs,
    io::{self, Cursor},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lychee-local-rebuild-{}-{}-{}",
            std::process::id(),
            storage::sha256(storage::utc_now().as_bytes()),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        assert!(!home.exists());
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

#[derive(Clone, Copy, Debug)]
enum Fault {
    ContentKey,
    Digest,
    Length,
    KeyKind,
    KeyDigest,
    MissingAlias,
    Budget,
    Cancel,
    SourceClose,
}
struct SourceEof<R> {
    reader: R,
    fail_close: bool,
}
impl<R: Read> Read for SourceEof<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.reader.read(out)?;
        if n == 0 && !out.is_empty() && self.fail_close {
            // A typed upstream checked-close diagnostic is still an error,
            // even after every expected decoded byte has already been read.
            return Err(io::Error::other(fault(
                "storage.close_failed",
                5,
                "injected source EOF close diagnostic",
            )));
        }
        Ok(n)
    }
}
fn reject_at_eof(failure: Fault) {
    let f = Fixture::new();
    let control = OperationControl::default();
    let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
    let keys = keys::Context::prepare_scoped(&f.0, None, true, &control, control.deadline(), None)
        .unwrap();
    let record = EncodingRecord {
        content_key: casc::hex(&Md5::digest(b"body")),
        decoded_bytes: 4,
        encoding_keys: Vec::new(),
        page: 0,
    };
    let reference =
        data_disk::publish_shared_blob_with_check(&f.0, b"body", 4, &control, &|| Ok(())).unwrap();
    let body = evidence::blob_path(&f.0, &reference.sha256).unwrap();
    let paths = data_disk::DataCachePaths::new(&f.0);
    let intent = paths
        .shared_blobs()
        .join(format!("{}.intent", reference.sha256));
    let complete = paths
        .shared_blobs()
        .join(format!("{}.complete", reference.sha256));
    let original_pair = (fs::read(&intent).unwrap(), fs::read(&complete).unwrap());
    let alias = "decoded-v1/local-eof-fixture";
    DataCacheMetadata::put_controlled_with_check(
        &f.0,
        alias,
        &Entry {
            content_key: record.content_key.clone(),
            blob: serde_json::to_value(&reference).unwrap(),
            keys: keys.source.clone(),
        },
        None,
        &control,
        || budget.check(),
    )
    .unwrap();
    // Authenticate the same original alias and exact native-missing reference
    // required by production before granting Core the narrow rebuild target.
    let entry: Entry =
        DataCacheMetadata::read_controlled_with_check(&f.0, alias, &control, || budget.check())
            .unwrap();
    fs::remove_file(&body).unwrap();
    let absent = evidence::open_verified_blob_bounded_optional_with_check(
        &f.0,
        &entry.blob,
        4,
        &control,
        |_| Ok(()),
        || Ok(()),
    )
    .unwrap();
    assert!(absent.is_none());
    let store = Store {
        home: &f.0,
        control: &control,
        budget: Some(&budget),
    };
    let mut encoded = Cursor::new(b"BLTE\0\0\0\0Nbody");
    let mut reader = blte::StreamingReader::new_incremental_with_check(
        &mut encoded,
        13,
        4,
        Md5::digest(b"body").into(),
        keys.keys(),
        &budget,
        None,
    )
    .unwrap();
    let eof_seen = std::cell::Cell::new(false);
    let revalidation_seen = std::cell::Cell::new(false);
    let mut checked = CheckedEof {
        reader: SourceEof {
            reader: &mut reader,
            fail_close: matches!(failure, Fault::SourceClose),
        },
        check: || {
            assert!(!eof_seen.replace(true), "one publication EOF check");
            match failure {
                Fault::MissingAlias => {
                    // Fixture-only loss, never a production deletion API.
                    fs::remove_file(f.0.join("state/data/cache-v1.sqlite"))?;
                }
                Fault::Cancel => {
                    control.cancel_handle().store(true, Ordering::Release);
                }
                Fault::Budget => {
                    budget.charge(Cost::work(100_000_001))?;
                }
                Fault::SourceClose => panic!("failed source EOF cannot revalidate an alias"),
                _ => {
                    let mut changed: Entry = DataCacheMetadata::read_controlled_with_check(
                        &f.0,
                        alias,
                        &control,
                        || budget.check(),
                    )?;
                    match failure {
                        Fault::ContentKey => changed.content_key = "00".repeat(16),
                        Fault::Digest => {
                            changed.blob["sha256"] = serde_json::json!("00".repeat(32))
                        }
                        Fault::Length => changed.blob["bytes"] = serde_json::json!(5),
                        Fault::KeyKind => changed.keys.kind = "file".into(),
                        Fault::KeyDigest => changed.keys.sha256 = "00".repeat(32),
                        _ => unreachable!(),
                    }
                    let generation = DataCacheMetadata::read_document_raw_controlled_with_check(
                        &f.0,
                        alias,
                        &control,
                        || budget.check(),
                    )?
                    .generation;
                    // Inject an external alias change. A normal metadata writer
                    // correctly rejects the missing managed body during quota
                    // inventory, before this EOF identity guard can observe it.
                    let connection =
                        rusqlite::Connection::open(f.0.join("state/data/cache-v1.sqlite"))?;
                    assert_eq!(
                        connection.execute(
                            "UPDATE documents SET generation=generation+1,value=?2 WHERE key=?1 AND generation=?3",
                            rusqlite::params![alias, serde_json::to_vec(&changed)?, generation],
                        )?,
                        1
                    );
                    connection
                        .close()
                        .map_err(|(_, error)| ToolkitError::from(error))?;
                }
            }
            revalidation_seen.set(true);
            validate_rebuild(&f.0, alias, &record, &keys, &reference, &store)
        },
    };
    let owned = budget.safety_only();
    let result = data_disk::rebuild_shared_blob_stream_with_check(
        &f.0,
        &mut checked,
        &reference,
        4,
        &control,
        move || owned.check(),
        &budget.safety_only(),
    );
    let error = match result {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("{failure:?}: failed EOF must not publish")
        }
    };
    assert_eq!(eof_seen.get(), !matches!(failure, Fault::SourceClose));
    match failure {
        Fault::Cancel => assert_eq!(error.exit_code, 7),
        Fault::Budget => assert_eq!(error.code, "records.query_resource_budget"),
        Fault::SourceClose => assert_eq!(error.code, "storage.close_failed"),
        _ => {
            assert!(
                revalidation_seen.get(),
                "{failure:?}: alias guard was reached"
            );
            assert_eq!(error.code, "vault.blob_integrity", "{failure:?}: {error:?}");
        }
    }
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert!(!body.exists());
    assert_eq!(
        (fs::read(intent).unwrap(), fs::read(complete).unwrap()),
        original_pair
    );
    assert_eq!(fs::read_dir(&paths.staging).unwrap().count(), 0);
    assert!(
        !paths
            .shared_blobs()
            .join(format!("{}.repair.intent", reference.sha256))
            .exists()
    );
    drop(checked);
    drop(reader);
}

#[test]
fn local_rebuild_eof_rejects_each_changed_alias_identity() {
    for failure in [
        Fault::ContentKey,
        Fault::Digest,
        Fault::Length,
        Fault::KeyKind,
        Fault::KeyDigest,
    ] {
        reject_at_eof(failure);
    }
}
#[test]
fn local_rebuild_eof_rejects_removed_alias() {
    reject_at_eof(Fault::MissingAlias);
}
#[test]
fn local_rebuild_eof_budget_failure_never_publishes() {
    reject_at_eof(Fault::Budget);
}
#[test]
fn local_rebuild_eof_cancellation_never_publishes() {
    reject_at_eof(Fault::Cancel);
}
#[test]
fn local_rebuild_source_eof_close_error_is_not_success() {
    reject_at_eof(Fault::SourceClose);
}
#[test]
fn local_rebuild_empty_read_cannot_authorize_alias_recheck() {
    let calls = std::cell::Cell::new(0);
    let mut source = CheckedEof {
        reader: Cursor::new(b"body"),
        check: || {
            calls.set(calls.get() + 1);
            Ok(())
        },
    };
    assert_eq!(source.read(&mut []).unwrap(), 0);
    assert_eq!(calls.get(), 0);
    let mut out = [0; 4];
    assert_eq!(source.read(&mut out).unwrap(), 4);
    assert_eq!(&out, b"body");
    assert_eq!(calls.get(), 0);
    assert_eq!(source.read(&mut []).unwrap(), 0);
    assert_eq!(calls.get(), 0);
    assert_eq!(source.read(&mut [0; 1]).unwrap(), 0);
    assert_eq!(calls.get(), 1);
}
