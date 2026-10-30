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
        self.store_for_reset(e, allow, false)
    }
    fn store_for_reset(
        &self,
        e: SlotEnvelope,
        allow: Rc<Cell<bool>>,
        slot_reset: bool,
    ) -> DiskPublications {
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
                (if actual.action == "unbind" {
                    RepairPublication::from_authenticated_record
                } else {
                    RepairPublication::from_authenticated_finalization
                })(&identity, "3.2.0", "41/99", actual)
                .map_err(|e| external(e.to_string()))
            },
            slot_reset,
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
fn repair_finalization_modified_main_requires_exact_authority_even_when_clean() {
    for action in ["confirm", "release"] {
        let f = Fixture::new();
        let e = envelope(action);
        let c = control();
        let allow = Rc::new(Cell::new(false));
        let mut core = core(&f, &e, allow.clone(), true);
        assert!(core.publish(&c, &e).is_err());
        f.unlocked();
        allow.set(true);
        core.publish(&c, &e).unwrap();
        let action = invoke(&mut core, &c, &e);
        let outcome = core.input(&c, &action);
        assert!(outcome.error.is_none());
        assert_eq!(core.sender.sent.get(), 6);
        f.unlocked();
        assert_eq!(
            fs::read(f.addon.join("Core.lua")).unwrap(),
            b"modified managed main"
        );
    }
}
#[test]
fn repair_finalization_revocation_stops_before_burst() {
    for action in ["confirm", "release"] {
        let f = Fixture::new();
        let e = envelope(action);
        let c = control();
        let allow = Rc::new(Cell::new(true));
        let mut core = core(&f, &e, allow.clone(), true);
        core.publish(&c, &e).unwrap();
        let input = invoke(&mut core, &c, &e);
        allow.set(false);
        let outcome = core.input(&c, &input);
        assert!(outcome.error.is_some());
        assert_eq!(core.sender.sent.get(), 0);
        f.unlocked();
    }
}

