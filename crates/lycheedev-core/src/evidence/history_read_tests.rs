//! NOT_RUN. Go archive.go InspectCapture/FetchCapture and vault/blob.go ReadBlob.
use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "lycheedev-historical-fetch-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self(home)
    }
    fn manifest(&self, length: i64, schema: &str, wrong_hash: bool) -> String {
        let reference = CaptureRef {
            schema: schema.into(),
            id: String::new(),
            blob: BlobRef {
                sha256: storage::sha256(b"body"),
                bytes: 0,
            },
            media_type: "application/json".into(),
            provenance: Provenance {
                kind: "game-report".into(),
                locator: "request".into(),
                snapshot: String::new(),
                operation_id: String::new(),
                source_commit: String::new(),
                base_snapshot: String::new(),
                base_source_commit: String::new(),
                data_build: String::new(),
                session: String::new(),
            },
            complete: true,
            truncated: false,
            captured_at: "2026-10-15T00:00:00Z".into(),
            dependencies: vec![],
        };
        let unsigned = String::from_utf8(storage::identity_json(&reference).unwrap())
            .unwrap()
            .replace("\"bytes\":0", &format!("\"bytes\":{length}"));
        let id = if wrong_hash {
            format!("CAP-{}", "0".repeat(64))
        } else {
            format!("CAP-{}", storage::sha256(unsigned.as_bytes()))
        };
        let raw = unsigned.replace("\"id\":\"\"", &format!("\"id\":\"{id}\""));
        let raw = serde_json::value::RawValue::from_string(raw).unwrap();
        Metadata::open(&self.0, true)
            .unwrap()
            .create(&format!("capture/{id}"), &raw)
            .unwrap();
        id
    }
    fn blob(&self, bytes: &[u8]) {
        let path = blob_path(&self.0, &storage::sha256(b"body")).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
#[test]
fn historical_fetch_signed_length_follows_schema_then_hash_then_blob() {
    for (schema, wrong_hash, want) in [
        ("wrong", true, "evidence.invalid_manifest"),
        ("lycheedev.capture.v1", true, "evidence.manifest_integrity"),
        ("lycheedev.capture.v1", false, "vault.blob_integrity"),
    ] {
        let f = Fixture::new();
        let id = f.manifest(-1, schema, wrong_hash);
        let error =
            fetch_capture_controlled(&f.0, &id, 0, &OperationControl::default()).unwrap_err();
        assert_eq!(error.message, want);
        assert_eq!(
            error.exit_code,
            if want == "vault.blob_integrity" { 4 } else { 5 }
        );
    }
}
#[test]
fn historical_fetch_authenticates_real_manifest_and_original_blob() {
    let f = Fixture::new();
    let id = f.manifest(4, "lycheedev.capture.v1", false);
    f.blob(b"body");
    let (capture, bytes) =
        fetch_capture_controlled(&f.0, &id, 4, &OperationControl::default()).unwrap();
    assert_eq!(capture.id, id);
    assert_eq!(bytes, b"body");
    assert_eq!(capture.blob.bytes, 4);
    f.blob(b"tampered");
    assert_eq!(
        fetch_capture_controlled(&f.0, &id, 100, &OperationControl::default())
            .unwrap_err()
            .code,
        "vault.blob_integrity"
    );
}
#[test]
fn historical_fetch_bound_precedes_blob_open() {
    let f = Fixture::new();
    let id = f.manifest(4, "lycheedev.capture.v1", false);
    let error = fetch_capture_controlled(&f.0, &id, 3, &OperationControl::default()).unwrap_err();
    assert_eq!(error.code, "vault.blob_limit");
    assert_eq!(error.message, "vault.blob_limit");
}

#[test]
fn historical_fetch_original_cancel_and_expired_deadline_precede_manifest_io() {
    use crate::control::TrackedCancellation;
    use std::time::{Duration, Instant};
    let f = Fixture::new();
    let cancellation = TrackedCancellation::new();
    cancellation.cancel();
    let stopped = OperationControl::tracked(Instant::now() + Duration::from_secs(1), cancellation);
    assert_eq!(
        fetch_capture_controlled(&f.0, "CAP-missing", 4096, &stopped)
            .unwrap_err()
            .code,
        "command.cancelled"
    );
    let expired = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        fetch_capture_controlled(&f.0, "CAP-missing", 4096, &expired)
            .unwrap_err()
            .code,
        "command.deadline"
    );
}
#[test]
fn historical_document_generation_and_original_bytes_share_one_row_snapshot() {
    let f = Fixture::new();
    let raw =
        serde_json::value::RawValue::from_string("{ \"same\":1, \"same\":2 }".into()).unwrap();
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata.create("work/OP-raw", &raw).unwrap();
    drop(metadata);
    let row =
        Metadata::read_document_raw_controlled(&f.0, "work/OP-raw", &OperationControl::default())
            .unwrap();
    assert_eq!(row.generation, 1);
    assert_eq!(row.bytes, raw.get().as_bytes());
    // Distinct generation and value are obtained by the same SELECT; neither
    // can come from an earlier Metadata::generation call.
    let mut metadata = Metadata::open(&f.0, true).unwrap();
    metadata.put("work/OP-raw", &false, Some(1)).unwrap();
    drop(metadata);
    let row =
        Metadata::read_document_raw_controlled(&f.0, "work/OP-raw", &OperationControl::default())
            .unwrap();
    assert_eq!(row.generation, 2);
    assert_eq!(row.bytes, b"false");
}
