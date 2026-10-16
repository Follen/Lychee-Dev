//! Fixed Go 3.2.0 3057a966 archive_dependencies_test.go,
//! archive_legacy_dependencies_test.go and capture_dependencies_test.go.
//! Offline synthetic captures only; execution evidence is recorded separately.
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    evidence::{self, CaptureAccounting, CaptureInput},
    metadata::Metadata,
    storage, workspace,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-capture-dependencies-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            SERIAL.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        workspace::initialize(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct Meter;
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
}
fn input<'a>(bytes: &'a [u8], kind: &str, locator: &str, media: &'a str) -> CaptureInput<'a> {
    CaptureInput {
        bytes,
        maximum_bytes: bytes.len(),
        media_type: media,
        provenance: json!({"kind":kind,"locator":locator,"snapshot":"PIN-fixture"}),
        complete: true,
        truncated: false,
    }
}
fn capture(
    home: &Path,
    body: &[u8],
    kind: &str,
    locator: &str,
    media: &str,
    dependencies: &[&str],
) -> Value {
    evidence::commit_capture_budgeted_with_dependencies(
        home,
        input(body, kind, locator, media),
        dependencies,
        &OperationControl::default(),
        &Meter,
    )
    .unwrap()
}
fn id(capture: &Value) -> &str {
    capture["id"].as_str().unwrap()
}
fn assert_owned(home: &Path, capture: &Value) {
    let error = evidence::remove_capture(home, id(capture)).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("evidence.capture_referenced", 3)
    );
    assert_eq!(evidence::read_capture(home, id(capture)).unwrap(), *capture);
    evidence::read_blob(home, &capture["blob"], 32 << 20).unwrap();
}
fn assert_deleted(home: &Path, capture: &Value) {
    let result = evidence::remove_capture(home, id(capture)).unwrap();
    assert_eq!(result["manifestRemoved"], true);
    assert_eq!(
        evidence::read_capture(home, id(capture)).unwrap_err().code,
        "vault.record_missing"
    );
}

#[test]
fn dependencies_publish_and_release_with_owner_without_aliasing_or_extra_blob() {
    let home = Workspace::new();
    let page = capture(&home.0, b"page bytes", "fixture", "page", "text/plain", &[]);
    let mut source_id = id(&page).to_owned();
    let owner = capture(
        &home.0,
        b"owner bytes",
        "fixture",
        "owner",
        "text/plain",
        &[&source_id],
    );
    source_id.clear();
    assert_eq!(owner["dependencies"], json!([id(&page)]));
    assert_eq!(evidence::read_capture(&home.0, id(&owner)).unwrap(), owner);
    let mut metadata = Metadata::open(&home.0, true).unwrap();
    metadata.put(&format!("evidence/keep/{}", id(&page)),
        &json!({"schema":"lycheedev.evidence-retention.v1","captureId":id(&page),"blob":page["blob"],"keptAt":"2026-09-23T08:00:00Z"}), None).unwrap();
    assert_owned(&home.0, &page);
    assert_deleted(&home.0, &owner);
    let removed = evidence::remove_capture(&home.0, id(&page)).unwrap();
    assert_eq!(removed["retentionRemoved"], true);
    assert_eq!(removed["blobRemoved"], true);

    let body = b"unpublished dependency owner";
    let error = evidence::commit_capture_budgeted_with_dependencies(
        &home.0,
        input(body, "fixture", "missing", "text/plain"),
        &[id(&page)],
        &OperationControl::default(),
        &Meter,
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("vault.record_missing", 3)
    );
    assert!(
        !evidence::blob_path(&home.0, &storage::sha256(body))
            .unwrap()
            .exists()
    );
    assert!(metadata.list::<Value>("capture/", 100).unwrap().is_empty());
}

