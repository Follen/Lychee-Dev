//! Source-only regression inventory: every test in this staged file is NOT_RUN.
//! Oracle mapping: readDisplayProof, verifyFinishedDisplay, validateCheckpoint,
//! acknowledgedReport in Go3057a966 internal/live/history_display.go; final PNG
//! cases map to the Go1.27.1 image/png DecodeConfig path, not full image Decode.
use super::super::{Captured, RawDocument};
use super::*;
use lycheedev_core::{
    evidence::{BlobRef, CaptureRef},
    selection::PinnedSet,
};
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap};

const OP: &str = "OP-history";
const BINDING: &str = "SESSION-history";
const SESSION: &str = "11111111111111111111111111111111";
const CHALLENGE: &str = "22222222222222222222222222222222";
const CODE: &[u8] = b"print(1)";
const BODY: &[u8] = b" { \"answer\" : 42 } \n";

#[derive(Default)]
struct Store {
    documents: BTreeMap<String, Vec<u8>>,
    captures: BTreeMap<String, (CaptureRef, Vec<u8>)>,
    fetched: RefCell<Vec<(String, usize)>>,
}
impl ReadStore for Store {
    fn document(&self, key: &str, control: &OperationControl) -> Result<RawDocument> {
        control.check()?;
        self.documents
            .get(key)
            .cloned()
            .map(|bytes| RawDocument {
                generation: 1,
                bytes,
            })
            .ok_or_else(|| ToolkitError::new("vault.record_missing", "missing fixture document", 3))
    }
    fn capture(&self, id: &str, maximum: usize, control: &OperationControl) -> Result<Captured> {
        control.check()?;
        self.fetched.borrow_mut().push((id.into(), maximum));
        let (reference, bytes) = self
            .captures
            .get(id)
            .ok_or_else(|| fault("fixture.capture_missing"))?;
        if bytes.len() > maximum {
            return Err(fault("fixture.capture_limit"));
        }
        Ok(Captured {
            reference: reference.clone(),
            bytes: bytes.clone(),
        })
    }
    fn pin(&self, _: &str, _: &OperationControl) -> Result<PinnedSet> {
        panic!("display history never reads pins")
    }
}
impl Store {
    fn put(&mut self, id: &str, media: &str, p: Provenance, bytes: Vec<u8>) {
        let reference = CaptureRef {
            schema: "lycheedev.capture.v1".into(),
            id: id.into(),
            blob: BlobRef {
                sha256: hex::encode(Sha256::digest(&bytes)),
                bytes: bytes.len() as u64,
            },
            media_type: media.into(),
            provenance: p,
            complete: true,
            truncated: false,
            captured_at: "2026-10-15T02:00:00Z".into(),
            dependencies: Vec::new(),
        };
        self.captures.insert(id.into(), (reference, bytes));
    }
    fn edit(&mut self, id: &str, edit: impl FnOnce(&mut Value)) {
        let (_, raw) = self.captures.get_mut(id).unwrap();
        let mut value: Value = serde_json::from_slice(raw).unwrap();
        edit(&mut value);
        *raw = serde_json::to_vec(&value).unwrap();
    }
    fn read(&self, record: &WorkRecord) -> Result<DisplayOutcome> {
        read_display_proof(self, record, BINDING, &OperationControl::default())
    }
}
fn record() -> WorkRecord {
    let mut record = WorkRecord::default();
    record.operation_id = OP.into();
    record.intent.snapshot = "PIN-history".into();
    record.intent.session = SESSION.into();
    record
}
fn proof() -> Value {
    json!({"schema":"lycheedev.display-completion.v1","operationId":OP,"binding":BINDING,
        "receipt":{"session":BINDING,"cleared":true,"messagesQueued":-3,"submissionComplete":false,
            "observedAt":"2026-10-15T02:00:00Z"},
        "checkpoint":"checkpoint","challenge":CHALLENGE,"frame":"frame","runtimeEpoch":19})
}
fn basic() -> (Store, WorkRecord) {
    let record = record();
    let mut store = Store::default();
    store
        .documents
        .insert(format!("display/{OP}"), br#"{"capture":"proof"}"#.to_vec());
    store.put(
        "proof",
        "application/json",
        display_provenance(&record, BINDING),
        serde_json::to_vec(&proof()).unwrap(),
    );
    (store, record)
}
fn checksum(raw: &[u8]) -> String {
    let (mut a, mut b) = (1u32, 0u32);
    for value in raw {
        a = (a + u32::from(*value)) % 65521;
        b = (b + a) % 65521;
    }
    format!("{:08x}", b << 16 | a)
}
fn finished() -> (Store, WorkRecord) {
    let (mut store, mut record) = basic();
    record.intent.goal = "finished".into();
    record.intent.request.0 = serde_json::to_vec(&json!({
        "schema":"lycheedev.report-intent.v1", "code": CODE,
        "expected":{"Release":"3.2.0","Kind":"reported","SessionNonce":SESSION,"RequestID":"request-history",
            "Character":"Lychee","Realm":"Realm","Product":"wow","Build":"120100","AfterSequence":3},
        "load":{"guid":"Player-1","reportScope":"character-v1"}
    })).unwrap();
    record.observation.0 = serde_json::to_vec(&json!({"schema":"lycheedev.report-observation.v1",
        "sourcePath":"retained.lua","sourceSHA256":"present","bodyId":"body","receiptId":"reported",
        "acknowledgementId":"ack"}))
    .unwrap();
    let input = report_input(&record, &OperationControl::default()).unwrap();
    let receipt = json!({"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"reported",
        "sessionNonce":SESSION,"requestId":"request-history","character":"Lychee","realm":"Realm",
        "product":"wow","build":"120100","sequence":7,"inputReady":false,
        "codeBytes":CODE.len(),"codeAdler32":checksum(CODE),"reportBytes":BODY.len(),"reportAdler32":checksum(BODY)});
    let mut ack = receipt.clone();
    ack["kind"] = json!("acknowledged");
    ack["sequence"] = json!(8);
    let checkpoint = json!({"schema":"lycheedev.signal.v1","release":"3.2.0","kind":"checkpoint",
        "sessionNonce":SESSION,"requestId":"request-history","character":"Lychee","realm":"Realm","guid":"Player-1",
        "product":"wow","build":"120100","sequence":9,"inputReady":false,"runtimeEpoch":19,
        "probeNonce":CHALLENGE,"workState":"finished","resourcesReleased":true,
        "codeSHA256":hex::encode(Sha256::digest(CODE)),"receipt":serde_json::to_string(&ack).unwrap()});
    store.put(
        "body",
        "application/json",
        provenance(
            "game-report",
            "request-history",
            &record.intent.snapshot,
            OP,
            "120100",
            SESSION,
        ),
        BODY.to_vec(),
    );
    store.put(
        "reported",
        "application/json",
        provenance(
            "game-receipt",
            "request-history",
            &record.intent.snapshot,
            OP,
            "120100",
            SESSION,
        ),
        serde_json::to_vec(&receipt).unwrap(),
    );
    store.put(
        "ack",
        "application/json",
        provenance(
            "decoded-game-acknowledgement",
            "request-history",
            &record.intent.snapshot,
            OP,
            "120100",
            SESSION,
        ),
        serde_json::to_vec(&ack).unwrap(),
    );
    store.put(
        "checkpoint",
        "application/json",
        checkpoint_provenance(&record, &input, CHALLENGE),
        serde_json::to_vec(&checkpoint).unwrap(),
    );
    store.put(
        "frame",
        "image/png",
        provenance(
            "wgc-finished-display",
            "checkpoint",
            &record.intent.snapshot,
            OP,
            "",
            SESSION,
        ),
        png_header(1, 1, 8, 6),
    );
    (store, record)
}
fn set_input(record: &mut WorkRecord, edit: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_slice(&record.intent.request.0).unwrap();
    edit(&mut value);
    record.intent.request.0 = serde_json::to_vec(&value).unwrap();
}
fn set_observation(record: &mut WorkRecord, edit: impl FnOnce(&mut Value)) {
    let mut value: Value = serde_json::from_slice(&record.observation.0).unwrap();
    edit(&mut value);
    record.observation.0 = serde_json::to_vec(&value).unwrap();
}
fn edit_nested_ack(store: &mut Store, edit: impl FnOnce(&mut Value)) {
    store.edit("checkpoint", |value| {
        let mut ack: Value = serde_json::from_str(value["receipt"].as_str().unwrap()).unwrap();
        edit(&mut ack);
        value["receipt"] = json!(serde_json::to_string(&ack).unwrap());
    });
}
fn message<T: std::fmt::Debug>(value: Result<T>) -> String {
    value.unwrap_err().message
}

#[test] // Go readDisplayProof: ErrMissingRecord returns the empty outcome.
fn history_display_missing_is_empty_without_capture_reads() {
    let store = Store::default();
    let value = serde_json::to_value(store.read(&record()).unwrap()).unwrap();
    assert_eq!(value, json!({"state":""}));
    assert!(store.fetched.borrow().is_empty());
}

#[test] // Go readDisplayProof: non-finished goals do not verify finalization.
fn history_display_nonfinished_preserves_receipt_without_inventing_requirements() {
    let (store, mut record) = basic();
    record.intent.request.0 = b"invalid report intent never read".to_vec();
    let value = serde_json::to_value(store.read(&record).unwrap()).unwrap();
    assert_eq!(value["state"], "cleared");
    assert_eq!(value["capture"], "proof");
    assert_eq!(value["receipt"]["messagesQueued"], -3);
    assert_eq!(value["receipt"]["submissionComplete"], false);
    assert_eq!(*store.fetched.borrow(), vec![("proof".into(), 4096)]);
}

#[test] // Go readDisplayProof: json.Unmarshal scalar/null/repeated semantics.
fn history_display_json_merge_null_case_and_signed_integer_match_go() {
    let (mut store, record) = basic();
    store.documents.insert(
        format!("display/{OP}"),
        br#"{"Capture":"proof","capture":null}"#.to_vec(),
    );
    store.captures.get_mut("proof").unwrap().1 = format!(r#"{{"Schema":"lycheedev.display-completion.v1","operationId":"{OP}","binding":"{BINDING}","Receipt":{{"session":"{BINDING}","cleared":true,"messagesQueued":-0}},"receipt":{{"observedAt":"2026-10-15T2:00:00,1200000Z"}},"receipt":null,"unknown":{{"execute":true}}}}"#).into_bytes();
    let value = serde_json::to_value(store.read(&record).unwrap()).unwrap();
    assert_eq!(value["receipt"]["observedAt"], "2026-10-15T02:00:00.12Z");
    assert_eq!(value["receipt"]["messagesQueued"], 0);
    for raw in ["null", "{}", r#"{"capture":1}"#, r#"{"capture":""}"#, "[]"] {
        store
            .documents
            .insert(format!("display/{OP}"), raw.as_bytes().to_vec());
        assert_eq!(
            message(store.read(&record)),
            "live.invalid_display_completion",
            "{raw}"
        );
    }
}

#[test] // Go readDisplayProof: identity, cleared and time.Time.IsZero checks.
fn history_display_receipt_checks_go_zero_instant_and_bad_types() {
    for (field, value) in [
        ("schema", json!("wrong")),
        ("operationId", json!("other")),
        ("binding", json!("other")),
    ] {
        let (mut store, record) = basic();
        store.edit("proof", |proof| proof[field] = value);
        assert_eq!(
            message(store.read(&record)),
            "live.invalid_display_completion"
        );
    }
    for (field, value) in [
        ("session", json!("other")),
        ("cleared", json!(false)),
        ("messagesQueued", json!(1.0)),
        ("observedAt", json!("0001-01-01T00:00:00Z")),
        ("observedAt", json!("0001-01-01T01:00:00+01:00")),
        ("observedAt", json!("0001-01-02T00:00:00+24:00")),
        ("observedAt", Value::Null),
    ] {
        let (mut store, record) = basic();
        store.edit("proof", |proof| proof["receipt"][field] = value);
        assert_eq!(
            message(store.read(&record)),
            "live.invalid_display_completion",
            "{field}"
        );
    }
    let (mut store, record) = basic();
    store.edit("proof", |proof| {
        proof["receipt"]["observedAt"] = json!("0001-01-01T00:00:00.000000001Z")
    });
    assert!(store.read(&record).is_ok());
}

fn poison_provenance(p: &mut Provenance, index: usize) {
    match index {
        0 => p.kind.push('x'),
        1 => p.locator.push('x'),
        2 => p.snapshot.push('x'),
        3 => p.operation_id.push('x'),
        4 => p.source_commit.push('x'),
        5 => p.base_snapshot.push('x'),
        6 => p.base_source_commit.push('x'),
        7 => p.data_build.push('x'),
        8 => p.session.push('x'),
        _ => unreachable!(),
    }
}

#[test] // All three Go display provenance structs are compared in full.
fn history_display_provenance_all_fields_and_capture_flags_are_exact() {
    for (id, category) in [
        ("proof", "live.invalid_display_completion"),
        ("checkpoint", "live.finalization_provenance_mismatch"),
        ("ack", "live.acknowledgement_provenance_mismatch"),
        ("frame", "live.finalization_frame_mismatch"),
    ] {
        for index in 0..12 {
            let (mut store, record) = finished();
            let reference = &mut store.captures.get_mut(id).unwrap().0;
            match index {
                0..=8 => poison_provenance(&mut reference.provenance, index),
                9 => reference.complete = false,
                10 => reference.truncated = true,
                11 => reference.media_type = "application/octet-stream".into(),
                _ => unreachable!(),
            }
            assert_eq!(message(store.read(&record)), category, "{id} field {index}");
        }
    }
}

#[test] // Go verifyFinishedDisplay / acknowledgedReport / ReadVerifiedReport.
fn history_finished_uses_original_body_and_exact_capture_bounds_in_order() {
    let (store, record) = finished();
    assert_eq!(store.read(&record).unwrap().state, "cleared");
    assert_eq!(
        *store.fetched.borrow(),
        vec![
            ("proof".into(), 4096),
            ("checkpoint".into(), 4096),
            ("body".into(), 512 << 10),
            ("reported".into(), 4096),
            ("ack".into(), 4096),
            ("frame".into(), 32 << 20)
        ]
    );
    let control = OperationControl::default();
    let input = report_input(&record, &control).unwrap();
    let observed = decode_record(&record.observation.0, &control).unwrap();
    let report = acknowledged_report(&store, &record, &input, &observed, &control).unwrap();
    assert_eq!(report.body, BODY);
    assert_eq!(report.sha256, hex::encode(Sha256::digest(BODY)));
}

#[test] // Go validateCheckpoint: Match first, then Load.GUID and raw code SHA.
fn history_finished_checkpoint_load_guid_code_and_epoch_are_bound() {
    for value in [Value::Null, json!({"guid":"Player-other"})] {
        let (store, mut record) = finished();
        set_input(&mut record, |input| input["load"] = value);
        assert_eq!(
            message(store.read(&record)),
            "live.checkpoint_identity_mismatch"
        );
    }
    let (mut store, record) = finished();
    store.edit("checkpoint", |value| {
        value["codeSHA256"] = json!("a".repeat(64))
    });
    assert_eq!(
        message(store.read(&record)),
        "live.checkpoint_identity_mismatch"
    );
    for (field, value) in [
        ("runtimeEpoch", json!(20)),
        ("sessionNonce", json!("3".repeat(32))),
        ("sequence", json!(0)),
    ] {
        let (mut store, record) = finished();
        store.edit("checkpoint", |checkpoint| checkpoint[field] = value);
        assert!(store.read(&record).is_err(), "{field}");
    }
}

#[test] // Go verifyFinishedDisplay: finalization facts precede report reads.
fn history_finished_requires_challenge_epoch_finished_state_and_release() {
    for (id, field, value) in [
        ("proof", "runtimeEpoch", json!(0)),
        ("proof", "challenge", json!("short")),
        ("checkpoint", "resourcesReleased", json!(false)),
        ("checkpoint", "workState", json!("acknowledged")),
    ] {
        let (mut store, record) = finished();
        store.edit(id, |v| v[field] = value);
        if field == "challenge" {
            store
                .captures
                .get_mut("checkpoint")
                .unwrap()
                .0
                .provenance
                .locator = "short".into();
        }
        assert_eq!(
            message(store.read(&record)),
            "live.finalization_incomplete",
            "{field}"
        );
        assert!(!store.fetched.borrow().iter().any(|(id, _)| id == "body"));
    }
}

#[test] // Go acknowledgedReport checks source evidence, then full report pair.
fn history_finished_requires_observation_and_authenticated_raw_report() {
    for field in ["schema", "sourcePath", "sourceSHA256", "acknowledgementId"] {
        let (store, mut record) = finished();
        set_observation(&mut record, |v| v[field] = json!(""));
        assert_eq!(
            message(store.read(&record)),
            "live.removal_evidence_missing",
            "{field}"
        );
    }
    for id in ["body", "reported"] {
        let (mut store, record) = finished();
        store
            .captures
            .get_mut(id)
            .unwrap()
            .0
            .provenance
            .source_commit = "extra".into();
        assert_eq!(
            message(store.read(&record)),
            "evidence.report_provenance_mismatch"
        );
    }
    let (mut store, record) = finished();
    store.captures.get_mut("body").unwrap().1 = br#"{"answer":42}"#.to_vec();
    assert_eq!(message(store.read(&record)), "bridge.report_body_mismatch");
}

#[test] // Both Go ACK joins bind all four payload fields independently.
fn history_finished_both_acknowledgements_join_all_report_digests() {
    for nested in [false, true] {
        for (field, value) in [
            ("codeBytes", json!(9)),
            ("codeAdler32", json!("00000000")),
            ("reportBytes", json!(17)),
            ("reportAdler32", json!("00000000")),
        ] {
            let (mut store, record) = finished();
            if nested {
                edit_nested_ack(&mut store, |v| v[field] = value);
            } else {
                store.edit("ack", |v| v[field] = value);
            }
            let category = if nested {
                "live.finalization_report_mismatch"
            } else {
                "live.acknowledgement_payload_mismatch"
            };
            assert_eq!(
                message(store.read(&record)),
                category,
                "nested {nested}: {field}"
            );
        }
        let (mut store, record) = finished();
        if nested {
            edit_nested_ack(&mut store, |v| v["sequence"] = json!(7));
        } else {
            store.edit("ack", |v| v["sequence"] = json!(7));
        }
        assert_eq!(
            message(store.read(&record)),
            "bridge.signal_identity_mismatch"
        );
    }
}

#[test] // FillSignalIdentity applies only to checkpoint ACK; readiness differs.
fn history_finished_compact_nested_identity_and_ack_readiness_match_go() {
    let (mut store, record) = finished();
    edit_nested_ack(&mut store, |v| {
        for field in ["sessionNonce", "character", "realm"] {
            v.as_object_mut().unwrap().remove(field);
        }
    });
    assert!(store.read(&record).is_ok());
    store.edit("ack", |v| {
        v.as_object_mut().unwrap().remove("sessionNonce");
    });
    assert_eq!(
        message(store.read(&record)),
        "bridge.signal_identity_mismatch"
    );
    let (mut store, mut record) = finished();
    set_input(&mut record, |v| {
        v["expected"]["RequireInputReady"] = json!(true)
    });
    // Direct acknowledgedReport clears this expected flag; finalization's
    // checkpoint Match and nested ACK preserve it. Verify the direct contract.
    store.edit("reported", |v| v["inputReady"] = json!(true));
    let control = OperationControl::default();
    let input = report_input(&record, &control).unwrap();
    let observed = decode_record(&record.observation.0, &control).unwrap();
    assert!(acknowledged_report(&store, &record, &input, &observed, &control).is_ok());
    assert_eq!(message(store.read(&record)), "bridge.input_not_ready");
}

#[test] // Go FetchCapture frame limit is 32 MiB; frame mismatch precedes PNG.
fn history_finished_frame_limit_and_png_errors_stay_visible() {
    let (mut store, record) = finished();
    store.captures.get_mut("frame").unwrap().1 = vec![0; FRAME_LIMIT + 1];
    assert_eq!(message(store.read(&record)), "fixture.capture_limit");
    store.captures.get_mut("frame").unwrap().1 = b"not png!".to_vec();
    assert_eq!(
        message(store.read(&record)),
        "png: invalid format: not a PNG file"
    );
    store.captures.get_mut("frame").unwrap().1 = vec![0; 7];
    assert_eq!(message(store.read(&record)), "unexpected EOF");
}

fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut raw = (data.len() as u32).to_be_bytes().to_vec();
    raw.extend_from_slice(kind);
    raw.extend_from_slice(data);
    raw.extend_from_slice(
        &png_crc(kind, data, &OperationControl::default())
            .unwrap()
            .to_be_bytes(),
    );
    raw
}
fn png_header(width: u32, height: u32, depth: u8, color: u8) -> Vec<u8> {
    let mut data = width.to_be_bytes().to_vec();
    data.extend_from_slice(&height.to_be_bytes());
    data.extend_from_slice(&[depth, color, 0, 0, 0]);
    let mut raw = b"\x89PNG\r\n\x1a\n".to_vec();
    raw.extend_from_slice(&chunk(b"IHDR", &data));
    raw
}
fn config(raw: &[u8]) -> Result<()> {
    png_decode_config(raw, &OperationControl::default())
}

#[test] // Go png.DecodeConfig returns after verified non-palette IHDR.
fn history_png_config_accepts_header_only_and_ignores_trailing_pixels() {
    for (depth, color) in [
        (1, 0),
        (2, 0),
        (4, 0),
        (8, 0),
        (8, 2),
        (8, 4),
        (8, 6),
        (16, 0),
        (16, 2),
        (16, 4),
        (16, 6),
    ] {
        let mut raw = png_header(1, 1, depth, color);
        assert!(config(&raw).is_ok(), "{depth}/{color}");
        raw.extend_from_slice(b"invalid later chunk and no pixels");
        assert!(config(&raw).is_ok());
    }
    // CRC oracle is independently known from the PNG format, not calculated by
    // the helper: IEND's empty payload has CRC ae426082.
    assert_eq!(
        png_crc(b"IEND", b"", &OperationControl::default()).unwrap(),
        0xae42_6082
    );
}

#[test] // Go parseIHDR validates methods, dimensions, depth and CRC in order.
fn history_png_config_rejects_invalid_header_without_pixel_allocation() {
    for (width, height, depth, color, category) in [
        (0, 1, 8, 6, "png: invalid format: non-positive dimension"),
        (
            0x8000_0000,
            1,
            8,
            6,
            "png: invalid format: non-positive dimension",
        ),
        (
            0x7fff_ffff,
            0x7fff_ffff,
            8,
            6,
            "png: unsupported feature: dimension overflow",
        ),
        (
            1,
            1,
            16,
            3,
            "png: unsupported feature: bit depth 16, color type 3",
        ),
        (
            1,
            1,
            1,
            2,
            "png: unsupported feature: bit depth 1, color type 2",
        ),
    ] {
        assert_eq!(
            message(config(&png_header(width, height, depth, color))),
            category
        );
    }
    for (index, value, category) in [
        (26, 1, "png: unsupported feature: compression method"),
        (27, 1, "png: unsupported feature: filter method"),
        (28, 2, "png: invalid format: invalid interlace method"),
    ] {
        let mut raw = png_header(1, 1, 8, 6);
        raw[index] = value;
        assert_eq!(message(config(&raw)), category);
    }
    let mut raw = png_header(1, 1, 8, 6);
    raw[32] ^= 1;
    assert_eq!(
        message(config(&raw)),
        "png: invalid format: invalid checksum"
    );
    assert!(config(&png_header(0x7fff_ffff, 1, 8, 6)).is_ok());
}

#[test] // Go paletted config requires PLTE then tRNS or an IDAT header.
fn history_png_palette_config_does_not_require_idat_payload_or_crc() {
    let mut raw = png_header(1, 1, 1, 3);
    assert_eq!(message(config(&raw)), "unexpected EOF");
    raw.extend_from_slice(&chunk(b"PLTE", &[0, 0, 0]));
    assert_eq!(message(config(&raw)), "unexpected EOF");
    // Even a huge claimed IDAT length is accepted before data/CRC are read.
    raw.extend_from_slice(&u32::MAX.to_be_bytes());
    raw.extend_from_slice(b"IDAT");
    assert!(config(&raw).is_ok());
    for transparency in [vec![], vec![255], vec![255; 256]] {
        let mut raw = png_header(1, 1, 1, 3);
        raw.extend_from_slice(&chunk(b"PLTE", &[0, 0, 0]));
        raw.extend_from_slice(&chunk(b"tRNS", &transparency));
        assert!(config(&raw).is_ok());
    }
}

#[test] // Go palette ordering, PLTE/tRNS bounds, unknown-chunk checksums.
fn history_png_palette_order_bounds_and_unknown_chunks_match_go() {
    for (kind, data, category) in [
        (b"PLTE", vec![0; 9], "png: invalid format: bad PLTE length"),
        (b"PLTE", vec![], "png: invalid format: bad PLTE length"),
        (b"tRNS", vec![], "png: invalid format: chunk out of order"),
        (b"IDAT", vec![], "png: invalid format: chunk out of order"),
        (b"IEND", vec![], "png: invalid format: chunk out of order"),
    ] {
        let mut raw = png_header(1, 1, 1, 3);
        raw.extend_from_slice(&chunk(kind, &data));
        assert_eq!(message(config(&raw)), category);
    }
    let mut raw = png_header(1, 1, 8, 3);
    raw.extend_from_slice(&chunk(b"PLTE", &[0, 0, 0]));
    raw.extend_from_slice(&chunk(b"tRNS", &vec![0; 257]));
    assert_eq!(
        message(config(&raw)),
        "png: invalid format: bad tRNS length"
    );
    let mut raw = b"\x89PNG\r\n\x1a\n".to_vec();
    raw.extend_from_slice(&chunk(b"UNKN", b"ignored before IHDR"));
    raw.extend_from_slice(&png_header(1, 1, 8, 6)[8..]);
    assert!(config(&raw).is_ok());
    raw[16] ^= 1;
    assert_eq!(
        message(config(&raw)),
        "png: invalid format: invalid checksum"
    );
    let mut raw = b"\x89PNG\r\n\x1a\n".to_vec();
    raw.extend_from_slice(&0x8000_0000u32.to_be_bytes());
    raw.extend_from_slice(b"UNKN");
    assert_eq!(
        message(config(&raw)),
        "png: invalid format: Bad chunk length: 2147483648"
    );
}

#[test] // Original cooperative control is retained through proof and PNG work.
fn history_display_cancelled_control_is_never_display_corruption() {
    use lycheedev_core::control::TrackedCancellation;
    let cancellation = TrackedCancellation::new();
    cancellation.cancel();
    let control = OperationControl::tracked(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        cancellation,
    );
    let expected = control.check().unwrap_err();
    let (store, record) = finished();
    let error = read_display_proof(&store, &record, BINDING, &control).unwrap_err();
    assert_eq!(error.code, expected.code);
    assert_eq!(error.message, expected.message);
    assert_eq!(
        png_decode_config(&png_header(1, 1, 8, 6), &control)
            .unwrap_err()
            .code,
        expected.code
    );
    assert_eq!(
        decode_display::<DisplayCompletion>(b"broken", &control)
            .unwrap_err()
            .code,
        expected.code
    );
    assert!(store.fetched.borrow().is_empty());
}
