//! Staged tests: NOT_RUN. Fixed oracle 3057a9666ab078270b63c7f06de748069b0d1f4b.
//! Exact Go test mapping and deliberately unrun status are in signal/freeze.json.
use super::*;
use serde_json::json;
use std::{io::Write, time::Instant};

fn encoded(s: &Signal) -> Vec<u8> {
    serde_json::to_vec(s).unwrap()
}
fn parse(s: &Signal) -> Result<Signal> {
    parse_signal(&encoded(s), &OperationControl::default())
}
fn session(kind: &str) -> Signal {
    Signal {
        schema: "lycheedev.signal.v1".into(),
        release: "2.0.0".into(),
        kind: kind.into(),
        session_nonce: "session".into(),
        request_id: "OP-test".into(),
        character: "character".into(),
        realm: "realm".into(),
        product: "retail".into(),
        build: "12.1.0.69875".into(),
        sequence: 3,
        ..Signal::default()
    }
}
fn expectation(s: &Signal) -> SignalExpectation {
    SignalExpectation {
        release: s.release.clone(),
        kind: s.kind.clone(),
        session_nonce: s.session_nonce.clone(),
        request_id: s.request_id.clone(),
        character: s.character.clone(),
        realm: s.realm.clone(),
        product: s.product.clone(),
        build: s.build.clone(),
        after_sequence: s.sequence.saturating_sub(1),
        ..SignalExpectation::default()
    }
}
fn identity() -> Signal {
    Signal {
        schema: "lycheedev.signal.v1".into(),
        release: "2.0.0".into(),
        kind: "identity".into(),
        probe_nonce: "0123456789abcdef".repeat(2),
        actor_state: "ok".into(),
        character: "Paladin".into(),
        realm: "Realm".into(),
        guid: "Player-1-123".into(),
        product: "retail".into(),
        build: "12.1.0.69875".into(),
        input_ready: true,
        ..Signal::default()
    }
}
fn cleared() -> Signal {
    Signal {
        cleanup_nonce: "b".repeat(32),
        guid: "Player-1-123".into(),
        ..session("cleared")
    }
}
fn report_fixture(body: &[u8], code: &[u8]) -> (Vec<u8>, SignalExpectation) {
    let control = OperationControl::default();
    let mut s = session("reported");
    s.report_bytes = body.len() as u32;
    s.report_adler32 = byte_checksum(body, &control).unwrap();
    s.code_bytes = code.len() as u32;
    if !code.is_empty() {
        s.code_adler32 = byte_checksum(code, &control).unwrap();
    }
    (encoded(&s), expectation(&s))
}
fn assert_code<T: std::fmt::Debug>(result: Result<T>, code: &str) {
    assert_eq!(result.unwrap_err().code, code);
}
fn raw_deflate(data: &[u8]) -> Vec<u8> {
    let mut writer = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    writer.write_all(data).unwrap();
    writer.finish().unwrap()
}

