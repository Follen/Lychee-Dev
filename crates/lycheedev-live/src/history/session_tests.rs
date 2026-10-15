//! New fixture mappings: all tests in this file are NOT_RUN. These are seam
//! tests, not disk/backend, Windows, Go execution, or real-client acceptance.
use super::super::{Captured, RawDocument};
use super::*;
use lycheedev_core::{
    ToolkitError,
    evidence::{BlobRef, CaptureRef, Provenance},
    selection::{SelectionSpec, SourcePin},
};
use std::cell::RefCell;

struct Fixture {
    record: SessionRecord,
    raw_record: Vec<u8>,
    payload: WindowSessionEvidence,
    capture: Captured,
    pin: PinnedSet,
    calls: RefCell<Vec<String>>,
    fail_at: Option<&'static str>,
    cancel_at: Option<&'static str>,
    cancellation: Option<lycheedev_core::control::TrackedCancellation>,
}
impl Fixture {
    fn new() -> Self {
        let mut pin = PinnedSet {
            schema: "lycheedev.selection.v1".into(),
            id: String::new(),
            spec: SelectionSpec {
                source: Some(SourcePin {
                    repository: "wow-ui-source".into(),
                    product: "retail".into(),
                    requested_ref: "retail".into(),
                    exact_commit: "a".repeat(40),
                    parser_revision: "fixture".into(),
                }),
                ..SelectionSpec::default()
            },
        };
        pin.id = format!(
            "PIN-{}",
            storage::sha256(&storage::identity_json(&pin).unwrap())
        );
        let mut target = ConnectionTarget::default();
        target.client.product = "retail".into();
        target.client.full_build = "12.1.0.12345".into();
        target.window.process_id = 73;
        target.window.handle = u64::MAX;
        target.window.process_started_at = 133_333_333_333_333_333;
        let payload = WindowSessionEvidence {
            schema: EVIDENCE_SCHEMA.into(),
            target,
            ready: Signal {
                schema: "lycheedev.signal.v1".into(),
                release: "3.2.0".into(),
                kind: "ready".into(),
                session_nonce: "a".repeat(32),
                character: "Character".into(),
                realm: "Realm".into(),
                guid: "Player-1-000001".into(),
                product: "retail".into(),
                build: "12.1.0.12345".into(),
                sequence: 1,
                input_ready: true,
                ..Signal::default()
            },
        };
        let capture = Captured {
            bytes: vec![],
            reference: CaptureRef {
                schema: "lycheedev.capture.v1".into(),
                id: String::new(),
                blob: BlobRef {
                    sha256: String::new(),
                    bytes: 0,
                },
                media_type: "application/json".into(),
                provenance: Provenance {
                    kind: "decoded-window-session".into(),
                    locator: format!("pid:73/window:{}", u64::MAX),
                    snapshot: pin.id.clone(),
                    operation_id: String::new(),
                    source_commit: String::new(),
                    base_snapshot: String::new(),
                    base_source_commit: String::new(),
                    data_build: "12.1.0.12345".into(),
                    session: "a".repeat(32),
                },
                complete: true,
                truncated: false,
                captured_at: "2026-10-15T00:00:00Z".into(),
                dependencies: vec![],
            },
        };
        let record = SessionRecord {
            schema: SESSION_SCHEMA.into(),
            snapshot: pin.id.clone(),
            ..SessionRecord::default()
        };
        let mut fixture = Self {
            record,
            raw_record: vec![],
            payload,
            capture,
            pin,
            calls: RefCell::new(vec![]),
            fail_at: None,
            cancel_at: None,
            cancellation: None,
        };
        fixture.seal_payload();
        fixture
    }
    fn seal_record(&mut self) {
        self.record.id = record_id(&self.record).unwrap();
        self.raw_record = storage::identity_json(&self.record).unwrap();
    }
    fn seal_capture(&mut self) {
        self.capture.reference.blob = BlobRef {
            sha256: storage::sha256(&self.capture.bytes),
            bytes: self.capture.bytes.len() as u64,
        };
        self.capture.reference.id.clear();
        self.capture.reference.id = format!(
            "CAP-{}",
            storage::sha256(&storage::identity_json(&self.capture.reference).unwrap())
        );
        self.record.capture_id = self.capture.reference.id.clone();
        self.seal_record();
    }
    fn seal_payload(&mut self) {
        self.capture.bytes = storage::identity_json(&self.payload).unwrap();
        self.seal_capture();
    }
    fn seal_pin(&mut self) {
        self.pin.id.clear();
        self.pin.id = format!(
            "PIN-{}",
            storage::sha256(&storage::identity_json(&self.pin).unwrap())
        );
        self.record.snapshot = self.pin.id.clone();
        self.capture.reference.provenance.snapshot = self.pin.id.clone();
        self.seal_capture();
    }
    fn call(&self, point: &'static str, control: &OperationControl) -> Result<()> {
        control.check()?;
        self.calls.borrow_mut().push(point.into());
        if self.fail_at == Some(point) {
            return Err(ToolkitError::new(
                "fixture.read_failed",
                format!("fixture.{point}"),
                5,
            ));
        }
        if self.cancel_at == Some(point) {
            self.cancellation.as_ref().unwrap().cancel();
        }
        Ok(())
    }
    fn read(&self) -> Result<RecordedSession> {
        read_session(self, &self.record.id, &OperationControl::default())
    }
    fn reason(&self) -> String {
        self.read().unwrap_err().message
    }
}
impl ReadStore for Fixture {
    fn document(&self, key: &str, control: &OperationControl) -> Result<RawDocument> {
        self.call("document", control)?;
        assert_eq!(key, format!("session/{}", self.record.id));
        Ok(RawDocument {
            generation: 1,
            bytes: self.raw_record.clone(),
        })
    }
    fn pin(&self, id: &str, control: &OperationControl) -> Result<PinnedSet> {
        self.call("pin", control)?;
        assert_eq!(id, self.pin.id);
        // Authenticate the typed pin through the exact wire reader, not a
        // hand-constructed success result masquerading as verified storage.
        super::super::pins::read_pin(&storage::identity_json(&self.pin)?, id, control)
    }
    fn capture(&self, id: &str, max: usize, control: &OperationControl) -> Result<Captured> {
        self.call("capture", control)?;
        assert_eq!(max, 16_384);
        assert_eq!(id, self.capture.reference.id);
        if self.capture.bytes.len() > max {
            return Err(failure("vault.blob_limit"));
        }
        let mut unsigned = self.capture.reference.clone();
        unsigned.id.clear();
        if id
            != format!(
                "CAP-{}",
                storage::sha256(&storage::identity_json(&unsigned)?)
            )
            || self.capture.reference.blob.sha256 != storage::sha256(&self.capture.bytes)
            || self.capture.reference.blob.bytes != self.capture.bytes.len() as u64
        {
            return Err(failure("fixture.capture_integrity"));
        }
        Ok(Captured {
            reference: self.capture.reference.clone(),
            bytes: self.capture.bytes.clone(),
        })
    }
}

