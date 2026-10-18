use super::*;
use serde_json::json;
use std::{cell::Cell, collections::BTreeMap};

fn token(n: u8) -> String {
    hex::encode([n; 16])
}
fn header(kind: Kind) -> Header {
    Header {
        kind,
        nonce: [2; 16],
        runtime: [1; 16],
        ticket: [3; 16],
        state: 1,
        sequence: 1,
        length: 0,
        checksum: 0,
    }
}
fn identity() -> Identity {
    Identity {
        schema: SCHEMA.into(),
        runtime: token(1),
        owner: token(4),
        fence: 1,
        next_slot: 2,
        slots: SLOT_COUNT,
        inventory: Some(SLOT_COUNT),
        input_state: SCHEMA.into(),
        character: "c".into(),
        realm: "r".into(),
        guid: "g".into(),
        build: "b".into(),
        product: "retail".into(),
        release: "3.2.0".into(),
    }
}
fn envelope() -> ExpectedEnvelope {
    ExpectedEnvelope {
        schema: SCHEMA.into(),
        index: 1,
        runtime: token(1),
        owner: token(4),
        fence: 1,
        nonce: token(2),
        ticket: token(3),
        action: "commit".into(),
        guid: "g".into(),
        build: "b".into(),
        ..ExpectedEnvelope::default()
    }
}
fn input() -> InputObservation {
    InputObservation {
        schema: SCHEMA.into(),
        runtime: token(1),
        owner: token(4),
        fence: 1,
        next_slot: 1,
        guid: "g".into(),
        build: "b".into(),
        sample_millis: 1200,
        bindings: Some(InputBindings {
            wake: "ALT-CTRL-F12".into(),
            reload: "ALT-CTRL-F11".into(),
            ..InputBindings::default()
        }),
        input_blocked: Some(false),
        ..InputObservation::default()
    }
}
fn diagnostic() -> InputDiagnostic {
    InputDiagnostic {
        schema: SCHEMA.into(),
        runtime: token(1),
        attempt_seq: 4,
        slot: 1,
        stage: "load_failed".into(),
        reason: "slot_addon_disabled".into(),
        received: false,
        sample_millis: 0,
    }
}
fn input_wire(s: &InputObservation, raw_diagnostic: Option<&str>) -> Vec<u8> {
    let mut payload = serde_json::to_string(s).unwrap();
    if let Some(raw) = raw_diagnostic {
        payload.pop();
        payload.push_str(",\"inputAttempt\":");
        payload.push_str(raw);
        payload.push('}');
    }
    let mut h = header(Kind::Input);
    h.nonce = h.runtime;
    h.ticket = ZERO_TOKEN;
    encode_record(h, payload.as_bytes()).unwrap()
}

