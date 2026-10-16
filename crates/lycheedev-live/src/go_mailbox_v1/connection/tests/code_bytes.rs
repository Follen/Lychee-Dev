//! Real owned journal files exercise raw source, JSON-copy and resumed identity.
//! Protocol/OS effects remain modeled; no game or public CLI execution claimed.
use super::*;
use crate::go_mailbox_v1::journal::{hydrate, snapshot};
use crate::go_mailbox_v1::state::CodeBytes;

// Unlike the recovery-only Peer, this fixture explicitly admits the selected
// publication action, records its exact envelope, and stops before any input.
struct PublicationStop {
    action: &'static str,
    error: ChannelError,
    envelopes: Vec<SlotEnvelope>,
}
impl PublicationStop {
    fn prepare() -> Self {
        Self {
            action: "prepare",
            error: ChannelError::Pending { causes: vec![] },
            envelopes: vec![],
        }
    }
    fn release() -> Self {
        Self {
            action: "release",
            error: error("fixture.release_intent_observed"),
            envelopes: vec![],
        }
    }
}
impl Backend for PublicationStop {
    fn publish(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()> {
        check_control(control)?;
        assert_eq!(envelope.action, self.action);
        lycheedev_core::mailbox::slot_payload(envelope)?;
        self.envelopes.push(envelope.clone());
        Err(self.error.clone())
    }
    fn observe(&mut self, _: &OperationControl, _: &ObservationQuery) -> ObservationDispatch {
        panic!("stopped publication must not observe")
    }
    fn observe_input(
        &mut self,
        _: &OperationControl,
        _: &SlotEnvelope,
        _: i64,
        _: &str,
    ) -> ChannelResult<InputObservation> {
        panic!("stopped publication must not observe input")
    }
    fn input(&mut self, _: &OperationControl, _: &InputAction) -> InputDispatch {
        panic!("stopped publication must not send input")
    }
    fn runtime_candidate(
        &mut self,
        _: &OperationControl,
        _: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        panic!("stopped publication must not discover runtime")
    }
    fn supersede(
        &mut self,
        _: &OperationControl,
        _: &SlotEnvelope,
        _: &Identity,
    ) -> ChannelResult<()> {
        panic!("stopped publication must not supersede")
    }
    fn consumed(&mut self, _: &OperationControl, _: &SlotEnvelope) -> ChannelResult<()> {
        panic!("stopped publication must not retire")
    }
}

fn prepared(raw: &[u8]) -> Operation {
    let mut operation = Operation::prepared_bytes(
        format!("LMO-{}", "4".repeat(32)),
        "5".repeat(32),
        CodeBytes::from(raw),
        5,
        OperationPolicy::Opaque,
    );
    operation.request = "bytes-request".into();
    operation
}
fn byte_request(code: &[u8]) -> ExecuteRequest<'_> {
    ExecuteRequest {
        request: "bytes-request",
        code,
        budget_seconds: 5,
        policy: OperationPolicy::Opaque,
    }
}

