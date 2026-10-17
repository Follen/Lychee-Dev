//! Independent pre-optimization snapshot oracle. It deliberately retains the
//! complete JSON-copy/artifact-extraction algorithm, not State::clone.
use super::super::state::*;
use super::super::{
    InputBindings, InputDiagnostic, InputDisposition, InputObservation, InputOutcome, SCHEMA,
    checksum,
};
use super::*;

fn old_snapshot(state: &State) -> Result<Snapshot> {
    let bytes =
        serde_json::to_vec(state).map_err(|_| ContractError("live.channel_journal_invalid"))?;
    let mut copy: State = serde_json::from_slice(&bytes)
        .map_err(|_| ContractError("live.channel_journal_invalid"))?;
    copy.artifacts.clear();
    let mut artifacts = BTreeMap::new();
    let mut put = |key: &str, bytes: Vec<u8>| {
        if bytes.is_empty() {
            return;
        }
        let digest = hash(&bytes);
        copy.artifacts.insert(key.into(), digest.clone());
        artifacts.entry(digest).or_insert(bytes);
    };
    if let Some(o) = &mut copy.operation {
        put("code", std::mem::take(&mut o.code).into_bytes());
        put("result", std::mem::take(&mut o.result_bytes.0));
    }
    if let Some(t) = &mut copy.transaction {
        put(
            "transaction",
            std::mem::take(&mut t.envelope.code).into_bytes(),
        );
    }
    if let Some(t) = copy.recovery.as_mut().and_then(|r| r.transaction.as_mut()) {
        put(
            "recovery",
            std::mem::take(&mut t.envelope.code).into_bytes(),
        );
    }
    Ok(Snapshot {
        state: copy,
        artifacts,
    })
}

fn equivalent(state: &State) -> Snapshot {
    let original = serde_json::to_vec(state).unwrap();
    let expected = old_snapshot(state).unwrap();
    let actual = snapshot(state).unwrap();
    assert_eq!(actual.state, expected.state, "all typed metadata fields");
    assert_eq!(
        actual.artifacts, expected.artifacts,
        "exact artifact bytes and keys"
    );
    assert_eq!(
        serde_json::to_vec(&actual.state).unwrap(),
        serde_json::to_vec(&expected.state).unwrap(),
        "complete raw snapshot bytes"
    );
    assert_eq!(
        prepare_event(&[], "snapshot<&>\u{2028}", &actual.state).unwrap(),
        prepare_event(&[], "snapshot<&>\u{2028}", &expected.state).unwrap(),
        "complete journal event and chain hash"
    );
    assert_eq!(
        serde_json::to_vec(state).unwrap(),
        original,
        "borrowed original remains untouched"
    );
    actual
}

fn code_state(code: Vec<u8>) -> State {
    State {
        operation: Some(Operation::prepared_bytes(
            "operation".into(),
            "ticket".into(),
            code.into(),
            9,
            OperationPolicy::Observation,
        )),
        ..State::default()
    }
}

fn sample() -> InputObservation {
    InputObservation {
        bindings: Some(InputBindings {
            wake: "ALT-CTRL-F12".into(),
            reload: "omitted legacy reload".into(),
            submit: "legacy submit".into(),
            close: String::new(),
        }),
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        owner: "1".repeat(32),
        fence: 7,
        next_slot: 8,
        guid: "guid".into(),
        build: "build".into(),
        sample_millis: 3000,
        input_blocked: Some(false),
        slot_reload_required: true,
        reason: String::new(),
        escape_limit: 0,
        input_attempt: Some(InputDiagnostic {
            schema: SCHEMA.into(),
            runtime: "2".repeat(32),
            attempt_seq: 1,
            slot: 8,
            stage: "received".into(),
            reason: String::new(),
            received: true,
            sample_millis: 17,
        }),
        observed_at_millis: 9000,
    }
}