#[test]
fn go_mailbox_v1_wire_exact_empty_body_golden() {
    let mut h = header(Kind::Body);
    h.state = 3;
    let wire = encode_record(h, b"").unwrap();
    // Independently specified Go header/trailer, including Adler32(empty)=1.
    assert_eq!(&wire[..8], b"LYCMEM06");
    assert_eq!(wire.len(), 120);
    assert_eq!(&wire[56..68], &[3, 3, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&wire[68..76], &[1, 0, 0, 0, 80, 0, 0, 0]);
    assert_eq!(&wire[80..88], b"LYCEND06");
    assert_eq!(decode_record(&wire).unwrap().payload, b"");
    assert_eq!(checksum(b"Wikipedia"), 0x11e60398);
}
#[test]
fn go_mailbox_v1_wire_all_kinds_and_payload_bounds() {
    for value in 1..=6 {
        let kind = Kind::try_from(value).unwrap();
        for size in [0, 1, 16384, MAX_PAYLOAD] {
            let payload = vec![0xa5; size];
            let bytes = encode_record(header(kind), &payload).unwrap();
            let decoded = decode_record(&bytes).unwrap();
            assert_eq!(decoded.header.kind, kind);
            assert_eq!(decoded.payload, payload);
        }
    }
    assert!(encode_record(header(Kind::Body), &vec![0; MAX_PAYLOAD + 1]).is_err());
    let mut h = header(Kind::Identity);
    h.state = 0;
    assert_eq!(
        encode_record(h, b""),
        Err(ContractError("bridge.memory_record_invalid"))
    );
}
#[test]
fn go_mailbox_v1_wire_rejects_corruption_and_old_rust_protocol() {
    let bytes = encode_record(header(Kind::Receipt), b"payload").unwrap();
    for position in [0, 8, 24, 40, 58, 60, 64, 68, 72, 76, 80, 87, 95, 111] {
        let mut bad = bytes.clone();
        bad[position] ^= 1;
        assert!(decode_record(&bad).is_err(), "position {position}");
    }
    for size in 0..bytes.len() {
        assert!(decode_record(&bytes[..size]).is_err());
    }
    let mut long = bytes.clone();
    long.push(0);
    assert!(decode_record(&long).is_err());
    for (field, value) in [(56, 0), (56, 7), (57, 0)] {
        let mut bad = bytes.clone();
        bad[field] = value;
        let sum = checksum(&bad[..76]);
        bad[76..80].copy_from_slice(&sum.to_le_bytes());
        assert_eq!(
            decode_header(&bad),
            Err(ContractError("bridge.memory_header_bounds"))
        );
    }
    let mut old = bytes;
    old[..8].copy_from_slice(b"LYCMEM07");
    assert_eq!(
        decode_header(&old),
        Err(ContractError("bridge.memory_header_invalid"))
    );
}
#[test]
fn go_mailbox_v1_token_exact_lowercase() {
    for invalid in [
        "",
        "schema",
        "0000000000000000000000000000000",
        "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "gggggggggggggggggggggggggggggggg",
    ] {
        assert!(parse_token(invalid).is_err());
    }
    assert_eq!(parse_token(&"0".repeat(32)).unwrap(), ZERO_TOKEN);
}

#[test]
fn go_mailbox_v1_identity_schema_and_capacity() {
    assert!(identity().validate().is_ok());
    for schema in [
        "lycheedev.slot.identity.v1",
        LEGACY_IDENTITY_SCHEMA,
        "lycheedev.slot.v3",
        "",
    ] {
        let mut i = identity();
        i.schema = schema.into();
        assert!(i.validate().is_err());
    }
    for slots in [64, 200] {
        let mut i = identity();
        i.slots = slots;
        i.inventory = None;
        assert!(i.validate().is_err());
    }
    for cursor in [500, 501, 502] {
        let mut i = identity();
        i.next_slot = cursor;
        assert_eq!(i.validate().is_ok(), cursor <= 501);
    }
    for inventory in [-1, 0, 500, 501] {
        let mut i = identity();
        i.inventory = Some(inventory);
        assert_eq!(i.validate().is_ok(), (0..=500).contains(&inventory));
    }
}
fn identity_wire(i: &Identity, h: Header) -> Vec<u8> {
    encode_record(h, &serde_json::to_vec(i).unwrap()).unwrap()
}
fn identity_header() -> Header {
    Header {
        nonce: ZERO_TOKEN,
        ticket: ZERO_TOKEN,
        sequence: 0,
        ..header(Kind::Identity)
    }
}
#[test]
fn go_mailbox_v1_identity_header_correlation() {
    let i = identity();
    for seq in [0, 12, u32::MAX] {
        let mut h = identity_header();
        h.sequence = seq;
        let wire = identity_wire(&i, h);
        assert!(matches!(
            decode_identity(&decode_record(&wire).unwrap(), "3.2.0"),
            Ok(IdentityPublication::Current(_))
        ));
    }
    for field in 0..5 {
        let mut h = identity_header();
        match field {
            0 => h.kind = Kind::Input,
            1 => h.state = 2,
            2 => h.nonce = [1; 16],
            3 => h.ticket = [1; 16],
            _ => h.runtime = [9; 16],
        }
        let wire = identity_wire(&i, h);
        assert!(decode_identity(&decode_record(&wire).unwrap(), "3.2.0").is_err());
    }
    let wire = identity_wire(&i, identity_header());
    assert!(decode_identity(&decode_record(&wire).unwrap(), "other").is_err());
}
#[test]
fn go_mailbox_v1_legacy_200_is_activation_only() {
    let mut i = identity();
    i.schema = LEGACY_IDENTITY_SCHEMA.into();
    i.slots = 200;
    i.inventory = Some(200);
    i.next_slot = 1;
    i.input_state = LEGACY_HYBRID_SCHEMA.into();
    let wire = identity_wire(&i, identity_header());
    let decoded = decode_identity(&decode_record(&wire).unwrap(), "3.2.0").unwrap();
    assert!(matches!(
        decoded,
        IdentityPublication::ManualActivationRequired(_)
    ));
    assert!(decoded.incompatibility().is_some());
    assert!(decoded.identity().validate().is_err());
    for field in 0..6 {
        let mut bad = i.clone();
        match field {
            0 => bad.schema = SCHEMA.into(),
            1 => bad.slots = 64,
            2 => bad.input_state = "future".into(),
            3 => bad.inventory = Some(201),
            4 => bad.next_slot = 202,
            _ => bad.guid.clear(),
        }
        let wire = identity_wire(&bad, identity_header());
        assert!(decode_identity(&decode_record(&wire).unwrap(), "3.2.0").is_err());
    }
}
#[test]
fn go_mailbox_v1_old_500_capability_stays_labelled() {
    // Fixed Go 3.2.3 input.go/protocol.go contract. Do not derive the oracle
    // from upgrade_input_capability, which decode_identity itself consumes.
    for (capability, accepted) in [
        ("lycheedev.input.v3", true),
        ("lycheedev.input.hybrid.v2", true),
        ("lycheedev.mailbox.v1", true),
        ("future", false),
        ("", false),
        ("Lycheedev mailbox protocol v1", false),
    ] {
        let mut i = identity();
        i.schema = LEGACY_IDENTITY_SCHEMA.into();
        i.input_state = capability.into();
        let wire = identity_wire(&i, identity_header());
        let result = decode_identity(&decode_record(&wire).unwrap(), "3.2.0");
        if accepted {
            assert_eq!(
                result,
                Ok(IdentityPublication::UpgradeCapability(i)),
                "capability: {capability:?}"
            );
        } else {
            assert_eq!(
                result,
                Err(ContractError("live.channel_identity_record_invalid")),
                "capability: {capability:?}"
            );
        }
    }
}

fn receipt() -> Receipt {
    Receipt {
        identity: identity(),
        nonce: token(2),
        ticket: token(3),
        action: "commit".into(),
        state: "reported".into(),
        ..Receipt::default()
    }
}
#[test]
fn go_mailbox_v1_receipt_exact_match_and_rejection() {
    let r = receipt();
    let e = envelope();
    let wire = encode_record(header(Kind::Receipt), &serde_json::to_vec(&r).unwrap()).unwrap();
    assert!(matches!(
        decode_receipt(&decode_record(&wire).unwrap(), &e),
        Ok(ReceiptMatch::Accepted(_))
    ));
    for field in 0..8 {
        let mut bad = e.clone();
        match field {
            0 => bad.schema = PREVIOUS_SCHEMA.into(),
            1 => bad.nonce = token(8),
            2 => bad.ticket = token(8),
            3 => bad.action = "prepare".into(),
            4 => bad.runtime = token(8),
            5 => bad.index = 2,
            6 => bad.owner = token(8),
            _ => bad.fence = 2,
        }
        assert!(decode_receipt(&decode_record(&wire).unwrap(), &bad).is_err());
    }
    let mut rejected = r;
    rejected.state = "rejected".into();
    rejected.reason = "slot_wake_rejected".into();
    let wire = encode_record(
        header(Kind::Receipt),
        &serde_json::to_vec(&rejected).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        decode_receipt(&decode_record(&wire).unwrap(), &e),
        Ok(ReceiptMatch::Rejected(_))
    ));
}
#[test]
fn go_mailbox_v1_confirmation_and_bind_zero_ticket() {
    for kind in [Kind::Receipt, Kind::Confirmation] {
        for state in ["reported", "rejected"] {
            let mut r = receipt();
            r.action = "confirm".into();
            r.state = state.into();
            let wire = encode_record(header(kind), &serde_json::to_vec(&r).unwrap()).unwrap();
            assert_eq!(
                decode_publication_receipt(&decode_record(&wire).unwrap()).is_ok(),
                kind == Kind::Confirmation || state == "rejected"
            );
        }
    }
    let mut r = receipt();
    r.action = "bind".into();
    r.ticket = token(0);
    let mut h = header(Kind::Receipt);
    h.ticket = ZERO_TOKEN;
    let wire = encode_record(h, &serde_json::to_vec(&r).unwrap()).unwrap();
    assert!(decode_publication_receipt(&decode_record(&wire).unwrap()).is_ok());
    r.identity.schema = PREVIOUS_SCHEMA.into();
    let wire = encode_record(h, &serde_json::to_vec(&r).unwrap()).unwrap();
    assert!(decode_publication_receipt(&decode_record(&wire).unwrap()).is_err());
}