#[test]
fn prepare_keeps_raw_code_while_owned_journal_json_copy_normalizes_only_saved_bytes() {
    let f = Fixture::new();
    f.edit(|state| state.bound = true);
    let (record, journal) = f.book().acquire(&f.id, &control()).unwrap();
    let mut driver = Driver::from_state(
        record.state,
        Peer::default(),
        journal,
        FakeClock(Rc::new(Cell::new(1000))),
        Tokens(90),
    )
    .unwrap();
    let raw = [0xff, 0xfe];
    driver
        .prepare_request(&control(), "bytes-request", raw, 5, OperationPolicy::Opaque)
        .unwrap();
    assert_eq!(
        driver.state.operation.as_ref().unwrap().code.as_bytes(),
        raw
    );
    let copy = snapshot(&driver.state).unwrap();
    let digest = copy.state.artifacts.get("code").unwrap();
    assert_eq!(copy.artifacts[digest], "\u{fffd}\u{fffd}".as_bytes());
    assert_eq!(
        fs::read(record.paths.artifact_directory().join(digest)).unwrap(),
        copy.artifacts[digest]
    );
    assert!(
        driver
            .historical_request(&control(), "bytes-request", raw, 5, OperationPolicy::Opaque)
            .unwrap_err()
            .to_string()
            .contains("live.channel_request_conflict")
    );
    assert!(
        driver
            .historical_request(
                &control(),
                "bytes-request",
                "\u{fffd}\u{fffd}",
                5,
                OperationPolicy::Opaque
            )
            .unwrap()
            .is_some()
    );
    driver.journal.close().unwrap();
    let restored = f.book().status(&f.id, &control()).unwrap();
    assert_eq!(
        restored.state.operation.as_ref().unwrap().code.as_bytes(),
        "\u{fffd}\u{fffd}".as_bytes()
    );
    assert!(
        historical(&restored, byte_request(&raw), &control())
            .unwrap_err()
            .to_string()
            .contains("live.channel_request_conflict")
    );
    assert!(
        historical(
            &restored,
            byte_request("\u{fffd}\u{fffd}".as_bytes()),
            &control()
        )
        .unwrap()
        .is_some()
    );
}

#[test]
fn completed_byte_history_compares_before_native_open_without_lossy_aliasing() {
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "bytes-request");
    f.edit(|state| state.operation.as_mut().unwrap().code = vec![0xff, 0xfe].into());
    let before = fs::read(f.log()).unwrap();
    let mut service = f.service();
    service.environment.open_error = Some(error("fixture.native_must_not_open"));
    let conflict = service.execute(&f.id, byte_request(&[0xff, 0xfe]), &control());
    assert!(code(&conflict, "live.channel_request_conflict"));
    let completed = service.execute(
        &f.id,
        byte_request("\u{fffd}\u{fffd}".as_bytes()),
        &control(),
    );
    assert!(completed.error().is_none(), "{:?}", completed.error());
    assert!(completed.into_report().unwrap().complete);
    assert_eq!(service.environment.counts.borrow().opens, 0);
    assert_eq!(fs::read(f.log()).unwrap(), before);
}

#[test]
fn actual_driver_transaction_publishes_original_bytes_before_a_later_restore() {
    let f = Fixture::new();
    f.edit(|state| state.bound = true);
    let (record, journal) = f.book().acquire(&f.id, &control()).unwrap();
    let mut driver = Driver::from_state(
        record.state,
        PublicationStop::prepare(),
        journal,
        FakeClock(Rc::new(Cell::new(1000))),
        Tokens(90),
    )
    .unwrap();
    let raw = [0xe2, 0x82, 0xff, 0];
    driver
        .prepare_request(&control(), "bytes-request", raw, 5, OperationPolicy::Opaque)
        .unwrap();
    assert!(driver.run(&control()).unwrap_err().is_pending());
    assert_eq!(driver.backend.envelopes.len(), 1);
    assert_eq!(driver.backend.envelopes[0].code.as_bytes(), raw);
    let envelope = &driver.state.transaction.as_ref().unwrap().envelope;
    assert_eq!(envelope.code.as_bytes(), raw);
    assert_eq!(envelope.code_bytes, 4);
    assert_eq!(envelope.code_checksum, super::super::super::checksum(&raw));
    let payload =
        String::from_utf8(lycheedev_core::mailbox::slot_payload(envelope).unwrap()).unwrap();
    assert!(payload.contains("  code = \"\\226\\130\\255\\000\",\n"));
    driver.journal.close().unwrap();
    let saved = f.state();
    let restored = &saved.transaction.as_ref().unwrap().envelope;
    assert_eq!(
        restored.code.as_bytes(),
        "\u{fffd}\u{fffd}\u{fffd}\0".as_bytes()
    );
    // JSON cloning changes only Code, never silently rewrites stored correlation
    // fields. SlotPayload recomputes codeBytes/checksum from restored Code.
    assert_eq!(restored.code_bytes, 4);
    let payload =
        String::from_utf8(lycheedev_core::mailbox::slot_payload(restored).unwrap()).unwrap();
    assert!(payload.contains(&format!(
        "codeBytes = 10, codeChecksum = {}",
        super::super::super::checksum(restored.code.as_bytes())
    )));
}