#[test]
fn dependencies_are_ordered_identity_and_tampering_rejects_publication() {
    let home = Workspace::new();
    let a = capture(&home.0, b"a", "fixture", "a", "text/plain", &[]);
    let b = capture(&home.0, b"b", "fixture", "b", "text/plain", &[]);
    let owner = capture(
        &home.0,
        b"owner",
        "fixture",
        "owner",
        "text/plain",
        &[id(&b), id(&a), id(&b)],
    );
    assert_eq!(owner["dependencies"], json!([id(&b), id(&a), id(&b)]));
    let key = format!("capture/{}", id(&owner));
    let mut changed: evidence::CaptureRef = serde_json::from_value(owner.clone()).unwrap();
    changed.dependencies.swap(0, 1);
    let mut metadata = Metadata::open(&home.0, true).unwrap();
    metadata.put(&key, &changed, Some(1)).unwrap();
    assert_eq!(
        evidence::read_capture(&home.0, id(&owner))
            .unwrap_err()
            .code,
        "evidence.capture_integrity"
    );
    let body = b"never published under corrupt parent";
    let error = evidence::commit_capture_budgeted_with_dependencies(
        &home.0,
        input(body, "fixture", "corrupt", "text/plain"),
        &[id(&owner)],
        &OperationControl::default(),
        &Meter,
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("evidence.capture_integrity", 4)
    );
    assert!(
        !evidence::blob_path(&home.0, &storage::sha256(body))
            .unwrap()
            .exists()
    );
}

#[test]
fn independent_pre_dependency_manifest_encoding_retains_original_identity() {
    let home = Workspace::new();
    let blob = evidence::publish_blob(&home.0, b"historical bytes", 1024).unwrap();
    // Independent old field layout: adding/reordering even an empty field in
    // CaptureRef changes this fixed oracle identity and must fail this test.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct OldCapture {
        schema: &'static str,
        id: String,
        blob: evidence::BlobRef,
        media_type: &'static str,
        provenance: evidence::Provenance,
        complete: bool,
        truncated: bool,
        captured_at: &'static str,
    }
    let mut old = OldCapture {
        schema: "lycheedev.capture.v1",
        id: String::new(),
        blob: serde_json::from_value(blob).unwrap(),
        media_type: "text/plain",
        provenance: serde_json::from_value(json!({"kind":"fixture","locator":"old"})).unwrap(),
        complete: true,
        truncated: false,
        captured_at: "2026-09-23T08:00:00Z",
    };
    old.id = format!(
        "CAP-{}",
        storage::sha256(&storage::identity_json(&old).unwrap())
    );
    let mut metadata = Metadata::open(&home.0, true).unwrap();
    let key = format!("capture/{}", old.id);
    metadata.put(&key, &old, None).unwrap();
    let observed = evidence::read_capture(&home.0, &old.id).unwrap();
    assert_eq!(observed, serde_json::to_value(&old).unwrap());
    assert!(observed.get("dependencies").is_none());
    for empty in [Value::Null, json!([])] {
        let mut serialized = observed.clone();
        serialized["dependencies"] = empty;
        let generation = metadata.generation(&key).unwrap().unwrap();
        metadata.put(&key, &serialized, Some(generation)).unwrap();
        assert_eq!(evidence::read_capture(&home.0, &old.id).unwrap(), observed);
    }
    assert_eq!(
        evidence::read_blob(&home.0, &observed["blob"], 1024).unwrap(),
        b"historical bytes"
    );
    assert_deleted(&home.0, &observed);
}

#[test]
fn legacy_capture_locator_is_exact_and_keeps_original_bytes() {
    let home = Workspace::new();
    let source = capture(
        &home.0,
        b"cache",
        "hotfix-cache",
        "fixture",
        "text/plain",
        &[],
    );
    let near = capture(
        &home.0,
        b"near",
        "fixture",
        &format!("capture:{}-suffix", id(&source)),
        "text/plain",
        &[],
    );
    let owner = capture(
        &home.0,
        b"{}",
        "hotfix-scan",
        &format!("capture:{}", id(&source)),
        "application/json",
        &[],
    );
    assert_owned(&home.0, &source);
    assert_deleted(&home.0, &owner);
    assert_deleted(&home.0, &source);
    assert_eq!(
        evidence::read_blob(&home.0, &near["blob"], 100).unwrap(),
        b"near"
    );
}

