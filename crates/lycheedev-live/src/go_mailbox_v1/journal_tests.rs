use super::super::state::{CHANNEL_SCHEMA, CleanupMethod, OperationPolicy, OperationStage};
use super::super::{Identity, SCHEMA, checksum};
use super::*;
fn state() -> State {
    State {
        schema: CHANNEL_SCHEMA.into(),
        id: format!("CON-{}", "1".repeat(32)),
        owner: "1".repeat(32),
        identity: Identity {
            schema: SCHEMA.into(),
            runtime: "2".repeat(32),
            owner: String::new(),
            fence: 0,
            next_slot: 1,
            slots: 500,
            inventory: None,
            input_state: SCHEMA.into(),
            character: "c".into(),
            realm: "r".into(),
            guid: "g".into(),
            build: "b".into(),
            product: "retail".into(),
            release: "3.2.0".into(),
        },
        ..State::default()
    }
}
#[test]
fn go_mailbox_v1_journal_snapshot_hydrates_exact_artifact_bytes() {
    let mut s = state();
    let mut op = Operation::prepared(
        format!("LMO-{}", "3".repeat(32)),
        "4".repeat(32),
        "return '<&>'".into(),
        30,
        OperationPolicy::Opaque,
    );
    op.stage = OperationStage::Complete;
    op.cleanup_method = CleanupMethod::Released;
    op.prepared_nonce = "5".repeat(32);
    op.challenge = "6".repeat(32);
    op.result_bytes = ResultBytes(b" { \"x\":\"<&>\" } \n".to_vec());
    op.report_bytes = op.result_bytes.0.len() as u32;
    op.report_checksum = checksum(&op.result_bytes.0);
    s.operation = Some(op);
    s.validate().unwrap();
    let snap = snapshot(&s).unwrap();
    assert!(snap.state.operation.as_ref().unwrap().code.is_empty());
    assert_eq!(snap.artifacts.len(), 2);
    assert!(snap.state.validate().is_err());
    let hydrated = hydrate(snap.state.clone(), |digest| {
        snap.artifacts
            .get(digest)
            .cloned()
            .ok_or(ContractError("missing"))
    })
    .unwrap();
    assert_eq!(hydrated, s);
    assert_eq!(
        hydrate(snap.state, |_| Ok(b"corrupt".to_vec())),
        Err(ContractError("live.channel_artifact_corrupt"))
    );
}
#[test]
fn go_mailbox_v1_journal_raw_compaction_keeps_order_numbers_and_go_html_escaping() {
    let raw = " { \"z\": 1.00, \"a\": \"<&>\u{2028}\u{2029}\", \"escaped\":\"\\u003c\" } ";
    assert_eq!(
        String::from_utf8(go_json_bytes(raw.as_bytes()).unwrap()).unwrap(),
        r#"{"z":1.00,"a":"\u003c\u0026\u003e\u2028\u2029","escaped":"\u003c"}"#
    );
}
#[test]
fn go_mailbox_v1_journal_chain_detects_corruption_and_explicit_torn_tail() {
    let s = state();
    let first = prepare_event(&[], "connect_intent", &s).unwrap();
    let read = read_log_bytes(&first);
    assert!(read.failure.is_none());
    assert_eq!(read.events.len(), 1);
    let second = prepare_event(&read.events, "budget_observed", &s).unwrap();
    let mut chain = first.clone();
    chain.extend(second);
    assert_eq!(read_log_bytes(&chain).events.len(), 2);
    chain.pop();
    let torn = read_log_bytes(&chain);
    assert_eq!(torn.events.len(), 1);
    assert_eq!(
        torn.failure,
        Some(ContractError("journal.memory_torn_tail"))
    );
    let mut corrupt = first;
    let index = corrupt.iter().position(|c| *c == b'1').unwrap();
    corrupt[index] = b'2';
    assert_eq!(
        read_log_bytes(&corrupt).failure,
        Some(ContractError("journal.memory_log_corrupt"))
    );
}
#[test]
fn go_mailbox_v1_journal_invalid_complete_line_is_never_torn_repair() {
    assert_eq!(
        read_log_bytes(b"{}\n").failure,
        Some(ContractError("journal.memory_log_corrupt"))
    );
    assert_eq!(
        read_log_bytes(b"{").failure,
        Some(ContractError("journal.memory_torn_tail"))
    );
    assert!(read_log_bytes(b"").failure.is_none());
    assert_eq!(
        read_log_bytes(&vec![b'x'; MAX_EVENT_BYTES + 1]).failure,
        Some(ContractError("journal.memory_log_limit"))
    );
}