// The files, 500-slot admission, publication checks and lease are real. Only
// their elapsed time and the process INPUT publications are controlled here.
struct DelayedDiskPublications {
    inner: DiskPublications,
    delay_next: Rc<Cell<bool>>,
    uptime: Rc<RefCell<i64>>,
    now: Rc<RefCell<Instant>>,
    events: Events,
}
impl DelayedDiskPublications {
    fn preparation_elapsed(&self) {
        if self.delay_next.replace(false) {
            *self.uptime.borrow_mut() += 1_000;
            *self.now.borrow_mut() += Duration::from_secs(1);
            log(&self.events, "disk_preparation_elapsed");
        }
    }
}
impl Publications for DelayedDiskPublications {
    fn acquire<'a>(&self, c: &'a OperationControl) -> ChannelResult<Box<dyn Publication + 'a>> {
        let lease = self.inner.acquire(c)?;
        self.preparation_elapsed();
        Ok(lease)
    }
    fn acquire_for<'a>(
        &self,
        c: &'a OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<Box<dyn Publication + 'a>> {
        let lease = self.inner.acquire_for(c, envelope)?;
        self.preparation_elapsed();
        Ok(lease)
    }
}
struct PreparedDiskSender {
    addon: PathBuf,
    sent: Rc<Cell<usize>>,
    events: Events,
}
impl Sender for PreparedDiskSender {
    fn send(
        &mut self,
        control: &InputControl,
        _: MailboxBindings,
        action: MailboxAction,
        guard: &mut dyn FnMut(&InputControl) -> ChannelResult<()>,
    ) -> Result<InputReceipt, InputFailure<ChannelError>> {
        let mut queued = 0;
        for _ in 0..6 {
            if let Err(cause) = guard(control) {
                return Err(InputFailure {
                    cause: Some(InputError::Guard(cause)),
                    cleanup_errors: vec![],
                    receipt: InputReceipt {
                        messages_queued: queued,
                        release_complete: true,
                        ..InputReceipt::default()
                    },
                });
            }
            match InstallationMaintenanceLease::acquire(&self.addon) {
                Ok(lease) => {
                    assert_eq!(action, MailboxAction::Reload);
                    lease.close().unwrap();
                }
                Err(_) => assert_eq!(action, MailboxAction::Invoke),
            }
            log(&self.events, "post");
            queued += 1;
            self.sent.set(self.sent.get() + 1);
        }
        Ok(InputReceipt {
            messages_queued: queued,
            submission_complete: true,
            release_complete: true,
        })
    }
}
fn disk_preparation_then_fresh_input(action_name: &str, pool_damage: Option<&str>) {
    let fixture = Fixture::new();
    fs::write(fixture.addon.join("Core.lua"), b"-- fixture\n").unwrap();
    let control = control();
    let original_deadline = control.deadline();
    let verified = lycheedev_delivery::installation::verify_main_addon_controlled(
        &fixture.addon,
        "3.2.0",
        &control,
    )
    .unwrap();
    let envelope = envelope(action_name);
    let original = input_record(&envelope, 10_000, 1);
    let expected_uptime = if action_name == "reload" {
        10_000
    } else {
        11_000
    };
    let mut refreshed = input_record(&envelope, expected_uptime, 2);
    refreshed.address = 0x2000;
    let old = make_core(vec![
        Ok(vec![original]),
        Ok(vec![refreshed.clone()]),
        Ok(vec![refreshed]),
    ]);
    let events = old.clock.events.clone();
    let delay_next = Rc::new(Cell::new(false));
    let sent = Rc::new(Cell::new(0));
    let mut core = NativeCore {
        reader: old.reader,
        publications: DelayedDiskPublications {
            inner: DiskPublications::verified(&verified, "3.2.0"),
            delay_next: delay_next.clone(),
            uptime: old.clock.uptime.clone(),
            now: old.clock.now.clone(),
            events: events.clone(),
        },
        sender: PreparedDiskSender {
            addon: fixture.addon.clone(),
            sent: sent.clone(),
            events: events.clone(),
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
    };
    if action_name != "reload" {
        core.publish(&control, &envelope).unwrap();
    }
    match pool_damage {
        Some("missing") => fs::remove_file(fixture.root.join(slots::MARKER)).unwrap(),
        Some("malformed") => fs::write(fixture.root.join(slots::MARKER), b"{").unwrap(),
        Some("slot_missing") => {
            fs::remove_file(slots::directory(&fixture.root, 1).join("Payload.lua")).unwrap();
        }
        None => {}
        Some(_) => unreachable!(),
    }
    fixture.unlocked();
    let observation = core.observe_input(&control, &envelope, 0, SCHEMA).unwrap();
    assert_eq!(observation.sample_millis, 10_000);
    let action = InputAction {
        kind: if action_name == "reload" {
            InputKind::Reload
        } else {
            InputKind::Invoke
        },
        envelope: envelope.clone(),
        observation: Some(observation),
        capability: SCHEMA.into(),
        after_millis: 0,
    };
    events.borrow_mut().clear();
    delay_next.set(true);
    let outcome = core.input(&control, &action);
    fixture.unlocked();
    assert!(outcome.error.is_none(), "{outcome:?}");
    assert_eq!(outcome.outcome.disposition, InputDisposition::Submitted);
    assert_eq!(sent.get(), 6);
    assert_eq!(control.deadline(), original_deadline);
    assert_eq!(*core.clock.uptime.borrow(), expected_uptime);
    assert!(core.witness.is_none());
    {
        let events = events.borrow();
        let prepared = events.iter().position(|e| e == "disk_preparation_elapsed");
        if action_name == "reload" {
            assert!(
                prepared.is_none(),
                "reload must not acquire the slot publication pool"
            );
        }
        let first_post = events.iter().position(|e| e == "post").unwrap();
        assert_eq!(
            events[prepared.map_or(0, |i| i + 1)..first_post]
                .iter()
                .filter(|e| *e == "read:5")
                .count(),
            2,
            "current authorization and final publication check must follow slow preparation"
        );
    }
    let retry = core.input(&control, &action);
    assert!(
        retry.error.is_some(),
        "consumed original witness cannot authorize another chord"
    );
    assert_eq!(retry.outcome.messages_queued, 0);
    assert_eq!(sent.get(), 6);
    fixture.unlocked();
}
#[test]
fn disk_reload_ignores_missing_malformed_or_damaged_pool() {
    for damage in ["missing", "malformed", "slot_missing"] {
        disk_preparation_then_fresh_input("reload", Some(damage));
    }
}
#[test]
fn disk_invoke_preparation_over_freshness_window_uses_fresh_same_profile() {
    disk_preparation_then_fresh_input("prepare", None);
}

#[test]
fn disk_runtime_retirement_uses_proof_without_requiring_intact_pool() {
    for damage in ["missing", "malformed", "slot_missing"] {
        let fixture = Fixture::new();
        let envelope = envelope("unbind");
        let allow = Rc::new(Cell::new(true));
        let mut core = core(&fixture, &envelope, allow, true);
        let control = control();
        core.publish(&control, &envelope).unwrap();
        let payload = slots::directory(&fixture.root, 1).join("Payload.lua");
        let marker = fixture.root.join(slots::MARKER);
        match damage {
            "missing" => fs::remove_file(&marker).unwrap(),
            "malformed" => fs::write(&marker, b"{").unwrap(),
            "slot_missing" => {
                fs::remove_file(slots::directory(&fixture.root, 2).join("Payload.lua")).unwrap();
            }
            _ => unreachable!(),
        }
        let marker_before = fs::read(&marker).ok();
        let payload_before = fs::read(&payload).unwrap();
        let old = identity(&"1".repeat(32));
        let current = identity(&"2".repeat(32));
        let proof = make_core(replacement_records(&current, 10_000, 11_100))
            .observe_runtime_replacement(&control, &old)
            .unwrap()
            .unwrap();
        core.retire_replaced_runtime(&control, &envelope, &proof)
            .unwrap();
        fixture.unlocked();
        assert!(
            core.consumed(&control, &envelope).is_err(),
            "ordinary admission remains strict"
        );
        if damage != "slot_missing" {
            assert_eq!(fs::read(&marker).ok(), marker_before);
            assert_eq!(fs::read(&payload).unwrap(), payload_before);
        }
        assert_eq!(core.sender.sent.get(), 0);
    }
}

#[test]
fn disk_slot_reset_retirement_preserves_original_bytes_after_authenticated_proof() {
    let fixture = Fixture::new();
    let envelope = envelope("unbind");
    let allow = Rc::new(Cell::new(true));
    let mut core = core(&fixture, &envelope, allow.clone(), true);
    core.publications = fixture.store_for_reset(envelope.clone(), allow, true);
    let control = control();
    core.publish(&control, &envelope).unwrap();
    let marker = fixture.root.join(slots::MARKER);
    let payload = slots::directory(&fixture.root, 1).join("Payload.lua");
    let marker_before = fs::read(&marker).unwrap();
    let payload_before = fs::read(&payload).unwrap();
    let old = identity(&"1".repeat(32));
    let current = identity(&"2".repeat(32));
    let proof = make_core(replacement_records(&current, 10_000, 11_100))
        .observe_runtime_replacement(&control, &old)
        .unwrap()
        .unwrap();
    core.retire_replaced_runtime(&control, &envelope, &proof)
        .unwrap();
    assert_eq!(fs::read(&marker).unwrap(), marker_before);
    assert_eq!(fs::read(&payload).unwrap(), payload_before);
    fs::write(&marker, b"{").unwrap();
    core.retire_replaced_runtime(&control, &envelope, &proof)
        .unwrap();
    assert_eq!(fs::read(&marker).unwrap(), b"{");
    assert_eq!(fs::read(&payload).unwrap(), payload_before);
    let mut wrong_target = proof.clone();
    wrong_target.process_id += 1;
    assert!(
        core.retire_replaced_runtime(&control, &envelope, &wrong_target)
            .is_err()
    );
    core.guard = Box::new(|_| Err(error("fixture.ownership_changed")));
    assert!(
        core.retire_replaced_runtime(&control, &envelope, &proof)
            .is_err()
    );
    assert_eq!(fs::read(&marker).unwrap(), b"{");
    assert_eq!(core.sender.sent.get(), 0);
    fixture.unlocked();
}