#[test]
fn all_four_typed_legacy_payloads_retain_only_their_declared_capture_fields() {
    for (kind, target_kind, locator, body) in [
        ("hotfix-scan", "hotfix-records", "scan", "scan"),
        (
            "hotfix-records",
            "hotfix-cache",
            "dbcache:sha256:fixture",
            "page",
        ),
        ("asset-export-manifest", "asset-image", "export", "export"),
        (
            "asset-demux-manifest",
            "asset-demux-source",
            "demux",
            "source",
        ),
        (
            "asset-demux-manifest",
            "asset-demux-frame",
            "demux",
            "frame",
        ),
    ] {
        let home = Workspace::new();
        let target = capture(
            &home.0,
            b"original",
            target_kind,
            "fixture",
            "text/plain",
            &[],
        );
        let raw = match body {
            "scan" => json!({"pages":[id(&target)]}),
            "page" => {
                json!({"snapshot":"PIN-fixture","source":{"schema":"lycheedev.capture.v1","id":id(&target)}})
            }
            "export" => {
                json!({"schema":"lycheedev.asset-export.v1","artifactCapture":{"schema":"lycheedev.capture.v1","id":id(&target)}})
            }
            "source" => json!({"schema":"lycheedev.asset-demux.v1","sourceCapture":id(&target)}),
            _ => json!({"schema":"lycheedev.asset-demux.v1","frames":[{"capture":id(&target)}]}),
        };
        let bytes = serde_json::to_vec(&raw).unwrap();
        let owner = capture(&home.0, &bytes, kind, locator, "application/json", &[]);
        assert_owned(&home.0, &target);
        assert_eq!(
            evidence::read_blob(&home.0, &owner["blob"], 1024).unwrap(),
            bytes
        );
        assert_deleted(&home.0, &owner);
        assert_deleted(&home.0, &target);
    }
}

#[test]
fn unrelated_payloads_do_not_become_references_and_explicit_dependencies_are_authoritative() {
    for mode in [
        "kind",
        "schema",
        "field",
        "media",
        "target",
        "provider",
        "snapshot",
        "dependencies",
    ] {
        let home = Workspace::new();
        let target_kind = match mode {
            "target" => "fixture",
            "provider" | "snapshot" => "hotfix-cache",
            "dependencies" => "asset-image",
            _ => "asset-demux-frame",
        };
        let target = capture(
            &home.0,
            b"original",
            target_kind,
            "fixture",
            "text/plain",
            &[],
        );
        let unrelated = capture(&home.0, b"other", "fixture", "unrelated", "text/plain", &[]);
        let kind = match mode {
            "kind" => "fixture",
            "provider" | "snapshot" => "hotfix-records",
            "dependencies" => "asset-export-manifest",
            _ => "asset-demux-manifest",
        };
        let media = if mode == "media" {
            "text/plain"
        } else {
            "application/json"
        };
        let schema = if mode == "schema" {
            "unrelated.v1"
        } else {
            "lycheedev.asset-demux.v1"
        };
        let body = match mode {
            "field" => json!({"schema":schema,"notes":{"frames":[{"capture":id(&target)}]}}),
            "provider" | "snapshot" => {
                json!({"snapshot":"wrong-snapshot","source":{"schema":"lycheedev.capture.v1","id":id(&target)}})
            }
            "dependencies" => {
                json!({"schema":"lycheedev.asset-export.v1","artifactCapture":{"schema":"lycheedev.capture.v1","id":id(&target)}})
            }
            _ => json!({"schema":schema,"frames":[{"capture":id(&target)}]}),
        };
        let locator = if mode == "provider" {
            "wago:sha256:remote"
        } else {
            "dbcache:sha256:fixture"
        };
        let deps = if mode == "dependencies" {
            vec![id(&unrelated)]
        } else {
            vec![]
        };
        let raw = serde_json::to_vec(&body).unwrap();
        let owner = capture(&home.0, &raw, kind, locator, media, &deps);
        assert_deleted(&home.0, &target);
        assert_eq!(
            evidence::read_blob(&home.0, &owner["blob"], 1024).unwrap(),
            raw
        );
    }
}

