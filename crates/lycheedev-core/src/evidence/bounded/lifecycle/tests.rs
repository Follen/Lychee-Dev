use super::*;
use crate::control::TrackedCancellation;
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-evidence-lifecycle-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], ""),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }
}
fn alive() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
fn reference(bytes: &[u8]) -> BlobRef {
    BlobRef {
        sha256: storage::sha256(bytes),
        bytes: bytes.len() as u64,
    }
}
#[test]
fn known_stream_checks_terminal_digest_before_same_directory_publication() {
    let f = Fixture::new();
    let invocation = alive();
    let expected = reference(b"correct");
    let mut reader = io::Cursor::new(b"corrupt".to_vec());
    let error = match publish_blob_reader_bounded(&f.0, &mut reader, &expected, 7, &invocation) {
        Err(error) => error,
        Ok(blob) => {
            blob.close().unwrap();
            panic!("bad digest admitted")
        }
    };
    assert_eq!(error.code, "vault.blob_integrity");
    assert_eq!(error.details["publication"]["state"], "not_published");
    assert_eq!(reader.position(), 7);
    let path = blob_path(&f.0, &expected.sha256).unwrap();
    assert!(!path.exists());
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 0);
}
#[test]
fn bytes_publish_stop_after_real_stage_creation_discards_stage_and_closes_parents() {
    let f = Fixture::new();
    let invocation = alive();
    let bytes = b"stage bytes";
    let reference = reference(bytes);
    let path = blob_path(&f.0, &reference.sha256).unwrap();
    let stopped = Cell::new(false);
    let error = publish_blob_bounded_with_check(&f.0, bytes, bytes.len(), &invocation, || {
        if fs::read_dir(path.parent().unwrap())
            .ok()
            .is_some_and(|rows| {
                rows.filter_map(std::result::Result::ok).any(|row| {
                    row.file_name()
                        .to_string_lossy()
                        .starts_with(".lycheedev-export-")
                })
            })
        {
            stopped.set(true);
        }
        if stopped.get() {
            Err(ToolkitError::new(
                "records.query_resource_budget",
                "stage admitted then stopped",
                3,
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, "records.query_resource_budget");
    assert!(stopped.get());
    assert!(!path.exists());
    assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 0);
    // Deleting this shard proves no retained deny-delete Directory/Stage owner.
    fs::remove_dir(path.parent().unwrap()).unwrap();
    invocation.check().unwrap();
}
#[test]
fn verified_close_observes_both_scopes_after_original_file_is_consumed() {
    let f = Fixture::new();
    let bytes = b"verified";
    let seed = publish_blob_bounded(&f.0, bytes, bytes.len(), &alive()).unwrap();
    let token = TrackedCancellation::new();
    let invocation =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let armed = Arc::new(AtomicBool::new(false));
    let flag = armed.clone();
    let blob = open_verified_blob_bounded_with_check(
        &f.0,
        &seed,
        bytes.len() as u64,
        &invocation,
        |_| Ok(()),
        move || {
            if flag.load(Ordering::Acquire) {
                Err(ToolkitError::new(
                    "records.query_resource_budget",
                    "independent stopped",
                    3,
                ))
            } else {
                Ok(())
            }
        },
    )
    .unwrap();
    token.cancel();
    armed.store(true, Ordering::Release);
    let error = blob.close().unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert!(
        error
            .details
            .to_string()
            .contains("records.query_resource_budget")
    );
    fs::write(blob_path(&f.0, &storage::sha256(bytes)).unwrap(), b"closed").unwrap();
}
#[test]
fn actual_bounded_capture_late_stop_retains_manifest_identity() {
    let f = Fixture::new();
    let token = TrackedCancellation::new();
    let invocation =
        OperationControl::tracked(Instant::now() + Duration::from_secs(30), token.clone());
    let error = commit_internal_observed(
        &f.0,
        CaptureInput {
            bytes: b"x",
            maximum_bytes: 1,
            media_type: "text/plain",
            provenance: serde_json::json!({"kind":"fixture","locator":"bounded"}),
            complete: true,
            truncated: false,
        },
        Some(&invocation),
        |phase| {
            if matches!(phase, CapturePublication::ManifestPublished) {
                token.cancel()
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, "command.cancelled");
    assert_eq!(error.details["capturePublication"]["state"], "committed");
    let id = error.details["capturePublication"]["id"].as_str().unwrap();
    assert_eq!(error.details["completedCaptures"], serde_json::json!([id]));
    assert_eq!(
        super::super::super::read_capture(&f.0, id).unwrap()["id"],
        id
    );
}

#[test]
fn removed_capture_republished_by_persistence_rejects_stale_generation() {
    for controlled in [false, true] {
        let f = Fixture::new();
        let control = alive();
        let bytes = b"recreated capture";
        let capture = commit_capture_bounded(
            &f.0,
            CaptureInput {
                bytes,
                maximum_bytes: bytes.len(),
                media_type: "text/plain",
                provenance: serde_json::json!({"kind":"fixture","locator":"recreated"}),
                complete: true,
                truncated: false,
            },
            &control,
        )
        .unwrap();
        let id = capture["id"].as_str().unwrap();
        let key = format!("capture/{id}");
        let original = Metadata::read_document_raw_controlled(&f.0, &key, &control).unwrap();
        assert_eq!(original.generation, 1);
        let removed = crate::evidence::remove_capture_controlled(&f.0, id, &control).unwrap();
        assert_eq!(removed["manifestRemoved"], true);
        assert_eq!(removed["blobRemoved"], true);

        // Public constructors include a new timestamp in the capture identity.
        // Reuse the exact original manifest through the production persistence
        // boundary to exercise recreation deterministically, without a clock hook.
        let blob = publish_blob_bounded(&f.0, bytes, bytes.len(), &control).unwrap();
        assert_eq!(blob, capture["blob"]);
        let state = Cell::new(crate::metadata::CaptureWriteState::NotCommitted);
        persist_capture(
            &f.0,
            &key,
            &original.bytes,
            controlled.then_some(&control),
            &mut || Ok(()),
            &state,
        )
        .unwrap();
        assert_eq!(state.get(), crate::metadata::CaptureWriteState::Committed);
        Metadata::with_owned_write(&f.0, &control, |writer| {
            let recreated = writer.document(&key)?;
            assert_eq!(recreated.generation, 2);
            assert_eq!(recreated.bytes, original.bytes);
            assert_eq!(
                writer.delete(&[(key.clone(), 1)]).unwrap_err().code,
                "vault.generation_conflict"
            );
            assert_eq!(writer.document(&key)?.generation, 2);
            Ok(())
        })
        .unwrap();
        assert_eq!(crate::evidence::read_capture(&f.0, id).unwrap(), capture);
    }
}