#[test]
fn history_session_valid_projection_defaults_after_record_authentication() {
    let fixture = Fixture::new();
    let session = fixture.read().unwrap();
    assert_eq!(&*fixture.calls.borrow(), &["document", "pin", "capture"]);
    assert_eq!(session.record.id, fixture.record.id);
    assert_eq!(session.record.bindings.wake_binding, "ALT-CTRL-]");
    assert!(fixture.record.bindings.wake_binding.is_empty());
    // Defaulting after hash validation intentionally means the returned
    // presentation is not the original hash input. No re-publication occurs.
    assert_ne!(record_id(&session.record).unwrap(), fixture.record.id);
    let projection = serde_json::to_value(session.connection()).unwrap();
    assert_eq!(
        projection["target"]["window"]["handle"],
        u64::MAX.to_string()
    );
    assert_eq!(projection["character"], "Character");
    assert_eq!(projection.as_object().unwrap().len(), 6);
    assert!(projection.get("inputReady").is_none());
    assert!(projection.get("bindings").is_none());
}

#[test]
fn history_session_record_identity_schema_and_hash_fail_before_dependencies() {
    let mut fixture = Fixture::new();
    let mut wrong = fixture.record.clone();
    wrong.id = "SESSION-other".into();
    fixture.raw_record = storage::identity_json(&wrong).unwrap();
    assert_eq!(fixture.reason(), "live.session_record_identity");
    fixture.calls.borrow_mut().clear();
    fixture.record.schema = "future".into();
    fixture.seal_record();
    assert_eq!(fixture.reason(), "live.session_record_integrity");
    fixture.record.schema = SESSION_SCHEMA.into();
    fixture.seal_record();
    fixture.record.region.max.x = 400;
    fixture.raw_record = storage::identity_json(&fixture.record).unwrap();
    assert_eq!(fixture.reason(), "live.session_record_integrity");
    assert!(fixture.calls.borrow().iter().all(|c| c == "document"));
}