fn dense_state() -> State {
    let mut s = code_state(b"return '\xff\xe2\x82<&>\0'".to_vec());
    s.progress_version = u64::MAX;
    s.schema = CHANNEL_SCHEMA.into();
    s.id = "CON-dense".into();
    s.owner = "owner".into();
    s.bound = true;
    s.closed = true;
    s.closing = true;
    s.archive = "old archive".into();
    s.artifacts.insert("obsolete".into(), "old digest".into());
    s.identity = super::super::Identity {
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        owner: "1".repeat(32),
        fence: u64::MAX,
        next_slot: 99,
        slots: 500,
        inventory: Some(0),
        input_state: "input".into(),
        character: "荔枝".into(),
        realm: "realm".into(),
        guid: "guid".into(),
        build: "build".into(),
        product: "retail".into(),
        release: "3.5.0".into(),
    };
    let budget = DurableBudget {
        started_at_ms: 1,
        deadline_ms: 8,
        last_observed_ms: 9,
        exhausted: true,
        clock_rollback: true,
        legacy: true,
    };
    s.connect_budget = Some(budget.clone());
    s.close_budget = Some(budget.clone());
    s.blocker = Some(Blocker {
        kind: "blocked".into(),
        installation: "path".into(),
        slot: 7,
        consumer: "consumer".into(),
        runtime: "runtime".into(),
        nonce: "nonce".into(),
        condition: "condition".into(),
    });
    s.process_end = ProcessEnd::PidReused;
    s.runtime_end = Some(RuntimeReplacementProof {
        schema: SCHEMA.into(),
        process_id: 18,
        process_started_at: u64::MAX,
        current: s.identity.clone(),
        witness: RuntimeReplacementWitness {
            first: sample(),
            first_sequence: 31,
            address: u64::MAX,
            length: 33,
            before_sha256: "before".into(),
            after_sha256: "after".into(),
            sequence: 34,
            observation: sample(),
        },
    });
    let tx = Transaction {
        release_wait_started_at_ms: 35,
        envelope: SlotEnvelope {
            schema: SCHEMA.into(),
            index: 36,
            start_slot: 37,
            runtime: "runtime".into(),
            owner: "owner".into(),
            fence: u64::MAX,
            nonce: "nonce".into(),
            ticket: "ticket".into(),
            action: "prepare".into(),
            guid: "guid".into(),
            build: "build".into(),
            code: b"\xff\0\xe2\x82".as_slice().into(),
            code_bytes: 41,
            code_checksum: 42,
            budget: 43,
            challenge: "challenge".into(),
            prepared_nonce: "prepared".into(),
            report_bytes: 44,
            report_checksum: 45,
        },
        phase: TransactionPhase::InputAttempted,
        receipt: Some(super::super::Receipt {
            identity: s.identity.clone(),
            nonce: "receipt".into(),
            ticket: "ticket".into(),
            action: "prepare".into(),
            state: "reported".into(),
            reason: "reason".into(),
            prepared_nonce: "prepared".into(),
            challenge: "challenge".into(),
            report_bytes: 46,
            report_checksum: 47,
        }),
        escape_attempts: 48,
        input_observation: Some(sample()),
    };
    s.transaction = Some(tx.clone());
    s.recovery = Some(RuntimeRecovery {
        from: s.identity.clone(),
        transaction: Some(tx),
        phase: RecoveryPhase::Bound,
    });
    s.reload = Some(ReloadAttempt {
        automatic: true,
        slot_enablement: true,
        slot_budget_source: Some(SlotBudgetSource::Operation {
            operation_id: "operation".into(),
        }),
        recovery_budget: Some(budget.clone()),
        reconcile_pending: true,
        request: "reload".into(),
        from: "runtime".into(),
        phase: ReloadPhase::Binding,
        input_step: 49,
        patch_transitions: 50,
    });
    s.input = Some(InputAttempt {
        id: "input".into(),
        exchange: "exchange".into(),
        runtime: "runtime".into(),
        kind: InputKind::Reload,
        after_millis: 51,
        recovery_after_millis: 52,
        escape_attempts: 53,
        observation: Some(sample()),
        outcome: Some(InputOutcome {
            disposition: InputDisposition::Uncertain,
            messages_queued: 54,
            at_millis: 55,
            reason: "uncertain".into(),
            retryable: false,
        }),
        after_diagnostic: Some(InputDiagnostic::default()),
    });
    let o = s.operation.as_mut().unwrap();
    o.recovery_budget = Some(budget);
    o.cleanup_method = CleanupMethod::ReloadRequired;
    o.attempt = 56;
    o.request = "request".into();
    o.origin = Some(s.identity.clone());
    o.stage = OperationStage::ReleaseReady;
    o.prepared_nonce = "prepared".into();
    o.challenge = "challenge".into();
    o.result_bytes =
        ResultBytes(b" { \"s\":\"\xff\", \"x\":1e400,\"x\":-0,\"e\":\"\\u003c\" }\n".to_vec());
    o.report_bytes = o.result_bytes.0.len() as u32;
    o.report_checksum = checksum(&o.result_bytes.0);
    s
}

#[test]
fn go_mailbox_v1_snapshot_owned_projection_matches_full_json_reference_and_omissions() {
    equivalent(&State::default());
    let state = dense_state();
    let actual = equivalent(&state);
    let sample = actual
        .state
        .input
        .as_ref()
        .unwrap()
        .observation
        .as_ref()
        .unwrap();
    assert_eq!(sample.observed_at_millis, 0);
    assert!(sample.bindings.as_ref().unwrap().reload.is_empty());
    assert_eq!(
        sample.input_attempt.as_ref().unwrap().sample_millis,
        sample.sample_millis
    );
    assert!(
        actual
            .state
            .input
            .as_ref()
            .unwrap()
            .after_diagnostic
            .is_some(),
        "ordinary diagnostic is not filtered"
    );
    assert!(actual.state.artifacts.get("obsolete").is_none());
    assert_eq!(
        state
            .input
            .as_ref()
            .unwrap()
            .observation
            .as_ref()
            .unwrap()
            .observed_at_millis,
        9000
    );
}