/// Synthetic bounded read source only. No process, window, path discovery or game.
struct FixtureMemory {
    first: Vec<u8>,
    second: Vec<u8>,
    reads: Cell<usize>,
    charged: Cell<u64>,
}
impl crate::mailbox::Memory for FixtureMemory {
    fn verify(&self) -> crate::mailbox::Result<()> {
        Ok(())
    }
    fn read(&self, address: u64, out: &mut [u8], module: bool) -> crate::mailbox::Result<()> {
        assert_eq!(address, 0x1000);
        assert!(!module);
        let n = self.reads.get();
        self.reads.set(n + 1);
        let data = if n == 0 { &self.first } else { &self.second };
        let bytes = data.get(..out.len()).ok_or("short fixture read")?;
        out.copy_from_slice(bytes);
        Ok(())
    }
    fn charge(&self, bytes: u64, _: u64) -> crate::mailbox::Result<()> {
        self.charged.set(self.charged.get() + bytes);
        Ok(())
    }
}
fn source(wire: &[u8]) -> FixtureMemory {
    FixtureMemory {
        first: wire.to_vec(),
        second: wire.to_vec(),
        reads: Cell::new(0),
        charged: Cell::new(0),
    }
}
fn body_selector(length: u32, checksum: u32) -> Selector {
    Selector {
        kind: Kind::Body,
        nonce: [2; 16],
        runtime: [1; 16],
        ticket: [3; 16],
        body: Some(BodyQualification { length, checksum }),
    }
}
#[test]
fn go_mailbox_v1_synthetic_memory_empty_body_and_allocation_gate() {
    let mut h = header(Kind::Body);
    h.state = 3;
    let wire = encode_record(h, b"").unwrap();
    let memory = source(&wire);
    assert_eq!(
        read_record(&memory, 0x1000, &body_selector(0, 1)).unwrap(),
        wire
    );
    assert_eq!(memory.reads.get(), 2);
    assert_eq!(memory.charged.get(), 120);
    for field in 0..4 {
        let mut selector = body_selector(0, 1);
        match field {
            0 => selector.body = None,
            1 => selector.nonce = ZERO_TOKEN,
            2 => selector.runtime = ZERO_TOKEN,
            _ => selector.ticket = ZERO_TOKEN,
        }
        let memory = source(&wire);
        assert_eq!(
            read_record(&memory, 0x1000, &selector),
            Err(ContractError("memory.record_mismatch"))
        );
        assert_eq!(memory.reads.get(), 1);
        assert_eq!(memory.charged.get(), 0);
    }
}
#[test]
fn go_mailbox_v1_synthetic_memory_torn_read_and_report_bounds() {
    let mut h = header(Kind::Body);
    h.state = 3;
    let wire = encode_record(h, b"body").unwrap();
    let mut memory = source(&wire);
    h.sequence += 1;
    memory.second = encode_record(h, b"body").unwrap();
    assert_eq!(
        read_record(&memory, 0x1000, &body_selector(4, checksum(b"body"))),
        Err(ContractError("memory.record_changed"))
    );
    for (length, sum) in [(3, checksum(b"body")), (4, 0)] {
        let memory = source(&wire);
        assert!(read_record(&memory, 0x1000, &body_selector(length, sum)).is_err());
        assert_eq!(memory.reads.get(), 1);
    }
}
#[test]
fn go_mailbox_v1_schema_only_public_map_missing_is_not_empty_record() {
    // Logical map contract only; native Lua hash-table layout coverage is pending.
    for _field in ["receipts", "bodies"] {
        let mut map = PublicMap {
            schema: SCHEMA,
            entries: BTreeMap::new(),
        };
        assert_eq!(map.lookup(&token(2)).unwrap(), None);
        assert!(map.lookup("schema").is_err());
        map.entries.insert(token(3), b"unrelated");
        assert_eq!(map.lookup(&token(2)).unwrap(), None);
        map.entries.insert(token(2), b"");
        assert_eq!(map.lookup(&token(2)).unwrap(), Some(b"".as_slice()));
        assert!(decode_record(map.lookup(&token(2)).unwrap().unwrap()).is_err());
    }
}