#[test]
fn demux_frames_remain_typed_dependencies_even_with_explicit_source_dependency() {
    let home = Workspace::new();
    let source = capture(
        &home.0,
        b"source",
        "asset-demux-source",
        "source",
        "video/VP9",
        &[],
    );
    let frame = capture(
        &home.0,
        b"frame",
        "asset-demux-frame",
        "frame",
        "image/png",
        &[],
    );
    let raw = serde_json::to_vec(&json!({"schema":"lycheedev.asset-demux.v1","sourceCapture":id(&source),"frames":[{"capture":id(&frame)}]})).unwrap();
    let owner = capture(
        &home.0,
        &raw,
        "asset-demux-manifest",
        "demux",
        "application/json",
        &[id(&source)],
    );
    assert_owned(&home.0, &source);
    assert_owned(&home.0, &frame);
    assert_deleted(&home.0, &owner);
    assert_deleted(&home.0, &source);
    assert_deleted(&home.0, &frame);
}

#[test]
fn legacy_payload_bounds_and_invalid_typed_fields_fail_without_deleting_target() {
    for (kind, target_kind, limit) in [
        ("hotfix-scan", "hotfix-records", 4 << 20),
        ("hotfix-records", "hotfix-cache", 32 << 20),
        ("asset-export-manifest", "asset-image", 1 << 20),
        ("asset-demux-manifest", "asset-demux-frame", 16 << 20),
    ] {
        let home = Workspace::new();
        let target = capture(
            &home.0,
            b"original",
            target_kind,
            "fixture",
            "text/plain",
            &[],
        );
        let raw = vec![b' '; limit + 1];
        let owner = capture(
            &home.0,
            &raw,
            kind,
            "dbcache:sha256:fixture",
            "application/json",
            &[],
        );
        assert_eq!(
            evidence::remove_capture(&home.0, id(&target))
                .unwrap_err()
                .code,
            "vault.blob_limit"
        );
        assert_eq!(
            evidence::read_capture(&home.0, id(&target)).unwrap(),
            target
        );
        assert_deleted(&home.0, &owner);
        assert_deleted(&home.0, &target);
    }
    for body in [br#"{"pages":42}"#.as_slice(), br#"{"pages":[false]}"#, b"{"] {
        let home = Workspace::new();
        let target = capture(
            &home.0,
            b"page",
            "hotfix-records",
            "page",
            "text/plain",
            &[],
        );
        capture(
            &home.0,
            body,
            "hotfix-scan",
            "scan",
            "application/json",
            &[],
        );
        assert_eq!(
            evidence::remove_capture(&home.0, id(&target))
                .unwrap_err()
                .exit_code,
            4
        );
        assert_eq!(
            evidence::read_capture(&home.0, id(&target)).unwrap(),
            target
        );
    }
}