#[test]
fn go_mailbox_v1_snapshot_every_invalid_utf8_byte_and_truncated_prefix_match_old_copy() {
    for byte in 0u8..=255 {
        equivalent(&code_state(vec![b'\'', byte, b'\'']));
    }
    for bytes in [
        vec![0xe2, 0x82],
        vec![0xf0, 0x9f, 0x8d],
        vec![0xed, 0xa0, 0x80],
        vec![0xc0, 0xaf],
        vec![0xff, 0xfe],
        "荔枝<&>\0\u{2028}\u{2029}".as_bytes().to_vec(),
    ] {
        equivalent(&code_state(bytes));
    }
    let bytes = b"a\xe2\x82";
    assert_eq!(&*json_code_bytes(bytes), "a\u{fffd}\u{fffd}".as_bytes());
    assert!(matches!(
        json_code_bytes(b"valid\0<&>"),
        std::borrow::Cow::Borrowed(_)
    ));
}

#[test]
fn go_mailbox_v1_snapshot_input_normalization_covers_every_nested_owner() {
    for invalid in 0..4 {
        let mut state = dense_state();
        let observations = [
            state
                .transaction
                .as_mut()
                .unwrap()
                .input_observation
                .as_mut()
                .unwrap(),
            state
                .recovery
                .as_mut()
                .unwrap()
                .transaction
                .as_mut()
                .unwrap()
                .input_observation
                .as_mut()
                .unwrap(),
            state.input.as_mut().unwrap().observation.as_mut().unwrap(),
        ];
        for o in observations {
            let d = o.input_attempt.as_mut().unwrap();
            match invalid {
                0 => d.runtime = "3".repeat(32),
                1 => d.attempt_seq = 0,
                2 => d.reason = "x".repeat(513),
                _ => d.sample_millis = -1,
            }
        }
        let proof = state.runtime_end.as_mut().unwrap();
        proof
            .witness
            .first
            .input_attempt
            .as_mut()
            .unwrap()
            .runtime
            .clear();
        proof
            .witness
            .observation
            .input_attempt
            .as_mut()
            .unwrap()
            .stage
            .clear();
        let copy = equivalent(&state);
        assert!(
            copy.state
                .input
                .unwrap()
                .observation
                .unwrap()
                .input_attempt
                .is_none()
        );
        assert!(
            copy.state
                .transaction
                .unwrap()
                .input_observation
                .unwrap()
                .input_attempt
                .is_none()
        );
        assert!(
            copy.state
                .recovery
                .unwrap()
                .transaction
                .unwrap()
                .input_observation
                .unwrap()
                .input_attempt
                .is_none()
        );
        let witness = copy.state.runtime_end.unwrap().witness;
        assert!(
            witness.first.input_attempt.is_none() && witness.observation.input_attempt.is_none()
        );
    }
}

#[test]
fn go_mailbox_v1_snapshot_large_artifacts_deduplicate_without_copying_into_metadata() {
    let mut state = dense_state();
    let code = vec![b'x'; 262144];
    state.operation.as_mut().unwrap().code = code.clone().into();
    state.transaction.as_mut().unwrap().envelope.code = code.clone().into();
    state
        .recovery
        .as_mut()
        .unwrap()
        .transaction
        .as_mut()
        .unwrap()
        .envelope
        .code = code.into();
    state.operation.as_mut().unwrap().result_bytes = ResultBytes(vec![0xff; MAX_PAYLOAD]);
    let copy = equivalent(&state);
    assert_eq!(copy.artifacts.len(), 2);
    let op = copy.state.operation.unwrap();
    assert_eq!(op.code.into_bytes().capacity(), 0);
    assert_eq!(op.result_bytes.0.capacity(), 0);
    assert_eq!(
        copy.state
            .transaction
            .unwrap()
            .envelope
            .code
            .into_bytes()
            .capacity(),
        0
    );
    assert_eq!(
        copy.state
            .recovery
            .unwrap()
            .transaction
            .unwrap()
            .envelope
            .code
            .into_bytes()
            .capacity(),
        0
    );
    assert_eq!(
        copy.state.artifacts["code"],
        copy.state.artifacts["transaction"]
    );
    assert_eq!(
        copy.state.artifacts["code"],
        copy.state.artifacts["recovery"]
    );
}

#[test]
fn go_mailbox_v1_snapshot_oversized_report_keeps_old_roundtrip_error() {
    let mut state = code_state(Vec::new());
    equivalent(&state);
    state.operation.as_mut().unwrap().result_bytes = ResultBytes(vec![0; MAX_PAYLOAD + 1]);
    assert_eq!(
        snapshot(&state).unwrap_err(),
        old_snapshot(&state).unwrap_err()
    );
    assert_eq!(
        snapshot(&state).unwrap_err(),
        ContractError("live.channel_journal_invalid")
    );
}