// Go: TestSignalsMatchExactIdentityAndFreshSequence.
#[test]
fn signals_match_exact_identity_and_fresh_sequence() {
    let mut s = session("loaded");
    s.session_nonce = "new-session".into();
    s.request_id = "request-1".into();
    s.sequence = 9;
    s.input_ready = true;
    s.code_bytes = 8;
    s.code_adler32 = "12345678".into();
    let mut parsed = parse(&s).unwrap();
    let mut expected = expectation(&s);
    expected.require_input_ready = true;
    parsed.matches(&expected).unwrap();
    expected.after_sequence = 9;
    assert!(parsed.matches(&expected).is_err());
    expected.after_sequence = 8;
    expected.session_nonce = "old-session".into();
    assert!(parsed.matches(&expected).is_err());
    expected.session_nonce = s.session_nonce;
    parsed.input_ready = false;
    assert_code(parsed.matches(&expected), "bridge.input_not_ready");
}
// Go: TestSignalSequenceMustFitLuaIntegerRange.
#[test]
fn sequence_must_fit_lua_integer_range() {
    let mut s = session("ready");
    s.sequence = LUA_INTEGER_MAX;
    parse(&s).unwrap();
    s.sequence += 1;
    assert!(parse(&s).is_err());
    assert!(s.matches(&SignalExpectation::default()).is_err());
}
// Go: TestIdentitySignalParsesOnlyAsAnIdentityMarker.
#[test]
fn identity_only_marker_variants() {
    for change in [
        (|_: &mut Signal| {}) as fn(&mut Signal),
        |s| {
            s.actor_state = "no_actor".into();
            s.character.clear();
            s.realm.clear();
            s.guid.clear();
            s.input_ready = false;
            s.input_reason = "input_not_logged_in".into();
        },
        |s| {
            s.actor_state = "actor_restricted".into();
            s.character.clear();
            s.realm.clear();
            s.guid.clear();
        },
        |s| {
            s.runtime_epoch = 7;
        },
        |s| {
            s.input_ready = false;
            s.input_reason = "input_keyboard_focus".into();
        },
    ] {
        let mut s = identity();
        change(&mut s);
        let parsed = parse(&s).unwrap();
        assert_eq!(encoded(&s), encoded(&parsed));
        parsed
            .matches(&SignalExpectation {
                kind: "identity".into(),
                release: s.release,
                probe_nonce: s.probe_nonce,
                actor_state: s.actor_state,
                product: s.product,
                build: s.build,
                ..SignalExpectation::default()
            })
            .unwrap();
    }
}
// Go: TestIdentitySignalRejectsProtocolCarryOver.
#[test]
fn identity_rejects_protocol_carryover() {
    for change in [
        (|s: &mut Signal| s.session_nonce = "a".repeat(32)) as fn(&mut Signal),
        |s| s.request_id = "REQ-one".into(),
        |s| s.reload_nonce = "b".repeat(32),
        |s| s.cleanup_nonce = "c".repeat(32),
        |s| s.sequence = 1,
        |s| {
            s.code_bytes = 4;
            s.code_adler32 = "00000001".into();
        },
        |s| {
            s.report_bytes = 4;
            s.report_adler32 = "00000001".into();
        },
        |s| s.code_adler32 = "00000001".into(),
        |s| {
            s.kind = "ready".into();
            s.session_nonce = "a".repeat(32);
            s.sequence = 1;
        },
        |s| {
            s.kind = "ready".into();
            s.session_nonce = "a".repeat(32);
            s.sequence = 1;
            s.probe_nonce.clear();
        },
        |s| {
            s.kind = "ready".into();
            s.session_nonce = "a".repeat(32);
            s.sequence = 1;
            s.probe_nonce.clear();
            s.actor_state.clear();
            s.input_reason = "input_keyboard_focus".into();
        },
    ] {
        let mut s = identity();
        change(&mut s);
        assert!(parse(&s).is_err());
    }
}
// Go: TestIdentitySignalProbeNonceAndActorStateAreExact.
#[test]
fn identity_nonce_actor_and_reason_exact() {
    for change in [
        (|s: &mut Signal| s.probe_nonce.clear()) as fn(&mut Signal),
        |s| {
            s.probe_nonce.truncate(31);
        },
        |s| s.probe_nonce.push('0'),
        |s| s.probe_nonce = s.probe_nonce.to_uppercase(),
        |s| s.probe_nonce = "z".repeat(32),
        |s| s.actor_state.clear(),
        |s| s.actor_state = "loading".into(),
        |s| s.character.clear(),
        |s| s.realm.clear(),
        |s| s.actor_state = "no_actor".into(),
        |s| {
            s.actor_state = "no_actor".into();
            s.character.clear();
            s.realm.clear();
        },
        |s| s.input_reason = "input_keyboard_focus".into(),
        |s| {
            s.input_ready = false;
            s.input_reason = "KeyboardFocus".into();
        },
    ] {
        let mut s = identity();
        change(&mut s);
        assert!(parse(&s).is_err());
    }
}
// Go: TestIdentityExpectationsNeverCrossKinds.
#[test]
fn identity_expectations_never_cross_kinds() {
    let identity = parse(&identity()).unwrap();
    let ready = parse(&session("ready")).unwrap();
    let good = SignalExpectation {
        kind: "identity".into(),
        probe_nonce: identity.probe_nonce.clone(),
        ..SignalExpectation::default()
    };
    identity.matches(&good).unwrap();
    ready.matches(&expectation(&ready)).unwrap();
    assert!(ready.matches(&good).is_err());
    assert!(identity.matches(&expectation(&ready)).is_err());
    for change in [
        (|e: &mut SignalExpectation| e.kind.clear()) as fn(&mut SignalExpectation),
        |e| e.probe_nonce = "f".repeat(32),
        |e| e.probe_nonce.clear(),
        |e| e.actor_state = "no_actor".into(),
        |e| e.build = "12.1.0.69876".into(),
        |e| e.after_sequence = 1,
    ] {
        let mut e = good.clone();
        change(&mut e);
        assert!(identity.matches(&e).is_err());
    }
    let mut unready = identity;
    unready.input_ready = false;
    unready.input_reason = "input_combat_lockdown".into();
    let parsed = parse(&unready).unwrap();
    parsed.matches(&good).unwrap();
    assert_code(
        parsed.matches(&SignalExpectation {
            require_input_ready: true,
            ..good
        }),
        "bridge.input_not_ready",
    );
}
// Go: TestParseSignalInflatesDeflatedPayload; extension adds 0x1E and bounds.
#[test]
fn archived_transports_are_bounded_and_equivalent() {
    use base64::Engine as _;
    let (plain, _) = report_fixture(br#"{"value":1}"#, b"");
    let compressed = raw_deflate(&plain);
    let raw = [vec![0x1f], compressed.clone()].concat();
    let encoded_transport = [
        vec![0x1e],
        base64::engine::general_purpose::STANDARD
            .encode(&compressed)
            .into_bytes(),
    ]
    .concat();
    for data in [&plain, &raw, &encoded_transport] {
        let s = parse_signal(data, &OperationControl::default()).unwrap();
        assert_eq!(s.kind, "reported");
    }
    let damaged = [vec![0x1f, 0x00], raw[2..12].to_vec()].concat();
    assert!(parse_signal(&damaged, &OperationControl::default()).is_err());
    for data in [
        vec![],
        vec![0x1f],
        vec![0x1e],
        vec![0x1e, b'!'],
        vec![0xff],
        vec![b' '; 4097],
        [vec![0x1f], raw_deflate(&vec![b' '; 4097])].concat(),
        [vec![0x1f], compressed[..compressed.len() - 1].to_vec()].concat(),
    ] {
        assert_code(
            parse_signal(&data, &OperationControl::default()),
            "bridge.signal_budget_or_encoding",
        );
    }
    // Exactly 4096 inflated bytes are allowed. Trailing compressed garbage is
    // ignored after the first stream, matching Go's flate reader.
    let mut padded = plain.clone();
    padded.resize(4096, b' ');
    let mut payload = [vec![0x1f], raw_deflate(&padded)].concat();
    payload.extend_from_slice(b"trailing");
    parse_signal(&payload, &OperationControl::default()).unwrap();
    let mut crlf = vec![0x1e];
    for pair in encoded_transport[1..].chunks(2) {
        crlf.extend_from_slice(pair);
        crlf.extend_from_slice(b"\r\n");
    }
    parse_signal(&crlf, &OperationControl::default()).unwrap();
    assert_eq!(decode_base64(b"Zg==\r\n").unwrap(), b"f");
    assert_eq!(decode_base64(b"Zh==").unwrap(), b"f");
    for bad in [b"Zg".as_slice(), b"Zg=", b"Zg===", b"Zg== ", b"Zg==Zg=="] {
        assert!(decode_base64(bad).is_err());
    }
}
// Go: TestReportPreservesExactBytesAndOwnsCopy.
#[test]
fn report_preserves_exact_bytes_and_owns_copy() {
    let mut body = "{\r\n\"text\":\"世界\",\"value\":1.0}\n"
        .as_bytes()
        .to_vec();
    let (mut raw, expected) = report_fixture(&body, b"return 1");
    let report = verify_report(
        &raw,
        &body,
        b"return 1",
        &expected,
        &OperationControl::default(),
    )
    .unwrap();
    assert_eq!(report.body, body);
    assert_eq!(report.sha256, hex::encode(Sha256::digest(&body)));
    body[0] = b'!';
    raw[0] = b'!';
    assert_eq!(report.receipt_bytes[0], b'{');
    assert_eq!(report.body[0], b'{');
}
// Go: TestReportRejectsMismatchedTransport.
#[test]
fn report_rejects_mismatched_transport() {
    let body = br#"{"value":1}"#;
    let code = b"return 1";
    let (raw, expected) = report_fixture(body, code);
    let c = OperationControl::default();
    assert!(verify_report(&raw, body, b"return 2", &expected, &c).is_err());
    assert!(verify_report(&raw, br#"{"value":2}"#, code, &expected, &c).is_err());
    assert!(verify_report(&raw, body, &vec![b'x'; CODE_LIMIT + 1], &expected, &c).is_err());
    for change in [
        (|e: &mut SignalExpectation| e.session_nonce = "foreign".into())
            as fn(&mut SignalExpectation),
        |e| e.request_id = "other".into(),
        |e| e.build = "12.1.0.99999".into(),
        |e| e.after_sequence = 3,
        |e| e.character.clear(),
    ] {
        let mut e = expected.clone();
        change(&mut e);
        assert!(verify_report(&raw, body, code, &e, &c).is_err());
    }
}
// Go: TestReportRejectsAmbiguousOrUnboundedJSON.
#[test]
fn report_rejects_ambiguous_or_unbounded_json() {
    let bodies = vec![
        br#"{"a":1,"a":2}"#.to_vec(),
        br#"{"a":1,"\u0061":2}"#.to_vec(),
        br#"{"nested":{"a":1,"a":2}}"#.to_vec(),
        b"{} {}".to_vec(),
        b"{\"a\":".to_vec(),
        format!("{}0{}", "[".repeat(34), "]".repeat(34)).into_bytes(),
        format!("[{}0]", "0,".repeat(32768)).into_bytes(),
        vec![b'"', 0xff, b'"'],
    ];
    for body in bodies {
        let (raw, e) = report_fixture(&body, b"");
        assert!(verify_report(&raw, &body, b"", &e, &OperationControl::default()).is_err());
    }
}
// Go: TestBuiltinReportAndMaximumPayload.
#[test]
fn builtin_report_and_maximum_payload() {
    let mut body = format!("{{\"text\":\"{}\"}}", "x".repeat(REPORT_LIMIT - 11)).into_bytes();
    assert_eq!(body.len(), REPORT_LIMIT);
    let (raw, e) = report_fixture(&body, b"");
    verify_report(&raw, &body, b"", &e, &OperationControl::default()).unwrap();
    body.push(b' ');
    let (raw, e) = report_fixture(&body, b"");
    assert!(verify_report(&raw, &body, b"", &e, &OperationControl::default()).is_err());
}
// Go: TestSignalRejectsDuplicateIdentity.
#[test]
fn signal_rejects_duplicate_identity() {
    let (raw, _) = report_fixture(b"{}", b"");
    let duplicate = [br#"{"sessionNonce":"foreign","#.to_vec(), raw[1..].to_vec()].concat();
    assert_code(
        parse_signal(&duplicate, &OperationControl::default()),
        "bridge.json_duplicate_or_invalid_key",
    );
}
// Go: FuzzReportTransport seed cases only; a Rust fuzz campaign is NOT_RUN.
#[test]
fn report_transport_fuzz_seed_invariants() {
    for (body, code) in [
        (br#"{"value":1}"#.as_slice(), b"return 1".as_slice()),
        (br#"{"value":1,"value":2}"#, b""),
        (&[b'"', 0xff, b'"'], b"return nil"),
    ] {
        let (raw, mut e) = report_fixture(body, code);
        let c = OperationControl::default();
        if let Ok(report) = verify_report(&raw, body, code, &e, &c) {
            assert_eq!(report.body, body);
            e.request_id.push_str("-other");
            assert!(verify_report(&raw, body, code, &e, &c).is_err());
        }
    }
}
// Go: TestClearedSignalRejectsIncompleteOrMixedEvidence.
#[test]
fn cleared_rejects_incomplete_or_mixed_evidence() {
    parse(&cleared()).unwrap();
    for change in [
        (|s: &mut Signal| s.cleanup_nonce.clear()) as fn(&mut Signal),
        |s| s.cleanup_nonce = "B".repeat(32),
        |s| s.cleanup_nonce = "abc".into(),
        |s| s.guid.clear(),
        |s| s.request_id.clear(),
        |s| s.reload_nonce = "c".repeat(32),
        |s| s.input_ready = true,
        |s| {
            s.code_bytes = 1;
            s.code_adler32 = "00000001".into();
        },
        |s| {
            s.report_bytes = 1;
            s.report_adler32 = "00000001".into();
        },
        |s| s.code_adler32 = "00000001".into(),
        |s| s.kind = "ready".into(),
    ] {
        let mut s = cleared();
        change(&mut s);
        assert!(parse(&s).is_err());
    }
}
// Go: three runtime-epoch tests from cleanup_test.go.
#[test]
fn cleared_runtime_epoch_boundaries_and_matching() {
    let mut s = cleared();
    s.runtime_epoch = LUA_INTEGER_MAX;
    parse(&s).unwrap();
    s.runtime_epoch += 1;
    assert!(parse(&s).is_err());
    let mut loaded = session("loaded");
    loaded.code_bytes = 1;
    loaded.code_adler32 = "00000001".into();
    loaded.runtime_epoch = 1;
    assert!(parse(&loaded).is_err());
    s.runtime_epoch = 42;
    let parsed = parse(&s).unwrap();
    let mut e = expectation(&s);
    e.cleanup_nonce = s.cleanup_nonce;
    e.runtime_epoch = 42;
    parsed.matches(&e).unwrap();
    e.runtime_epoch += 1;
    assert!(parsed.matches(&e).is_err());
}
// Go: TestReceiverBindingsFiniteSetAndFocusedClose.
#[test]
fn receiver_bindings_finite_set_and_focused_close() {
    for (w, s, c, valid) in [
        ("ALT-CTRL-]", "ALT-CTRL-SHIFT-]", "ALT-CTRL-[", true),
        ("ALT-CTRL-F12", "ALT-CTRL-SHIFT-F12", "ALT-CTRL-[", true),
        ("ALT-CTRL-F1", "ALT-CTRL-SHIFT-F2", "ALT-CTRL-F3", true),
        ("ALT-CTRL-A", "ALT-CTRL-SHIFT-]", "ALT-CTRL-[", false),
        ("CTRL-ALT-F1", "ALT-CTRL-SHIFT-]", "ALT-CTRL-[", false),
        ("ESCAPE", "ALT-CTRL-SHIFT-]", "ALT-CTRL-[", false),
        ("ALT-CTRL-]", "ALT-CTRL-]", "ALT-CTRL-[", false),
        ("ALT-CTRL-]", "ALT-CTRL-SHIFT-]", "ALT-CTRL-SHIFT-]", false),
        ("ALT-CTRL-[", "ALT-CTRL-SHIFT-]", "ALT-CTRL-SHIFT-[", false),
    ] {
        assert_eq!(validation::receiver_bindings(w, s, c).is_ok(), valid);
    }
    for key in 1..=12 {
        validation::receiver_bindings(
            &format!("ALT-CTRL-F{key}"),
            &format!("ALT-CTRL-SHIFT-F{key}"),
            "ALT-CTRL-[",
        )
        .unwrap();
    }
    for key in ["F0", "F13", "F01", "F+1", "f1"] {
        assert!(
            validation::receiver_bindings(
                &format!("ALT-CTRL-{key}"),
                "ALT-CTRL-SHIFT-]",
                "ALT-CTRL-["
            )
            .is_err()
        );
    }
}

// Additional direct-source contract coverage: no equivalent named Go test.
#[test]
fn strict_json_wire_semantics_and_budgets() {
    let raw = encoded(&session("ready"));
    let c = OperationControl::default();
    for extra in [r#""unknown":null,"#, r#""unknown":0,"#] {
        let data = [vec![b'{'], extra.as_bytes().to_vec(), raw[1..].to_vec()].concat();
        assert!(parse_signal(&data, &c).is_err());
    }
    let duplicate = [
        br#"{"sequence":1,"\u0073equence":2,"#.to_vec(),
        raw[1..].to_vec(),
    ]
    .concat();
    assert_code(
        parse_signal(&duplicate, &c),
        "bridge.json_duplicate_or_invalid_key",
    );
    // Distinct case variants are not duplicate JSON keys; typed last value wins,
    // and null does not erase an earlier scalar. Go's long-s/Kelvin folds remain.
    let alias = String::from_utf8(raw.clone())
        .unwrap()
        .replace(
            "\"sequence\":3",
            "\"Sequence\":2,\"ſequence\":3,\"SEQUENCE\":null",
        )
        .replace("\"kind\"", "\"Kind\"");
    assert_eq!(parse_signal(alias.as_bytes(), &c).unwrap().sequence, 3);
    for bad in ["-0", "3.0", "3e0", "18446744073709551616", "-1"] {
        let data = String::from_utf8(raw.clone())
            .unwrap()
            .replace("\"sequence\":3", &format!("\"sequence\":{bad}"));
        assert!(parse_signal(data.as_bytes(), &c).is_err());
    }
    assert_code(
        json::validate(b"{\"a\":{\"b\":{\"c\":0}}}", 2, 32, &c),
        "bridge.json_depth_limit",
    );
    json::validate(b"{\"a\":{\"b\":{}}}", 2, 32, &c).unwrap();
    let entries = format!("[{}0]", "0,".repeat(31));
    json::validate(entries.as_bytes(), 2, 32, &c).unwrap();
    let entries = format!("[{}0]", "0,".repeat(32));
    assert_code(
        json::validate(entries.as_bytes(), 2, 32, &c),
        "bridge.json_entry_limit",
    );
    assert_code(
        json::validate(br#"{"\ud800":0,"\ufffd":1}"#, 2, 32, &c),
        "bridge.json_duplicate_or_invalid_key",
    );
    assert_code(
        parse_signal(&[raw, b" {}".to_vec()].concat(), &c),
        "bridge.json_trailing_data",
    );
    for good in [
        b"null".as_slice(),
        b"1e10000",
        br#""\ud800""#,
        b"-0",
        b"[1,true,null]",
    ] {
        let (r, e) = report_fixture(good, b"");
        verify_report(&r, good, b"", &e, &c).unwrap();
    }
}

fn receiver(kind: &str) -> Signal {
    Signal {
        schema: "lycheedev.signal.v1".into(),
        release: "3.2.0".into(),
        kind: kind.into(),
        product: "retail".into(),
        build: "12.1.0.69875".into(),
        sequence: 1,
        runtime_epoch: 1,
        receiver_nonce: "a".repeat(32),
        ..Signal::default()
    }
}
#[test]
fn every_receiver_kind_and_validation_group() {
    let mut ready = receiver("receiver_ready");
    ready.input_ready = true;
    ready.wake_binding = "ALT-CTRL-]".into();
    ready.submit_binding = "ALT-CTRL-SHIFT-]".into();
    ready.close_binding = "ALT-CTRL-[".into();
    parse(&ready).unwrap();
    for (change, code) in [
        (
            (|s: &mut Signal| s.sequence = LUA_INTEGER_MAX) as fn(&mut Signal),
            "bridge.receiver_signal_identity",
        ),
        (|s| s.runtime_epoch = 0, "bridge.receiver_signal_identity"),
        (
            |s| s.receiver_nonce = "A".repeat(32),
            "bridge.receiver_signal_identity",
        ),
        (
            |s| s.probe_nonce = "a".repeat(32),
            "bridge.receiver_signal_pollution",
        ),
        (
            |s| s.prior_session_sequence = 1,
            "bridge.receiver_prior_session_invalid",
        ),
        (
            |s| s.error_code = "bad.code".into(),
            "bridge.receiver_error_code",
        ),
        (|s| s.input_ready = false, "bridge.receiver_ready_invalid"),
        (
            |s| s.close_binding = "ALT-CTRL-SHIFT-]".into(),
            "bridge.receiver_bindings_invalid",
        ),
        (
            |s| s.receiver_protocol = "x".repeat(33),
            "bridge.receiver_protocol_invalid",
        ),
    ] {
        let mut s = ready.clone();
        change(&mut s);
        assert_code(parse(&s), code);
    }
    ready.session_nonce = "b".repeat(32);
    ready.prior_session_sequence = LUA_INTEGER_MAX;
    ready.prior_session_epoch = 1;
    parse(&ready).unwrap();
    ready.prior_session_epoch = 2;
    assert_code(parse(&ready), "bridge.receiver_prior_session_invalid");
    for kind in [
        "receiver_staged",
        "receiver_commit_ready",
        "receiver_accepted",
    ] {
        let mut s = receiver(kind);
        s.request_id = "REQ-one".into();
        s.attempt_id = "b".repeat(16);
        s.body_bytes = 256;
        s.body_adler32 = "00000001".into();
        s.input_ready = kind != "receiver_accepted";
        s.accepted = kind == "receiver_accepted";
        if kind == "receiver_commit_ready" {
            s.commit_nonce = "c".repeat(16);
        }
        parse(&s).unwrap();
        let mut e = SignalExpectation {
            kind: kind.into(),
            receiver_nonce: s.receiver_nonce.clone(),
            attempt_id: s.attempt_id.clone(),
            body_bytes: 256,
            body_adler32: s.body_adler32.clone(),
            commit_nonce: s.commit_nonce.clone(),
            require_accepted: s.accepted,
            ..SignalExpectation::default()
        };
        s.matches(&e).unwrap();
        e.body_bytes = 1;
        assert!(s.matches(&e).is_err());
        let mut bad = s.clone();
        bad.body_bytes = 257;
        assert_code(parse(&bad), "bridge.receiver_stage_receipt_invalid");
        bad = s.clone();
        bad.accepted = !bad.accepted;
        assert!(parse(&bad).is_err());
        bad = s.clone();
        bad.commit_nonce = if kind == "receiver_commit_ready" {
            String::new()
        } else {
            "c".repeat(16)
        };
        assert!(parse(&bad).is_err());
        if kind == "receiver_accepted" {
            s.error_code = "dispatch_failed".into();
            parse(&s).unwrap();
        }
    }
    for kind in [
        "receiver_rejected",
        "receiver_cancelled",
        "receiver_timeout",
    ] {
        let mut s = receiver(kind);
        s.error_code = "input_failed".into();
        s.body_bytes = u32::MAX;
        s.request_id = "arbitrary / text".into();
        parse(&s).unwrap();
        let mut bad = s.clone();
        bad.input_ready = true;
        assert_code(parse(&bad), "bridge.receiver_rejection_invalid");
        bad = s.clone();
        bad.attempt_id = "bad".into();
        assert_code(parse(&bad), "bridge.receiver_attempt_invalid");
        bad = s.clone();
        bad.body_adler32 = "BAD".into();
        assert_code(parse(&bad), "bridge.receiver_digest_invalid");
        bad = s;
        bad.wake_binding = "ALT-CTRL-]".into();
        assert_code(parse(&bad), "bridge.receiver_bindings_unexpected");
    }
}
#[test]
fn reset_and_session_kinds_preserve_fixed_go_admission() {
    let mut reset = identity();
    reset.kind = "reset".into();
    reset.actor_state.clear();
    reset.input_ready = false;
    parse(&reset).unwrap();
    let mut bad = reset.clone();
    bad.guid.clear();
    assert_code(parse(&bad), "bridge.invalid_reset_actor");
    bad = reset.clone();
    bad.input_ready = true;
    assert_code(parse(&bad), "bridge.invalid_reset_signal");
    reset.release = "\n".into();
    reset.product.clear();
    reset.build.clear();
    reset.wake_binding = "historical oddity".into();
    parse(&reset).unwrap();
    for kind in [
        "ready",
        "loaded",
        "reported",
        "report_error",
        "acknowledged",
        "cancelled",
        "cleared",
    ] {
        let mut s = session(kind);
        if kind == "loaded" {
            s.code_bytes = 1;
            s.code_adler32 = "00000001".into();
        }
        if kind == "reported" {
            s.report_bytes = 1;
            s.report_adler32 = "00000001".into();
        }
        if kind == "report_error" {
            s.error_code = "report_failed".into();
        }
        if kind == "cleared" {
            s.cleanup_nonce = "b".repeat(32);
            s.guid = "Player-1-123".into();
        }
        parse(&s).unwrap();
        if kind != "ready" {
            s.session_nonce.clear();
            parse(&s).unwrap();
        }
        let mut bad = s.clone();
        bad.release.clear();
        assert_code(parse(&bad), "bridge.invalid_signal");
        bad = s.clone();
        bad.work_state = "running".into();
        assert_code(parse(&bad), "bridge.unexpected_checkpoint_fields");
        bad = s.clone();
        bad.report_scope = "invalid".into();
        assert_code(parse(&bad), "bridge.invalid_report_scope");
        bad = s;
        bad.receiver_nonce = "a".repeat(32);
        assert_code(parse(&bad), "bridge.unexpected_receiver_fields");
    }
    let mut error_signal = session("report_error");
    error_signal.error_code = "report_failed".into();
    let mut e = expectation(&error_signal);
    e.kind = "reported".into();
    assert!(error_signal.matches(&e).is_err());
    e.allow_report_error = true;
    error_signal.matches(&e).unwrap();
    let body = b"{}";
    assert_code(
        verify_report(
            &encoded(&error_signal),
            body,
            b"",
            &e,
            &OperationControl::default(),
        ),
        "bridge.report_body_mismatch",
    );
}
#[test]
fn compact_fill_does_not_repair_contradictions_or_epoch() {
    let baseline = SignalIdentity {
        release: "3.2.0".into(),
        character: "C".into(),
        realm: "R".into(),
        guid: "G".into(),
        product: "retail".into(),
        build: "1".into(),
        session_nonce: "session".into(),
        runtime_epoch: 42,
    };
    let filled = fill_signal_identity(Signal::default(), &baseline);
    assert_eq!(filled.character, "C");
    assert_eq!(filled.guid, "G");
    assert_eq!(filled.session_nonce, "session");
    assert_eq!(filled.runtime_epoch, 0);
    let mut explicit = filled.clone();
    explicit.character = "foreign".into();
    explicit.runtime_epoch = 3;
    let actual = fill_signal_identity(explicit, &baseline);
    assert_eq!(actual.character, "foreign");
    assert_eq!(actual.runtime_epoch, 3);
    let body = b"{}";
    let (raw, e) = report_fixture(body, b"");
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    for key in ["character", "realm", "sessionNonce"] {
        value.as_object_mut().unwrap().remove(key);
    }
    verify_report(
        &serde_json::to_vec(&value).unwrap(),
        body,
        b"",
        &e,
        &OperationControl::default(),
    )
    .unwrap();
    value["character"] = json!("foreign");
    assert!(
        verify_report(
            &serde_json::to_vec(&value).unwrap(),
            body,
            b"",
            &e,
            &OperationControl::default()
        )
        .is_err()
    );
}
fn checkpoint() -> Signal {
    Signal {
        kind: "checkpoint".into(),
        session_nonce: "a".repeat(32),
        probe_nonce: "b".repeat(32),
        code_sha256: "c".repeat(64),
        guid: "Player-1-123".into(),
        runtime_epoch: 1,
        work_state: "loaded".into(),
        ..session("checkpoint")
    }
}
#[test]
fn checkpoint_nested_receipt_and_all_states() {
    for state in ["loaded", "running"] {
        let mut s = checkpoint();
        s.work_state = state.into();
        parse(&s).unwrap();
    }
    for state in ["reported", "report_error", "acknowledged", "finished"] {
        let mut outer = checkpoint();
        outer.work_state = state.into();
        let kind = if state == "finished" {
            "acknowledged"
        } else {
            state
        };
        let mut inner = session(kind);
        inner.session_nonce.clear();
        inner.character.clear();
        inner.realm.clear();
        if kind == "reported" {
            inner.report_bytes = 2;
            inner.report_adler32 = "017500f9".into();
        }
        if kind == "report_error" {
            inner.error_code = "report_failed".into();
        }
        outer.receipt = String::from_utf8(encoded(&inner)).unwrap();
        parse(&outer).unwrap();
        let mut bad = outer.clone();
        bad.receipt.clear();
        assert_code(parse(&bad), "bridge.checkpoint_receipt_missing");
        inner.request_id = "other".into();
        bad = outer.clone();
        bad.receipt = String::from_utf8(encoded(&inner)).unwrap();
        assert_code(parse(&bad), "bridge.checkpoint_receipt_mismatch");
        inner.request_id = outer.request_id.clone();
        inner.session_nonce = "foreign".into();
        bad.receipt = String::from_utf8(encoded(&inner)).unwrap();
        assert_code(parse(&bad), "bridge.checkpoint_receipt_identity");
        inner.session_nonce.clear();
        inner.kind = if kind == "acknowledged" {
            "cancelled"
        } else {
            "acknowledged"
        }
        .into();
        inner.error_code.clear();
        bad.receipt = String::from_utf8(encoded(&inner)).unwrap();
        assert_code(parse(&bad), "bridge.checkpoint_receipt_kind");
    }
    let mut s = checkpoint();
    s.runtime_epoch = u64::MAX;
    s.resources_released = true;
    parse(&s).unwrap(); // Fixed Go bypasses general epoch cap here.
    s.receipt = "{}".into();
    assert_code(parse(&s), "bridge.invalid_checkpoint_receipt");
    s.receipt.clear();
    s.work_state = "unknown".into();
    assert_code(parse(&s), "bridge.invalid_checkpoint_state");
    s = checkpoint();
    s.code_sha256 = "C".repeat(64);
    assert_code(parse(&s), "bridge.invalid_checkpoint");
}
#[test]
fn report_json_depth_entry_and_checksum_boundaries() {
    let c = OperationControl::default();
    for good in [
        format!("{}0{}", "[".repeat(32), "]".repeat(32)),
        format!("[{}0]", "0,".repeat(32767)),
    ] {
        let (raw, e) = report_fixture(good.as_bytes(), b"");
        verify_report(&raw, good.as_bytes(), b"", &e, &c).unwrap();
    }
    for (bad, want) in [
        (
            format!("{}0{}", "[".repeat(33), "]".repeat(33)),
            "bridge.json_depth_limit",
        ),
        (
            format!("[{}0]", "0,".repeat(32768)),
            "bridge.json_entry_limit",
        ),
    ] {
        let (raw, e) = report_fixture(bad.as_bytes(), b"");
        assert_code(verify_report(&raw, bad.as_bytes(), b"", &e, &c), want);
    }
    assert_eq!(byte_checksum(b"", &c).unwrap(), "00000001");
    assert_eq!(byte_checksum(b"Wikipedia", &c).unwrap(), "11e60398");
    let code = vec![b'x'; CODE_LIMIT];
    let (raw, e) = report_fixture(b"{}", &code);
    verify_report(&raw, b"{}", &code, &e, &c).unwrap();
    let (raw, e) = report_fixture(b"{}", b"");
    let mut value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    value["codeAdler32"] = json!("00000001");
    assert_code(
        verify_report(&serde_json::to_vec(&value).unwrap(), b"{}", b"", &e, &c),
        "bridge.report_code_mismatch",
    );
}
#[test]
fn original_control_deadline_and_cancel_are_not_replaced() {
    use lycheedev_core::control::TrackedCancellation;
    let expired = OperationControl::uncancelable(Instant::now());
    assert_code(parse_signal(b"{}", &expired), "command.deadline");
    let (raw, e) = report_fixture(b"{}", b"");
    assert_code(
        verify_report(&raw, b"{}", b"", &e, &expired),
        "command.deadline",
    );
    let cancellation = TrackedCancellation::new();
    let control = OperationControl::tracked(
        Instant::now() + std::time::Duration::from_secs(60),
        cancellation.clone(),
    );
    cancellation.cancel();
    assert_code(parse_signal(&raw, &control), "command.cancelled");
    assert_code(
        verify_report(&raw, b"{}", b"", &e, &control),
        "command.cancelled",
    );
}
#[test]
fn expectation_persistence_reuses_go_record_semantics() {
    use crate::go_mailbox_v1::admission::decode_wire_record;
    let e:SignalExpectation=decode_wire_record(br#"{"RequestID":"first","requestId":"last","RequestId":null,"AfterSequence":3,"AfterSequence":null,"ignored":true}"#).unwrap();
    assert_eq!(e.request_id, "last");
    assert_eq!(e.after_sequence, 3);
    let serialized = serde_json::to_value(e).unwrap();
    assert_eq!(serialized["RequestID"], "last");
    assert!(serialized.get("requestId").is_none());
}

// Go StdEncoding.decodeQuantum retains offsets in the original CR/LF input.
#[test]
fn historical_base64_errors_preserve_go_original_byte_offsets() {
    for (raw, offset) in [
        (b"!".as_slice(), 0),
        (b"A".as_slice(), 0),
        (b"AA=".as_slice(), 3),
        (b"AA\n=X".as_slice(), 3),
        (b"AA==\r\n!".as_slice(), 6),
    ] {
        assert_eq!(
            decode_base64(raw).unwrap_err().to_string(),
            format!("illegal base64 data at input byte {offset}")
        );
    }
    let error = parse_signal(&[0x1e, b'!'], &OperationControl::default()).unwrap_err();
    assert_eq!(
        error.message,
        "bridge.signal_budget_or_encoding: illegal base64 data at input byte 0"
    );
}
#[test]
fn historical_unknown_signal_field_retains_original_spelling() {
    let error = parse_signal(br#"{"FutureField":null}"#, &OperationControl::default()).unwrap_err();
    assert_eq!(error.message, "json: unknown field \"FutureField\"");
}

#[test]
fn historical_deflate_truncation_retains_go_unexpected_eof_suffix() {
    let control = OperationControl::default();
    assert_eq!(
        parse_signal(&[0x1f, 0x00], &control).unwrap_err().message,
        "bridge.signal_budget_or_encoding: unexpected EOF"
    );
    assert_eq!(
        parse_signal(&[0x1e, b'\n'], &control).unwrap_err().message,
        "bridge.signal_budget_or_encoding: unexpected EOF"
    );
    assert_eq!(
        parse_signal(&[0x1f], &control).unwrap_err().message,
        "bridge.signal_budget_or_encoding"
    );
}