#[test]
fn legacy_duplicate_null_and_reused_array_fields_follow_go_decode_order() {
    for mode in [
        "page",
        "export",
        "scan",
        "frame",
        "frame-shorter",
        "unicode-fold",
    ] {
        let home = Workspace::new();
        let target_kind = match mode {
            "page" => "hotfix-cache",
            "export" => "asset-image",
            "scan" => "hotfix-records",
            _ => "asset-demux-frame",
        };
        let target = capture(
            &home.0,
            b"target",
            target_kind,
            "fixture",
            "text/plain",
            &[],
        );
        let target_id = id(&target);
        let (kind, raw) = match mode {
            "page" => (
                "hotfix-records",
                format!(
                    r#"{{"snapshot":"PIN-fixture","source":{{"schema":"lycheedev.capture.v1"}},"SOURCE":{{"id":"{target_id}"}},"source":null}}"#
                ),
            ),
            "export" => (
                "asset-export-manifest",
                format!(
                    r#"{{"schema":"lycheedev.asset-export.v1","artifactCapture":{{"schema":"lycheedev.capture.v1"}},"ArtifactCapture":{{"id":"{target_id}"}}}}"#
                ),
            ),
            "scan" => (
                "hotfix-scan",
                format!(r#"{{"pages":["{target_id}"],"PAGES":[null]}}"#),
            ),
            "frame-shorter" => (
                "asset-demux-manifest",
                format!(
                    r#"{{"schema":"lycheedev.asset-demux.v1","frames":[{{}},{{"capture":"{target_id}"}}],"frames":[{{}}],"frames":[{{}},null]}}"#
                ),
            ),
            "unicode-fold" => (
                "asset-demux-manifest",
                format!(
                    r#"{{"ſchema":"lycheedev.asset-demux.v1","frameſ":[{{"capture":"{target_id}"}}]}}"#
                ),
            ),
            _ => (
                "asset-demux-manifest",
                format!(
                    r#"{{"schema":"lycheedev.asset-demux.v1","frames":[{{"capture":"{target_id}"}}],"FRAMES":[{{"capture":null}}]}}"#
                ),
            ),
        };
        capture(
            &home.0,
            raw.as_bytes(),
            kind,
            "dbcache:sha256:fixture",
            "application/json",
            &[],
        );
        assert_owned(&home.0, &target);
    }
    for reset in ["null", "[]"] {
        let home = Workspace::new();
        let target = capture(
            &home.0,
            b"page",
            "hotfix-records",
            "page",
            "text/plain",
            &[],
        );
        let raw = format!(
            r#"{{"pages":["{}"],"pages":{reset},"pages":[null]}}"#,
            id(&target)
        );
        capture(
            &home.0,
            raw.as_bytes(),
            "hotfix-scan",
            "scan",
            "application/json",
            &[],
        );
        assert_deleted(&home.0, &target);
    }
}

// Admission is performed inside BEGIN IMMEDIATE before the dependency read.
// Blocking this precise callback proves the production capture/remove routes
// share the SQLite writer boundary, without relying on scheduler sleeps.
struct BlockingMeter {
    trigger: usize,
    blocked: Cell<bool>,
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
impl CaptureAccounting for BlockingMeter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, bytes: usize) -> Result<()> {
        if bytes == self.trigger && !self.blocked.replace(true) {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        Ok(())
    }
}

#[test]
fn dependency_publication_excludes_concurrent_removal_until_owner_commits() {
    let home = Workspace::new();
    let page = capture(&home.0, b"page bytes", "fixture", "page", "text/plain", &[]);
    let stored: evidence::CaptureRef = serde_json::from_value(page.clone()).unwrap();
    let trigger = serde_json::to_vec(&stored).unwrap().len() * 24 + 4096;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker_home = home.0.clone();
    let page_id = id(&page).to_owned();
    let worker = std::thread::spawn(move || {
        evidence::commit_capture_budgeted_with_dependencies(
            &worker_home,
            input(b"owner bytes", "fixture", "owner", "text/plain"),
            &[&page_id],
            &OperationControl::default(),
            &BlockingMeter {
                trigger,
                blocked: Cell::new(false),
                entered: entered_tx,
                release: release_rx,
            },
        )
    });
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(60));
    let error = evidence::remove_capture_controlled(&home.0, id(&page), &short).unwrap_err();
    release_tx.send(()).unwrap();
    let owner = worker.join().unwrap().unwrap();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.deadline", 5)
    );
    assert_owned(&home.0, &page);
    assert_deleted(&home.0, &owner);
    assert_deleted(&home.0, &page);
}

struct DenyReferenceMeter {
    trigger: usize,
}
impl CaptureAccounting for DenyReferenceMeter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, bytes: usize) -> Result<()> {
        if bytes == self.trigger {
            Err(ToolkitError::new(
                "query.budget_exceeded",
                "dependency allocation denied",
                3,
            ))
        } else {
            Ok(())
        }
    }
}
#[test]
fn dependency_validation_preserves_original_ledger_failure_and_rolls_back() {
    let home = Workspace::new();
    let page = capture(&home.0, b"page", "fixture", "page", "text/plain", &[]);
    let stored: evidence::CaptureRef = serde_json::from_value(page.clone()).unwrap();
    let meter = DenyReferenceMeter {
        trigger: serde_json::to_vec(&stored).unwrap().len() * 24 + 4096,
    };
    let body = b"allocation-refused body";
    let error = evidence::commit_capture_budgeted_with_dependencies(
        &home.0,
        input(body, "fixture", "owner", "text/plain"),
        &[id(&page)],
        &OperationControl::default(),
        &meter,
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("query.budget_exceeded", 3)
    );
    assert!(
        !evidence::blob_path(&home.0, &storage::sha256(body))
            .unwrap()
            .exists()
    );
    assert_deleted(&home.0, &page);
}

