use lycheedev_core::{
    Result, ToolkitError,
    control::OperationControl,
    evidence::{self, CaptureAccounting, CaptureHeader, CaptureInput},
    metadata::Metadata,
    workspace,
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    fs,
    io::Read,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "lycheedev-capture-budget-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        workspace::initialize(&path).unwrap();
        Self(path)
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Meter {
    maximum: usize,
    allocated: Cell<usize>,
    maximum_work: usize,
    work: Cell<usize>,
    cancel: Option<Arc<AtomicBool>>,
}
impl Meter {
    fn new(maximum: usize) -> Self {
        Self {
            maximum,
            allocated: Cell::new(0),
            maximum_work: usize::MAX,
            work: Cell::new(0),
            cancel: None,
        }
    }
}
impl CaptureAccounting for Meter {
    fn check(&self) -> Result<()> {
        Ok(())
    }
    fn admit_allocation(&self, bytes: usize) -> Result<()> {
        let next = self.allocated.get().checked_add(bytes).unwrap();
        if next > self.maximum {
            return Err(
                ToolkitError::new("query.budget_exceeded", "capture metadata denied", 3)
                    .with_details(json!({"producer":"shared-query-budget"})),
            );
        }
        self.allocated.set(next);
        Ok(())
    }
    fn admit_work(&self, units: usize) -> Result<()> {
        let next = self.work.get().checked_add(units).unwrap();
        if next > self.maximum_work {
            return Err(ToolkitError::new(
                "query.budget_exceeded",
                "capture work denied",
                3,
            ));
        }
        self.work.set(next);
        if let Some(cancel) = &self.cancel {
            cancel.store(true, Ordering::Release);
        }
        Ok(())
    }
}
fn input<'a>(bytes: &'a [u8], locator: &str) -> CaptureInput<'a> {
    CaptureInput {
        bytes,
        maximum_bytes: bytes.len(),
        media_type: "application/json",
        provenance: json!({"kind":"fixture","locator":locator}),
        complete: true,
        truncated: false,
    }
}
fn captures(home: &Workspace) -> Vec<Value> {
    Metadata::open(&home.0, false)
        .unwrap()
        .list("capture/", 100)
        .unwrap()
}

#[test]
fn metadata_budget_refusal_preserves_original_capture_and_blob_for_both_body_paths() {
    let home = Workspace::new();
    let bytes = b"original evidence";
    let control = OperationControl::default();
    let original =
        evidence::commit_capture_bounded(&home.0, input(bytes, "original"), &control).unwrap();
    let before = captures(&home);
    let error = evidence::commit_capture_budgeted(
        &home.0,
        input(bytes, "new"),
        &control,
        &Meter::new(2048),
    )
    .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("query.budget_exceeded", 3)
    );
    assert_eq!(error.details, json!({"producer":"shared-query-budget"}));
    assert_eq!(captures(&home), before);
    assert_eq!(
        evidence::read_capture(&home.0, original["id"].as_str().unwrap()).unwrap(),
        original
    );
    assert_eq!(
        evidence::read_blob(&home.0, &original["blob"], 100).unwrap(),
        bytes
    );

    let verified =
        evidence::open_verified_blob_bounded(&home.0, &original["blob"], 100, &control, |_| Ok(()))
            .unwrap();
    let error = evidence::commit_verified_capture_budgeted(
        &verified,
        CaptureHeader {
            media_type: "application/json",
            provenance: json!({"kind":"fixture","locator":"verified-new"}),
            complete: true,
            truncated: false,
        },
        &control,
        &Meter::new(2048),
    )
    .unwrap_err();
    assert_eq!(error.code, "query.budget_exceeded");
    assert_eq!(captures(&home), before);
    assert_eq!(
        verified.reference().sha256,
        original["blob"]["sha256"].as_str().unwrap()
    );
}

