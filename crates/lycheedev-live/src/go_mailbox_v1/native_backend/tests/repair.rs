//! Real Delivery files/leases and the production NativeCore with a scripted
//! reader/sender. No Windows input or game acceptance is claimed.
use super::*;
use lycheedev_core::coordination::InstallationMaintenanceLease;
use lycheedev_delivery::{
    slot_publication::{RecordedInstallationIdentity, RepairPublication},
    slots,
};
use std::{
    cell::Cell,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    addon: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "native-repair-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let root = fs::canonicalize(root).unwrap();
        let addon = root.join("Lychee Dev");
        fs::create_dir(&addon).unwrap();
        fs::write(addon.join("Core.lua"), b"modified managed main").unwrap();
        let receipt = lycheedev_delivery::installation::Receipt {
            schema: "lycheedev.installation.v1".into(),
            component: "addon".into(),
            version: "3.2.0".into(),
            commit: "a".repeat(40),
            resources: vec![lycheedev_delivery::installation::Resource {
                path: "addon/Core.lua".into(),
                bytes: 11,
                sha256: storage::sha256(b"-- fixture\n"),
            }],
        };
        fs::write(
            addon.join(lycheedev_delivery::installation::MARKER),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        let mut files = Vec::new();
        for index in 1..=slots::COUNT {
            let path = slots::directory(&root, index);
            fs::create_dir(&path).unwrap();
            for (name, bytes) in slots::files(index, "3.2.0") {
                fs::write(path.join(name), bytes).unwrap();
            }
            files.push(slots::SlotFile {
                payload_hash: storage::sha256(&fs::read(path.join("Payload.lua")).unwrap()),
                ..Default::default()
            });
        }
        let pool = SlotPool {
            schema: slots::SCHEMA.into(),
            version: "3.2.0".into(),
            state: "ready".into(),
            files,
            ..Default::default()
        };
        fs::write(root.join(slots::MARKER), serde_json::to_vec(&pool).unwrap()).unwrap();
        Self { root, addon }
    }
    fn store(&self, e: SlotEnvelope, allow: Rc<Cell<bool>>) -> DiskPublications {
        let identity =
            RecordedInstallationIdentity::from_authenticated_record(self.addon.clone()).unwrap();
        DiskPublications::for_repair(
            self.addon.clone(),
            "3.2.0",
            "41/99".into(),
            move |actual, c| {
                check_control(c)?;
                if !allow.get() || actual != &e {
                    return Err(error("fixture.repair_authority_denied"));
                }
                RepairPublication::from_authenticated_record(&identity, "3.2.0", "41/99", actual)
                    .map_err(|e| external(e.to_string()))
            },
        )
        .unwrap()
    }
    fn unlocked(&self) {
        InstallationMaintenanceLease::acquire(&self.addon)
            .unwrap()
            .close()
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct RepairSender {
    addon: PathBuf,
    sent: Rc<Cell<usize>>,
    cancel: Option<lycheedev_core::control::TrackedCancellation>,
}
impl Sender for RepairSender {
    fn send(
        &mut self,
        c: &InputControl,
        _: MailboxBindings,
        _: MailboxAction,
        guard: &mut dyn FnMut(&InputControl) -> ChannelResult<()>,
    ) -> Result<InputReceipt, InputFailure<ChannelError>> {
        assert!(
            InstallationMaintenanceLease::acquire(&self.addon).is_err(),
            "publication lease must remain held through burst"
        );
        let mut queued = 0;
        for _ in 0..6 {
            if let Err(cause) = guard(c) {
                return Err(InputFailure {
                    cause: Some(InputError::Guard(cause)),
                    cleanup_errors: vec![],
                    receipt: InputReceipt {
                        messages_queued: queued,
                        submission_complete: false,
                        release_complete: true,
                    },
                });
            }
            queued += 1;
            self.sent.set(self.sent.get() + 1);
            if let Some(cancel) = self.cancel.take() {
                cancel.cancel();
            }
        }
        Ok(InputReceipt {
            messages_queued: queued,
            submission_complete: true,
            release_complete: true,
        })
    }
}
type RepairCore = NativeCore<FakeSource, DiskPublications, RepairSender, FakeClock>;
fn core(f: &Fixture, e: &SlotEnvelope, allow: Rc<Cell<bool>>, fresh: bool) -> RepairCore {
    let old = make_core(vec![
        Ok(vec![input_record(e, 10_000, 1)]),
        Ok(vec![input_record(e, if fresh { 10_000 } else { 9_000 }, 2)]),
    ]);
    NativeCore {
        reader: old.reader,
        publications: f.store(e.clone(), allow),
        sender: RepairSender {
            addon: f.addon.clone(),
            sent: Rc::new(Cell::new(0)),
            cancel: None,
        },
        clock: old.clock,
        guard: old.guard,
        release: old.release,
        consumer: old.consumer,
        pid: old.pid,
        creation: old.creation,
        witness: None,
        sample_wait: SampleWait::default(),
        diagnostic_exchange: String::new(),
    }
}
fn invoke(core: &mut RepairCore, c: &OperationControl, e: &SlotEnvelope) -> InputAction {
    InputAction {
        kind: InputKind::Invoke,
        envelope: e.clone(),
        observation: Some(core.observe_input(c, e, 0, SCHEMA).unwrap()),
        capability: SCHEMA.into(),
        after_millis: 0,
    }
}
#[test]
fn repair_modified_main_unbind_uses_real_publication_and_holds_lease_through_burst() {
    let f = Fixture::new();
    let e = envelope("unbind");
    let c = control();
    let mut normal = DiskPublications::recorded(f.addon.clone(), "3.2.0")
        .unwrap()
        .acquire(&c)
        .unwrap();
    assert!(normal.publish("41/99", &e).is_err());
    normal.close().unwrap();
    let mut core = core(&f, &e, Rc::new(Cell::new(true)), true);
    core.publish(&c, &e).unwrap();
    f.unlocked();
    let action = invoke(&mut core, &c, &e);
    let outcome = core.input(&c, &action);
    assert!(outcome.error.is_none());
    assert_eq!(outcome.outcome.disposition, InputDisposition::Submitted);
    assert_eq!(core.sender.sent.get(), 6);
    f.unlocked();
    assert_eq!(
        fs::read(f.addon.join("Core.lua")).unwrap(),
        b"modified managed main"
    );
}
#[test]
fn repair_rechecks_durable_authority_before_input_and_refuses_business_publish() {
    let f = Fixture::new();
    let e = envelope("unbind");
    let c = control();
    let allow = Rc::new(Cell::new(false));
    let mut core = core(&f, &e, allow.clone(), true);
    let before = fs::read(f.root.join(slots::MARKER)).unwrap();
    assert!(core.publish(&c, &e).is_err());
    assert!(core.publish(&c, &envelope("prepare")).is_err());
    assert_eq!(fs::read(f.root.join(slots::MARKER)).unwrap(), before);
    f.unlocked();
    allow.set(true);
    core.publish(&c, &e).unwrap();
    let action = invoke(&mut core, &c, &e);
    allow.set(false);
    let outcome = core.input(&c, &action);
    assert!(outcome.error.is_some());
    assert_eq!(outcome.outcome.disposition, InputDisposition::NotSent);
    assert_eq!(core.sender.sent.get(), 0);
    f.unlocked();
}
#[test]
fn repair_freshness_changed_publication_and_cancelled_burst_never_replay() {
    for cancel in [false, true] {
        let f = Fixture::new();
        let e = envelope("unbind");
        let c = OperationControl::default();
        let mut core = core(&f, &e, Rc::new(Cell::new(true)), cancel);
        core.publish(&c, &e).unwrap();
        let action = invoke(&mut core, &c, &e);
        if cancel {
            core.sender.cancel = Some(c.tracked_cancel_handle().unwrap());
        }
        let outcome = core.input(&c, &action);
        assert!(outcome.error.is_some());
        assert_eq!(core.sender.sent.get(), usize::from(cancel));
        assert_eq!(
            outcome.outcome.disposition,
            if cancel {
                InputDisposition::Uncertain
            } else {
                InputDisposition::NotSent
            }
        );
        f.unlocked();
        // Even a fresh invocation cannot reuse this consumed one-use witness.
        let before = core.sender.sent.get();
        let retry = core.input(&control(), &action);
        assert!(retry.error.is_some());
        assert_eq!(core.sender.sent.get(), before);
        f.unlocked();
    }
}

#[test]
fn repair_non_unbind_cleanup_keeps_original_clean_main_gate() {
    for action in ["confirm", "release"] {
        let f = Fixture::new();
        let e = envelope(action);
        let c = control();
        // This denied repair authority must never authorize non-unbind work.
        let mut core = core(&f, &e, Rc::new(Cell::new(false)), true);
        assert!(core.publish(&c, &e).is_err());
        f.unlocked();
        fs::write(f.addon.join("Core.lua"), b"-- fixture\n").unwrap();
        core.publish(&c, &e).unwrap();
        let action = invoke(&mut core, &c, &e);
        let outcome = core.input(&c, &action);
        assert!(outcome.error.is_none());
        assert_eq!(core.sender.sent.get(), 6);
        f.unlocked();
    }
}
