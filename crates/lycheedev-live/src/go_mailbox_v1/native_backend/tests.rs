use super::*;
use crate::go_mailbox_v1::{InputBindings, InputDiagnostic, MAX_EXACT_INTEGER, checksum};
use lycheedev_core::control::TrackedCancellation;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

pub(super) type Events = Rc<RefCell<Vec<String>>>;
fn log(events: &Events, value: &str) {
    events.borrow_mut().push(value.into());
}
pub(super) struct FakeSource {
    pub(super) queue: VecDeque<ChannelResult<Vec<Record>>>,
    pub(super) events: Events,
    pub(super) close_error: Option<ChannelError>,
}
impl RecordReader for FakeSource {
    fn find(
        &mut self,
        c: &OperationControl,
        s: &Selector,
        accept: Option<&mut dyn FnMut(&Record) -> bool>,
        _: bool,
    ) -> ChannelResult<Vec<Record>> {
        log(&self.events, &format!("read:{}", s.kind as u8));
        check_control(c)?;
        let mut records = self.queue.pop_front().unwrap_or_else(|| Ok(Vec::new()))?;
        if let Some(accept) = accept {
            records.retain(|r| accept(r));
        }
        check_control(c)?;
        Ok(records)
    }
}
impl Source for FakeSource {
    fn evidence(&self) -> ReaderEvidence {
        ReaderEvidence::default()
    }
    fn close(self) -> ChannelResult<()> {
        log(&self.events, "reader_close");
        self.close_error.map_or(Ok(()), Err)
    }
}
#[derive(Default)]
struct PublicationState {
    held: bool,
    acquire_error: Option<ChannelError>,
    publish_error: Option<ChannelError>,
    verify_error: Option<ChannelError>,
    route_error: Option<ChannelError>,
    inspect_error: Option<ChannelError>,
    close_error: Option<ChannelError>,
    consumed: usize,
    retired: usize,
}
struct FakePublications {
    state: Rc<RefCell<PublicationState>>,
    events: Events,
}
struct FakeLease<'a> {
    state: Rc<RefCell<PublicationState>>,
    events: Events,
    control: &'a OperationControl,
}
impl Publications for FakePublications {
    fn acquire<'a>(&self, c: &'a OperationControl) -> ChannelResult<Box<dyn Publication + 'a>> {
        log(&self.events, "acquire");
        check_control(c)?;
        let mut state = self.state.borrow_mut();
        if let Some(e) = &state.acquire_error {
            return Err(e.clone());
        }
        assert!(!state.held);
        state.held = true;
        Ok(Box::new(FakeLease {
            state: self.state.clone(),
            events: self.events.clone(),
            control: c,
        }))
    }
}
impl Publication for FakeLease<'_> {
    fn inspect(&mut self) -> ChannelResult<SlotPool> {
        log(&self.events, "inspect");
        check_control(self.control)?;
        self.state
            .borrow()
            .inspect_error
            .clone()
            .map_or_else(|| Ok(SlotPool::default()), Err)
    }
    fn publish(&mut self, _: &str, _: &SlotEnvelope) -> ChannelResult<()> {
        log(&self.events, "publish");
        check_control(self.control)?;
        self.state
            .borrow()
            .publish_error
            .clone()
            .map_or(Ok(()), Err)
    }
    fn verify(&mut self, _: &str, _: &SlotEnvelope) -> ChannelResult<()> {
        log(&self.events, "verify");
        check_control(self.control)?;
        self.state.borrow().verify_error.clone().map_or(Ok(()), Err)
    }
    fn verify_route(&mut self, _: &SlotEnvelope) -> ChannelResult<()> {
        log(&self.events, "route");
        check_control(self.control)?;
        self.state.borrow().route_error.clone().map_or(Ok(()), Err)
    }
    fn consumed(&mut self, _: &str, _: &SlotEnvelope) -> ChannelResult<()> {
        log(&self.events, "consumed");
        check_control(self.control)?;
        self.state.borrow_mut().consumed += 1;
        Ok(())
    }
    fn retire(&mut self, _: &str, _: &SlotEnvelope, _: &str) -> ChannelResult<()> {
        log(&self.events, "retire");
        check_control(self.control)?;
        self.state.borrow_mut().retired += 1;
        Ok(())
    }
    fn close(self: Box<Self>) -> ChannelResult<()> {
        log(&self.events, "close");
        let mut state = self.state.borrow_mut();
        assert!(state.held);
        state.held = false;
        state.close_error.clone().map_or(Ok(()), Err)
    }
}
struct FakeSender {
    publications: Rc<RefCell<PublicationState>>,
    events: Events,
    queued: u32,
    failure: Option<ChannelError>,
    after_first: Option<Box<dyn FnMut()>>,
}
impl Sender for FakeSender {
    fn send(
        &mut self,
        control: &InputControl,
        _: MailboxBindings,
        action: MailboxAction,
        guard: &mut dyn FnMut(&InputControl) -> ChannelResult<()>,
    ) -> Result<InputReceipt, InputFailure<ChannelError>> {
        log(&self.events, "send");
        assert_eq!(
            self.publications.borrow().held,
            action == MailboxAction::Invoke
        );
        let child = control.ordered_child(control.deadline()).unwrap();
        // n=0 is engine admission; n=1 is still BEFORE the first post.
        // Scripted transport failure occurs only after `queued` normal posts.
        let mut posted = 0;
        for n in 0..=self.queued {
            if let Err(cause) = guard(&child) {
                return Err(InputFailure {
                    cause: Some(InputError::Guard(cause)),
                    cleanup_errors: vec![],
                    receipt: InputReceipt {
                        messages_queued: posted,
                        submission_complete: false,
                        release_complete: true,
                    },
                });
            }
            if n > 0 {
                posted += 1;
            }
            if n == 0 {
                if let Some(callback) = &mut self.after_first {
                    callback();
                }
            }
        }
        let receipt = InputReceipt {
            messages_queued: posted,
            submission_complete: self.failure.is_none(),
            release_complete: true,
        };
        match self.failure.take() {
            Some(cause) => Err(InputFailure {
                cause: Some(InputError::Guard(cause)),
                cleanup_errors: vec!["cleanup-fault".into()],
                receipt,
            }),
            None => Ok(receipt),
        }
    }
}
#[derive(Clone)]
struct FakeClock {
    now: Rc<RefCell<Instant>>,
    uptime: Rc<RefCell<i64>>,
    events: Events,
    wait_error: Option<ChannelError>,
}
impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.now.borrow()
    }
    fn uptime(&self) -> i64 {
        *self.uptime.borrow()
    }
    fn wait(&mut self, c: &OperationControl, duration: Duration) -> ChannelResult<()> {
        log(&self.events, &format!("wait:{}", duration.as_millis()));
        check_control(c)?;
        assert!(c.deadline() <= self.now() + REPLACEMENT_LIMIT);
        if let Some(e) = &self.wait_error {
            return Err(e.clone());
        }
        *self.now.borrow_mut() += duration;
        *self.uptime.borrow_mut() += duration.as_millis() as i64;
        Ok(())
    }
}
type Core = NativeCore<FakeSource, FakePublications, FakeSender, FakeClock>;
fn make_core(records: Vec<ChannelResult<Vec<Record>>>) -> Core {
    let events = Events::default();
    let state = Rc::new(RefCell::new(PublicationState::default()));
    let g = events.clone();
    NativeCore {
        reader: TracedReader::new(
            FakeSource {
                queue: records.into(),
                events: events.clone(),
                close_error: None,
            },
            None,
        ),
        publications: FakePublications {
            state: state.clone(),
            events: events.clone(),
        },
        sender: FakeSender {
            publications: state,
            events: events.clone(),
            queued: 6,
            failure: None,
            after_first: None,
        },
        clock: FakeClock {
            now: Rc::new(RefCell::new(Instant::now())),
            uptime: Rc::new(RefCell::new(10_000)),
            events,
            wait_error: None,
        },
        guard: Box::new(move |c| {
            log(&g, "guard");
            check_control(c)
        }),
        release: "3.2.0".into(),
        consumer: "41/99".into(),
        pid: 41,
        creation: 99,
        witness: None,
        sample_wait: SampleWait::default(),
        diagnostic_exchange: String::new(),
    }
}
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(60))
}
fn identity(runtime: &str) -> Identity {
    Identity {
        schema: SCHEMA.into(),
        runtime: runtime.into(),
        next_slot: 1,
        slots: SLOT_COUNT,
        input_state: SCHEMA.into(),
        character: "Example".into(),
        realm: "Realm".into(),
        guid: "Player-1".into(),
        build: "120100".into(),
        product: "retail".into(),
        release: "3.2.0".into(),
        ..Identity::default()
    }
}
fn envelope(action: &str) -> SlotEnvelope {
    SlotEnvelope {
        schema: SCHEMA.into(),
        index: 1,
        runtime: "1".repeat(32),
        owner: "a".repeat(32),
        fence: 1,
        nonce: "b".repeat(32),
        ticket: "c".repeat(32),
        action: action.into(),
        guid: "Player-1".into(),
        build: "120100".into(),
        ..SlotEnvelope::default()
    }
}
fn sample(i: &Identity, at: i64) -> InputObservation {
    InputObservation {
        bindings: Some(InputBindings {
            wake: "ALT-CTRL-F12".into(),
            reload: "ALT-CTRL-F11".into(),
            ..InputBindings::default()
        }),
        schema: SCHEMA.into(),
        runtime: i.runtime.clone(),
        owner: i.owner.clone(),
        fence: i.fence,
        next_slot: i.next_slot,
        guid: i.guid.clone(),
        build: i.build.clone(),
        sample_millis: at,
        input_blocked: Some(false),
        ..InputObservation::default()
    }
}
fn record(kind: Kind, value: &impl serde::Serialize, runtime: &str, sequence: u32) -> Record {
    let payload = serde_json::to_vec(value).unwrap();
    Record {
        address: 0x1000,
        nonce: if kind == Kind::Input {
            runtime.into()
        } else {
            "0".repeat(32)
        },
        runtime: runtime.into(),
        ticket: "0".repeat(32),
        kind: kind as u8,
        state: 1,
        sequence,
        checksum: checksum(&payload),
        payload,
    }
}
fn input_record(e: &SlotEnvelope, at: i64, seq: u32) -> Record {
    let mut i = identity(&e.runtime);
    i.owner = e.owner.clone();
    i.fence = e.fence;
    i.next_slot = e.index;
    record(Kind::Input, &sample(&i, at), &i.runtime, seq)
}
fn action(core: &mut Core, c: &OperationControl, e: &SlotEnvelope) -> InputAction {
    InputAction {
        kind: if e.action == "reload" {
            InputKind::Reload
        } else {
            InputKind::Invoke
        },
        envelope: e.clone(),
        observation: Some(core.observe_input(c, e, 0, SCHEMA).unwrap()),
        capability: SCHEMA.into(),
        after_millis: 0,
    }
}
fn contains(error: &ChannelError, code: &str) -> bool {
    error.to_string().contains(code)
}
fn events(core: &Core) -> Vec<String> {
    core.clock.events.borrow().clone()
}