#[test]
fn history_session_record_typed_duplicate_null_case_unknown_and_signed_rectangle() {
    let mut fixture = Fixture::new();
    fixture.record.region.min.x = -4;
    fixture.record.region.min.y = i64::MIN;
    fixture.seal_record();
    let raw = String::from_utf8(fixture.raw_record.clone()).unwrap();
    fixture.raw_record = format!("{},\"SCHEMA\":null,\"region\":{{\"Min\":{{\"x\":-4}}}},\"region\":null,\"future\":{{\"input\":true}}}}", raw.strip_suffix('}').unwrap()).into_bytes();
    let session = fixture.read().unwrap();
    assert_eq!(session.record.region.min.x, -4);
    assert_eq!(session.record.region.min.y, i64::MIN);
    fixture.record.region.min.x = 0;
    fixture.seal_record();
    let raw = String::from_utf8(fixture.raw_record.clone()).unwrap();
    fixture.raw_record = raw.replacen("\"X\":0", "\"X\":-0", 1).into_bytes();
    assert_eq!(fixture.read().unwrap().record.region.min.x, 0);
    fixture.raw_record = raw.replacen("\"X\":0", "\"X\":0.0", 1).into_bytes();
    assert!(fixture.read().is_err());
}

#[test]
fn history_session_missing_wake_replaces_whole_bindings_but_nonempty_invalid_rejects() {
    let mut fixture = Fixture::new();
    fixture.record.bindings.submit_binding = "untrusted old value".into();
    fixture.record.bindings.close_binding = "also ignored".into();
    fixture.seal_record();
    assert_eq!(
        fixture.read().unwrap().record.bindings.close_binding,
        "ALT-CTRL-["
    );
    fixture.record.bindings.wake_binding = "CTRL-ALT-]".into();
    fixture.seal_record();
    assert_eq!(fixture.reason(), "desktop.receiver_binding_invalid");
    fixture.record.bindings = default_receiver_bindings();
    fixture.record.bindings.close_binding = "ALT-CTRL-]".into();
    fixture.seal_record();
    assert_eq!(fixture.reason(), "desktop.receiver_binding_duplicate");
    fixture.record.bindings.wake_binding = "ALT-CTRL-[".into();
    fixture.record.bindings.close_binding = "ALT-CTRL-SHIFT-[".into();
    fixture.seal_record();
    assert_eq!(fixture.reason(), "desktop.receiver_close_key_conflict");
}

#[test]
fn history_session_evidence_requires_original_canonical_bytes_and_schema() {
    for variant in 0..5 {
        let mut fixture = Fixture::new();
        match variant {
            0 => fixture.capture.bytes.push(b'\n'),
            1 => {
                fixture.capture.bytes = String::from_utf8(fixture.capture.bytes)
                    .unwrap()
                    .replacen("\"schema\"", "\"Schema\"", 1)
                    .into_bytes()
            }
            2 => {
                let old = String::from_utf8(fixture.capture.bytes).unwrap();
                fixture.capture.bytes =
                    format!("{},\"unknown\":false}}", old.strip_suffix('}').unwrap()).into_bytes();
            }
            3 => {
                let old = String::from_utf8(fixture.capture.bytes).unwrap();
                fixture.capture.bytes =
                    format!("{},\"ready\":null}}", old.strip_suffix('}').unwrap()).into_bytes();
            }
            _ => {
                fixture.payload.schema = "future".into();
                fixture.capture.bytes = storage::identity_json(&fixture.payload).unwrap();
            }
        }
        fixture.seal_capture();
        assert_eq!(
            fixture.reason(),
            "live.session_evidence_invalid",
            "variant {variant}"
        );
    }
}

