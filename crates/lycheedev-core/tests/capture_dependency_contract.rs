//! Current typed capture dependency and reference contract: publication,
//! ordered identity, removal protection, concurrency and rollback. Offline
//! synthetic captures only; installed-data coverage lives in Data acceptance tests.
use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    evidence::{self, CaptureAccounting, CaptureInput},
    metadata::Metadata,
    storage, workspace,
};
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
            "lychee-capture-dependency-contract-{}-{}-{}",
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
                // The provider case isolates the locator guard; only the
                // separate snapshot case may fail the snapshot comparison.
                let snapshot = if mode == "snapshot" {
                    "wrong-snapshot"
                } else {
                    "PIN-fixture"
                };
                json!({"snapshot":snapshot,"source":{"schema":"lycheedev.capture.v1","id":id(&target)}})
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