// Golden provenance: standalone Go generator is checked in beside the fixture,
// with the exact fixed-Go MemoryEvent/memoryEventHash implementation. Initial
// bytes and SHA256 were independently constructed with Python hashlib from the
// Go contract, not this Rust implementation. Go regeneration is NOT_RUN locally
// (no Go compiler); CI must execute the generator and compare its JSON output.
// This test is intentionally independent of prepare_event/read_log self-roundtrip.
#[test]
fn go_mailbox_v1_journal_fixed_oracle_preimages_hashes_and_complete_chain() {
    let golden: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../tests/rust-parity/mailbox-journal-golden.json"
    ))
    .unwrap();
    assert_eq!(
        golden["oracleCommit"],
        "3057a9666ab078270b63c7f06de748069b0d1f4b"
    );
    let raw = [
        " { \"z\": 1.00, \"a\": \"<&>\u{2028}\u{2029}\", \"escaped\":\"\\u003c\" } ",
        " { \"n\": -0.0, \"nested\": { \"b\": 1e+02, \"a\": \"<&>\" } } ",
    ];
    let kinds = ["connect<&>\u{2028}\u{2029}", "budget_observed"];
    let mut predecessor = String::new();
    for index in 0..2 {
        let mut event = MemoryEvent {
            sequence: index as i64 + 1,
            previous: predecessor.clone(),
            kind: kinds[index].into(),
            data: serde_json::value::RawValue::from_string(raw[index].into()).unwrap(),
            checksum: String::new(),
        };
        let expected_hash = golden["checksums"][index].as_str().unwrap();
        assert_eq!(
            event_bytes(&event, true).unwrap(),
            golden["preimages"][index].as_str().unwrap().as_bytes()
        );
        assert_eq!(event_hash(&event).unwrap(), expected_hash);
        event.checksum = expected_hash.into();
        let mut line = event_bytes(&event, false).unwrap();
        line.push(b'\n');
        assert_eq!(line, golden["lines"][index].as_str().unwrap().as_bytes());
        predecessor = expected_hash.into();
    }
    let chain = golden["chain"].as_str().unwrap().as_bytes();
    let read = read_log_bytes(chain);
    assert!(read.failure.is_none());
    assert_eq!(read.events.len(), 2);
    assert_eq!(
        read.events[1].previous,
        golden["checksums"][0].as_str().unwrap()
    );
    assert_eq!(
        read.events[1].checksum,
        golden["checksums"][1].as_str().unwrap()
    );
    // Independently supplied predecessor is part of the hash, not decoration.
    let mut changed = read.events[1].clone();
    changed.previous = "0".repeat(64);
    assert_ne!(
        event_hash(&changed).unwrap(),
        golden["checksums"][1].as_str().unwrap()
    );
}

#[derive(Default)]
struct SaveJournal {
    appends: Vec<(String, State)>,
}
impl StateJournal for SaveJournal {
    fn append(
        &mut self,
        control: &OperationControl,
        event: &str,
        state: &State,
    ) -> ChannelResult<()> {
        check_control(control)?;
        self.appends.push((event.into(), state.clone()));
        Ok(())
    }
    fn historical_request(
        &mut self,
        _: &OperationControl,
        _: &str,
    ) -> ChannelResult<Option<State>> {
        panic!("save-only fixture must not inspect history")
    }
    fn write_result(&mut self, _: &OperationControl, _: &Operation) -> ChannelResult<()> {
        panic!("save-only fixture must not write a result")
    }
    fn checkpoint(&mut self, _: &OperationControl, _: &mut State, _: bool) -> ChannelResult<()> {
        panic!("save-only fixture must not rotate history")
    }
}

#[test]
fn go_mailbox_v1_journal_cancelled_save_preserves_go_progress_before_append() {
    let mut s = state();
    s.schema = "lycheedev.channel.v1".into();
    s.progress_version = 41;
    let mut journal = SaveJournal::default();
    let cancelled = OperationControl::default();
    cancelled
        .cancel_handle()
        .store(true, std::sync::atomic::Ordering::Release);
    assert_eq!(
        save_state(&mut journal, &cancelled, &mut s, "input_attempted"),
        Err(super::super::observations::ChannelError::Cancelled)
    );
    assert_eq!(s.schema, CHANNEL_SCHEMA);
    assert_eq!(s.progress_version, 42);
    assert!(journal.appends.is_empty());
    // The caller's independent bounded timeout-fact flush keeps the failed
    // business save's version; it does not claim that an earlier append occurred.
    save_state(
        &mut journal,
        &OperationControl::default(),
        &mut s,
        "budget_observed",
    )
    .unwrap();
    assert_eq!(journal.appends.len(), 1);
    assert_eq!(journal.appends[0].1.progress_version, 42);
}

#[test]
fn go_mailbox_v1_journal_cancelled_budget_and_blocker_saves_never_advance_progress() {
    for event in ["budget_observed", "budget_migrated", "blocker_observed"] {
        let mut s = state();
        s.progress_version = 41;
        let mut journal = SaveJournal::default();
        let cancelled = OperationControl::default();
        cancelled
            .cancel_handle()
            .store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            save_state(&mut journal, &cancelled, &mut s, event),
            Err(super::super::observations::ChannelError::Cancelled)
        );
        assert_eq!(s.progress_version, 41, "{event}");
        assert!(journal.appends.is_empty());
        save_state(&mut journal, &OperationControl::default(), &mut s, event).unwrap();
        assert_eq!(journal.appends[0].1.progress_version, 41, "{event}");
    }
}

#[test]
fn go_mailbox_v1_journal_prepared_event_returns_exact_bytes_and_existing_checksum() {
    let state = state();
    let first = prepare_event(&[], "<&>荔枝", &state).unwrap();
    let first_event: MemoryEvent = serde_json::from_slice(&first).unwrap();
    let prepared = prepare_event_at(1, &first_event.checksum, "next\n<&>", &state).unwrap();
    assert_eq!(
        prepared.bytes,
        prepare_event(&[first_event], "next\n<&>", &state).unwrap()
    );
    let decoded: MemoryEvent = serde_json::from_slice(&prepared.bytes).unwrap();
    assert_eq!(prepared.checksum, decoded.checksum);
    assert_eq!(prepared.checksum, event_hash(&decoded).unwrap());
    assert_eq!(
        prepare_event_at(MAX_EVENTS, "", "no", &state).err(),
        Some(ContractError("journal.memory_log_limit"))
    );
}