#[test]
fn unknown_manifest_extensions_do_not_block_unrelated_capture_removal() {
    for partial in [false, true] {
        let home = Workspace::new();
        let target = capture(&home.0, b"target", "fixture", "target", "text/plain", &[]);
        let unrelated = capture(&home.0, b"other", "fixture", "other", "text/plain", &[]);
        let mut extended = if partial {
            json!({"mediaType":"text/plain"})
        } else {
            unrelated.clone()
        };
        extended["note"] = json!({"harmlessExtension":true});
        if !partial {
            extended["blob"]["extension"] = json!([1, 2, 3]);
            extended["provenance"]["note"] = json!("unknown provenance field");
        }
        let key = format!("capture/{}", id(&unrelated));
        let mut metadata = Metadata::open(&home.0, true).unwrap();
        metadata.put(&key, &extended, Some(1)).unwrap();
        assert_deleted(&home.0, &target);
        assert_eq!(metadata.read::<Value>(&key).unwrap(), extended);
        assert_eq!(
            evidence::read_blob(&home.0, &unrelated["blob"], 100).unwrap(),
            b"other"
        );
    }
}

#[test]
fn selected_legacy_consumer_ignores_extensions_but_authenticates_known_manifest_fields() {
    let home = Workspace::new();
    let target = capture(
        &home.0,
        b"page",
        "hotfix-records",
        "target",
        "text/plain",
        &[],
    );
    let raw = serde_json::to_vec(&json!({"pages":[id(&target)]})).unwrap();
    let owner = capture(
        &home.0,
        &raw,
        "hotfix-scan",
        "scan",
        "application/json",
        &[],
    );
    let mut extended = owner.clone();
    extended["note"] = json!("future extension");
    extended["provenance"]["note"] = json!(42);
    extended["blob"]["note"] = Value::Null;
    let key = format!("capture/{}", id(&owner));
    let mut metadata = Metadata::open(&home.0, true).unwrap();
    metadata.put(&key, &extended, Some(1)).unwrap();
    assert_owned(&home.0, &target);
    assert_eq!(metadata.read::<Value>(&key).unwrap(), extended);
    extended["provenance"]["snapshot"] = json!("changed identity");
    metadata.put(&key, &extended, Some(2)).unwrap();
    let error = evidence::remove_capture(&home.0, id(&target)).unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.failed", 5)
    );
    assert_eq!(error.message, "evidence.manifest_integrity");
    assert_eq!(
        evidence::read_capture(&home.0, id(&target)).unwrap(),
        target
    );
}

