use super::*;
use serde_json::json;

pub(super) fn identity() -> Identity {
    Identity {
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        owner: "1".repeat(32),
        fence: 1,
        next_slot: 1,
        slots: 500,
        inventory: Some(500),
        input_state: SCHEMA.into(),
        character: "c".into(),
        realm: "r".into(),
        guid: "g".into(),
        build: "b".into(),
        product: "retail".into(),
        release: "3.2.0".into(),
    }
}
fn state() -> State {
    State {
        schema: CHANNEL_SCHEMA.into(),
        id: format!("CON-{}", "1".repeat(32)),
        owner: "1".repeat(32),
        identity: identity(),
        ..State::default()
    }
}
fn operation() -> Operation {
    Operation::prepared(
        format!("LMO-{}", "3".repeat(32)),
        "4".repeat(32),
        "return 1".into(),
        30,
        OperationPolicy::Opaque,
    )
}
fn envelope() -> SlotEnvelope {
    SlotEnvelope {
        schema: SCHEMA.into(),
        index: 1,
        start_slot: 1,
        runtime: "2".repeat(32),
        owner: "1".repeat(32),
        fence: 1,
        nonce: "5".repeat(32),
        ticket: "4".repeat(32),
        action: "prepare".into(),
        guid: "g".into(),
        build: "b".into(),
        ..SlotEnvelope::default()
    }
}
#[test]
fn go_mailbox_v1_state_roundtrip_and_explicit_phase_names() {
    let mut s = state();
    s.operation = Some(operation());
    s.transaction = Some(Transaction {
        release_wait_started_at_ms: 0,
        envelope: envelope(),
        phase: TransactionPhase::Published,
        receipt: None,
        escape_attempts: 0,
        input_observation: None,
    });
    assert!(s.validate().is_ok());
    let raw = serde_json::to_vec(&s).unwrap();
    let copy: State = serde_json::from_slice(&raw).unwrap();
    assert_eq!(s, copy);
    let value: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(value["transaction"]["phase"], "published");
    assert_eq!(value["operation"]["stage"], "prepared");
    assert_eq!(value["operation"]["policy"], "opaque");
    assert!(value.get("closed").is_none());
    assert!(value["operation"].get("resultBytes").is_none());
    for phase in ["RUN", "RECEIVED", "commit_sent", ""] {
        let mut bad = value.clone();
        bad["transaction"]["phase"] = json!(phase);
        assert!(serde_json::from_value::<State>(bad).is_err());
    }
}
#[test]
fn go_mailbox_v1_state_rejects_old_schema_bad_routes_and_unconfirmed_receipts() {
    for edit in 0..7 {
        let mut s = state();
        match edit {
            0 => s.schema = "lycheedev.channel.v1".into(),
            1 => s.id = "CON-other".into(),
            2 => s.owner = "F".repeat(32),
            3 => s.identity.slots = 200,
            4 => {
                s.closed = true;
                s.bound = true;
            }
            5 => s.archive = "not a digest".into(),
            _ => {
                s.operation = Some(operation());
                s.closed = true;
            }
        }
        assert!(s.validate().is_err(), "{edit}");
    }
    let mut s = state();
    let mut tx = Transaction {
        release_wait_started_at_ms: 0,
        envelope: envelope(),
        phase: TransactionPhase::Intent,
        receipt: None,
        escape_attempts: 0,
        input_observation: None,
    };
    tx.envelope.runtime = "9".repeat(32);
    s.transaction = Some(tx.clone());
    assert!(s.validate().is_err());
    tx.envelope.runtime = s.identity.runtime.clone();
    tx.phase = TransactionPhase::Received;
    s.transaction = Some(tx);
    assert!(s.validate().is_err());
}
#[test]
fn go_mailbox_v1_state_result_bytes_preserve_whitespace_html_and_binary_base64() {
    assert_eq!(
        serde_json::to_string(&ResultBytes(vec![0, 1, 2, 253, 254, 255])).unwrap(),
        "\"AAEC/f7/\""
    );
    assert_eq!(
        serde_json::from_str::<ResultBytes>("\"AAEC/f7/\"")
            .unwrap()
            .0,
        vec![0, 1, 2, 253, 254, 255]
    );
    let bytes = b" { \"ok\":true, \"text\":\"<tag>&\" }\n".to_vec();
    let mut s = state();
    let mut op = operation();
    op.stage = OperationStage::Complete;
    op.prepared_nonce = "5".repeat(32);
    op.challenge = "6".repeat(32);
    op.report_bytes = bytes.len() as u32;
    op.report_checksum = checksum(&bytes);
    op.result_bytes = ResultBytes(bytes.clone());
    op.cleanup_method = CleanupMethod::Released;
    s.operation = Some(op);
    assert!(s.validate().is_ok());
    let copy: State = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    assert_eq!(copy.operation.unwrap().result_bytes.0, bytes);
    s.operation.as_mut().unwrap().result_bytes.0.push(b' ');
    assert!(s.validate().is_err());
}
#[test]
fn go_mailbox_v1_state_legacy_result_rejected_even_null() {
    let raw = serde_json::to_value(operation()).unwrap();
    for legacy in [json!(null), json!({"ok":true}), json!("")] {
        let mut v = raw.clone();
        v["result"] = legacy;
        assert!(serde_json::from_value::<Operation>(v).is_err());
    }
    let mut future = raw;
    future["unrelatedFutureField"] = json!(1);
    assert!(serde_json::from_value::<Operation>(future).is_ok());
}
#[test]
fn go_mailbox_v1_state_unknown_input_and_recovery_time_survive_json() {
    let mut s = state();
    s.input = Some(InputAttempt {
        id: "7".repeat(32),
        exchange: "original exchange".into(),
        runtime: s.identity.runtime.clone(),
        kind: InputKind::Invoke,
        after_millis: 1000,
        recovery_after_millis: 1400,
        escape_attempts: 0,
        observation: None,
        outcome: None,
        after_diagnostic: None,
    });
    assert!(s.validate().is_ok());
    let restored: State = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
    let attempt = restored.input.unwrap();
    assert!(attempt.outcome.is_none());
    assert_eq!(attempt.recovery_after_millis, 1400);
    assert!(!super::super::may_prepare_again(attempt.outcome.as_ref()));
}
#[test]
fn go_mailbox_v1_state_durable_budget_shape_never_wraps_or_unexhausts() {
    let base = DurableBudget {
        started_at_ms: 1000,
        deadline_ms: 121000,
        last_observed_ms: 1000,
        ..DurableBudget::default()
    };
    assert!(base.validate().is_ok());
    for change in 0..5 {
        let mut b = base.clone();
        match change {
            0 => b.started_at_ms = 0,
            1 => b.deadline_ms = 1000,
            2 => b.deadline_ms = i64::MAX,
            3 => b.last_observed_ms = 121000,
            _ => b.clock_rollback = true,
        };
        assert!(b.validate().is_err());
    }
    let mut exhausted = base;
    exhausted.exhausted = true;
    exhausted.clock_rollback = true;
    assert!(exhausted.validate().is_ok());
}
#[test]
fn go_mailbox_v1_state_request_keys_are_utf8_bytes_and_no_controls() {
    assert!(validate_request("查询 123").is_ok());
    assert!(validate_request(&"x".repeat(256)).is_ok());
    for invalid in ["".into(), "a\nb".into(), "a\x7fb".into(), "中".repeat(86)] {
        assert!(validate_request(&invalid).is_err());
    }
}
#[test]
fn go_mailbox_v1_state_cleanup_stages_and_result_digest_are_independent() {
    let mut s = state();
    let mut op = operation();
    op.cleanup_method = CleanupMethod::Released;
    s.operation = Some(op.clone());
    assert!(s.validate().is_err());
    op.stage = OperationStage::ReleaseReady;
    op.cleanup_method = CleanupMethod::ReloadRequired;
    op.prepared_nonce = "5".repeat(32);
    op.challenge = "6".repeat(32);
    op.result_bytes = ResultBytes(br#"{"ok":false}"#.to_vec());
    op.report_bytes = op.result_bytes.0.len() as u32;
    op.report_checksum = checksum(&op.result_bytes.0);
    s.operation = Some(op);
    assert!(s.validate().is_ok()); // A verified report with ok=false is not lost.
    s.operation.as_mut().unwrap().report_checksum ^= 1;
    assert!(s.validate().is_err());
}

#[test]
fn go_mailbox_v1_state_raw_byte_reports_preserve_cap_checksum_and_base64() {
    let mut cap = vec![b'x'; MAX_PAYLOAD];
    cap[0] = b'"';
    cap[1] = 0xff;
    cap[MAX_PAYLOAD - 1] = b'"';
    for bytes in [b"{\"s\":\"\xff\",\"s\":1e400}".to_vec(), cap] {
        let mut s = state();
        let mut op = operation();
        op.stage = OperationStage::Complete;
        op.prepared_nonce = "5".repeat(32);
        op.challenge = "6".repeat(32);
        op.report_bytes = bytes.len() as u32;
        op.report_checksum = checksum(&bytes);
        op.result_bytes = ResultBytes(bytes.clone());
        op.cleanup_method = CleanupMethod::Released;
        s.operation = Some(op);
        s.validate().unwrap();
        let copy: State = serde_json::from_slice(&serde_json::to_vec(&s).unwrap()).unwrap();
        copy.validate().unwrap();
        assert_eq!(copy.operation.unwrap().result_bytes.0, bytes);
        s.operation.as_mut().unwrap().report_checksum ^= 1;
        assert!(s.validate().is_err());
    }
    let mut s = state();
    let mut op = operation();
    op.stage = OperationStage::Complete;
    op.prepared_nonce = "5".repeat(32);
    op.challenge = "6".repeat(32);
    op.result_bytes = ResultBytes([b"\"".as_slice(), &vec![b'x'; MAX_PAYLOAD - 1], b"\""].concat());
    op.report_bytes = op.result_bytes.0.len() as u32;
    op.report_checksum = checksum(&op.result_bytes.0);
    s.operation = Some(op);
    assert!(s.validate().is_err());
}
