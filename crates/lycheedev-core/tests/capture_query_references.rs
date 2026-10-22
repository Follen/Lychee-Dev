//! External fixed reference records.QueryData's WriteMetadata publication
//! boundary. Required SQL inputs are atomic checks, not persistent
//! CaptureDraft dependencies.
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
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
struct Home(PathBuf);
impl Home {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "lychee-query-references-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        workspace::initialize(&path).unwrap();
        Self(path)
    }
}
impl Drop for Home {
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
fn input(bytes: &[u8]) -> CaptureInput<'_> {
    CaptureInput {
        bytes,
        maximum_bytes: bytes.len(),
        media_type: "application/json",
        provenance: json!({"kind":"data-query","locator":"sql:sha256:fixture","snapshot":"PIN-fixture"}),
        complete: true,
        truncated: false,
    }
}
fn source(home: &Home) -> Value {
    evidence::commit_capture(
        &home.0,
        b"raw cache",
        "application/octet-stream",
        json!({"kind":"hotfix-cache","locator":"dbcache:sha256:fixture","snapshot":"PIN-fixture"}),
        true,
        false,
    )
    .unwrap()
}
fn id(capture: &Value) -> &str {
    capture["id"].as_str().unwrap()
}
fn publish(
    home: &Home,
    bytes: &[u8],
    required: &[&str],
    meter: &impl CaptureAccounting,
) -> Result<Value> {
    evidence::commit_capture_budgeted_referencing(
        &home.0,
        input(bytes),
        required,
        &OperationControl::default(),
        meter,
    )
}
#[test]
fn query_references_preserve_identity_shape_without_lifetime_ownership() {
    let home = Home::new();
    let source = source(&home);
    let raw =
        serde_json::to_vec(&json!({"effectiveSources":[{"captures":[source.clone()]}]})).unwrap();
    let query = publish(&home, &raw, &[id(&source)], &Meter).unwrap();
    assert!(query.get("dependencies").is_none());
    let mut typed: evidence::CaptureRef = serde_json::from_value(query.clone()).unwrap();
    let expected = typed.id.clone();
    typed.id.clear();
    assert_eq!(
        expected,
        format!(
            "CAP-{}",
            storage::sha256(&storage::identity_json(&typed).unwrap())
        )
    );
    assert_eq!(
        evidence::remove_capture(&home.0, id(&source)).unwrap()["manifestRemoved"],
        true,
        "The fixed reference does not classify data-query payloads as persistent dependencies"
    );
    assert_eq!(evidence::read_capture(&home.0, id(&query)).unwrap(), query);
}
#[test]
fn query_references_reject_missing_and_aba_tampering_before_blob_publication() {
    let home = Home::new();
    let source = source(&home);
    let key = format!("capture/{}", id(&source));
    let mut metadata = Metadata::open(&home.0, true).unwrap();
    let body = b"missing query";
    assert_eq!(
        publish(&home, body, &["CAP-absent"], &Meter)
            .unwrap_err()
            .code,
        "vault.record_missing"
    );
    assert!(
        !evidence::blob_path(&home.0, &storage::sha256(body))
            .unwrap()
            .exists()
    );
    let mut changed = source.clone();
    changed["complete"] = json!(false);
    metadata.put(&key, &changed, Some(1)).unwrap();
    let body = b"tampered query";
    assert_eq!(
        publish(&home, body, &[id(&source)], &Meter)
            .unwrap_err()
            .code,
        "evidence.capture_integrity"
    );
    assert!(
        !evidence::blob_path(&home.0, &storage::sha256(body))
            .unwrap()
            .exists()
    );
    // Restoring byte-identical immutable identity is legal; generation itself
    // is not evidence of a different capture. A new body under the old ID is not.
    metadata.put(&key, &source, Some(2)).unwrap();
    assert!(publish(&home, b"restored query", &[id(&source)], &Meter).is_ok());
    assert_eq!(metadata.list::<Value>("capture/", 100).unwrap().len(), 2);
}
struct Deny;
impl CaptureAccounting for Deny {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_dependency(&self, _: usize) -> Result<()> {
        Err(ToolkitError::new(
            "query.budget_exceeded",
            "reference budget exhausted",
            3,
        ))
    }
}
#[test]
fn query_reference_budget_failure_rolls_back_and_later_query_recovers() {
    let home = Home::new();
    let source = source(&home);
    let body = b"budget denied query";
    assert_eq!(
        publish(&home, body, &[id(&source)], &Deny)
            .unwrap_err()
            .code,
        "query.budget_exceeded"
    );
    assert!(
        !evidence::blob_path(&home.0, &storage::sha256(body))
            .unwrap()
            .exists()
    );
    assert_eq!(
        Metadata::open(&home.0, false)
            .unwrap()
            .list::<Value>("capture/", 100)
            .unwrap()
            .len(),
        1
    );
    assert!(publish(&home, body, &[id(&source)], &Meter).is_ok());
}
struct Blocking {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    blocked: Cell<bool>,
}
impl CaptureAccounting for Blocking {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_work(&self, _: usize) -> Result<()> {
        Ok(())
    }
    fn admit_dependency(&self, _: usize) -> Result<()> {
        if !self.blocked.replace(true) {
            self.entered.send(()).unwrap();
            self.release.recv_timeout(Duration::from_secs(10)).unwrap();
        }
        Ok(())
    }
}
#[test]
fn query_reference_publication_excludes_removal_only_until_commit() {
    let home = Home::new();
    let source = source(&home);
    let source_id = id(&source).to_owned();
    let path = home.0.clone();
    let (entered, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        evidence::commit_capture_budgeted_referencing(
            &path,
            input(b"concurrent query"),
            &[&source_id],
            &OperationControl::default(),
            &Blocking {
                entered,
                release: resume,
                blocked: Cell::new(false),
            },
        )
    });
    wait.recv_timeout(Duration::from_secs(5)).unwrap();
    let short = OperationControl::uncancelable(Instant::now() + Duration::from_millis(60));
    let result = evidence::remove_capture_controlled(&home.0, id(&source), &short);
    release.send(()).unwrap();
    let query = worker.join().unwrap().unwrap();
    assert_eq!(result.unwrap_err().code, "command.deadline");
    assert!(query.get("dependencies").is_none());
    assert_eq!(
        evidence::remove_capture(&home.0, id(&source)).unwrap()["manifestRemoved"],
        true
    );
}
