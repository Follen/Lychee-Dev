//! Fixed-Go archive/inspect entrypoint regressions. Rust execution NOT_RUN.
use super::*;
use crate::control::{StopCause, TrackedCancellation};
use std::{
    collections::BTreeMap,
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
    fn invoke(&self, command: &str, id: Option<&str>) -> Invocation {
        Invocation {
            command: command.into(),
            positional: id.map(str::to_owned),
            flags: BTreeMap::new(),
            cwd: self.home.clone(),
            home: self.home.clone(),
            control: OperationControl::uncancelable(Instant::now() + Duration::from_secs(30)),
        }
    }
    fn raw(&self, key: &str, raw: &str) {
        let raw = serde_json::value::RawValue::from_string(raw.into()).unwrap();
        let mut m = Metadata::open(&self.home, true).unwrap();
        m.create(key, &raw).unwrap();
        m.close().unwrap();
    }
    fn manifest(&self, bytes: i64, locator: &str) -> (String, String) {
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
        super::super::dispatch(&f.invoke("evidence show", Some(&id)))
            .unwrap()
            .data,
        expected
    );
    assert!(super::super::dispatch(&f.invoke("evidence verify", Some(&id))).is_err());
    f.body(b"body");
    let verified = super::super::dispatch(&f.invoke("evidence verify", Some(&id)))
        .unwrap()
        .data;
    assert_eq!(verified["capture"], expected);
    assert_eq!(verified["verified"], true);
    f.body(b"Body");
    assert_eq!(
        super::super::dispatch(&f.invoke("evidence verify", Some(&id)))
            .unwrap_err()
            .code,
        "vault.blob_integrity"
    );
    f.cleanup();
}
#[test]
fn signed_blob_lengths_are_preserved_by_show_and_list_but_verify_rejects_before_io() {
    let f = Fixture::new();
    let (id, _) = f.manifest(-1, "signed");
    assert_eq!(
        super::super::dispatch(&f.invoke("evidence show", Some(&id)))
            .unwrap()
            .data["blob"]["bytes"],
        -1
    );
    assert_eq!(
        super::super::dispatch(&f.invoke("evidence list", None))
            .unwrap()
            .data["captures"][0]["blob"]["bytes"],
        -1
    );
    assert_eq!(
        super::super::dispatch(&f.invoke("evidence verify", Some(&id)))
            .unwrap_err()
            .code,
        "vault.blob_integrity"
    );
    assert!(
        !f.home
            .join("blobs")
            .join(&storage::sha256(b"body")[..2])
            .exists()
    );
    f.cleanup();
}
#[test]
fn verification_does_not_apply_a_producer_size_limit_to_historical_references() {
    let f = Fixture::new();
    let (id, _) = f.manifest((1_i64 << 40) + 1, "large-reference");
    let error = super::super::dispatch(&f.invoke("evidence verify", Some(&id))).unwrap_err();
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
    let mut inv = f.invoke("evidence list", None);
    inv.flags.insert("--limit".into(), "1".into());
    let values = super::super::dispatch(&inv).unwrap().data;
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
    let mut inv = f.invoke("evidence list", None);
    inv.flags.insert("--limit".into(), "1".into());
    assert_eq!(
        super::super::dispatch(&inv).unwrap().data["captures"][0]["id"],
        id
    );
    inv.flags.insert("--limit".into(), "2".into());
    let error = super::super::dispatch(&inv).unwrap_err();
    assert_ne!(error.code, "vault.record_missing");
    assert!(error.message.contains("signed Go integer"));
    f.cleanup();
}
#[test]
fn list_rejects_tampered_manifests_and_returns_empty_array_for_initialized_empty_archive() {
    let f = Fixture::new();
    let m = Metadata::open(&f.home, true).unwrap();
    m.close().unwrap();
    assert_eq!(
        super::super::dispatch(&f.invoke("evidence list", None))
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
    let error = super::super::dispatch(&f.invoke("evidence list", None)).unwrap_err();
    assert_eq!(error.message, "evidence.manifest_integrity");
    f.cleanup();
}
#[test]
fn historical_unknown_duplicate_case_null_and_raw_time_rules_reach_default_show() {
    let f = Fixture::new();
    let (id, raw) = f.manifest(4, "original");
    let raw = raw.replacen("{", "{\"schema\":\"wrong\",\"extension\":[1],", 1);
    let raw = format!("{},\"SCHEMA\":null}}", raw.trim_end_matches('}'));
    let mut m = Metadata::open(&f.home, true).unwrap();
    m.put(
        &format!("capture/{id}"),
        &serde_json::value::RawValue::from_string(raw.clone()).unwrap(),
        Some(1),
    )
    .unwrap();
    m.close().unwrap();
    assert_eq!(
        super::super::dispatch(&f.invoke("evidence show", Some(&id)))
            .unwrap()
            .data["schema"],
        "lycheedev.capture.v1"
    );
    let mut m = Metadata::open(&f.home, true).unwrap();
    m.put(
        &format!("capture/{id}"),
        &serde_json::value::RawValue::from_string(raw.replace("00:00:00Z", "00:00:00\\u005a"))
            .unwrap(),
        Some(2),
    )
    .unwrap();
    m.close().unwrap();
    assert!(super::super::dispatch(&f.invoke("evidence show", Some(&id))).is_err());
    f.cleanup();
}
#[test]
fn missing_or_unsupported_metadata_is_readonly_and_never_initialized() {
    for route in ["evidence show", "evidence verify", "evidence list"] {
        let f = Fixture::new();
        let argument = (route != "evidence list").then_some("CAP-absent");
        assert_eq!(
            super::super::dispatch(&f.invoke(route, argument))
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
                super::super::dispatch(&f.invoke(route, argument))
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
    let mut inv = f.invoke("evidence show", Some(&id));
    inv.control = OperationControl::tracked(Instant::now() + Duration::from_secs(30), stop.clone());
    stop.cancel();
    assert_eq!(
        super::super::dispatch(&inv).unwrap_err().code,
        "command.cancelled"
    );
    assert_eq!(
        inv.control.stop_cause().unwrap(),
        Some(StopCause::Cancelled)
    );
    let db = rusqlite::Connection::open(f.home.join("state/toolkit.sqlite")).unwrap();
    db.execute_batch("BEGIN EXCLUSIVE").unwrap();
    inv.control = OperationControl::uncancelable(Instant::now() + Duration::from_millis(30));
    let start = Instant::now();
    let error = super::super::dispatch(&inv).unwrap_err();
    assert_eq!(error.code, "command.deadline");
    assert_eq!(error.exit_code, 5);
    assert!(start.elapsed() < Duration::from_secs(2));
    db.execute_batch("ROLLBACK").unwrap();
    db.close().unwrap();
    f.cleanup();
}
#[test]
fn default_show_observes_go_depth_limit_on_original_ignored_json() {
    for depth in [10_000, 10_001] {
        let f = Fixture::new();
        let (id, raw) = f.manifest(4, "depth");
        let raw = format!(
            "{},\"ignored\":{}0{}}}",
            raw.trim_end_matches('}'),
            "[".repeat(depth - 1),
            "]".repeat(depth - 1)
        );
        // The ordinary RawValue writer carries original historical bytes. It
        // does not invoke the reader's Go grammar validator before this test.
        let mut m = Metadata::open(&f.home, true).unwrap();
        m.put(
            &format!("capture/{id}"),
            &serde_json::value::RawValue::from_string(raw).unwrap(),
            Some(1),
        )
        .unwrap();
        m.close().unwrap();
        let result = super::super::dispatch(&f.invoke("evidence show", Some(&id)));
        if depth == 10_000 {
            assert_eq!(result.unwrap().data["id"], id)
        } else {
            assert_eq!(result.unwrap_err().message, "invalid capture JSON");
        }
        f.cleanup();
    }
}
