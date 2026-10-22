//! Direct shared-contract fixtures; no Live or Delivery dependency is required.
//! External fixed wire reference: bridge/slot.go at
//! 3057a9666ab078270b63c7f06de748069b0d1f4b.
use lycheedev_core::mailbox::{
    ContractError, ExpectedEnvelope, MAX_EXACT_INTEGER, SCHEMA, SLOT_COUNT, SlotEnvelope,
    ZERO_TOKEN, checksum, parse_token, slot_payload,
};

fn envelope() -> SlotEnvelope {
    SlotEnvelope {
        schema: SCHEMA.into(),
        index: 7,
        runtime: "2".repeat(32),
        owner: "1".repeat(32),
        fence: 1,
        nonce: "3".repeat(32),
        ticket: "4".repeat(32),
        action: "prepare".into(),
        guid: "g".into(),
        build: "b".into(),
        ..SlotEnvelope::default()
    }
}

#[test]
fn wire_mailbox_v1_shared_slot_json_omits_only_optional_zero_fields() {
    let value = envelope();
    let expected = concat!(
        r#"{"schema":"Lycheedev mailbox protocol v1","index":7,"runtime":"22222222222222222222222222222222","owner":"11111111111111111111111111111111","fence":1,"nonce":"33333333333333333333333333333333","ticket":"44444444444444444444444444444444","action":"prepare","guid":"g","build":"b"}"#,
    );
    assert_eq!(serde_json::to_string(&value).unwrap(), expected);
    assert_eq!(
        serde_json::from_str::<SlotEnvelope>(expected).unwrap(),
        value
    );
    assert_eq!(
        serde_json::from_str::<SlotEnvelope>("{}").unwrap(),
        SlotEnvelope::default()
    );
}

#[test]
fn wire_mailbox_v1_shared_slot_json_preserves_all_fields_and_exact_integer() {
    let value = SlotEnvelope {
        start_slot: 2,
        fence: MAX_EXACT_INTEGER,
        code: "return \"中\"\n".into(),
        code_bytes: 99,
        code_checksum: 1234,
        budget: 5,
        challenge: "5".repeat(32),
        prepared_nonce: "6".repeat(32),
        report_bytes: 12,
        report_checksum: 34,
        ..envelope()
    };
    let expected = concat!(
        r#"{"schema":"Lycheedev mailbox protocol v1","index":7,"startSlot":2,"runtime":"22222222222222222222222222222222","owner":"11111111111111111111111111111111","fence":9007199254740991,"nonce":"33333333333333333333333333333333","ticket":"44444444444444444444444444444444","action":"prepare","guid":"g","build":"b","code":"return \"中\"\n","codeBytes":99,"codeChecksum":1234,"budget":5,"challenge":"55555555555555555555555555555555","preparedNonce":"66666666666666666666666666666666","reportBytes":12,"reportChecksum":34}"#,
    );
    assert_eq!(serde_json::to_string(&value).unwrap(), expected);
    assert_eq!(
        serde_json::from_str::<SlotEnvelope>(expected).unwrap(),
        value
    );
}

#[test]
fn wire_mailbox_v1_shared_expected_envelope_keeps_identity_and_start_fallback() {
    let mut value = envelope();
    let expected: ExpectedEnvelope = value.expected();
    assert_eq!(
        expected,
        ExpectedEnvelope {
            schema: SCHEMA.into(),
            index: 7,
            start_slot: 0,
            runtime: "2".repeat(32),
            owner: "1".repeat(32),
            fence: 1,
            nonce: "3".repeat(32),
            ticket: "4".repeat(32),
            action: "prepare".into(),
            guid: "g".into(),
            build: "b".into(),
        }
    );
    assert_eq!(value.slot_start(), 7);
    assert_eq!(expected.slot_start(), 7);
    assert_eq!(serde_json::to_value(&expected).unwrap()["startSlot"], 0);
    value.start_slot = 2;
    assert_eq!(value.slot_start(), 2);
    assert_eq!(value.expected().slot_start(), 2);
}

#[test]
fn wire_mailbox_v1_shared_payload_exact_unicode_lua_bytes() {
    let value = SlotEnvelope {
        start_slot: 2,
        code: "return \"中\"\n--\\\0\u{7f}".into(),
        code_bytes: 999,
        code_checksum: 999,
        budget: 5,
        ..envelope()
    };
    let expected = r#"LycheeDevSlotEnvelope = {
  startSlot = 2,
  schema = "Lycheedev mailbox protocol v1",
  runtime = "22222222222222222222222222222222",
  owner = "11111111111111111111111111111111",
  nonce = "33333333333333333333333333333333",
  ticket = "44444444444444444444444444444444",
  action = "prepare",
  guid = "g",
  build = "b",
  code = "return \"\228\184\173\"\010--\\\000\127",
  challenge = "",
  preparedNonce = "",
  index = 7, fence = 1, codeBytes = 18, codeChecksum = 1153959565, budget = 5, reportBytes = 0, reportChecksum = 0,
}
"#;
    assert_eq!(slot_payload(&value).unwrap(), expected.as_bytes());
    assert_eq!(value.code_bytes, 999);
    assert_eq!(value.code_checksum, 999);
}