#[test]
fn go_mailbox_v1_disabled_diagnostic_never_grants_receipt_or_replay() {
    let raw = serde_json::to_string(&diagnostic()).unwrap();
    let wire = input_wire(&input(), Some(&raw));
    let s = decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400).unwrap();
    assert_eq!(s.input_blocked, Some(false));
    assert_eq!(s.input_attempt.unwrap().reason, "slot_addon_disabled");
    assert!(decode_publication_receipt(&decode_record(&wire).unwrap()).is_err());
    assert!(!may_prepare_again(None));
    let mut wrong = envelope();
    wrong.runtime = token(9);
    assert_eq!(
        decode_input(&decode_record(&wire).unwrap(), &wrong, 1000, 1400),
        Err(ContractError("live.channel_input_target_changed"))
    );
}
#[test]
fn go_mailbox_v1_optional_diagnostic_is_strict_and_non_authoritative() {
    let good = serde_json::to_string(&diagnostic()).unwrap();
    let mut variants = vec![
        good.clone(),
        "null".into(),
        "\"private suffix\"".into(),
        "{\"attemptSeq\":\"bad\"}".into(),
        format!("{{ {} }}", " ".repeat(513)),
    ];
    let mut value = serde_json::to_value(diagnostic()).unwrap();
    for field in ["received", "reason"] {
        let mut v = value.clone();
        v.as_object_mut().unwrap().remove(field);
        variants.push(v.to_string());
        v[field] = serde_json::Value::Null;
        variants.push(v.to_string());
    }
    value["reason"] = json!("slot_addon_disabled: private suffix");
    variants.push(value.to_string());
    for (index, raw) in variants.iter().enumerate() {
        let wire = input_wire(&input(), Some(raw));
        let s = decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400).unwrap();
        assert_eq!(s.input_attempt.is_some(), index == 0, "{raw}");
        assert_eq!(s.input_blocked, Some(false));
    }
    let padded = good.replacen('{', &format!("{{{}", " ".repeat(513)), 1);
    assert!(parse_input_diagnostic(padded.as_bytes(), &token(1), 1200).is_none());
    let wire = input_wire(&input(), Some(&padded));
    assert!(
        decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400)
            .unwrap()
            .input_attempt
            .is_none()
    );
}
#[test]
fn go_mailbox_v1_input_readiness_and_freshness_are_separate_gates() {
    for (sample, valid) in [
        (899, false),
        (900, true),
        (1200, true),
        (1500, true),
        (1501, false),
    ] {
        let mut s = input();
        s.sample_millis = sample;
        let wire = input_wire(&s, None);
        assert_eq!(
            decode_input(&decode_record(&wire).unwrap(), &envelope(), 0, 1400).is_ok(),
            valid
        );
    }
    for (sample, valid) in [(1099, false), (1100, true)] {
        let mut s = input();
        s.sample_millis = sample;
        let wire = input_wire(&s, None);
        assert_eq!(
            decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400).is_ok(),
            valid
        );
    }
    let mut s = input();
    s.input_blocked = Some(true);
    s.reason = "combat".into();
    let wire = input_wire(&s, None);
    assert_eq!(
        decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400)
            .unwrap()
            .input_blocked,
        Some(true)
    );
    s.escape_limit = 1;
    let wire = input_wire(&s, None);
    assert!(decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400).is_err());
}
#[test]
fn go_mailbox_v1_two_key_binding_contract() {
    for n in 1..=12 {
        let mut s = input();
        s.bindings.as_mut().unwrap().wake = format!("ALT-CTRL-F{n}");
        assert_eq!(s.valid_bindings(), n != 11);
    }
    for invalid in [
        "ALT-CTRL-F01",
        "ALT-CTRL-F13",
        "ALT-CTRL-SHIFT-F12",
        "CTRL-ALT-F12",
        "ALT-CTRL-]",
    ] {
        let mut s = input();
        s.bindings.as_mut().unwrap().wake = invalid.into();
        assert!(!s.valid_bindings());
    }
    for raw in [
        r#"{"wake":"x"}"#,
        r#"{"wake":"x","reload":"y","close":"z"}"#,
        r#"{"wake":"x","reload":"y","extra":1}"#,
    ] {
        assert!(serde_json::from_str::<InputBindings>(raw).is_err());
    }
    let mut s = input();
    s.bindings = Some(InputBindings::default());
    s.input_blocked = Some(true);
    s.reason = "input_binding_unavailable".into();
    assert!(s.valid_readiness() && s.valid_bindings());
    let wire = input_wire(&s, None);
    assert!(decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400).is_ok());
}
#[test]
fn go_mailbox_v1_unknown_input_never_replays() {
    assert!(!may_prepare_again(None));
    for disposition in [
        InputDisposition::Submitted,
        InputDisposition::Uncertain,
        InputDisposition::NotSent,
    ] {
        for count in [0, 6] {
            let out = InputOutcome {
                disposition: disposition.clone(),
                messages_queued: count,
                at_millis: 0,
                reason: String::new(),
                retryable: false,
            };
            assert_eq!(
                may_prepare_again(Some(&out)),
                disposition == InputDisposition::NotSent && count == 0
            );
        }
    }
}