#[test]
fn code_admission_uses_raw_length_and_rejects_empty_oversize_or_bytecode_before_save() {
    for raw in [vec![], vec![b'x'; 262145], vec![27, b'L', b'u', b'a']] {
        let f = Fixture::new();
        f.edit(|state| state.bound = true);
        let before = fs::read(f.log()).unwrap();
        let (record, journal) = f.book().acquire(&f.id, &control()).unwrap();
        let mut driver = Driver::from_state(
            record.state,
            Peer::default(),
            journal,
            FakeClock(Rc::new(Cell::new(1000))),
            Tokens(90),
        )
        .unwrap();
        let error = driver
            .prepare_request(&control(), "bytes-request", raw, 5, OperationPolicy::Opaque)
            .unwrap_err();
        assert!(error.to_string().contains("live.channel_request_invalid"));
        assert!(driver.state.operation.is_none());
        driver.journal.close().unwrap();
        assert_eq!(fs::read(f.log()).unwrap(), before);
    }
}

#[test]
fn go_first_write_can_exceed_artifact_read_bound_then_restore_and_existing_write_fail() {
    let f = Fixture::new();
    f.edit(|state| state.bound = true);
    let (record, journal) = f.book().acquire(&f.id, &control()).unwrap();
    let mut driver = Driver::from_state(
        record.state,
        Peer::default(),
        journal,
        FakeClock(Rc::new(Cell::new(1000))),
        Tokens(90),
    )
    .unwrap();
    let raw = vec![0xff; 262144];
    driver
        .prepare_request(
            &control(),
            "bytes-request",
            &raw,
            5,
            OperationPolicy::Opaque,
        )
        .unwrap();
    assert_eq!(
        driver.state.operation.as_ref().unwrap().code.as_bytes(),
        raw
    );
    let copy = snapshot(&driver.state).unwrap();
    let digest = copy.state.artifacts["code"].clone();
    assert_eq!(copy.artifacts[&digest].len(), 786432);
    let path = record.paths.artifact_directory().join(digest);
    assert_eq!(fs::metadata(&path).unwrap().len(), 786432);
    let after = fs::read(record.paths.log_path()).unwrap();
    let error = save_state(
        &mut driver.journal,
        &control(),
        &mut driver.state,
        "same_existing",
    )
    .unwrap_err();
    assert!(error.to_string().contains("live.channel_artifact_invalid"));
    driver.journal.close().unwrap();
    assert_eq!(fs::read(record.paths.log_path()).unwrap(), after);
    let error = f.book().status(&f.id, &control()).unwrap_err();
    assert!(error.to_string().contains("live.channel_artifact_invalid"));
    assert_eq!(fs::metadata(path).unwrap().len(), 786432);
}

#[test]
fn json_control_escapes_do_not_expand_the_decoded_artifact_limit() {
    let f = Fixture::new();
    let (mut record, mut journal) = f.book().acquire(&f.id, &control()).unwrap();
    record.state.operation = Some(prepared(&vec![0; 262144]));
    let encoded = serde_json::to_vec(&record.state).unwrap();
    assert!(encoded.len() > 6 * 262144);
    save_state(&mut journal, &control(), &mut record.state, "control_limit").unwrap();
    journal.close().unwrap();
    assert_eq!(
        f.state().operation.unwrap().code.as_bytes(),
        vec![0; 262144]
    );
}