#[test]
fn wire_mailbox_v1_shared_payload_empty_code_preserves_supplied_summary() {
    let value = SlotEnvelope {
        code_bytes: 18,
        code_checksum: 1_153_959_565,
        report_bytes: 23,
        report_checksum: 45,
        ..envelope()
    };
    let expected = r#"LycheeDevSlotEnvelope = {
  schema = "Lycheedev mailbox protocol v1",
  runtime = "22222222222222222222222222222222",
  owner = "11111111111111111111111111111111",
  nonce = "33333333333333333333333333333333",
  ticket = "44444444444444444444444444444444",
  action = "prepare",
  guid = "g",
  build = "b",
  code = "",
  challenge = "",
  preparedNonce = "",
  index = 7, fence = 1, codeBytes = 18, codeChecksum = 1153959565, budget = 0, reportBytes = 23, reportChecksum = 45,
}
"#;
    assert_eq!(slot_payload(&value).unwrap(), expected.as_bytes());
}

#[test]
fn wire_mailbox_v1_shared_admission_preserves_envelope_and_token_boundaries() {
    let invalid = ContractError("bridge.invalid_slot_envelope");
    let mut value = envelope();
    for index in [0, SLOT_COUNT + 1] {
        value.index = index;
        assert_eq!(value.validate(), Err(invalid));
    }
    value.index = SLOT_COUNT;
    assert_eq!(value.validate(), Ok(()));
    for start in [-1, SLOT_COUNT + 1] {
        value.start_slot = start;
        assert_eq!(value.validate(), Err(invalid));
    }
    value.start_slot = SLOT_COUNT;
    value.fence = MAX_EXACT_INTEGER;
    assert_eq!(value.validate(), Ok(()));
    for fence in [0, MAX_EXACT_INTEGER + 1] {
        value.fence = fence;
        assert_eq!(value.validate(), Err(invalid));
    }
    value.fence = 1;
    value.code = "中".repeat(87_381).into();
    value.code.push_byte(b'x');
    assert_eq!(value.code.len(), 262_144);
    assert_eq!(value.validate(), Ok(()));
    value.code.push_byte(b'x');
    assert_eq!(slot_payload(&value), Err(invalid));
    value.code.clear();
    value.schema = "lycheedev.mailbox.v1".into();
    assert_eq!(value.validate(), Err(invalid));
    for field in ["runtime", "owner", "nonce", "ticket"] {
        let mut value = envelope();
        let token = match field {
            "runtime" => &mut value.runtime,
            "owner" => &mut value.owner,
            "nonce" => &mut value.nonce,
            "ticket" => &mut value.ticket,
            _ => unreachable!(),
        };
        *token = "A".repeat(32);
        assert_eq!(
            slot_payload(&value),
            Err(ContractError("bridge.invalid_slot_token"))
        );
    }
    // Static payload admission deliberately does not authorize any action.
    let value = SlotEnvelope {
        runtime: "0".repeat(32),
        owner: "0".repeat(32),
        nonce: "0".repeat(32),
        ticket: "0".repeat(32),
        action: String::new(),
        guid: String::new(),
        build: String::new(),
        budget: -1,
        ..envelope()
    };
    assert_eq!(value.validate(), Ok(()));
}

#[test]
fn wire_mailbox_v1_shared_token_and_checksum_primitives_keep_wire_values() {
    assert_eq!(parse_token(&"0".repeat(32)).unwrap(), ZERO_TOKEN);
    assert_eq!(parse_token(&"ab".repeat(16)).unwrap(), [0xab; 16]);
    for bad in [
        "a".repeat(31),
        "a".repeat(33),
        "A".repeat(32),
        "中".repeat(16),
    ] {
        assert_eq!(
            parse_token(&bad),
            Err(ContractError("live.channel_token_invalid"))
        );
    }
    // Independent Adler-32 values include both sides of the reduction chunk.
    assert_eq!(checksum(b""), 1);
    assert_eq!(checksum(b"Wikipedia"), 300_286_872);
    assert_eq!(checksum(&vec![255; 5552]), 4_052_720_524);
    assert_eq!(checksum(&vec![255; 5553]), 2_385_091_723);
}