#[test]
fn go_mailbox_v1_fixed_go_binary_record_vector() {
    // Fixed Go TestMemoryRecordIntegrityAndBounds inputs. Golden bytes were
    // independently encoded with Python struct/zlib, not this Rust encoder.
    let golden = hex::decode("4c59434d454d30366e6f6e63652d6f6e652d756e6971756572756e74696d652d756e6971756500007469636b65742d756e6971756500000003030000110000000d000000d206bb3650000000c11541cfe4b8ade696870062696e6172794c5943454e4430366e6f6e63652d6f6e652d756e6971756572756e74696d652d756e697175650000").unwrap();
    let decoded = decode_record(&golden).unwrap();
    assert_eq!(decoded.header.kind, Kind::Body);
    assert_eq!(decoded.header.sequence, 17);
    assert_eq!(decoded.payload, "中文\0binary".as_bytes());
    assert_eq!(
        encode_record(decoded.header, decoded.payload).unwrap(),
        golden
    );
}

#[test]
fn go_mailbox_v1_diagnostic_dto_roundtrip_preserves_evidence_not_authority() {
    for (stage, reason, received) in [
        ("load_failed", "slot_addon_disabled", false),
        ("received", "", true),
    ] {
        let mut d = diagnostic();
        d.stage = stage.into();
        d.reason = reason.into();
        d.received = received;
        let wire = input_wire(&input(), Some(&serde_json::to_string(&d).unwrap()));
        let observed =
            decode_input(&decode_record(&wire).unwrap(), &envelope(), 1000, 1400).unwrap();
        let saved = serde_json::to_vec(&observed).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(value["inputAttempt"]["reason"], reason);
        assert_eq!(value["inputAttempt"]["received"], received);
        assert!(value.get("observedAtMillis").is_none());
        let restored: InputObservation = serde_json::from_slice(&saved).unwrap();
        assert_eq!(restored.input_attempt, observed.input_attempt);
        assert_eq!(restored.observed_at_millis, 0);
        assert!(!may_prepare_again(None));
        // Restoring evidence creates no receipt, outcome or host freshness.
        let mut wrong = envelope();
        wrong.runtime = token(9);
        assert_eq!(
            decode_input(&decode_record(&wire).unwrap(), &wrong, 1000, 1400),
            Err(ContractError("live.channel_input_target_changed"))
        );
    }
}