#[test]
fn history_session_canonical_html_escaping_and_quoted_u64_identity() {
    let mut fixture = Fixture::new();
    fixture.payload.target.window.title = "<title>&\u{2028}\u{2029}".into();
    fixture.seal_payload();
    assert!(fixture.read().is_ok());
    let raw = String::from_utf8(fixture.capture.bytes.clone()).unwrap();
    assert!(raw.contains("\\u003ctitle\\u003e\\u0026\\u2028\\u2029"));
    fixture.capture.bytes = raw.replace("\\u003c", "<").into_bytes();
    fixture.seal_capture();
    assert_eq!(fixture.reason(), "live.session_evidence_invalid");
}

#[test]
fn history_session_signal_parse_match_and_session_only_constraints_are_ordered() {
    let cases: Vec<(Box<dyn Fn(&mut Signal)>, &str)> = vec![
        (
            Box::new(|s| s.schema = "wrong".into()),
            "bridge.invalid_signal",
        ),
        (
            Box::new(|s| s.session_nonce.clear()),
            "bridge.signal_missing_session",
        ),
        (
            Box::new(|s| s.input_ready = false),
            "bridge.input_not_ready",
        ),
        (Box::new(|s| s.guid.clear()), "live.invalid_session_signal"),
        (
            Box::new(|s| s.request_id = "old-request".into()),
            "live.invalid_session_signal",
        ),
        (
            Box::new(|s| s.reload_nonce = "b".repeat(32)),
            "live.invalid_session_signal",
        ),
        (
            Box::new(|s| {
                s.code_bytes = 1;
                s.code_adler32 = "00000001".into();
            }),
            "live.invalid_session_signal",
        ),
        (
            Box::new(|s| {
                s.report_bytes = 1;
                s.report_adler32 = "00000001".into();
            }),
            "live.invalid_session_signal",
        ),
        (
            Box::new(|s| s.code_adler32 = "00000000".into()),
            "live.invalid_session_signal",
        ),
        (
            Box::new(|s| s.report_adler32 = "00000000".into()),
            "live.invalid_session_signal",
        ),
        (
            Box::new(|s| s.sequence = 9_007_199_254_740_992),
            "bridge.invalid_signal",
        ),
    ];
    for (edit, expected) in cases {
        let mut fixture = Fixture::new();
        edit(&mut fixture.payload.ready);
        fixture.seal_payload();
        assert_eq!(fixture.reason(), expected);
    }
}

#[test]
fn history_session_target_expectation_requires_exact_build_product_labels_and_nonce() {
    for change in 0..5 {
        let mut fixture = Fixture::new();
        match change {
            0 => fixture.payload.target.client.product = "classic".into(),
            1 => fixture.payload.target.client.full_build = "12.1.0.12346".into(),
            2 => fixture.payload.ready.character.clear(),
            3 => fixture.payload.ready.realm.clear(),
            _ => fixture.payload.ready.session_nonce = "A".repeat(32),
        }
        fixture.seal_payload();
        assert_eq!(fixture.reason(), "live.invalid_session_expectation");
    }
    let mut expected = SignalExpectation {
        character: "界".repeat(43),
        ..SignalExpectation::default()
    };
    assert_eq!(
        session_expectation(&ConnectionTarget::default(), &expected)
            .unwrap_err()
            .message,
        "live.invalid_binding_identity"
    );
    expected.character = "bad\u{7f}".into();
    assert_eq!(
        session_expectation(&ConnectionTarget::default(), &expected)
            .unwrap_err()
            .message,
        "live.invalid_binding_identity"
    );
}

