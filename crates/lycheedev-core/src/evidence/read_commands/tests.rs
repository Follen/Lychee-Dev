//! Fixed-Go archive/inspect entrypoint regressions. Rust execution NOT_RUN.
use super::*;
use crate::control::{StopCause, TrackedCancellation};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};
struct Fixture {
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let home = std::env::temp_dir().join(format!(
            "lycheedev-evidence-read-r37-{}-{}-{}",
            std::process::id(),
            storage::utc_now().replace([':', '.'], "_"),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        workspace::initialize(&home).unwrap();
        Self { home }
    }
    fn context(&self) -> crate::request::RequestContext {
        crate::request::RequestContext {
            home: self.home.clone(),
            cwd: self.home.clone(),
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(30)),
        }
    }
    fn raw(&self, key: &str, raw: &str) {
        let raw = serde_json::value::RawValue::from_string(raw.into()).unwrap();
        let mut m = Metadata::open(&self.home, true).unwrap();
        m.create(key, &raw).unwrap();
        m.close().unwrap();
    }
    fn manifest(&self, bytes: u64, locator: &str) -> (String, String) {
        let capture = CaptureRef {
            schema: "lycheedev.capture.v1".into(),
            id: String::new(),
            blob: BlobRef {
                sha256: storage::sha256(b"body"),
                bytes: 0,
            },
            media_type: "application/json".into(),
            provenance: Provenance {
                kind: "fixture".into(),
                locator: locator.into(),
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
        let unsigned = String::from_utf8(storage::identity_json(&capture).unwrap())
            .unwrap()
            .replace("\"bytes\":0", &format!("\"bytes\":{bytes}"));
        let id = format!("CAP-{}", storage::sha256(unsigned.as_bytes()));
        let raw = unsigned.replace("\"id\":\"\"", &format!("\"id\":\"{id}\""));
        self.raw(&format!("capture/{id}"), &raw);
        (id, raw)
    }
    fn body(&self, body: &[u8]) {
        let path = blob_path(&self.home, &storage::sha256(b"body")).unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }
    fn cleanup(self) {
        fs::remove_dir_all(self.home).unwrap()
    }
}
#[test]
fn show_does_not_read_payload_and_verify_retains_library_receipt_for_existing_cli_projection() {
    let f = Fixture::new();
    let (id, raw) = f.manifest(4, "ordinary");
    let expected: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::Show { id: id.clone() })
            .unwrap()
            .data,
        expected
    );
    assert!(super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::Verify { id: id.clone() }).is_err());
    f.body(b"body");
    let verified = super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::Verify { id: id.clone() })
        .unwrap()
        .data;
    assert_eq!(verified["capture"], expected);
    assert_eq!(verified["verified"], true);
    f.body(b"Body");
    assert_eq!(
        super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::Verify { id: id.clone() })
            .unwrap_err()
            .code,
        "vault.blob_integrity"
    );
    f.cleanup();
}
#[test]
fn verification_does_not_apply_a_producer_size_limit_to_historical_references() {
    let f = Fixture::new();
    let (id, _) = f.manifest((1_u64 << 40) + 1, "large-reference");
    let error = super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::Verify { id: id.clone() }).unwrap_err();
    assert_ne!(error.code, "vault.blob_limit");
    assert_ne!(error.code, "vault.blob_integrity"); // Valid reference reaches the missing file open.
    f.cleanup();
}
#[test]
fn list_reinspects_embedded_ids_after_closing_page_cursor_instead_of_trusting_key_or_value() {
    let f = Fixture::new();
    let (id, raw) = f.manifest(4, "referenced");
    // This row is first in key order. Go fully decodes it but does not authenticate
    // its schema or key: it re-reads capture/<embedded ID> and authenticates that.
    f.raw(
        "capture/000",
        &raw.replace("lycheedev.capture.v1", "not-the-authenticated-row"),
    );
    let values = super::super::dispatch(
        &f.context(), crate::evidence::EvidenceRequest::List { limit: 1 },
    ).unwrap().data;
    assert_eq!(values["captures"].as_array().unwrap().len(), 1);
    assert_eq!(values["captures"][0]["id"], id);
    assert_eq!(values["captures"][0]["schema"], "lycheedev.capture.v1");
    f.cleanup();
}
#[test]
fn list_decodes_all_typed_fields_before_reinspection_and_does_not_scan_beyond_limit() {
    let f = Fixture::new();
    let (id, _) = f.manifest(4, "limit");
    f.raw(
        "capture/zzzz",
        "{\"id\":\"missing\",\"blob\":{\"bytes\":\"bad\"}}",
    );
    assert_eq!(
        super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::List { limit: 1 })
            .unwrap().data["captures"][0]["id"], id
    );
    let error = super::super::dispatch(
        &f.context(), crate::evidence::EvidenceRequest::List { limit: 2 },
    ).unwrap_err();
    assert_ne!(error.code, "vault.record_missing");
    assert_eq!(error.code, "input.invalid");
    assert!(error.message.contains("expected u64"), "{}", error.message);
    f.cleanup();
}
#[test]
fn list_rejects_tampered_manifests_and_returns_empty_array_for_initialized_empty_archive() {
    let f = Fixture::new();
    let m = Metadata::open(&f.home, true).unwrap();
    m.close().unwrap();
    assert_eq!(
        super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::List { limit: 100 })
            .unwrap()
            .data,
        json!({"captures":[]})
    );
    let (id, raw) = f.manifest(4, "before");
    let mut m = Metadata::open(&f.home, true).unwrap();
    m.put(
        &format!("capture/{id}"),
        &serde_json::value::RawValue::from_string(raw.replace("before", "after")).unwrap(),
        Some(1),
    )
    .unwrap();
    m.close().unwrap();
    let error = super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::List { limit: 100 }).unwrap_err();
    assert_eq!(error.code, "evidence.capture_integrity");
    assert_eq!(error.message, "capture manifest content differs from identity");
    f.cleanup();
}
#[test]
fn missing_or_unsupported_metadata_is_readonly_and_never_initialized() {
    for route in ["evidence show", "evidence verify", "evidence list"] {
        let f = Fixture::new();
        let request = match route {
            "evidence show" => crate::evidence::EvidenceRequest::Show { id: "CAP-absent".into() },
            "evidence verify" => crate::evidence::EvidenceRequest::Verify { id: "CAP-absent".into() },
            _ => crate::evidence::EvidenceRequest::List { limit: 100 },
        };
        assert_eq!(
            super::super::dispatch(&f.context(), request.clone())
                .unwrap_err()
                .code,
            "vault.record_missing"
        );
        assert!(!f.home.join("state/toolkit.sqlite").exists());
        for version in [0, -1, 2] {
            let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
            db.pragma_update(None, "user_version", version).unwrap();
            db.close().unwrap();
            assert_eq!(
                super::super::dispatch(&f.context(), request.clone())
                    .unwrap_err()
                    .code,
                "vault.unsupported_workspace"
            );
            let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
            assert_eq!(
                db.pragma_query_value(None, "user_version", |row| row.get::<_, i32>(0))
                    .unwrap(),
                version
            );
            db.close().unwrap();
        }
        f.cleanup();
    }
}
#[test]
fn original_cancel_and_sqlite_wait_deadline_are_preserved_without_raw_token_exposure() {
    let f = Fixture::new();
    let (id, _) = f.manifest(4, "scope");
    let stop = TrackedCancellation::new();
    let mut context = f.context();
    context.control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    stop.cancel();
    let request = crate::evidence::EvidenceRequest::Show { id: id.clone() };
    assert_eq!(super::super::dispatch(&context, request.clone()).unwrap_err().code, "command.cancelled");
    assert_eq!(context.control.stop_cause().unwrap(), Some(StopCause::Cancelled));
    let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    context.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(30));
    let start = Instant::now();
    let error = super::super::dispatch(&context, request).unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert_eq!(error.exit_code, 5);
    assert!(start.elapsed() < Duration::from_secs(2));
    db.execute_batch("ROLLBACK").unwrap();
    db.close().unwrap();
    f.cleanup();
}
#[test]
fn raw_escape_and_unknown_fields_are_rejected_by_the_typed_reader() {
    let f = Fixture::new();
    let (id, raw) = f.manifest(4, "original");
    let raw = raw.replacen("{", "{\"extension\":[1],", 1);
    let mut m = Metadata::open(&f.home, true).unwrap();
    m.put(
        &format!("capture/{id}"),
        &serde_json::value::RawValue::from_string(raw).unwrap(),
        Some(1),
    )
    .unwrap();
    m.close().unwrap();
    let error = super::super::dispatch(&f.context(), crate::evidence::EvidenceRequest::Show { id: id.clone() }).unwrap_err();
    assert_eq!(error.code, "input.invalid");
    assert!(
        error.message.contains("unknown field"),
        "{}",
        error.message
    );
    f.cleanup();
}

mod supplemental;