#[test]
fn go_mailbox_v1_dto_optional_fields_and_raw_bounds_match_go() {
    let r = receipt();
    let value = serde_json::to_value(&r).unwrap();
    for key in [
        "reportBytes",
        "reportChecksum",
        "reason",
        "challenge",
        "preparedNonce",
    ] {
        assert!(value.get(key).is_none(), "unexpected zero field {key}");
    }
    assert_eq!(
        serde_json::from_slice::<Receipt>(&serde_json::to_vec(&r).unwrap()).unwrap(),
        r
    );
    let mut with_report = r;
    with_report.report_bytes = 9;
    with_report.report_checksum = 17;
    let value = serde_json::to_value(with_report).unwrap();
    assert_eq!(value["reportBytes"], 9);
    assert_eq!(value["reportChecksum"], 17);
    assert!(
        serde_json::to_value(input())
            .unwrap()
            .get("inputAttempt")
            .is_none()
    );
    let mut d = diagnostic();
    d.sample_millis = 1;
    let mut value = serde_json::to_value(input()).unwrap();
    value["inputAttempt"] = serde_json::to_value(d).unwrap();
    // Containing INPUT time wins over an optional diagnostic's supplied time.
    let restored: InputObservation =
        serde_json::from_slice(&serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(restored.input_attempt.unwrap().sample_millis, 1200);
    let raw = serde_json::to_string(&diagnostic()).unwrap();
    let padded = raw.replacen('{', &format!("{{{}", " ".repeat(513)), 1);
    let wire = input_wire(&input(), Some(&padded));
    let record = decode_record(&wire).unwrap();
    let restored: InputObservation = serde_json::from_slice(record.payload).unwrap();
    assert!(restored.input_attempt.is_none());
    // The 2048-byte outer cap belongs to wire records, not journal DTO JSON.
    let mut long_journal = serde_json::to_string(&input()).unwrap();
    long_journal.pop();
    long_journal.push_str(",\"ignored\":\"");
    long_journal.push_str(&"x".repeat(2100));
    long_journal.push_str("\"}");
    assert!(serde_json::from_str::<InputObservation>(&long_journal).is_ok());
}

#[test]
fn go_mailbox_v1_missing_outcome_time_remains_unknown_after_roundtrip() {
    for disposition in ["submitted", "uncertain"] {
        let json = format!(r#"{{"disposition":"{disposition}","messagesQueued":6}}"#);
        let outcome: InputOutcome = serde_json::from_str(&json).unwrap();
        assert_eq!(outcome.at_millis, 0);
        assert!(outcome.validate().is_ok());
        assert!(!may_prepare_again(Some(&outcome)));
        let mut explicit: serde_json::Value = serde_json::from_str(&json).unwrap();
        explicit["atMillis"] = json!(0);
        assert_eq!(
            serde_json::from_slice::<InputOutcome>(&serde_json::to_vec(&explicit).unwrap())
                .unwrap(),
            outcome
        );
        let saved = serde_json::to_value(&outcome).unwrap();
        assert_eq!(saved["atMillis"], 0);
        assert!(saved.get("reason").is_none());
        assert!(saved.get("retryable").is_none());
    }
    let known_zero: InputOutcome = serde_json::from_str(r#"{"disposition":"not_sent"}"#).unwrap();
    assert!(known_zero.validate().is_ok());
    assert!(may_prepare_again(Some(&known_zero)));
}

#[test]
fn go_mailbox_v1_body_static_header_table_before_payload_read() {
    for (state, sequence, valid) in [
        (3, 7, true),
        (3, u32::MAX, true),
        (1, 7, false),
        (3, 0, false),
    ] {
        let mut h = header(Kind::Body);
        h.state = state;
        h.sequence = sequence;
        let payload = br#"{"ok":true}"#;
        let wire = encode_record(h, payload).unwrap();
        let memory = source(&wire);
        let result = read_record(
            &memory,
            0x1000,
            &body_selector(payload.len() as u32, checksum(payload)),
        );
        assert_eq!(result.is_ok(), valid);
        assert_eq!(memory.reads.get(), if valid { 2 } else { 1 });
        assert_eq!(memory.charged.get() > 0, valid);
    }
}

#[test]
fn go_mailbox_v1_old_wire_names_rejected_even_with_valid_header_checksum() {
    let wire = encode_record(header(Kind::Identity), b"{}").unwrap();
    for version in ["04", "05", "07"] {
        let mut old = wire.clone();
        old[..8].copy_from_slice(format!("LYCMEM{version}").as_bytes());
        let sum = checksum(&old[..76]);
        old[76..80].copy_from_slice(&sum.to_le_bytes());
        assert_eq!(
            decode_record(&old),
            Err(ContractError("bridge.memory_header_invalid"))
        );
        let mut old = wire.clone();
        old[82..90].copy_from_slice(format!("LYCEND{version}").as_bytes());
        assert_eq!(
            decode_record(&old),
            Err(ContractError("bridge.memory_body_invalid"))
        );
    }
}

#[test]
fn go_mailbox_v1_stale_two_key_upgrade_is_capability_only() {
    let mut observation = input();
    observation.schema = PREVIOUS_SCHEMA.into();
    observation.sample_millis = 200;
    let wire = input_wire(&observation, None);
    let record = decode_record(&wire).unwrap();
    let mut e = envelope();
    e.action = "reload".into();
    e.index = 400;
    let inspected = inspect_input(&record, &e, 0, 1400).unwrap();
    assert_eq!(inspected.freshness, InputFreshness::Stale);
    assert!(inspected.has_two_key_reload_capability());
    assert_eq!(inspected.observation.observed_at_millis, 0);
    assert_eq!(
        decode_input(&record, &e, 0, 1400),
        Err(ContractError("live.channel_input_state_stale"))
    );
    assert!(!may_prepare_again(None));
    e.action = "commit".into();
    assert_eq!(
        inspect_input(&record, &e, 0, 1400),
        Err(ContractError("live.channel_input_state_invalid"))
    );
    observation.bindings = Some(InputBindings {
        wake: "ALT-CTRL-F12".into(),
        submit: "ALT-CTRL-SHIFT-F12".into(),
        close: "ALT-CTRL-[".into(),
        reload: String::new(),
    });
    let wire = input_wire(&observation, None);
    e.action = "reload".into();
    assert_eq!(
        inspect_input(&decode_record(&wire).unwrap(), &e, 0, 1400),
        Err(ContractError("live.channel_input_state_invalid"))
    );
}
