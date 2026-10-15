use super::status::*;
use super::*;
use lycheedev_core::{
    evidence::{BlobRef, CaptureRef},
    selection::PinnedSet,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap};
struct Fixture {
    work: Value,
    generation: i64,
    captures: BTreeMap<String, Captured>,
    reads: RefCell<Vec<String>>,
}
impl Fixture {
    fn pending() -> Self {
        Self {
            work: json!({"schema":"lycheedev.work.v1","operationId":"OP-old","generation":1,"intent":{"kind":"probe","resource":"window/1/2/3","snapshot":"PIN-test","session":"session","goal":"finished","request":{"schema":"lycheedev.report-intent.v1","expected":{"Kind":"reported","Release":"3.2.0","SessionNonce":"session","RequestID":"request","Character":"paladin","Realm":"realm","Product":"retail","Build":"12.1.0.69875"}}},"stage":"prepared","status":"pending","observation":null}),
            generation: 1,
            captures: BTreeMap::new(),
            reads: RefCell::new(vec![]),
        }
    }
    fn add(&mut self, id: &str, kind: &str, bytes: Vec<u8>) {
        let reference = CaptureRef {
            schema: "lycheedev.capture.v1".into(),
            id: id.into(),
            blob: BlobRef {
                sha256: lycheedev_core::storage::sha256(&bytes),
                bytes: bytes.len() as u64,
            },
            media_type: "application/json".into(),
            provenance: store::provenance(
                kind,
                "request",
                "PIN-test",
                "OP-old",
                "12.1.0.69875",
                "session",
            ),
            complete: true,
            truncated: false,
            captured_at: "2026-10-15T00:00:00Z".into(),
            dependencies: vec![],
        };
        self.captures
            .insert(id.into(), Captured { reference, bytes });
    }
    fn verified() -> Self {
        let mut f = Self::pending();
        let body=br#"{ "probeStatus":"completed", "acceptedBudgetSeconds":5, "result":{"passed":true}, "wide":9007199254740993 }"#.to_vec();
        let (mut a, mut b) = (1u32, 0u32);
        for byte in &body {
            a = (a + u32::from(*byte)) % 65521;
            b = (b + a) % 65521;
        }
        let receipt=serde_json::to_vec(&json!({"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"reported","sessionNonce":"session","requestId":"request","character":"paladin","realm":"realm","product":"retail","build":"12.1.0.69875","sequence":1,"inputReady":false,"reportBytes":body.len(),"reportAdler32":format!("{:08x}",(b<<16)|a)})).unwrap();
        f.add("CAP-body", "game-report", body);
        f.add("CAP-receipt", "game-receipt", receipt);
        f.work["stage"] = json!("verified");
        f.work["status"] = json!("running");
        f.work["intent"]["goal"] = json!("");
        f.work["intent"]["request"]["budgetSeconds"] = json!(5);
        f.work["observation"] = json!({"bodyId":"CAP-body","receiptId":"CAP-receipt"});
        f
    }
}
impl ReadStore for Fixture {
    fn document(&self, key: &str, c: &OperationControl) -> Result<RawDocument> {
        c.check()?;
        self.reads.borrow_mut().push(key.into());
        if key == "work/OP-old" {
            Ok(RawDocument {
                generation: self.generation,
                bytes: serde_json::to_vec(&self.work)?,
            })
        } else {
            Err(ToolkitError::new(
                "vault.record_missing",
                "vault.record_missing",
                2,
            ))
        }
    }
    fn capture(&self, id: &str, max: usize, c: &OperationControl) -> Result<Captured> {
        c.check()?;
        self.reads.borrow_mut().push(id.into());
        let cap = self
            .captures
            .get(id)
            .ok_or_else(|| fault("fixture.capture_missing"))?;
        assert!(cap.bytes.len() <= max);
        Ok(Captured {
            reference: cap.reference.clone(),
            bytes: cap.bytes.clone(),
        })
    }
    fn pin(&self, _: &str, _: &OperationControl) -> Result<PinnedSet> {
        panic!("OP status must not load pin or connect")
    }
}
#[test]
fn historical_status_pending_is_not_completion_or_input_authority() {
    let f = Fixture::pending();
    let value = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert!(!value.complete);
    assert_eq!(value.report.state, "unavailable");
    assert_eq!(value.cleanup, "pending");
    assert_eq!(
        value.next_action.unwrap().args,
        ["live", "abandon", "OP-old"]
    );
    assert_eq!(*f.reads.borrow(), ["work/OP-old"]);
}
#[test]
fn historical_status_generation_and_identity_checked_before_evidence() {
    for key in ["generation", "schema", "operationId"] {
        let mut f = Fixture::verified();
        f.work[key] = if key == "generation" {
            json!(2)
        } else {
            json!("wrong")
        };
        assert_eq!(
            read_status(&f, "OP-old", &OperationControl::default())
                .unwrap_err()
                .message,
            "journal: corrupt work record"
        );
        assert_eq!(*f.reads.borrow(), ["work/OP-old"]);
    }
}
#[test]
fn historical_status_verified_pending_cleanup_preserves_exact_raw_report() {
    let f = Fixture::verified();
    let value = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert!(!value.complete);
    assert_eq!(value.cleanup, "pending");
    assert_eq!(value.report.state, "verified");
    assert_eq!(
        value.report.content.unwrap().0,
        f.captures["CAP-body"].bytes
    );
    assert_eq!(value.business.unwrap().state, "passed");
}
#[test]
fn historical_status_abandoned_keeps_verified_report_but_never_complete() {
    let mut f = Fixture::verified();
    f.work["stage"] = json!("abandoned");
    f.work["status"] = json!("abandoned");
    f.work["intent"]["goal"] = json!("finished");
    let value = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert!(!value.complete);
    assert_eq!(value.report.state, "verified");
    assert_eq!(value.cleanup, "abandoned");
    assert!(value.next_action.is_none());
}
#[test]
fn historical_status_declared_budget_and_provenance_are_independent_gates() {
    let mut f = Fixture::verified();
    f.work["intent"]["request"]["budgetSeconds"] = json!(6);
    assert_eq!(
        read_status(&f, "OP-old", &OperationControl::default())
            .unwrap_err()
            .message,
        "live.execution_budget_mismatch"
    );
    let mut f = Fixture::verified();
    f.captures
        .get_mut("CAP-body")
        .unwrap()
        .reference
        .provenance
        .source_commit = "foreign".into();
    assert_eq!(
        read_status(&f, "OP-old", &OperationControl::default())
            .unwrap_err()
            .message,
        "evidence.report_provenance_mismatch"
    );
}
#[test]
fn historical_status_finished_requires_display_proof_even_after_cleanup() {
    let mut f = Fixture::verified();
    f.work["stage"] = json!("cleaned");
    f.work["status"] = json!("completed");
    f.work["intent"]["goal"] = json!("finished");
    let value = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert_eq!(value.cleanup, "complete");
    assert!(!value.complete);
    assert_eq!(value.display.unwrap().state, "pending");
}
#[test]
fn historical_status_cancelled_without_report_does_not_invent_result() {
    let mut f = Fixture::pending();
    f.work["stage"] = json!("cleaned");
    f.work["status"] = json!("cancelled");
    let value = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert_eq!(value.cleanup, "complete");
    assert!(!value.complete);
    assert_eq!(value.report.state, "unavailable");
}
#[test]
fn historical_status_original_stop_is_preserved_before_document() {
    let f = Fixture::pending();
    let c = OperationControl::uncancelable(
        std::time::Instant::now() - std::time::Duration::from_secs(1),
    );
    assert_eq!(
        read_status(&f, "OP-old", &c).unwrap_err().code,
        "command.deadline"
    );
    assert!(f.reads.borrow().is_empty());
}
#[test]
fn historical_report_code_repeated_arrays_and_null_reuse_go_storage() {
    let input: records::ReportIntent = decode_record(
        br#"{"code":[65,66],"code":[null],"code":[null,null]}"#,
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(input.code.bytes(), b"AB");
    let input: records::ReportIntent = decode_record(
        br#"{"code":[65],"code":null,"code":[null]}"#,
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(input.code.bytes(), [0]);
}

// Go fixedReloadOutcome: invalid intent precedes pending; pending skips capture.
#[test]
fn historical_fixed_reload_pending_and_completed_evidence_gates() {
    let mut f = Fixture::pending();
    f.work["intent"]["kind"] = json!("reload");
    f.work["intent"]["request"] = json!({"schema":"lycheedev.fixed-reload.v1","release":"3.2.0","identityNonce":"a".repeat(32),"target":{"client":{"directory":"C:/WoW","product":"retail","fullBuild":"12.1.0.69875"},"window":{"processId":73,"processStartedAt":"1"}}});
    let result = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert!(!result.complete);
    assert_eq!(*f.reads.borrow(), ["work/OP-old"]);
    f.work["stage"] = json!("cleaned");
    f.work["status"] = json!("completed");
    f.work["observation"] = json!({"schema":"lycheedev.fixed-reload-observation.v1","identityCapture":"CAP-identity","nextStep":6});
    let signal=serde_json::to_vec(&json!({"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"identity","probeNonce":"a".repeat(32),"actorState":"ok","character":"paladin","realm":"realm","guid":"Player-1-123","product":"retail","build":"12.1.0.69875","inputReady":true})).unwrap();
    f.add("CAP-identity", "decoded-fixed-reload", signal);
    f.captures
        .get_mut("CAP-identity")
        .unwrap()
        .reference
        .provenance
        .locator = "a".repeat(32);
    let result = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert!(result.complete);
    assert_eq!(result.runtime_capture, "CAP-identity");
    f.captures
        .get_mut("CAP-identity")
        .unwrap()
        .reference
        .provenance
        .operation_id = "OP-other".into();
    assert_eq!(
        read_status(&f, "OP-old", &OperationControl::default())
            .unwrap_err()
            .message,
        "live.fixed_reload_evidence_mismatch"
    );
}
#[test]
fn historical_fixed_reload_invalid_observation_precedes_pending_return() {
    let mut f = Fixture::pending();
    f.work["intent"]["kind"] = json!("reload");
    f.work["intent"]["request"] = json!({"schema":"lycheedev.fixed-reload.v1","release":"3.2.0","identityNonce":"nonce","target":{"client":{"directory":"C:/WoW"},"window":{"processId":1,"processStartedAt":"1"}}});
    f.work["observation"] = json!({"schema":"wrong"});
    assert_eq!(
        read_status(&f, "OP-old", &OperationControl::default())
            .unwrap_err()
            .message,
        "live.fixed_reload_observation_invalid"
    );
    assert_eq!(*f.reads.borrow(), ["work/OP-old"]);
}
#[test]
fn historical_standalone_reload_pending_then_ready_requires_all_provenance() {
    let mut f = Fixture::pending();
    let nonce = "a".repeat(32);
    let request = format!("RELOAD-{nonce}");
    f.work["intent"]["kind"] = json!("reload");
    f.work["intent"]["request"] = json!({"schema":"lycheedev.reload-intent.v1","binding":"SESSION-old","installation":std::env::temp_dir().to_str().unwrap(),"reloadNonce":nonce,"guid":"Player-1-123","expected":{"Kind":"ready","Release":"3.2.0","SessionNonce":"session","RequestID":request,"ReloadNonce":nonce,"Product":"retail","Build":"12.1.0.69875"}});
    assert!(
        !read_status(&f, "OP-old", &OperationControl::default())
            .unwrap()
            .complete
    );
    f.work["stage"] = json!("cleaned");
    f.work["status"] = json!("completed");
    f.work["observation"] =
        json!({"schema":"lycheedev.reload-observation.v1","readyCapture":"CAP-ready"});
    f.add("CAP-ready","decoded-standalone-reload",serde_json::to_vec(&json!({"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"ready","sessionNonce":"session","requestId":request,"reloadNonce":nonce,"guid":"Player-1-123","product":"retail","build":"12.1.0.69875","sequence":1,"inputReady":true})).unwrap());
    f.captures
        .get_mut("CAP-ready")
        .unwrap()
        .reference
        .provenance
        .locator = nonce;
    assert!(
        read_status(&f, "OP-old", &OperationControl::default())
            .unwrap()
            .complete
    );
    f.captures
        .get_mut("CAP-ready")
        .unwrap()
        .reference
        .provenance
        .base_snapshot = "unexpected".into();
    assert_eq!(
        read_status(&f, "OP-old", &OperationControl::default())
            .unwrap_err()
            .message,
        "live.invalid_reload_receipt"
    );
}
#[test]
fn historical_report_error_capture_requires_failed_dispatch_and_exact_identity() {
    let mut f = Fixture::pending();
    f.work["stage"] = json!("dispatch_requested");
    f.work["status"] = json!("failed");
    f.work["observation"] = json!({"reportErrorCapture":"CAP-error"});
    f.add("CAP-error","decoded-game-report-error",serde_json::to_vec(&json!({"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"report_error","sessionNonce":"session","requestId":"request","product":"retail","build":"12.1.0.69875","sequence":1,"errorCode":"probe_failed","inputReady":false})).unwrap());
    let result = read_status(&f, "OP-old", &OperationControl::default()).unwrap();
    assert_eq!(result.report.state, "unavailable");
    assert_eq!(result.report.error_code, "probe_failed");
    assert!(!result.complete);
    f.captures
        .get_mut("CAP-error")
        .unwrap()
        .reference
        .provenance
        .locator = "different".into();
    assert_eq!(
        read_status(&f, "OP-old", &OperationControl::default())
            .unwrap_err()
            .message,
        "live.report_error_evidence_mismatch"
    );
}
