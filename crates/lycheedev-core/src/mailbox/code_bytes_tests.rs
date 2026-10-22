//! Fixed Go encoding/json string and bridge/slot.go byte boundaries.
use super::*;

#[test]
fn code_bytes_keep_invalid_and_valid_replacement_bytes_distinct_in_memory() {
    let invalid = CodeBytes::from_bytes(vec![0xff, 0xfe]);
    let replacement = CodeBytes::from("\u{fffd}\u{fffd}");
    assert_ne!(invalid, replacement);
    assert_eq!(invalid.len(), 2);
    assert_eq!(replacement.len(), 6);
    assert_eq!(invalid.as_bytes(), &[0xff, 0xfe]);
}
#[test]
fn code_bytes_json_replaces_each_bad_byte_including_truncated_sequences() {
    for (bytes, expected) in [
        (vec![0xff, 0xfe], r#""\ufffd\ufffd""#),
        (vec![0xe2, 0x82], r#""\ufffd\ufffd""#),
        (vec![0xf0, 0x9f, 0x92], r#""\ufffd\ufffd\ufffd""#),
        (vec![0xed, 0xa0, 0x80], r#""\ufffd\ufffd\ufffd""#),
        (vec![0xc0, 0xaf], r#""\ufffd\ufffd""#),
    ] {
        let code = CodeBytes::from_bytes(bytes.clone());
        assert_eq!(serde_json::to_string(&code).unwrap(), expected);
        let restored: CodeBytes = serde_json::from_str(expected).unwrap();
        assert_eq!(restored.len(), bytes.len() * 3);
        assert_eq!(code.as_bytes(), bytes);
    }
}
#[test]
fn code_bytes_json_preserves_valid_unicode_nul_and_escaping() {
    let raw = [
        b'a', b'"', b'\\', 0, 0xff, 0xef, 0xbf, 0xbd, 0xe4, 0xb8, 0xad,
    ];
    let code = CodeBytes::from_bytes(raw.to_vec());
    assert_eq!(
        serde_json::to_string(&code).unwrap(),
        "\"a\\\"\\\\\\u0000\\ufffd\u{fffd}中\""
    );
    let restored: CodeBytes = serde_json::from_slice(&serde_json::to_vec(&code).unwrap()).unwrap();
    assert_eq!(restored.as_bytes(), "a\"\\\0\u{fffd}\u{fffd}中".as_bytes());
    assert_eq!(code.as_bytes(), raw);
}
#[test]
fn code_bytes_valid_utf8_serialization_remains_the_existing_string_contract() {
    for text in ["", "return 1", "中\u{fffd}", "\0\r\n\t\"\\", "<&>\u{2028}"] {
        assert_eq!(
            serde_json::to_vec(&CodeBytes::from(text)).unwrap(),
            serde_json::to_vec(text).unwrap()
        );
    }
}
#[test]
fn slot_payload_uses_original_code_byte_escapes_length_and_checksum() {
    let raw = vec![0xff, 0xfe, 0xe2, 0x82, 0, b'"', b'\\'];
    let envelope = SlotEnvelope {
        schema: SCHEMA.into(),
        index: 1,
        runtime: "1".repeat(32),
        owner: "2".repeat(32),
        fence: 1,
        nonce: "3".repeat(32),
        ticket: "4".repeat(32),
        action: "prepare".into(),
        code: raw.clone().into(),
        code_bytes: 900,
        code_checksum: 42,
        ..Default::default()
    };
    let payload = String::from_utf8(slot_payload(&envelope).unwrap()).unwrap();
    assert!(payload.contains("  code = \"\\255\\254\\226\\130\\000\\\"\\\\\",\n"));
    assert!(payload.contains(&format!("codeBytes = 7, codeChecksum = {}", checksum(&raw))));
    assert_eq!(envelope.code.as_bytes(), raw);
    assert_eq!(envelope.code_bytes, 900);
    assert_eq!(envelope.code_checksum, 42);
}
#[test]
fn slot_code_limit_measures_original_bytes_before_json_expansion() {
    let mut envelope = SlotEnvelope {
        schema: SCHEMA.into(),
        index: 1,
        runtime: "1".repeat(32),
        owner: "2".repeat(32),
        fence: 1,
        nonce: "3".repeat(32),
        ticket: "4".repeat(32),
        code: vec![0xff; 262144].into(),
        ..Default::default()
    };
    assert!(slot_payload(&envelope).is_ok());
    envelope.code.push_byte(0xff);
    assert_eq!(
        slot_payload(&envelope),
        Err(ContractError("bridge.invalid_slot_envelope"))
    );
}
