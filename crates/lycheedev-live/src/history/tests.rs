use super::*;
use serde_json::{Value, json};
fn decode(raw: &str) -> Result<Value> {
    Ok(serde_json::to_value(decode_bootstrap(
        raw.as_bytes(),
        "BTP-old",
        &OperationControl::default(),
    )?)?)
}
#[test]
fn history_bootstrap_defaults_are_historical_not_input_authority() {
    let value = decode(r#"{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old"}"#).unwrap();
    assert_eq!(value["phase"], "");
    assert_eq!(value["target"]["window"]["handle"], "0");
    assert_eq!(value["receiver"]["attempt"], 0);
    assert_eq!(value["createdAt"], "0001-01-01T00:00:00Z");
    assert_eq!(
        value["bindings"],
        json!({"wakeBinding":"","submitBinding":"","closeBinding":""})
    );
    assert!(value.get("business").is_none());
    assert!(value.get("claimed").is_none());
}
#[test]
fn history_bootstrap_duplicate_null_nested_and_unknown_match_go() {
    let value = decode(r#"{"Schema":"lycheedev.bootstrap-receiver.v1","ID":"BTP-old","phase":"unknown","phase":null,"target":{"window":{"handle":"18446744073709551615"}},"TARGET":{"window":{"processId":4}},"receiver":{"attempt":-0,"phase":"queued"},"receiver":null,"futureField":{"doNotReplay":true},"business":{"kind":"identity"},"business":{"release":"3.2.0"},"createdAt":"2026-10-15T02:00:00.120000000Z"}"#).unwrap();
    assert_eq!(value["phase"], "unknown");
    assert_eq!(value["target"]["window"]["handle"], "18446744073709551615");
    assert_eq!(value["target"]["window"]["processId"], 4);
    assert_eq!(value["receiver"]["phase"], "queued");
    assert_eq!(value["business"]["kind"], "identity");
    assert_eq!(value["business"]["release"], "3.2.0");
    assert_eq!(value["createdAt"], "2026-10-15T02:00:00.12Z");
    assert!(value.get("futureField").is_none());
}
#[test]
fn history_bootstrap_invalid_identity_types_and_times_are_corrupt() {
    for extra in [
        r#""schema":"wrong""#,
        r#""id":"BTP-other""#,
        r#""receiver":{"attempt":1.0}"#,
        r#""target":{"window":{"handle":1}}"#,
        r#""target":{"window":{"handle":"+1"}}"#,
        r#""priorReadyEpoch":-1"#,
        r#""createdAt":"2026-10-15 02:00:00Z""#,
        r#""createdAt":"2026-10-15T02:00:00\u005a""#,
        r#""claimed":"false""#,
    ] {
        let raw =
            format!(r#"{{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old",{extra}}}"#);
        let error = decode(&raw).unwrap_err();
        assert_eq!(error.message, "live.bootstrap_attempt_corrupt", "{raw}");
        assert_eq!(error.exit_code, 5);
    }
}
#[test]
fn history_bootstrap_pointer_and_slice_nulls_clear_previous_values() {
    let value = decode(r#"{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old","business":{"kind":"ready"},"business":null,"history":[null,{}],"history":null}"#).unwrap();
    assert!(value.get("business").is_none());
    assert!(value.get("history").is_none());
}
#[test]
fn history_bootstrap_read_honors_original_cancel_and_byte_bound() {
    use lycheedev_core::control::{OperationControl, TrackedCancellation};
    let cancellation = TrackedCancellation::new();
    cancellation.cancel();
    let control = OperationControl::tracked(
        std::time::Instant::now() + std::time::Duration::from_secs(30),
        cancellation,
    );
    assert!(decode_bootstrap(b"{}", "BTP-old", &control).is_err());
    let large = vec![b' '; 1_048_577];
    assert_eq!(
        decode_bootstrap(&large, "BTP-old", &OperationControl::default())
            .unwrap_err()
            .message,
        "live.bootstrap_attempt_corrupt"
    );
}

#[test]
fn history_bootstrap_repeated_slice_reuses_slots_until_empty_or_null() {
    let value = decode(r#"{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old","history":[{"phase":"queued","attempt":2},{"phase":"second"}],"history":[{}],"history":[null,{}]}"#).unwrap();
    assert_eq!(value["history"][0]["phase"], "queued");
    assert_eq!(value["history"][0]["attempt"], 2);
    assert_eq!(value["history"][1]["phase"], "second");
    for reset in ["null", "[]"] {
        let raw = format!(
            r#"{{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old","history":[{{"phase":"queued"}}],"history":{reset},"history":[{{}}]}}"#
        );
        assert_eq!(decode(&raw).unwrap()["history"][0]["phase"], "");
    }
}

#[test]
fn history_bootstrap_raw_invalid_utf8_and_go_time_layout() {
    let raw =
        b"{\"schema\":\"lycheedev.bootstrap-receiver.v1\",\"id\":\"BTP-old\",\"phase\":\"\xff\"}";
    let value = serde_json::to_value(
        decode_bootstrap(raw, "BTP-old", &OperationControl::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(value["phase"], "\u{fffd}");
    let value = decode(r#"{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old","createdAt":"2026-10-15T2:00:00,120000000000Z"}"#).unwrap();
    assert_eq!(value["createdAt"], "2026-10-15T02:00:00.12Z");
    assert!(decode(r#"{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old","updatedAt":"2026-10-15T02:00:00z"}"#).is_err());
}

#[test]
fn history_bootstrap_unknown_json_depth_matches_go_scanner() {
    for (arrays, accepted) in [(9_999, true), (10_000, false)] {
        let raw = format!(
            r#"{{"schema":"lycheedev.bootstrap-receiver.v1","id":"BTP-old","unknown":{}0{}}}"#,
            "[".repeat(arrays),
            "]".repeat(arrays)
        );
        assert_eq!(decode(&raw).is_ok(), accepted);
    }
}
