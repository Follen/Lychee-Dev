use super::super::{Header, encode_record};
use super::*;
use std::{collections::VecDeque, sync::atomic::Ordering};
fn identity() -> Identity {
    Identity {
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        owner: "1".repeat(32),
        fence: 1,
        next_slot: 2,
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
fn query(kind: ObservationKind) -> ObservationQuery {
    ObservationQuery {
        kind,
        identity: identity(),
        envelope: SlotEnvelope {
            schema: SCHEMA.into(),
            index: 1,
            start_slot: 1,
            runtime: "2".repeat(32),
            owner: "1".repeat(32),
            fence: 1,
            nonce: "3".repeat(32),
            ticket: "4".repeat(32),
            action: "prepare".into(),
            guid: "g".into(),
            build: "b".into(),
            ..SlotEnvelope::default()
        },
    }
}
fn receipt() -> Receipt {
    Receipt {
        identity: identity(),
        nonce: "3".repeat(32),
        ticket: "4".repeat(32),
        action: "prepare".into(),
        state: "reported".into(),
        ..Receipt::default()
    }
}
fn record(kind: Kind, sequence: u32, payload: Vec<u8>) -> Record {
    let h = Header {
        nonce: [0x33; 16],
        runtime: [0x22; 16],
        ticket: [0x44; 16],
        kind,
        state: if kind == Kind::Body { 3 } else { 1 },
        sequence,
        length: 0,
        checksum: 0,
    };
    crate::mailbox::decode(&encode_record(h, &payload).unwrap()).unwrap()
}
struct FakeReader {
    queued: VecDeque<Vec<Record>>,
    calls: Vec<Kind>,
    cancel_after_read: bool,
}
impl RecordReader for FakeReader {
    fn find(
        &mut self,
        control: &OperationControl,
        selector: &Selector,
        _accept: Option<&mut dyn FnMut(&Record) -> bool>,
        _eager: bool,
    ) -> ChannelResult<Vec<Record>> {
        self.calls.push(selector.kind);
        if self.cancel_after_read {
            control.cancel_handle().store(true, Ordering::Release);
        }
        Ok(self.queued.pop_front().unwrap_or_default()) // Deliberately ignores Accept.
    }
}
fn fake(records: Vec<Vec<Record>>) -> FakeReader {
    FakeReader {
        queued: records.into(),
        calls: Vec::new(),
        cancel_after_read: false,
    }
}
#[test]
fn go_mailbox_v1_observations_requalify_fake_head_and_body_without_sequence_equality() {
    let payload = b" { \"ok\":false, \"result\":\"<&>\" }\n".to_vec();
    let mut r = receipt();
    r.report_bytes = payload.len() as u32;
    r.report_checksum = checksum(&payload);
    let head = record(Kind::Receipt, 8, serde_json::to_vec(&r).unwrap());
    let body = record(Kind::Body, 3, payload.clone());
    let mut reader = fake(vec![vec![head], vec![body]]);
    let observed = observe_records(
        &OperationControl::default(),
        &mut reader,
        &query(ObservationKind::Result),
    )
    .unwrap();
    assert_eq!(
        observed,
        Observation::Reported {
            receipt: r,
            payload
        }
    );
    assert_eq!(reader.calls, vec![Kind::Receipt, Kind::Body]);
}
#[test]
fn go_mailbox_v1_observations_rejected_receipt_retained_and_target_checked() {
    let mut r = receipt();
    r.state = "rejected".into();
    r.reason = "slot_wake_rejected".into();
    let bytes = serde_json::to_vec(&r).unwrap();
    let mut reader = fake(vec![vec![record(Kind::Receipt, 1, bytes.clone())]]);
    assert_eq!(
        observe_records(
            &OperationControl::default(),
            &mut reader,
            &query(ObservationKind::Receipt)
        )
        .unwrap(),
        Observation::Rejected { receipt: r }
    );
    let mut q = query(ObservationKind::Receipt);
    q.identity.product = "classic".into();
    let mut reader = fake(vec![vec![record(Kind::Receipt, 1, bytes)]]);
    assert_eq!(
        observe_records(&OperationControl::default(), &mut reader, &q),
        Err(ContractError("live.channel_receipt_target_mismatch").into())
    );
}
#[test]
fn go_mailbox_v1_observations_missing_is_pending_not_report_and_bad_head_cannot_read_body() {
    let mut reader = fake(vec![vec![]]);
    assert_eq!(
        observe_records(
            &OperationControl::default(),
            &mut reader,
            &query(ObservationKind::Result)
        )
        .unwrap(),
        Observation::Pending { diagnostic: None }
    );
    let mut r = receipt();
    r.state = "prepared".into();
    let mut reader = fake(vec![vec![record(
        Kind::Receipt,
        1,
        serde_json::to_vec(&r).unwrap(),
    )]]);
    assert_eq!(
        observe_records(
            &OperationControl::default(),
            &mut reader,
            &query(ObservationKind::Result)
        ),
        Err(ContractError("live.channel_receipt_mismatch").into())
    );
    assert_eq!(reader.calls.len(), 1);
    let r = receipt();
    let mut reader = fake(vec![vec![record(
        Kind::Receipt,
        1,
        serde_json::to_vec(&r).unwrap(),
    )]]);
    assert_eq!(
        observe_records(
            &OperationControl::default(),
            &mut reader,
            &query(ObservationKind::Result)
        ),
        Err(ContractError("live.channel_report_length").into())
    );
}
#[test]
fn go_mailbox_v1_observations_body_kind_and_checksum_rechecked_for_fake_adapter() {
    let payload = br#"{"ok":true}"#.to_vec();
    let mut r = receipt();
    r.report_bytes = payload.len() as u32;
    r.report_checksum = checksum(&payload);
    for change in 0..4 {
        let head = record(Kind::Receipt, 2, serde_json::to_vec(&r).unwrap());
        let mut body = record(Kind::Body, 1, payload.clone());
        match change {
            0 => body.kind = 1,
            1 => body.state = 1,
            2 => body.sequence = 0,
            _ => body.runtime = "9".repeat(32),
        };
        let mut reader = fake(vec![vec![head], vec![body]]);
        assert_eq!(
            observe_records(
                &OperationControl::default(),
                &mut reader,
                &query(ObservationKind::Result)
            ),
            Err(ContractError("live.channel_report_record_invalid").into())
        );
    }
}
#[test]
fn go_mailbox_v1_observations_cancellation_after_fake_read_discards_data() {
    let r = receipt();
    let mut reader = fake(vec![vec![record(
        Kind::Receipt,
        1,
        serde_json::to_vec(&r).unwrap(),
    )]]);
    reader.cancel_after_read = true;
    let error = observe_records(
        &OperationControl::default(),
        &mut reader,
        &query(ObservationKind::Receipt),
    )
    .unwrap_err();
    assert!(error.to_string().contains("command.cancelled"));
    assert_eq!(reader.calls.len(), 1);
}
#[test]
fn go_mailbox_v1_joined_errors_keep_typed_causes_and_never_hide_hard_faults() {
    let pending = ChannelError::Pending { causes: vec![] };
    let timeout = ChannelError::Joined {
        causes: vec![
            pending.clone(),
            ChannelError::Joined {
                causes: vec![ChannelError::DeadlineExceeded],
            },
        ],
    };
    assert!(timeout.is_pending());
    assert!(timeout.optional_observation_timeout());
    for hard in [
        ChannelError::Cancelled,
        ChannelError::External {
            causes: vec!["io.failed".into()],
        },
        ChannelError::Contract(ContractError("process.identity_changed")),
    ] {
        let mixed = ChannelError::Joined {
            causes: vec![timeout.clone(), hard.clone()],
        };
        assert!(mixed.is_pending());
        assert!(mixed.any_cause(&|e| e == &hard));
        assert!(!mixed.optional_observation_timeout());
    }
    assert!(!ChannelError::Joined { causes: vec![] }.optional_observation_timeout());
    assert!(
        !ChannelError::Pending {
            causes: vec!["io.failed"]
        }
        .optional_observation_timeout()
    );
}
#[test]
fn go_mailbox_v1_slot_available_keeps_original_blocker_through_join() {
    let blocker = Blocker {
        kind: "slot_occupied".into(),
        installation: "installation".into(),
        slot: 499,
        consumer: "consumer".into(),
        runtime: "2".repeat(32),
        nonce: "3".repeat(32),
        condition: "slot_identity_changed".into(),
    };
    // No manually joined Blocked error: SlotAvailable must carry that wrapper
    // relationship itself, including the original full payload and display.
    let error = ChannelError::SlotAvailable {
        index: 500,
        blocker: blocker.clone(),
    };
    assert!(error.is_blocked());
    assert!(error.is_pending());
    assert_eq!(error.to_string(), "live.channel_blocked: slot_occupied");
    assert_eq!(error.slot_available_blocker(), Some(&blocker));
    assert!(error.any_cause(&|e| matches!(e, ChannelError::Blocked(b) if b == &blocker)));
    assert!(error.any_cause(
        &|e| matches!(e, ChannelError::SlotAvailable { index: 500, blocker } if blocker.slot == 499)
    ));
    assert!(error.optional_observation_timeout());
}
#[test]
fn go_mailbox_v1_observation_dispatch_keeps_facts_and_independent_errors() {
    let r = receipt();
    let dispatch = ObservationDispatch {
        observation: Some(Observation::Receipt(r.clone())),
        error: Some(ChannelError::Cancelled),
    };
    assert_eq!(dispatch.observation, Some(Observation::Receipt(r)));
    assert_eq!(dispatch.error, Some(ChannelError::Cancelled));
    let diagnostic = InputDiagnostic {
        schema: SCHEMA.into(),
        runtime: "2".repeat(32),
        attempt_seq: 1,
        slot: 1,
        stage: "load_failed".into(),
        reason: "slot_addon_disabled".into(),
        received: false,
        sample_millis: 1,
    };
    assert!(diagnostic.valid());
    let dispatch = ObservationDispatch {
        observation: Some(Observation::Pending {
            diagnostic: Some(diagnostic.clone()),
        }),
        error: Some(ChannelError::Joined {
            causes: vec![
                ChannelError::Pending { causes: vec![] },
                ChannelError::External {
                    causes: vec!["transport.read_failed".into()],
                },
            ],
        }),
    };
    assert_eq!(
        dispatch.observation,
        Some(Observation::Pending {
            diagnostic: Some(diagnostic)
        })
    );
    let error = dispatch.error.unwrap();
    assert!(error.is_pending());
    assert!(!error.optional_observation_timeout());
}

#[test]
fn go_mailbox_v1_blocked_wrappers_follow_go_unwrap_and_nested_join_timeout_table() {
    for (kind, optional) in [
        ("runtime_retirement_pending", true),
        ("slot_occupied", true),
        ("slot_reservation", false),
        ("slot_pool_full", false),
        ("publication_lock", false),
    ] {
        let blocker = Blocker {
            kind: kind.into(),
            slot: 499,
            ..Blocker::default()
        };
        for error in [
            ChannelError::Blocked(blocker.clone()),
            ChannelError::SlotAvailable {
                index: 500,
                blocker,
            },
        ] {
            assert!(error.is_pending(), "{kind}");
            assert!(error.is_blocked(), "{kind}");
            assert_eq!(error.optional_observation_timeout(), optional, "{kind}");
            assert_eq!(
                error.any_cause(&|e| matches!(
                    e,
                    ChannelError::Contract(ContractError(
                        "live.channel_shared_publication_pending"
                    ))
                )),
                !optional
            );
            let nested = ChannelError::Joined {
                causes: vec![
                    ChannelError::DeadlineExceeded,
                    ChannelError::Joined {
                        causes: vec![error],
                    },
                ],
            };
            assert!(nested.is_pending());
            assert!(nested.is_blocked());
            assert_eq!(nested.optional_observation_timeout(), optional);
            for fault in [
                ChannelError::Cancelled,
                ChannelError::External {
                    causes: vec!["io.failed".into()],
                },
                ChannelError::Contract(ContractError("process.identity_changed")),
            ] {
                let mixed = ChannelError::Joined {
                    causes: vec![nested.clone(), fault],
                };
                assert!(mixed.is_pending());
                assert!(mixed.is_blocked());
                assert!(!mixed.optional_observation_timeout());
            }
        }
    }
}

#[test]
fn go_mailbox_v1_observations_raw_byte_json_uses_go_scanner_without_utf8_gate() {
    for (payload, valid) in [
        (
            b" { \"s\":\"\xff\", \"x\":1E+400, \"x\":-0 } ".to_vec(),
            true,
        ),
        (b"{\"s\":\"\x1f\"}".to_vec(), false),
        (b"{\"s\":\"\\\xff\"}".to_vec(), false),
        (b"{\"s\":\"\xff\"} trailing".to_vec(), false),
    ] {
        let mut r = receipt();
        r.report_bytes = payload.len() as u32;
        r.report_checksum = checksum(&payload);
        let mut reader = fake(vec![
            vec![record(Kind::Receipt, 8, serde_json::to_vec(&r).unwrap())],
            vec![record(Kind::Body, 3, payload.clone())],
        ]);
        let observed = observe_records(
            &OperationControl::default(),
            &mut reader,
            &query(ObservationKind::Result),
        );
        if valid {
            assert_eq!(
                observed.unwrap(),
                Observation::Reported {
                    receipt: r,
                    payload
                }
            );
        } else {
            assert_eq!(
                observed,
                Err(ContractError("live.channel_report_json").into())
            );
        }
    }
}