#[test]
fn legacy_strings_replace_invalid_utf8_and_surrogates_without_losing_real_references() {
    for invalid in [
        br"\ud800".as_slice(),
        br"\udfff",
        br"\ud800\ud800",
        br"\ud83d\ude00",
        &[0xff],
        &[0xf0, 0x90, 0x80],
        &[0xed, 0xa0, 0x80],
    ] {
        for actual_reference in [false, true] {
            let home = Workspace::new();
            let target = capture(
                &home.0,
                b"page",
                "hotfix-records",
                "page",
                "text/plain",
                &[],
            );
            let mut raw = br#"{"pages":[""#.to_vec();
            raw.extend_from_slice(invalid);
            raw.push(b'"');
            if actual_reference {
                // A valid escaped real ID must still become a reference.
                raw.extend_from_slice(br#", "\u0043"#);
                raw.extend_from_slice(&id(&target).as_bytes()[1..]);
                raw.push(b'"');
            }
            raw.extend_from_slice(b"]}");
            let owner = capture(
                &home.0,
                &raw,
                "hotfix-scan",
                "scan",
                "application/json",
                &[],
            );
            if actual_reference {
                assert_owned(&home.0, &target);
            } else {
                assert_deleted(&home.0, &target);
            }
            assert_eq!(
                evidence::read_blob(&home.0, &owner["blob"], 1024).unwrap(),
                raw
            );
        }
    }
}

#[test]
fn legacy_capture_timestamps_follow_go_json_reader_including_fallback_boundaries() {
    // Oracle: Go 1.27.1 time.Time.UnmarshalJSON, parseStrictRFC3339 and
    // Parse(RFC3339). A valid time with an unrelated source permits removal;
    // an invalid typed timestamp must preserve the target even without a ref.
    let valid = [
        r#""2026-10-14T22:00:00Z""#,
        r#""2026-10-14T2:00:00Z""#,
        r#""2026-10-14T22:00:00,123456789012Z""#,
        r#""2026-10-14T22:00:00.0000000001+00:60""#,
        r#""2026-10-14T22:00:00+24:00""#,
        r#""2026-10-14T22:00:00-24:60""#,
        r#""0000-02-29T00:00:00-00:00""#,
        "null",
    ];
    let invalid = [
        r#""2026-10-14 22:00:00Z""#,
        r#""2026-10-14T22:00:00UTC""#,
        r#""2026-10-14T22:00:00+0100""#,
        r#""2026-10-14\u005422:00:00Z""#,
        r#""2026-10-14T22:00:00\u005a""#,
        r#""2026-10-14T22:00:00+25:00""#,
        r#""2026-10-14T22:00:00+00:61""#,
        r#""2026-10-14T24:00:00Z""#,
        r#""2026-10-14T22:00:60Z""#,
        r#""2026-02-29T22:00:00Z""#,
        r#""2026-10-14T22:00:00.Z""#,
        r#""2026-10-14t22:00:00z""#,
        "42",
    ];
    for (is_valid, timestamps) in [(true, valid.as_slice()), (false, invalid.as_slice())] {
        for timestamp in timestamps {
            for (kind, target_kind, body) in [
                ("hotfix-records", "hotfix-cache", "page"),
                ("asset-export-manifest", "asset-image", "export"),
            ] {
                let home = Workspace::new();
                let target = capture(&home.0, b"target", target_kind, "target", "text/plain", &[]);
                let embedded = format!(
                    r#"{{"schema":"lycheedev.capture.v1","id":"unrelated","capturedAt":{timestamp}}}"#
                );
                let raw = if body == "page" {
                    format!(r#"{{"snapshot":"PIN-fixture","source":{embedded}}}"#)
                } else {
                    format!(
                        r#"{{"schema":"lycheedev.asset-export.v1","artifactCapture":{embedded}}}"#
                    )
                };
                capture(
                    &home.0,
                    raw.as_bytes(),
                    kind,
                    "dbcache:sha256:fixture",
                    "application/json",
                    &[],
                );
                let result = evidence::remove_capture(&home.0, id(&target));
                if is_valid {
                    assert!(result.is_ok(), "valid {kind} {timestamp}: {result:?}");
                } else {
                    assert_eq!(
                        result.unwrap_err().exit_code,
                        4,
                        "invalid {kind} {timestamp}"
                    );
                    assert_eq!(
                        evidence::read_capture(&home.0, id(&target)).unwrap(),
                        target
                    );
                }
            }
        }
    }
}

#[test]
fn normalization_never_repairs_malformed_json_or_validates_unknown_time_fields() {
    for raw in [
        br#"{"notes":{"capturedAt":"x\q"},"pages":[]}"#.as_slice(),
        br#"{"notes":{"capturedAt":"\uXXXX"},"pages":[]}"#,
        b"{\"notes\":{\"capturedAt\":\"\\n\n\"},\"pages\":[]}",
    ] {
        let home = Workspace::new();
        let target = capture(
            &home.0,
            b"page",
            "hotfix-records",
            "page",
            "text/plain",
            &[],
        );
        capture(&home.0, raw, "hotfix-scan", "scan", "application/json", &[]);
        assert_eq!(
            evidence::remove_capture(&home.0, id(&target))
                .unwrap_err()
                .exit_code,
            4
        );
        assert_eq!(
            evidence::read_capture(&home.0, id(&target)).unwrap(),
            target
        );
    }
    let home = Workspace::new();
    let target = capture(
        &home.0,
        b"page",
        "hotfix-records",
        "page",
        "text/plain",
        &[],
    );
    let raw = br#"{"notes":{"capturedAt":"2026-10-14\u005422:00:00Z"},"pages":[]}"#;
    capture(&home.0, raw, "hotfix-scan", "scan", "application/json", &[]);
    assert_deleted(&home.0, &target);
}

fn replace_original_manifest_bytes(home: &Path, capture: &Value, raw: &[u8]) {
    // Bypass Value serialization deliberately: the fixture's original order,
    // repeated keys and escape bytes are the regression input being tested.
    let connection = rusqlite::Connection::open(home.join("state/toolkit.sqlite")).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE documents SET generation=generation+1,value=?1 WHERE key=?2",
                rusqlite::params![raw, format!("capture/{}", id(capture))]
            )
            .unwrap(),
        1
    );
}
fn original_manifest_json(capture: &Value) -> String {
    serde_json::to_string(&serde_json::from_value::<evidence::CaptureRef>(capture.clone()).unwrap())
        .unwrap()
}

