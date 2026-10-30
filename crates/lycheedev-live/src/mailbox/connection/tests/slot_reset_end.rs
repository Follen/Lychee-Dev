//! Real CON journal and Delivery retirement files; process lifetime is an
//! explicit identity-checking fixture. No native reader or input is opened.
use super::*;
use crate::mailbox::{
    InputDisposition, InputOutcome,
    driver::budget,
    state::{InputAttempt, InputKind},
};
use lycheedev_delivery::slots::{self, SlotFile, SlotPool};

struct Ended {
    calls: Rc<Cell<usize>>,
}
impl ProcessLifetimeObserver for Ended {
    fn observe(
        &mut self,
        original: &lycheedev_windows::ProcessIdentity,
        c: &OperationControl,
    ) -> ChannelResult<ProcessEnd> {
        check_control(c)?;
        assert_eq!((original.pid, original.created), (17, 19));
        self.calls.set(self.calls.get() + 1);
        Ok(ProcessEnd::ProcessExited)
    }
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!(
        "lycheedev-slot-reset-ended-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&root).unwrap();
    let root = fs::canonicalize(root).unwrap();
    let client = root.join("client");
    fs::create_dir_all(client.join("Interface/AddOns/Lychee Dev")).unwrap();
    let target = ConnectionTarget {
        client: ClientRecord {
            directory: lycheedev_core::client::display_path(&fs::canonicalize(client).unwrap()),
            product: "retail".into(),
            full_build: "b".into(),
            ..Default::default()
        },
        window: WindowRecord {
            process_id: 17,
            process_started_at: 19,
            handle: 23,
            ..Default::default()
        },
    };
    let record = ConnectionBook::open(&root)
        .unwrap()
        .create(
            target.clone(),
            identity('2'),
            &FakeClock(Rc::new(Cell::new(1000))),
            &mut Tokens(50),
            &control(),
        )
        .unwrap();
    Fixture {
        root,
        target,
        id: record.state.id,
    }
}

#[test]
fn ended_slot_reset_close_preserves_pending_disk_bytes_and_original_evidence() {
    for slot_reset in [false, true] {
        for stage in [OperationStage::Running, OperationStage::ReleaseReady] {
            let f = fixture();
            assert!(
                !f.target.client.directory.starts_with("\\\\?\\"),
                "the stored client uses public path spelling"
            );
            f.operation(stage, "original-business-request");
            f.edit(|state| {
                state.closing = true;
                state.close_budget = Some(budget(1000, 45_000).unwrap());
                let operation = state.operation.as_mut().unwrap();
                operation.recovery_budget = Some(budget(1000, 45_000).unwrap());
                let e = SlotEnvelope {
                    schema: SCHEMA.into(),
                    index: 1,
                    runtime: state.identity.runtime.clone(),
                    owner: state.owner.clone(),
                    fence: 1,
                    guid: state.identity.guid.clone(),
                    build: state.identity.build.clone(),
                    nonce: "9".repeat(32),
                    ticket: operation.ticket.clone(),
                    action: if stage == OperationStage::ReleaseReady {
                        "release"
                    } else {
                        "commit"
                    }
                    .into(),
                    report_bytes: operation.report_bytes,
                    report_checksum: operation.report_checksum,
                    ..Default::default()
                };
                state.transaction = Some(Transaction {
                    envelope: e.clone(),
                    phase: TransactionPhase::InputAttempted,
                    receipt: None,
                    input_observation: None,
                    release_wait_started_at_ms: 0,
                });
                state.input = Some(InputAttempt {
                    id: "d".repeat(32),
                    exchange: e.nonce,
                    runtime: e.runtime,
                    kind: InputKind::Invoke,
                    after_millis: 1000,
                    recovery_after_millis: 0,
                    observation: None,
                    after_diagnostic: None,
                    outcome: Some(InputOutcome {
                        disposition: InputDisposition::Uncertain,
                        messages_queued: 3,
                        at_millis: 10_000,
                        reason: "original-uncertain-input".into(),
                        retryable: false,
                    }),
                });
            });
            let before = f.state();
            let e = &before.transaction.as_ref().unwrap().envelope;
            let parent = f.target.addon_parent();
            let slot = slots::directory(&parent, 1);
            fs::create_dir(&slot).unwrap();
            let payload = slot.join("Payload.lua");
            let original_payload = b"-- original pending payload\n";
            fs::write(&payload, original_payload).unwrap();
            let mut files = vec![SlotFile::default(); SLOT_COUNT as usize];
            files[0] = SlotFile {
                payload_hash: lycheedev_core::storage::sha256(b"-- before publication\n"),
                pending_hash: lycheedev_core::storage::sha256(original_payload),
                nonce: e.nonce.clone(),
                runtime: e.runtime.clone(),
                consumer: "17/19".into(),
                ..Default::default()
            };
            let marker = parent.join(slots::MARKER);
            let original_marker = serde_json::to_vec(&SlotPool {
                schema: slots::SCHEMA.into(),
                version: "3.2.0".into(),
                state: "ready".into(),
                files,
                ..Default::default()
            })
            .unwrap();
            fs::write(&marker, &original_marker).unwrap();
            let calls = Rc::new(Cell::new(0));
            let native = NativeEnvironment::new(
                &f.root,
                "3.2.0",
                Ended {
                    calls: calls.clone(),
                },
            )
            .unwrap();
            let mut service = ConnectionService::open(
                &f.root,
                native,
                FakeClock(Rc::new(Cell::new(1100))),
                Tokens(100),
                Schedule::default(),
            )
            .unwrap();
            let outcome = if slot_reset {
                service.close_for_repair_mode(&f.id, true, &control())
            } else {
                service.disconnect(&f.id, &control())
            };
            assert!(
                outcome.error().is_none(),
                "{slot_reset}/{stage:?}: {:?}",
                outcome.error()
            );
            let report = outcome.into_report().unwrap();
            assert!(report.closed);
            assert_eq!(report.complete, stage == OperationStage::ReleaseReady);
            assert_eq!(
                calls.get(),
                2,
                "process end is rechecked under the original CON lease"
            );
            let after = f.state();
            assert_eq!(after.id, before.id);
            assert_eq!(after.process_end, ProcessEnd::ProcessExited);
            assert_eq!(after.connect_budget, before.connect_budget);
            assert_eq!(after.close_budget, before.close_budget);
            assert_eq!(after.input, before.input);
            let op = after.operation.unwrap();
            let original = before.operation.unwrap();
            assert_eq!(op.request, original.request);
            assert_eq!(op.result_bytes, original.result_bytes);
            assert_eq!(op.recovery_budget, original.recovery_budget);
            assert_eq!(
                op.stage,
                if stage == OperationStage::ReleaseReady {
                    OperationStage::Complete
                } else {
                    OperationStage::ExecutionUnknown
                }
            );
            assert_eq!(fs::read(&payload).unwrap(), original_payload);
            if slot_reset {
                assert_eq!(fs::read(&marker).unwrap(), original_marker);
            } else {
                let retired: SlotPool =
                    serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
                assert!(retired.files[0].retired_process);
                assert!(retired.files[0].pending_hash.is_empty());
            }
            assert!(!inspect_window_owner(&parent, &f.target.resource(), &control()).occupied);
            drop(service);
            fs::remove_dir_all(&f.root).unwrap();
        }
    }
}