#[test]
fn native_invoke_holds_exact_publication_through_burst_and_consumes_no_claim() {
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
    let c = control();
    let a = action(&mut core, &c, &e);
    let out = core.input(&c, &a);
    assert_eq!(out.outcome.disposition, InputDisposition::Submitted);
    assert!(out.error.is_none());
    let events = events(&core);
    let at = |s: &str| events.iter().position(|e| e == s).unwrap();
    assert!(at("verify") < at("route") && at("route") < at("send") && at("send") < at("close"));
    assert_eq!(events.iter().filter(|e| e.as_str() == "read:5").count(), 2);
    assert_eq!(core.publications.state.borrow().consumed, 0);
    core.consumed(&c, &e).unwrap();
    assert_eq!(core.publications.state.borrow().consumed, 1);
}
#[test]
fn native_reload_checks_pool_and_closes_before_sender() {
    let e = envelope("reload");
    let r = input_record(&e, 10_000, 1);
    let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
    let c = control();
    let a = action(&mut core, &c, &e);
    let out = core.input(&c, &a);
    assert!(out.error.is_none());
    let events = events(&core);
    assert!(events.iter().position(|e| e == "inspect") < events.iter().position(|e| e == "close"));
    assert!(events.iter().position(|e| e == "close") < events.iter().position(|e| e == "send"));
    assert!(!events.iter().any(|e| e == "verify" || e == "route"));
}
#[test]
fn native_publication_route_and_reload_close_failures_never_send() {
    for failure in 0..3 {
        let e = envelope(if failure == 2 { "reload" } else { "prepare" });
        let mut core = make_core(vec![Ok(vec![input_record(&e, 10_000, 1)])]);
        let c = control();
        let a = action(&mut core, &c, &e);
        {
            let mut s = core.publications.state.borrow_mut();
            match failure {
                0 => s.verify_error = Some(external("bytes-changed")),
                1 => s.route_error = Some(external("route-changed")),
                _ => s.close_error = Some(external("close-failed")),
            }
        }
        let out = core.input(&c, &a);
        assert_eq!(out.outcome.disposition, InputDisposition::NotSent);
        assert!(!events(&core).iter().any(|e| e == "send"));
        assert!(!core.publications.state.borrow().held);
    }
}
#[test]
fn native_persisted_input_dto_has_no_witness_and_cannot_send() {
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let mut original = make_core(vec![Ok(vec![r])]);
    let c = control();
    let mut a = action(&mut original, &c, &e);
    let bytes = serde_json::to_vec(a.observation.as_ref().unwrap()).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("address"));
    a.observation = Some(serde_json::from_slice(&bytes).unwrap());
    let mut restored = make_core(vec![]);
    let out = restored.input(&c, &a);
    assert_eq!(out.outcome.messages_queued, 0);
    assert!(out.error.unwrap().is_pending());
    assert!(!events(&restored).iter().any(|e| e == "send"));
}
#[test]
fn native_input_witness_is_one_use_even_after_zero_or_partial_send() {
    for queued in [0, 2, 6] {
        let e = envelope("prepare");
        let r = input_record(&e, 10_000, 1);
        let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
        let c = control();
        let a = action(&mut core, &c, &e);
        core.sender.queued = queued;
        core.sender.failure = Some(pending());
        let out = core.input(&c, &a);
        assert_eq!(
            out.outcome.disposition,
            if queued == 0 {
                InputDisposition::NotSent
            } else {
                InputDisposition::Uncertain
            }
        );
        assert_eq!(out.outcome.retryable, queued == 0);
        assert!(contains(out.error.as_ref().unwrap(), "cleanup-fault"));
        let again = core.input(&c, &a);
        assert_eq!(again.outcome.messages_queued, 0);
        assert_eq!(
            events(&core)
                .iter()
                .filter(|e| e.as_str() == "send")
                .count(),
            1
        );
    }
}
#[test]
fn native_changed_current_publication_or_binding_blocks_before_messages() {
    for change in 0..4 {
        let e = envelope("prepare");
        let r = input_record(&e, 10_000, 1);
        let mut changed = r.clone();
        match change {
            0 => changed.address += 16,
            1 => {
                changed = input_record(&e, 10_001, 2);
            }
            2 | 3 => {
                let mut s: InputObservation = serde_json::from_slice(&r.payload).unwrap();
                if change == 2 {
                    s.bindings.as_mut().unwrap().wake = "ALT-CTRL-F10".into();
                } else {
                    s.input_blocked = Some(true);
                    s.reason = "combat".into();
                }
                changed = record(Kind::Input, &s, &e.runtime, 1);
            }
            _ => unreachable!(),
        }
        let mut core = make_core(vec![Ok(vec![r]), Ok(vec![changed])]);
        let c = control();
        let a = action(&mut core, &c, &e);
        let out = core.input(&c, &a);
        assert_eq!(out.outcome.messages_queued, 0);
        assert!(out.error.unwrap().is_pending());
    }
}
#[test]
fn native_input_refresh_lookup_preserves_path_and_cancellation_causes() {
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let fault = join(
        error("memory.mailbox_unavailable: path_changed"),
        ChannelError::Cancelled,
    );
    let mut core = make_core(vec![Ok(vec![r]), Err(fault)]);
    let c = control();
    let a = action(&mut core, &c, &e);
    let out = core.input(&c, &a);
    let error = out.error.unwrap();
    assert_eq!(out.outcome.messages_queued, 0);
    assert!(contains(&error, "path_changed") && contains(&error, "command.cancelled"));
}
#[test]
fn native_partial_input_and_publication_close_errors_are_joined() {
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
    let c = control();
    let a = action(&mut core, &c, &e);
    core.sender.queued = 3;
    core.sender.failure = Some(external("send-fault"));
    core.publications.state.borrow_mut().close_error = Some(external("lease-fault"));
    let out = core.input(&c, &a);
    let error = out.error.unwrap();
    assert_eq!(out.outcome.disposition, InputDisposition::Uncertain);
    for cause in ["send-fault", "cleanup-fault", "lease-fault"] {
        assert!(contains(&error, cause));
    }
}
#[test]
fn native_stale_grace_does_not_move_for_newer_expired_samples() {
    let e = envelope("prepare");
    let r = input_record(&e, 9000, 1);
    let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
    let c = control();
    let first = core.observe_input(&c, &e, 0, SCHEMA).unwrap_err();
    assert!(contains(&first, "input_observation_stale"));
    *core.clock.now.borrow_mut() += Duration::from_secs(2);
    let later = core.observe_input(&c, &e, 0, SCHEMA).unwrap_err();
    assert!(later.is_pending() && !contains(&later, "input_observation_stale"));
    assert!(core.witness.is_none());
}
#[test]
fn native_missing_changed_and_blocked_input_do_not_forge_execution() {
    let e = envelope("prepare");
    let mut wrong = e.clone();
    wrong.owner = "d".repeat(32);
    let mut core = make_core(vec![Ok(vec![]), Ok(vec![input_record(&wrong, 10_000, 1)])]);
    let c = control();
    assert!(
        core.observe_input(&c, &e, 0, SCHEMA)
            .unwrap_err()
            .is_pending()
    );
    assert!(
        core.observe_input(&c, &e, 0, SCHEMA)
            .unwrap_err()
            .is_pending()
    );
    assert!(core.witness.is_none());
}
#[test]
fn native_publish_rechecks_guard_under_lock_and_joins_checked_close() {
    let mut core = make_core(vec![]);
    let c = control();
    let e = envelope("bind");
    core.publications.state.borrow_mut().close_error = Some(external("close-fault"));
    let mut checks = 0;
    core.guard = Box::new(move |_| {
        checks += 1;
        if checks == 2 {
            Err(external("admission-changed"))
        } else {
            Ok(())
        }
    });
    let error = core.publish(&c, &e).unwrap_err();
    assert!(contains(&error, "admission-changed") && contains(&error, "close-fault"));
    assert!(!events(&core).iter().any(|e| e == "publish"));
    assert!(!core.publications.state.borrow().held);
}
#[test]
fn native_original_cancellation_prevents_input_and_never_extends_deadline() {
    let cancellation = TrackedCancellation::new();
    let c = OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        cancellation.clone(),
    );
    let child = InputControl(c.clone())
        .ordered_child(Instant::now() + Duration::from_secs(60))
        .unwrap();
    assert_eq!(child.deadline(), c.deadline());
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let mut core = make_core(vec![Ok(vec![r])]);
    let a = action(&mut core, &c, &e);
    cancellation.cancel();
    let out = core.input(&c, &a);
    assert_eq!(out.outcome.messages_queued, 0);
    assert!(
        out.error
            .unwrap()
            .any_cause(&|e| matches!(e, ChannelError::Cancelled))
    );
    assert!(!events(&core).iter().any(|e| e == "acquire" || e == "send"));
}
#[test]
fn native_retired_actions_and_capabilities_do_not_acquire_or_send() {
    let e = envelope("prepare");
    let c = control();
    for (kind, capability) in [
        (InputKind::Escape, SCHEMA),
        (InputKind::Invoke, PREVIOUS_SCHEMA),
    ] {
        let mut core = make_core(vec![]);
        let out = core.input(
            &c,
            &InputAction {
                kind,
                envelope: e.clone(),
                observation: None,
                capability: capability.into(),
                after_millis: 0,
            },
        );
        assert_eq!(out.outcome.messages_queued, 0);
        assert!(events(&core).is_empty());
    }
}
fn replacement_records(i: &Identity, first: i64, latest: i64) -> Vec<ChannelResult<Vec<Record>>> {
    vec![
        Ok(vec![record(Kind::Identity, i, &i.runtime, 1)]),
        Ok(vec![record(Kind::Input, &sample(i, first), &i.runtime, 1)]),
        Ok(vec![record(Kind::Identity, i, &i.runtime, 1)]),
        Ok(vec![record(Kind::Input, &sample(i, latest), &i.runtime, 2)]),
    ]
}
#[test]
fn native_retirement_requires_two_fresh_advancing_publications_in_same_process() {
    let old = identity(&"1".repeat(32));
    let current = identity(&"2".repeat(32));
    let mut core = make_core(replacement_records(&current, 10_000, 11_100));
    let c = control();
    let proof = core.observe_runtime_replacement(&c, &old).unwrap().unwrap();
    proof.validate(&old).unwrap();
    assert_eq!(proof.process_id, 41);
    assert_eq!(proof.process_started_at, 99);
    assert_eq!(proof.witness.first.sample_millis, 10_000);
    assert_eq!(proof.witness.observation.sample_millis, 11_100);
    assert_eq!(proof.witness.address, 0x1000);
    assert!(events(&core).iter().any(|e| e == "wait:1100"));
    assert!(!events(&core).iter().any(|e| e == "send" || e == "acquire"));
    core.retire_replaced_runtime(&c, &envelope("prepare"), &proof)
        .unwrap();
    assert_eq!(core.publications.state.borrow().retired, 1);
}
#[test]
fn native_retirement_retained_or_stale_input_never_proves_destruction() {
    let old = identity(&"1".repeat(32));
    let current = identity(&"2".repeat(32));
    for (first, latest) in [(9000, 11_100), (10_000, 10_000)] {
        let mut core = make_core(replacement_records(&current, first, latest));
        assert!(
            core.observe_runtime_replacement(&control(), &old)
                .unwrap()
                .is_none()
        );
        assert_eq!(core.publications.state.borrow().retired, 0);
    }
}
#[test]
fn native_retirement_invalid_process_and_shape_stop_before_wait() {
    let old = identity(&"1".repeat(32));
    let current = identity(&"2".repeat(32));
    let mut core = make_core(replacement_records(&current, 10_000, 11_100));
    core.pid = 0;
    assert!(core.observe_runtime_replacement(&control(), &old).is_err());
    assert!(events(&core).is_empty());
    let mut invalid = current.clone();
    invalid.owner = "a".repeat(32);
    invalid.fence = MAX_EXACT_INTEGER + 1;
    let mut core = make_core(replacement_records(&invalid, 10_000, 11_100));
    assert!(
        core.observe_runtime_replacement(&control(), &old)
            .unwrap()
            .is_none()
    );
    assert!(!events(&core).iter().any(|e| e.starts_with("wait:")));
}
#[test]
fn native_retirement_publication_gap_and_wait_stop_keep_original_causes() {
    let old = identity(&"1".repeat(32));
    let current = identity(&"2".repeat(32));
    let mut records = replacement_records(&current, 10_000, 11_100);
    records[2] = Err(error("memory.mailbox_unavailable: path_changed"));
    let mut core = make_core(records);
    assert!(contains(
        &core
            .observe_runtime_replacement(&control(), &old)
            .unwrap_err(),
        "path_changed"
    ));
    assert_eq!(
        events(&core)
            .iter()
            .filter(|e| e.starts_with("read:"))
            .count(),
        3
    );
    let mut core = make_core(replacement_records(&current, 10_000, 11_100));
    core.clock.wait_error = Some(ChannelError::Cancelled);
    let error = core
        .observe_runtime_replacement(&control(), &old)
        .unwrap_err();
    assert!(error.is_pending() && error.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
}
#[test]
fn native_candidate_is_untrusted_and_requires_fixed_actor_and_newer_token() {
    let old = identity(&"1".repeat(32));
    let current = identity(&"2".repeat(32));
    let mut core = make_core(vec![Ok(vec![record(
        Kind::Identity,
        &current,
        &current.runtime,
        1,
    )])]);
    assert_eq!(
        core.runtime_candidate(&control(), &old).unwrap(),
        Some(current)
    );
    assert!(
        !events(&core)
            .iter()
            .any(|e| e == "send" || e == "retire" || e == "acquire")
    );
}
#[test]
fn native_receipt_diagnostic_probe_once_per_exchange_is_not_receipt_or_input_authority() {
    let e = envelope("prepare");
    let mut i = identity(&e.runtime);
    i.owner = e.owner.clone();
    i.fence = e.fence;
    i.next_slot = 2;
    let mut s = sample(&i, 10_000);
    s.input_attempt = Some(InputDiagnostic {
        schema: SCHEMA.into(),
        runtime: i.runtime.clone(),
        attempt_seq: 1,
        slot: 1,
        stage: "load_failed".into(),
        reason: "slot_addon_disabled".into(),
        received: false,
        sample_millis: 10_000,
    });
    let mut core = make_core(vec![
        Ok(vec![]),
        Ok(vec![record(Kind::Input, &s, &i.runtime, 2)]),
        Ok(vec![]),
    ]);
    let q = ObservationQuery {
        kind: ObservationKind::Receipt,
        envelope: e,
        identity: i,
    };
    let c = control();
    let out = core.observe(&c, &q);
    assert!(matches!(
        out.observation,
        Some(Observation::Pending {
            diagnostic: Some(_)
        })
    ));
    core.observe(&c, &q);
    assert_eq!(
        events(&core)
            .iter()
            .filter(|e| e.as_str() == "read:5")
            .count(),
        1
    );
    assert!(core.witness.is_none());
    assert_eq!(core.publications.state.borrow().consumed, 0);
}
#[test]
fn native_close_is_one_shot_and_keeps_business_plus_cleanup_errors() {
    let events = Events::default();
    let mut source = TracedReader::new(
        FakeSource {
            queue: VecDeque::new(),
            events: events.clone(),
            close_error: Some(external("process-close")),
        },
        None,
    );
    let result: ChannelResult<()> = finish(Err(external("business-fault")), source.close());
    let error = result.unwrap_err();
    assert!(contains(&error, "business-fault") && contains(&error, "process-close"));
    source.close().unwrap();
    assert_eq!(events.borrow().as_slice(), ["reader_close"]);
    assert!(
        source
            .find(
                &control(),
                &selector(Kind::Identity, ZERO_TOKEN),
                None,
                true
            )
            .is_err()
    );
}

#[test]
fn native_retirement_fixed_go_discriminator_matrix_and_relocation() {
    let cases = [
        "valid",
        "freshness_boundary",
        "future_boundary",
        "sequence_stalled",
        "old_runtime",
        "initial_expired",
        "latest_expired",
        "future",
        "identity_guid",
        "identity_owner",
        "identity_fence",
        "identity_slot",
        "build_changed",
        "product_changed",
        "release_changed",
        "no_capability",
        "nonce",
        "ticket",
        "kind",
        "sequence_zero",
        "sequence_wrap",
        "payload_limit",
        "missing_observation",
        "invalid_json",
        "runtime_changed",
    ];
    for mode in cases {
        let old = identity(&"1".repeat(32));
        let mut current = identity(&"2".repeat(32));
        let mut next = current.clone();
        let mut initial = sample(&current, 10_000);
        let mut latest = sample(&current, 11_100);
        let mut sequence = 2;
        match mode {
            "freshness_boundary" => {
                initial.sample_millis = 9500;
                latest.sample_millis = 10600;
            }
            "future_boundary" => latest.sample_millis = 11200,
            "sequence_stalled" => sequence = 1,
            "old_runtime" => {
                current = old.clone();
                initial = sample(&old, 10_000);
            }
            "initial_expired" => initial.sample_millis = 9499,
            "latest_expired" => latest.sample_millis = 10599,
            "future" => latest.sample_millis = 11201,
            "identity_guid" => latest.guid = "other-character".into(),
            "identity_owner" => {
                latest.owner = "3".repeat(32);
                latest.fence = 1;
            }
            "identity_fence" => {
                next.owner = "3".repeat(32);
                next.fence = 2;
                latest.owner = next.owner.clone();
                latest.fence = 1;
            }
            "identity_slot" => latest.next_slot += 1,
            "build_changed" => {
                next.build = "other-build".into();
                latest.build = next.build.clone();
            }
            "product_changed" => next.product = "classic".into(),
            "release_changed" => next.release = "other-release".into(),
            "no_capability" => next.input_state = "future-capability".into(),
            "sequence_zero" => sequence = 0,
            "sequence_wrap" => sequence = u32::MAX,
            "payload_limit" => {
                latest.reason = "x".repeat(2200);
                latest.input_blocked = Some(true);
            }
            "missing_observation" => latest.input_blocked = None,
            "runtime_changed" => {
                next.runtime = "3".repeat(32);
                latest.runtime = next.runtime.clone();
            }
            _ => {}
        }
        let before = record(Kind::Input, &initial, &current.runtime, 1);
        let mut after = record(Kind::Input, &latest, &next.runtime, sequence);
        after.address = 0x4000;
        match mode {
            "nonce" => after.nonce = "9".repeat(32),
            "ticket" => after.ticket = "9".repeat(32),
            "kind" => after.kind = Kind::Identity as u8,
            "invalid_json" => {
                after.payload = b"{".to_vec();
                after.checksum = checksum(&after.payload);
            }
            _ => {}
        }
        let mut core = make_core(vec![
            Ok(vec![record(Kind::Identity, &current, &current.runtime, 1)]),
            Ok(vec![before]),
            Ok(vec![record(Kind::Identity, &next, &next.runtime, 1)]),
            Ok(vec![after]),
        ]);
        let result = core.observe_runtime_replacement(&control(), &old);
        // Unlike the helper-only Go case, production discovery itself rejects
        // an incompatible release before the eligibility helper is entered.
        if mode == "release_changed" {
            assert!(result.is_err(), "{mode}");
            continue;
        }
        let proof = result.unwrap_or_else(|e| panic!("{mode}: {e}"));
        if matches!(mode, "valid" | "freshness_boundary" | "future_boundary") {
            let proof = proof.unwrap_or_else(|| panic!("{mode}: missing proof"));
            assert_eq!(proof.witness.address, 0x4000);
            proof.validate(&old).unwrap();
        } else {
            assert!(proof.is_none(), "{mode}: accepted unsafe replacement");
        }
        assert!(
            events(&core)
                .iter()
                .filter(|e| e.starts_with("read:"))
                .count()
                <= 4
        );
    }
}

#[test]
fn native_runtime_retirement_wrong_target_never_touches_disk() {
    let old = identity(&"1".repeat(32));
    let current = identity(&"2".repeat(32));
    let mut core = make_core(replacement_records(&current, 10_000, 11_100));
    let c = control();
    let proof = core.observe_runtime_replacement(&c, &old).unwrap().unwrap();
    for change in 0..4 {
        let mut bad = proof.clone();
        match change {
            0 => bad.process_id += 1,
            1 => bad.process_started_at += 1,
            2 => bad.current.runtime = old.runtime.clone(),
            _ => bad.current.release = "other".into(),
        }
        assert!(
            core.retire_replaced_runtime(&c, &envelope("prepare"), &bad)
                .is_err()
        );
    }
    assert_eq!(core.publications.state.borrow().retired, 0);
    assert!(!events(&core).iter().any(|e| e == "acquire"));
}

#[test]
fn native_previous_two_key_schema_allows_only_fresh_reload() {
    let e = envelope("reload");
    let mut s: InputObservation =
        serde_json::from_slice(&input_record(&e, 10_000, 1).payload).unwrap();
    s.schema = PREVIOUS_SCHEMA.into();
    let r = record(Kind::Input, &s, &e.runtime, 1);
    let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
    let c = control();
    let observation = core.observe_input(&c, &e, 0, PREVIOUS_SCHEMA).unwrap();
    let out = core.input(
        &c,
        &InputAction {
            kind: InputKind::Reload,
            envelope: e,
            observation: Some(observation),
            capability: PREVIOUS_SCHEMA.into(),
            after_millis: 0,
        },
    );
    assert_eq!(out.outcome.disposition, InputDisposition::Submitted);
    assert!(out.error.is_none());
}

#[test]
fn native_expired_control_after_fresh_read_cannot_send_or_consume() {
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let mut core = make_core(vec![Ok(vec![r])]);
    let a = action(&mut core, &control(), &e);
    let expired = OperationControl::uncancelable(Instant::now() - Duration::from_secs(1));
    let out = core.input(&expired, &a);
    assert_eq!(out.outcome.disposition, InputDisposition::NotSent);
    assert!(
        out.error
            .unwrap()
            .any_cause(&|e| matches!(e, ChannelError::DeadlineExceeded))
    );
    assert!(!events(&core).iter().any(|e| e == "send" || e == "consumed"));
}

#[test]
fn native_open_legacy_identity_remains_explicit_manual_activation() {
    let mut legacy = identity(&"2".repeat(32));
    legacy.schema = crate::go_mailbox_v1::LEGACY_IDENTITY_SCHEMA.into();
    legacy.input_state = crate::go_mailbox_v1::LEGACY_INPUT_SCHEMA.into();
    legacy.slots = 200;
    let mut core = make_core(vec![Ok(vec![record(
        Kind::Identity,
        &legacy,
        &legacy.runtime,
        1,
    )])]);
    let error = core.discover(&control(), "", "").unwrap_err();
    assert!(
        contains(&error, "mailbox_incompatible")
            && contains(&error, "legacy_identity_requires_activation")
    );
    assert!(!events(&core).iter().any(|e| e == "send"));
}

#[test]
fn native_guard_failure_during_cancellation_retains_both_causes() {
    let cancellation = TrackedCancellation::new();
    let c = OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        cancellation.clone(),
    );
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let mut core = make_core(vec![Ok(vec![r])]);
    let a = action(&mut core, &c, &e);
    let mut calls = 0;
    core.guard = Box::new(move |_| {
        calls += 1;
        if calls == 2 {
            cancellation.cancel();
            Err(external("ownership-changed"))
        } else {
            Ok(())
        }
    });
    let out = core.input(&c, &a);
    let error = out.error.unwrap();
    assert_eq!(out.outcome.messages_queued, 0);
    assert!(contains(&error, "ownership-changed"));
    assert!(error.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
}

#[test]
fn native_cancel_between_admission_and_first_post_is_still_known_zero() {
    let cancellation = TrackedCancellation::new();
    let c = OperationControl::tracked(
        Instant::now() + Duration::from_secs(30),
        cancellation.clone(),
    );
    let e = envelope("prepare");
    let r = input_record(&e, 10_000, 1);
    let mut core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
    let a = action(&mut core, &c, &e);
    core.sender.after_first = Some(Box::new(move || cancellation.cancel()));
    let out = core.input(&c, &a);
    assert_eq!(out.outcome.messages_queued, 0);
    assert_eq!(out.outcome.disposition, InputDisposition::NotSent);
    assert!(
        out.error
            .unwrap()
            .any_cause(&|e| matches!(e, ChannelError::Cancelled))
    );
}

#[test]
fn native_driver_recovery_watermark_can_tighten_same_witness_at_exact_future_boundary() {
    use crate::go_mailbox_v1::{
        driver,
        journal::StateJournal,
        state::{CHANNEL_SCHEMA, InputAttempt, Operation, State},
    };
    struct Journal {
        events: Events,
        saved: Vec<(String, State)>,
    }
    impl StateJournal for Journal {
        fn append(
            &mut self,
            c: &OperationControl,
            event: &str,
            state: &State,
        ) -> ChannelResult<()> {
            check_control(c)?;
            log(&self.events, &format!("save:{event}"));
            self.saved.push((event.into(), state.clone()));
            Ok(())
        }
        fn historical_request(
            &mut self,
            _: &OperationControl,
            _: &str,
        ) -> ChannelResult<Option<State>> {
            Ok(None)
        }
        fn write_result(&mut self, _: &OperationControl, _: &Operation) -> ChannelResult<()> {
            Ok(())
        }
        fn checkpoint(
            &mut self,
            _: &OperationControl,
            _: &mut State,
            _: bool,
        ) -> ChannelResult<()> {
            Ok(())
        }
    }
    struct WallClock;
    impl driver::Clock for WallClock {
        fn unix_millis(&self) -> i64 {
            1_000_000
        }
    }
    struct Tokens(u64);
    impl driver::TokenSource for Tokens {
        fn token(&mut self) -> ChannelResult<String> {
            self.0 += 1;
            Ok(format!("{:032x}", self.0))
        }
    }
    let e = envelope("prepare");
    let mut i = identity(&e.runtime);
    i.owner = e.owner.clone();
    i.fence = e.fence;
    let r = input_record(&e, 10_100, 1);
    let core = make_core(vec![Ok(vec![r.clone()]), Ok(vec![r])]);
    let trace = core.clock.events.clone();
    let state = State {
        schema: CHANNEL_SCHEMA.into(),
        id: format!("CON-{}", e.owner),
        owner: e.owner.clone(),
        identity: i,
        bound: true,
        input: Some(InputAttempt {
            id: "e".repeat(32),
            exchange: "d".repeat(32),
            runtime: e.runtime.clone(),
            kind: InputKind::Invoke,
            after_millis: 0,
            recovery_after_millis: 0,
            escape_attempts: 0,
            observation: None,
            outcome: None,
            after_diagnostic: None,
        }),
        ..State::default()
    };
    let mut driver = driver::Driver::from_state(
        state,
        core,
        Journal {
            events: trace.clone(),
            saved: Vec::new(),
        },
        WallClock,
        Tokens(0),
    )
    .unwrap();
    driver
        .submit_input(&control(), &e.nonce, &e.runtime, InputKind::Invoke, &e)
        .unwrap();
    let recovered = driver
        .journal
        .saved
        .iter()
        .find(|(event, _)| event == "input_time_recovered")
        .unwrap();
    assert_eq!(
        recovered.1.input.as_ref().unwrap().recovery_after_millis,
        10_000
    );
    let current = driver.state.input.as_ref().unwrap();
    assert_eq!(current.after_millis, 10_000);
    assert_eq!(
        current.outcome.as_ref().unwrap().disposition,
        InputDisposition::Submitted
    );
    let events = trace.borrow();
    assert!(
        events.iter().position(|e| e == "save:input_intent")
            < events.iter().position(|e| e == "send")
    );
    assert_eq!(events.iter().filter(|e| e.as_str() == "send").count(), 1);
}

// These traverse the actual continuation and actual NativeCore replacement
// adapter together. FakeSource replaces only OS reads; ordered-child admission
// and guarded two-sample retirement are the production implementations.
fn retirement_driver(
    core: Core,
) -> crate::go_mailbox_v1::driver::Driver<
    Core,
    crate::go_mailbox_v1::recovery::test_support::Journal,
    crate::go_mailbox_v1::recovery::test_support::FakeClock,
    crate::go_mailbox_v1::recovery::test_support::Tokens,
> {
    use crate::go_mailbox_v1::{driver::Driver, recovery::test_support as support};
    let mut driver = Driver::new(
        identity(&"1".repeat(32)),
        core,
        support::Journal::default(),
        support::FakeClock(Rc::new(std::cell::Cell::new(1000))),
        support::Tokens(10),
    )
    .unwrap();
    driver.state.bound = true;
    driver.state.identity.owner = driver.state.owner.clone();
    driver.state.identity.fence = 1;
    driver
}
#[test]
fn continuation_to_actual_native_retirement_keeps_ordered_parent_and_shortest_deadline() {
    use crate::go_mailbox_v1::{
        recovery::test_support::Schedule, runtime_retirement::RetirementTarget,
    };
    let current = identity(&"2".repeat(32));
    let mut core = make_core(replacement_records(&current, 10_000, 11_100));
    let parent = OperationControl::uncancelable(Instant::now() + Duration::from_secs(5));
    let deadline = parent.deadline();
    let observed = Rc::new(std::cell::Cell::new(0));
    let checked = observed.clone();
    core.guard = Box::new(move |control| {
        assert_eq!(control.deadline(), deadline);
        control
            .ordered_child(Instant::now() + Duration::from_secs(60))
            .map_err(toolkit_error)?;
        checked.set(checked.get() + 1);
        check_control(control)
    });
    let mut driver = retirement_driver(core);
    driver.request_close(&parent).unwrap();
    let close_deadline = driver.state.close_budget.as_ref().unwrap().deadline_ms;
    driver
        .continue_or_retire(
            &parent,
            RetirementTarget {
                process_id: 41,
                process_started_at: 99,
            },
            true,
            &mut Schedule::default(),
        )
        .unwrap();
    assert!(driver.state.closed && driver.state.runtime_end.is_some());
    assert_eq!(
        driver.state.close_budget.as_ref().unwrap().deadline_ms,
        close_deadline
    );
    assert!(observed.get() >= 2);
    assert!(
        !events(&driver.backend)
            .iter()
            .any(|event| event == "send" || event == "acquire")
    );
    driver.backend.close().unwrap();
}
#[test]
fn continuation_native_retirement_observes_original_parent_cancellation() {
    use crate::go_mailbox_v1::{
        recovery::test_support::Schedule, runtime_retirement::RetirementTarget,
    };
    let current = identity(&"2".repeat(32));
    let mut core = make_core(replacement_records(&current, 10_000, 11_100));
    let signal = TrackedCancellation::new();
    let parent = OperationControl::tracked(Instant::now() + Duration::from_secs(5), signal.clone());
    let observed = Rc::new(std::cell::Cell::new(false));
    let checked = observed.clone();
    core.guard = Box::new(move |control| {
        // Called after both production ordered_child boundaries, not before.
        checked.set(true);
        signal.cancel();
        check_control(control)
    });
    let mut driver = retirement_driver(core);
    driver.request_close(&parent).unwrap();
    let result = driver
        .continue_or_retire(
            &parent,
            RetirementTarget {
                process_id: 41,
                process_started_at: 99,
            },
            true,
            &mut Schedule::default(),
        )
        .unwrap_err();
    assert!(observed.get());
    assert!(result.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
    assert!(!driver.state.closed);
    assert!(driver.state.runtime_end.is_none());
    assert!(!events(&driver.backend).iter().any(|event| event == "send"));
    driver.backend.close().unwrap();
}
#[test]
fn continuation_native_retirement_keeps_deadline_before_late_parent_cancellation() {
    use crate::go_mailbox_v1::{
        recovery::test_support::Schedule, runtime_retirement::RetirementTarget,
    };
    let current = identity(&"2".repeat(32));
    let mut core = make_core(replacement_records(&current, 10_000, 11_100));
    let signal = TrackedCancellation::new();
    let parent = OperationControl::tracked(Instant::now() + Duration::from_secs(1), signal.clone());
    let deadline = parent.deadline();
    let observed = Rc::new(std::cell::Cell::new(false));
    let checked = observed.clone();
    core.guard = Box::new(move |control| {
        assert_eq!(control.deadline(), deadline);
        checked.set(true);
        std::thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(1),
        );
        signal.cancel();
        check_control(control)
    });
    let mut driver = retirement_driver(core);
    driver.request_close(&parent).unwrap();
    let result = driver
        .continue_or_retire(
            &parent,
            RetirementTarget {
                process_id: 41,
                process_started_at: 99,
            },
            true,
            &mut Schedule::default(),
        )
        .unwrap_err();
    assert!(observed.get());
    assert!(result.any_cause(&|e| matches!(e, ChannelError::DeadlineExceeded)));
    assert!(!result.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
    assert!(!driver.state.closed);
    driver.backend.close().unwrap();
}