#[test]
fn history_session_snapshot_exact_source_product_and_data_change_builds() {
    let mut fixture = Fixture::new();
    fixture.pin.spec.source.as_mut().unwrap().product = "classic".into();
    fixture.seal_pin();
    assert_eq!(fixture.reason(), "live.session_snapshot_mismatch");
    // Family-wide source support is valid for pin selection but not sufficient
    // for the historical session's exact Source.Product comparison.
    fixture.pin.spec.source.as_mut().unwrap().product = "forever".into();
    fixture.payload.target.client.product = "forever-asia-beta".into();
    fixture.payload.ready.product = "forever-asia-beta".into();
    fixture.seal_pin();
    fixture.seal_payload();
    assert_eq!(fixture.reason(), "live.session_snapshot_mismatch");
    let mut pin = fixture.pin.clone();
    pin.spec.source = None;
    pin.spec.data = Some(lycheedev_core::selection::DataPin {
        product: "forever-asia-beta".into(),
        full_build: "different".into(),
        ..Default::default()
    });
    assert_eq!(
        snapshot_matches(&pin, &fixture.payload.target)
            .unwrap_err()
            .message,
        "live.session_snapshot_mismatch"
    );
    pin.spec.data = None;
    pin.spec.changes = Some(lycheedev_core::selection::ChangePin {
        product: "forever-asia-beta".into(),
        full_build: "different".into(),
        ..Default::default()
    });
    assert_eq!(
        snapshot_matches(&pin, &fixture.payload.target)
            .unwrap_err()
            .message,
        "live.session_snapshot_mismatch"
    );
}

#[test]
fn history_session_provenance_compares_every_field_and_completeness() {
    for field in 0..12 {
        let mut fixture = Fixture::new();
        let reference = &mut fixture.capture.reference;
        match field {
            0 => reference.provenance.kind = "other".into(),
            1 => reference.provenance.locator = "pid:74/window:1".into(),
            2 => reference.provenance.snapshot = "PIN-other".into(),
            3 => reference.provenance.operation_id = "OP-other".into(),
            4 => reference.provenance.source_commit = "unexpected".into(),
            5 => reference.provenance.base_snapshot = "unexpected".into(),
            6 => reference.provenance.base_source_commit = "unexpected".into(),
            7 => reference.provenance.data_build = "different".into(),
            8 => reference.provenance.session = "b".repeat(32),
            9 => reference.media_type = "text/plain".into(),
            10 => reference.complete = false,
            _ => {
                reference.complete = false;
                reference.truncated = true;
            }
        }
        fixture.seal_capture();
        assert_eq!(
            fixture.reason(),
            "live.session_evidence_provenance",
            "field {field}"
        );
    }
}

#[test]
fn history_session_store_errors_and_capture_budget_remain_unmodified() {
    for point in ["document", "pin", "capture"] {
        let mut fixture = Fixture::new();
        fixture.fail_at = Some(point);
        let error = fixture.read().unwrap_err();
        assert_eq!(error.code, "fixture.read_failed");
        assert_eq!(error.message, format!("fixture.{point}"));
    }
    let mut fixture = Fixture::new();
    fixture.capture.bytes = vec![b' '; SESSION_CAPTURE_LIMIT + 1];
    fixture.seal_capture();
    assert_eq!(fixture.reason(), "vault.blob_limit");
    let mut fixture = Fixture::new();
    fixture.capture.bytes[0] = b'[';
    assert_eq!(fixture.reason(), "fixture.capture_integrity");
}

#[test]
fn history_session_original_cancellation_is_checked_at_every_store_boundary() {
    for point in ["before", "document", "pin", "capture"] {
        let mut fixture = Fixture::new();
        let cancel = lycheedev_core::control::TrackedCancellation::new();
        let control = OperationControl::tracked(
            std::time::Instant::now() + std::time::Duration::from_secs(30),
            cancel.clone(),
        );
        fixture.cancel_at = Some(point);
        fixture.cancellation = Some(cancel.clone());
        if point == "before" {
            cancel.cancel();
        }
        let error = read_session(&fixture, &fixture.record.id, &control).unwrap_err();
        assert_eq!(error.code, control.check().unwrap_err().code);
        if point == "before" {
            assert!(fixture.calls.borrow().is_empty());
        }
        if point == "document" {
            assert_eq!(&*fixture.calls.borrow(), &["document"]);
        }
        if point == "pin" {
            assert_eq!(&*fixture.calls.borrow(), &["document", "pin"]);
        }
    }
}