#[test]
fn cancellation_and_oversize_metadata_never_insert_new_capture_documents() {
    let home = Workspace::new();
    let bytes = b"body";
    let control = OperationControl::default();
    let original =
        evidence::commit_capture_bounded(&home.0, input(bytes, "original"), &control).unwrap();
    let before = captures(&home);
    let cancel = Arc::new(AtomicBool::new(false));
    let controlled = OperationControl::new(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        cancel.clone(),
    );
    let meter = Meter {
        maximum: usize::MAX,
        allocated: Cell::new(0),
        maximum_work: usize::MAX,
        work: Cell::new(0),
        cancel: Some(cancel),
    };
    let error =
        evidence::commit_capture_budgeted(&home.0, input(bytes, "cancelled"), &controlled, &meter)
            .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("command.cancelled", 7)
    );
    assert_eq!(captures(&home), before);

    let verified =
        evidence::open_verified_blob_bounded(&home.0, &original["blob"], 100, &control, |_| Ok(()))
            .unwrap();
    let error = evidence::commit_verified_capture_budgeted(
        &verified,
        CaptureHeader {
            media_type: "application/json",
            provenance: json!({"kind":"fixture","locator":"x".repeat(1_048_576)}),
            complete: true,
            truncated: false,
        },
        &control,
        &Meter::new(usize::MAX),
    )
    .unwrap_err();
    assert_eq!(error.code, "input.invalid");
    assert_eq!(error.message, "metadata document exceeds 1 MiB");
    assert_eq!(captures(&home), before);
    assert_eq!(
        evidence::read_blob(&home.0, &original["blob"], 100).unwrap(),
        bytes
    );
}

#[test]
fn materialized_body_work_refusal_preserves_existing_evidence_before_capture_insertion() {
    let home = Workspace::new();
    let bytes = b"original body";
    let control = OperationControl::default();
    let original =
        evidence::commit_capture_bounded(&home.0, input(bytes, "original"), &control).unwrap();
    let before = captures(&home);
    let meter = Meter {
        maximum_work: 0,
        ..Meter::new(usize::MAX)
    };
    let error =
        evidence::commit_capture_budgeted(&home.0, input(bytes, "work-denied"), &control, &meter)
            .unwrap_err();
    assert_eq!(
        (error.code.as_str(), error.exit_code),
        ("query.budget_exceeded", 3)
    );
    assert_eq!(error.message, "capture work denied");
    assert_eq!(meter.work.get(), 0);
    assert_eq!(captures(&home), before);
    assert_eq!(
        evidence::read_capture(&home.0, original["id"].as_str().unwrap()).unwrap(),
        original
    );
    assert_eq!(
        evidence::read_blob(&home.0, &original["blob"], 100).unwrap(),
        bytes
    );
}

#[test]
fn budgeted_capture_public_identity_and_metadata_match_existing_reader_with_unicode() {
    let home = Workspace::new();
    let bytes = b"{\"result\":true}";
    let control = OperationControl::default();
    let meter = Meter::new(usize::MAX);
    let capture = evidence::commit_capture_budgeted(
        &home.0,
        input(bytes, "<tag>&界🙂\u{2028}\u{2029}"),
        &control,
        &meter,
    )
    .unwrap();
    let id = capture["id"].as_str().unwrap();
    assert_eq!(evidence::read_capture(&home.0, id).unwrap(), capture);
    assert_eq!(
        evidence::read_blob(&home.0, &capture["blob"], 100).unwrap(),
        bytes
    );
    let mut verified =
        evidence::open_verified_blob_bounded(&home.0, &capture["blob"], 100, &control, |_| Ok(()))
            .unwrap();
    let verified_capture = evidence::commit_verified_capture_budgeted(
        &verified,
        CaptureHeader {
            media_type: "application/json",
            provenance: json!({"kind":"fixture","locator":"verified <界>"}),
            complete: true,
            truncated: false,
        },
        &control,
        &meter,
    )
    .unwrap();
    assert_eq!(
        evidence::read_capture(&home.0, verified_capture["id"].as_str().unwrap()).unwrap(),
        verified_capture
    );
    let mut first = [0];
    verified.read_exact(&mut first).unwrap();
    assert_eq!(first[0], bytes[0]);
    let mut metadata = Metadata::open(&home.0, true).unwrap();
    let key = format!("capture/{id}");
    metadata
        .put(
            &key,
            &serde_json::from_value::<evidence::CaptureRef>(capture.clone()).unwrap(),
            None,
        )
        .unwrap();
    assert_eq!(metadata.generation(&key).unwrap(), Some(1));
    let mut changed = serde_json::from_value::<evidence::CaptureRef>(capture).unwrap();
    changed.provenance.locator = "different".into();
    assert_eq!(
        metadata.put(&key, &changed, None).unwrap_err().code,
        "vault.generation_conflict"
    );
    metadata.put(&key, &changed, Some(1)).unwrap();
    assert_eq!(metadata.generation(&key).unwrap(), Some(2));
    assert!(meter.allocated.get() > 0);
}