#[test]
fn existing_small_artifacts_are_verified_and_never_overwritten() {
    for corrupt in [false, true] {
        let f = Fixture::new();
        let (mut record, mut journal) = f.book().acquire(&f.id, &control()).unwrap();
        record.state.operation = Some(prepared(&[0xff, 0xfe]));
        let copy = snapshot(&record.state).unwrap();
        let digest = &copy.state.artifacts["code"];
        let path = record.paths.artifact_directory().join(digest);
        fs::create_dir_all(record.paths.artifact_directory()).unwrap();
        let bytes = if corrupt {
            b"corrupt".to_vec()
        } else {
            copy.artifacts[digest].clone()
        };
        fs::write(&path, &bytes).unwrap();
        let before = fs::read(record.paths.log_path()).unwrap();
        let result = save_state(
            &mut journal,
            &control(),
            &mut record.state,
            "existing_artifact",
        );
        if corrupt {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("live.channel_artifact_corrupt")
            );
            assert_eq!(fs::read(record.paths.log_path()).unwrap(), before);
        } else {
            result.unwrap();
        }
        journal.close().unwrap();
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn hydrate_accepts_exact_authenticated_non_utf8_code_artifact_without_normalizing() {
    let f = Fixture::new();
    let mut state = f.state();
    state.operation = Some(prepared(b"placeholder"));
    let mut copy = snapshot(&state).unwrap();
    let raw = vec![0xff, 0xfe, 0xe2, 0x82];
    let digest = lycheedev_core::storage::sha256(&raw);
    copy.state.artifacts.insert("code".into(), digest.clone());
    let restored = hydrate(copy.state, |requested| {
        assert_eq!(requested, digest);
        Ok(raw.clone())
    })
    .unwrap();
    assert_eq!(restored.operation.unwrap().code.as_bytes(), raw);
}

#[test]
fn hydrate_transaction_and_recovery_code_are_raw_go_strings_too() {
    use crate::go_mailbox_v1::state::{RecoveryPhase, RuntimeRecovery};
    let f = Fixture::new();
    for recovery in [false, true] {
        let mut state = f.state();
        let mut from = state.identity.clone();
        if recovery {
            from.runtime = "1".repeat(32);
        }
        let envelope = SlotEnvelope {
            schema: SCHEMA.into(),
            index: 1,
            runtime: from.runtime.clone(),
            owner: state.owner.clone(),
            fence: 1,
            nonce: "7".repeat(32),
            ticket: "8".repeat(32),
            action: "prepare".into(),
            guid: from.guid.clone(),
            build: from.build.clone(),
            code: "placeholder".into(),
            ..Default::default()
        };
        let transaction = Transaction {
            envelope,
            phase: TransactionPhase::Intent,
            receipt: None,
            escape_attempts: 0,
            input_observation: None,
            release_wait_started_at_ms: 0,
        };
        if recovery {
            state.recovery = Some(RuntimeRecovery {
                from,
                transaction: Some(transaction),
                phase: RecoveryPhase::Binding,
            });
        } else {
            state.transaction = Some(transaction);
        }
        state.validate().unwrap();
        let mut copy = snapshot(&state).unwrap();
        let raw = vec![0xff, 0xfe];
        let digest = lycheedev_core::storage::sha256(&raw);
        copy.state.artifacts.insert(
            if recovery { "recovery" } else { "transaction" }.into(),
            digest.clone(),
        );
        let restored = hydrate(copy.state, |requested| {
            assert_eq!(requested, digest);
            Ok(raw.clone())
        })
        .unwrap();
        let envelope = if recovery {
            restored.recovery.unwrap().transaction.unwrap().envelope
        } else {
            restored.transaction.unwrap().envelope
        };
        assert_eq!(envelope.code.as_bytes(), raw);
    }
}

#[test]
fn decoded_code_expansion_below_artifact_limit_can_still_fail_restored_state_limit() {
    let f = Fixture::new();
    let (mut record, mut journal) = f.book().acquire(&f.id, &control()).unwrap();
    record.state.operation = Some(prepared(&vec![0xff; 87382]));
    record.state.validate().unwrap();
    let copy = snapshot(&record.state).unwrap();
    assert_eq!(copy.artifacts[&copy.state.artifacts["code"]].len(), 262146);
    save_state(&mut journal, &control(), &mut record.state, "expanded_code").unwrap();
    journal.close().unwrap();
    let error = f.book().status(&f.id, &control()).unwrap_err();
    assert!(error.to_string().contains("live.channel_journal_invalid"));
}

fn verified_operation(raw: &[u8]) -> Operation {
    let mut operation = prepared(raw);
    operation.stage = OperationStage::ResultVerified;
    operation.prepared_nonce = "6".repeat(32);
    operation.challenge = "7".repeat(32);
    operation.result_bytes =
        ResultBytes(br#"{ "ok":true,"n":9007199254740993,"x":1,"x":2 }"#.to_vec());
    operation.report_bytes = operation.result_bytes.0.len() as u32;
    operation.report_checksum = crate::go_mailbox_v1::checksum(&operation.result_bytes.0);
    operation
}

#[test]
fn actual_driver_writes_verified_byte_code_result_then_release_intent_without_poisoning() {
    let f = Fixture::new();
    f.edit(|state| state.bound = true);
    let (record, journal) = f.book().acquire(&f.id, &control()).unwrap();
    let mut driver = Driver::from_state(
        record.state,
        PublicationStop::release(),
        journal,
        FakeClock(Rc::new(Cell::new(1000))),
        Tokens(90),
    )
    .unwrap();
    let raw = b"--\xff\nreturn 1";
    driver
        .prepare_request(&control(), "bytes-request", raw, 5, OperationPolicy::Opaque)
        .unwrap();
    // Seed already-verified protocol facts, then traverse the production Driver
    // ResultVerified -> standalone result write -> ReleaseReady -> release intent.
    // This does not claim a modeled receipt is actual Windows/game evidence.
    let verified = verified_operation(raw);
    let operation = driver.state.operation.as_mut().unwrap();
    operation.stage = verified.stage;
    operation.prepared_nonce = verified.prepared_nonce;
    operation.challenge = verified.challenge;
    operation.result_bytes = verified.result_bytes;
    operation.report_bytes = verified.report_bytes;
    operation.report_checksum = verified.report_checksum;
    driver.save(&control(), "fixture_result_verified").unwrap();
    let original = driver.state.operation.as_ref().unwrap().clone();
    let result_path = record
        .paths
        .result_directory()
        .join(format!("{}.json", original.id));
    assert!(!result_path.exists());
    let error = driver.run(&control()).unwrap_err();
    assert_eq!(error.to_string(), "fixture.release_intent_observed");
    let live = driver.state.operation.as_ref().unwrap();
    assert_eq!(live.code.as_bytes(), raw);
    assert_eq!(live.stage, OperationStage::ReleaseReady);
    assert_eq!(live.result_bytes, original.result_bytes);
    assert_eq!(live.report_bytes, original.report_bytes);
    assert_eq!(live.report_checksum, original.report_checksum);
    assert_eq!(driver.backend.envelopes.len(), 1);
    assert_eq!(driver.backend.envelopes[0].action, "release");
    assert_eq!(
        driver.state.transaction.as_ref().unwrap().phase,
        TransactionPhase::Intent
    );
    let result_file = fs::read(&result_path).unwrap();
    let persisted: Operation = serde_json::from_slice(&result_file).unwrap();
    assert_eq!(persisted.stage, OperationStage::ResultVerified);
    assert_eq!(persisted.code.as_bytes(), "--\u{fffd}\nreturn 1".as_bytes());
    assert_eq!(persisted.result_bytes, original.result_bytes);
    assert_eq!(persisted.report_checksum, original.report_checksum);
    assert_eq!(result_file, durable_journal::json(&original).unwrap());
    // The journal remains usable. A spurious result-invalid failure would poison
    // this history read and prevent the real result_durable/release intent saves.
    let history = driver
        .historical_request(
            &control(),
            "bytes-request",
            "--\u{fffd}\nreturn 1",
            5,
            OperationPolicy::Opaque,
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        history.operation.as_ref().unwrap().stage,
        OperationStage::ReleaseReady
    );
    let chain =
        crate::go_mailbox_v1::journal::read_log_bytes(&fs::read(record.paths.log_path()).unwrap());
    assert!(chain.failure.is_none());
    let kinds = chain
        .events
        .iter()
        .map(|event| event.kind.as_str())
        .collect::<Vec<_>>();
    let verified_at = kinds
        .iter()
        .position(|kind| *kind == "fixture_result_verified")
        .unwrap();
    let durable_at = kinds
        .iter()
        .position(|kind| *kind == "result_durable")
        .unwrap();
    let release_at = kinds
        .iter()
        .rposition(|kind| *kind == "slot_intent")
        .unwrap();
    assert!(verified_at < durable_at && durable_at < release_at);
    driver.journal.close().unwrap();
    let restored = f.state();
    let operation = restored.operation.unwrap();
    assert_eq!(operation.code.as_bytes(), "--\u{fffd}\nreturn 1".as_bytes());
    assert_eq!(operation.result_bytes, original.result_bytes);
    assert_eq!(operation.report_checksum, original.report_checksum);
    assert_eq!(operation.stage, OperationStage::ReleaseReady);
    assert_eq!(restored.transaction.unwrap().envelope.action, "release");
}

#[test]
fn result_persistent_code_normalization_does_not_weaken_other_operation_identity() {
    for field in 0..11 {
        let f = Fixture::new();
        let (mut record, mut journal) = f.book().acquire(&f.id, &control()).unwrap();
        record.state.bound = true;
        let mut operation = verified_operation(b"--\xff\nreturn 1");
        operation.origin = Some(record.state.identity.clone());
        record.state.operation = Some(operation.clone());
        save_state(
            &mut journal,
            &control(),
            &mut record.state,
            "verified_identity",
        )
        .unwrap();
        match field {
            0 => operation.id = format!("LMO-{}", "9".repeat(32)),
            1 => operation.ticket = "9".repeat(32),
            2 => operation.request = "different-request".into(),
            3 => operation.policy = OperationPolicy::Observation,
            4 => operation.budget = 6,
            5 => operation.prepared_nonce = "9".repeat(32),
            6 => operation.challenge = "9".repeat(32),
            7 => operation.origin.as_mut().unwrap().runtime = "9".repeat(32),
            8 => {
                operation.result_bytes = ResultBytes(br#"{"ok":false}"#.to_vec());
                operation.report_bytes = operation.result_bytes.0.len() as u32;
                operation.report_checksum =
                    crate::go_mailbox_v1::checksum(&operation.result_bytes.0);
            }
            9 => operation.code = b"--different\nreturn 1".as_slice().into(),
            10 => operation.attempt = 2,
            _ => unreachable!(),
        }
        let before = fs::read(record.paths.log_path()).unwrap();
        let error = journal.write_result(&control(), &operation).unwrap_err();
        assert!(
            error.to_string().contains("live.channel_result_invalid"),
            "field {field}: {error}"
        );
        assert!(
            !record
                .paths
                .result_directory()
                .join(format!("{}.json", operation.id))
                .exists()
        );
        assert_eq!(fs::read(record.paths.log_path()).unwrap(), before);
        assert!(
            journal
                .load(&control())
                .unwrap_err()
                .to_string()
                .contains("live.channel_journal_reload_required")
        );
        journal.close().unwrap();
    }
}