#[test]
fn raw_manifest_field_order_object_merging_null_and_literals_cannot_hide_references() {
    for mode in [
        "media-case-order",
        "provenance-merge-null",
        "dependencies-case-null",
        "overwritten-literal",
    ] {
        let home = Workspace::new();
        let target = capture(
            &home.0,
            b"page",
            "hotfix-records",
            "page",
            "text/plain",
            &[],
        );
        let raw_body = serde_json::to_vec(&json!({"pages":[id(&target)]})).unwrap();
        let owner = capture(
            &home.0,
            &raw_body,
            "hotfix-scan",
            "scan",
            "application/json",
            &[],
        );
        let original = original_manifest_json(&owner);
        let raw = match mode {
            "media-case-order" => original.replace(
                r#""mediaType":"application/json""#,
                r#""mediaType":"text/plain","MEDIATYPE":"application/json""#,
            ),
            "provenance-merge-null" => {
                let provenance: evidence::Provenance =
                    serde_json::from_value(owner["provenance"].clone()).unwrap();
                let old = format!(
                    "\"provenance\":{}",
                    serde_json::to_string(&provenance).unwrap()
                );
                original.replace(&old, &format!(r#"{old},"PROVENANCE":{{"kind":"fixture"}},"provenance":{{"kind":"hotfix-scan"}},"provenance":null"#))
            }
            "dependencies-case-null" => format!(
                r#"{},"dependencies":["unrelated"],"DEPENDENCIES":null}}"#,
                original.strip_suffix('}').unwrap()
            ),
            _ => format!(
                r#"{},"dependencies":["{}"],"dependencies":[]}}"#,
                original.strip_suffix('}').unwrap(),
                id(&target)
            ),
        };
        assert_ne!(raw, original, "fixture did not change {mode}");
        replace_original_manifest_bytes(&home.0, &owner, raw.as_bytes());
        assert_owned(&home.0, &target);
        let connection = rusqlite::Connection::open(home.0.join("state/toolkit.sqlite")).unwrap();
        let observed: Vec<u8> = connection
            .query_row(
                "SELECT value FROM documents WHERE key=?1",
                [format!("capture/{}", id(&owner))],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(observed, raw.as_bytes());
    }
}

#[test]
fn raw_manifest_order_can_exclude_consumer_and_raw_timestamp_escapes_still_error() {
    let home = Workspace::new();
    let target = capture(
        &home.0,
        b"page",
        "hotfix-records",
        "page",
        "text/plain",
        &[],
    );
    let raw_body = serde_json::to_vec(&json!({"pages":[id(&target)]})).unwrap();
    let owner = capture(&home.0, &raw_body, "hotfix-scan", "scan", "text/plain", &[]);
    let raw = original_manifest_json(&owner).replace(
        r#""mediaType":"text/plain""#,
        r#""MEDIATYPE":"application/json","mediaType":"text/plain""#,
    );
    replace_original_manifest_bytes(&home.0, &owner, raw.as_bytes());
    assert_deleted(&home.0, &target);

    let home = Workspace::new();
    let target = capture(
        &home.0,
        b"page",
        "hotfix-records",
        "page",
        "text/plain",
        &[],
    );
    let owner = capture(
        &home.0,
        br#"{"pages":[]}"#,
        "hotfix-scan",
        "scan",
        "application/json",
        &[],
    );
    let timestamp = owner["capturedAt"].as_str().unwrap();
    let raw = original_manifest_json(&owner).replace(timestamp, &timestamp.replace('T', r"\u0054"));
    replace_original_manifest_bytes(&home.0, &owner, raw.as_bytes());
    assert_eq!(
        evidence::remove_capture(&home.0, id(&target))
            .unwrap_err()
            .exit_code,
        4
    );
    assert_eq!(
        evidence::read_capture(&home.0, id(&target)).unwrap(),
        target
    );
}
