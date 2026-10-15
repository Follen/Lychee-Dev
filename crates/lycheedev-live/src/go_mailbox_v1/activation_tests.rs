//! Persistent project fixtures plus an explicitly modeled protocol peer. No
//! Windows/game/key acceptance is claimed by these tests.
use super::*;
use crate::go_mailbox_v1::{
    Identity,
    admission::{
        ClientRecord, ConnectionBook, ConnectionTarget, WindowRecord, inspect_window_owner,
    },
    connection::{
        ConnectionBackend, ConnectionEnvironment, ConnectionService, TargetRequest,
        activation_service::{ActivationBackend, ActivationEnvironment},
    },
    durable_journal::WindowOwner,
    observations::{
        Backend, InputAction, InputDispatch, ObservationDispatch, ObservationQuery, check_control,
    },
    recovery::test_support::{FakeClock, Peer, Schedule, Tokens, identity},
    runtime_retirement::RuntimeRetirementBackend,
    state::{ProcessEnd, RuntimeReplacementProof, SlotEnvelope},
};
use lycheedev_core::control::OperationControl;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    target: ConnectionTarget,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "formal-activation-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let client = root.join("client");
        fs::create_dir_all(client.join("Interface/AddOns")).unwrap();
        let target = ConnectionTarget {
            client: ClientRecord {
                directory: client.display().to_string(),
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
        Self { root, target }
    }
    fn request(&self) -> TargetRequest {
        TargetRequest {
            installation: self.target.client.directory.clone(),
            pid: 17,
            ..Default::default()
        }
    }
    fn path(&self, id: &str) -> PathBuf {
        self.root
            .join(".lycheedev/live/connections")
            .join(format!("{id}.activation.json"))
    }
    fn mutate(&self, id: &str, edit: impl FnOnce(&mut Activation)) {
        let path = self.path(id);
        let mut a: Activation = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        edit(&mut a);
        fs::write(path, serde_json::to_vec(&a).unwrap()).unwrap();
    }
    fn read(&self, id: &str) -> Activation {
        serde_json::from_slice(&fs::read(self.path(id)).unwrap()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[derive(Default)]
struct Counts {
    opens: usize,
    closes: usize,
    inputs: usize,
    actions: Vec<crate::go_mailbox_v1::state::InputKind>,
    candidates: Vec<Identity>,
    capability: bool,
    close_error: bool,
}
struct Env {
    target: ConnectionTarget,
    counts: Rc<RefCell<Counts>>,
}
struct Model {
    counts: Rc<RefCell<Counts>>,
    peer: Peer,
}
impl ConnectionEnvironment for Env {
    type Backend = Model;
    fn resolve(
        &mut self,
        _: &TargetRequest,
        c: &OperationControl,
    ) -> ChannelResult<ConnectionTarget> {
        check_control(c)?;
        Ok(self.target.clone())
    }
    fn open(
        &mut self,
        _: &ConnectionTarget,
        _: Option<&WindowOwner>,
        c: &OperationControl,
    ) -> ChannelResult<Model> {
        check_control(c)?;
        self.counts.borrow_mut().opens += 1;
        Ok(Model {
            counts: self.counts.clone(),
            peer: Peer::default(),
        })
    }
    fn process_end(
        &mut self,
        _: &ConnectionTarget,
        _: &OperationControl,
    ) -> ChannelResult<ProcessEnd> {
        Ok(ProcessEnd::None)
    }
    fn retire_process(
        &mut self,
        _: &ConnectionTarget,
        _: &str,
        _: &[SlotEnvelope],
        _: &OperationControl,
    ) -> ChannelResult<()> {
        panic!("activation cannot retire process resources")
    }
}
impl ActivationEnvironment for Env {
    fn guard_activation(
        &mut self,
        _: &ConnectionTarget,
        c: &OperationControl,
    ) -> ChannelResult<()> {
        check_control(c)
    }
}
impl ConnectionBackend for Model {
    fn discover(&mut self, c: &OperationControl, _: &str, _: &str) -> ChannelResult<Vec<Identity>> {
        check_control(c)?;
        Ok(self.counts.borrow().candidates.clone())
    }
    fn checked_close(&mut self) -> ChannelResult<()> {
        let mut c = self.counts.borrow_mut();
        c.closes += 1;
        if c.close_error {
            Err(error("fixture.close_failed"))
        } else {
            Ok(())
        }
    }
    fn scans(&self) -> &str {
        "fixture"
    }
}
impl ActivationBackend for Model {
    fn activation_capability(&mut self, _: &Identity, _: &OperationControl) -> ChannelResult<bool> {
        Ok(self.counts.borrow().capability)
    }
}
impl Backend for Model {
    fn observe(&mut self, c: &OperationControl, q: &ObservationQuery) -> ObservationDispatch {
        self.peer.observe(c, q)
    }
    fn publish(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.peer.publish(c, e)
    }
    fn observe_input(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        a: i64,
        cap: &str,
    ) -> ChannelResult<super::super::InputObservation> {
        self.peer.observe_input(c, e, a, cap)
    }
    fn input(&mut self, c: &OperationControl, a: &InputAction) -> InputDispatch {
        self.counts.borrow_mut().inputs += 1;
        self.counts.borrow_mut().actions.push(a.kind.clone());
        self.peer.input(c, a)
    }
    fn runtime_candidate(
        &mut self,
        c: &OperationControl,
        i: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        self.peer.runtime_candidate(c, i)
    }
    fn supersede(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        i: &Identity,
    ) -> ChannelResult<()> {
        self.peer.supersede(c, e, i)
    }
    fn consumed(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.peer.consumed(c, e)
    }
}
impl RuntimeRetirementBackend for Model {
    fn observe_runtime_replacement(
        &mut self,
        c: &OperationControl,
        i: &Identity,
    ) -> ChannelResult<Option<RuntimeReplacementProof>> {
        self.peer.observe_runtime_replacement(c, i)
    }
    fn retire_replaced_runtime(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        p: &RuntimeReplacementProof,
    ) -> ChannelResult<()> {
        self.peer.retire_replaced_runtime(c, e, p)
    }
}
type Service = ConnectionService<Env, FakeClock, Tokens, Schedule>;
fn service(f: &Fixture, counts: &Rc<RefCell<Counts>>, clock: &Rc<Cell<i64>>) -> Service {
    ConnectionService::open(
        &f.root,
        Env {
            target: f.target.clone(),
            counts: counts.clone(),
        },
        FakeClock(clock.clone()),
        Tokens(100),
        Schedule::default(),
    )
    .unwrap()
}
fn start(f: &Fixture, counts: &Rc<RefCell<Counts>>, clock: &Rc<Cell<i64>>) -> (Service, String) {
    let mut service = service(f, counts, clock);
    let outcome = service.activate(&f.request(), "activation-key", &OperationControl::default());
    assert!(outcome.error.is_some());
    let result = outcome.result.unwrap();
    assert_eq!(result.stage, "activation_waiting_manual_reload");
    (service, result.session)
}
#[test]
fn manual_activation_wait_is_read_only_unbudgeted_and_reuses_exact_con() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    let original = fs::read(f.path(&id)).unwrap();
    clock.set(9_000_000);
    let status = service
        .status_with_activation(&id, &OperationControl::default())
        .result
        .unwrap();
    assert_eq!(status.continuation.kind, "wait_external");
    assert_eq!(status.continuation.remaining_budget_ms, None);
    assert!(!status.complete);
    assert!(!status.bound);
    let retried = service
        .activate(&f.request(), "activation-key", &OperationControl::default())
        .result
        .unwrap();
    assert_eq!(retried.session, id);
    assert_eq!(counts.borrow().inputs, 0);
    assert_eq!(fs::read(f.path(&id)).unwrap(), original);
    assert!(
        fs::read_dir(f.path(&id).parent().unwrap())
            .unwrap()
            .all(|entry| {
                let name = entry.unwrap().file_name().to_string_lossy().into_owned();
                name.ends_with(".target.json") || name.ends_with(".activation.json")
            })
    );
}
#[test]
fn activation_disconnect_is_durable_idempotent_and_never_opens_native() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    let opens = counts.borrow().opens;
    for turn in 0..3 {
        let out = if turn == 1 {
            service.resume_with_activation(&id, &OperationControl::default())
        } else {
            service.disconnect_with_activation(&id, &OperationControl::default())
        };
        assert!(out.error.is_none(), "{:?}", out.error);
        let r = out.result.unwrap();
        assert!(r.closed);
        assert!(!r.complete);
        assert_eq!(r.stage, "activation_cancelled");
    }
    assert_eq!(counts.borrow().opens, opens);
    assert_eq!(counts.borrow().inputs, 0);
    assert!(
        !inspect_window_owner(
            &f.target.addon_parent(),
            &f.target.resource(),
            &OperationControl::default()
        )
        .occupied
    );
    assert_eq!(f.read(&id).phase, ActivationPhase::Cancelled);
}
#[test]
fn activation_cannot_cancel_unknown_input_or_fake_not_sent() {
    for kind in [
        "input_attempted",
        "runtime_selected",
        "nonzero",
        "uncertain",
        "submitted",
        "invalid_not_sent",
    ] {
        let f = Fixture::new();
        let counts = Rc::new(RefCell::new(Counts::default()));
        let clock = Rc::new(Cell::new(1000));
        let (mut service, id) = start(&f, &counts, &clock);
        f.mutate(&id, |a| {
            a.phase = ActivationPhase::Prepared;
            match kind {
                "input_attempted" => a.phase = ActivationPhase::InputAttempted,
                "runtime_selected" => a.phase = ActivationPhase::RuntimeSelected,
                "nonzero" => a.input_step = 1,
                _ => {
                    a.outcome = Some(InputOutcome {
                        disposition: if kind == "uncertain" {
                            InputDisposition::Uncertain
                        } else if kind == "submitted" {
                            InputDisposition::Submitted
                        } else {
                            InputDisposition::NotSent
                        },
                        messages_queued: 1,
                        at_millis: 0,
                        reason: String::new(),
                        retryable: false,
                    })
                }
            }
        });
        let bytes = fs::read(f.path(&id)).unwrap();
        let out = service.disconnect_with_activation(&id, &OperationControl::default());
        assert!(out.error.is_some(), "{kind}");
        assert!(out.result.is_none_or(|r| !r.closed));
        assert_eq!(bytes, fs::read(f.path(&id)).unwrap());
        assert_eq!(counts.borrow().inputs, 0);
        assert!(
            inspect_window_owner(
                &f.target.addon_parent(),
                &f.target.resource(),
                &OperationControl::default()
            )
            .occupied
        );
    }
}
#[test]
fn activation_expiry_stops_before_native_and_persists_sticky_budget() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    f.mutate(&id, |a| a.phase = ActivationPhase::InputAttempted);
    let opens = counts.borrow().opens;
    clock.set(1_000_000);
    let out = service.resume_with_activation(&id, &OperationControl::default());
    assert!(out.error.is_some());
    assert_eq!(out.result.unwrap().continuation.kind, "budget_exhausted");
    assert!(f.read(&id).budget.unwrap().exhausted);
    clock.set(1000);
    assert!(
        service
            .resume_with_activation(&id, &OperationControl::default())
            .error
            .is_some()
    );
    assert_eq!(counts.borrow().opens, opens);
}
#[test]
fn activation_clock_rollback_status_does_not_mutate_then_resume_persists() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    f.mutate(&id, |a| a.phase = ActivationPhase::Prepared);
    let bytes = fs::read(f.path(&id)).unwrap();
    clock.set(999);
    assert_eq!(
        service
            .status_with_activation(&id, &OperationControl::default())
            .result
            .unwrap()
            .continuation
            .kind,
        "budget_exhausted"
    );
    assert_eq!(fs::read(f.path(&id)).unwrap(), bytes);
    assert!(
        service
            .resume_with_activation(&id, &OperationControl::default())
            .error
            .is_some()
    );
    assert!(f.read(&id).budget.unwrap().clock_rollback);
}
#[test]
fn activation_unknown_legacy_attempt_never_resends_or_resets_deadline() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    f.mutate(&id, |a| {
        a.phase = ActivationPhase::InputAttempted;
        a.input_step = 1;
        a.prior_runtime = "2".repeat(32);
    });
    counts.borrow_mut().candidates = vec![identity('2')];
    let deadline = f.read(&id).budget.unwrap().deadline_ms;
    for _ in 0..2 {
        let outcome = service.resume_with_activation(&id, &OperationControl::default());
        assert!(outcome.error.is_some());
        assert_eq!(outcome.result.unwrap().stage, "activation_waiting_runtime");
    }
    assert_eq!(counts.borrow().inputs, 0);
    assert_eq!(f.read(&id).budget.unwrap().deadline_ms, deadline);
    assert_eq!(f.read(&id).phase, ActivationPhase::InputAttempted);
}
#[test]
fn activation_active_driver_returns_structured_blocker_without_mutation() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    let bytes = fs::read(f.path(&id)).unwrap();
    let book = ConnectionBook::open(&f.root).unwrap();
    let mut lease = book
        .acquire_activation(&id, &OperationControl::default())
        .unwrap();
    let out = service.resume_with_activation(&id, &OperationControl::default());
    let r = out.result.unwrap();
    assert!(out.error.is_some());
    assert_eq!(r.continuation.kind, "wait_active_driver");
    assert_eq!(r.continuation.blocker.unwrap().consumer, id);
    assert_eq!(fs::read(f.path(&id)).unwrap(), bytes);
    lease.close().unwrap();
}
#[test]
fn activation_old_generations_missing_phase_and_unsafe_manual_are_rejected() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (service, id) = start(&f, &counts, &clock);
    let original = f.read(&id);
    for schema in [
        "lycheedev.channel-activation.v1",
        "lycheedev.channel-activation.v2",
        "lycheedev.channel-activation.v4",
    ] {
        let mut a = original.clone();
        a.schema = schema.into();
        fs::write(f.path(&id), serde_json::to_vec(&a).unwrap()).unwrap();
        assert!(
            service
                .status_with_activation(&id, &OperationControl::default())
                .error
                .is_some()
        );
    }
    let mut value = serde_json::to_value(&original).unwrap();
    value.as_object_mut().unwrap().remove("phase");
    fs::write(f.path(&id), serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(
        service
            .status_with_activation(&id, &OperationControl::default())
            .error
            .is_some()
    );
    let mut a = original;
    a.outcome = Some(InputOutcome {
        disposition: InputDisposition::Submitted,
        messages_queued: 2,
        at_millis: 0,
        reason: String::new(),
        retryable: false,
    });
    assert!(a.validate().is_err());
}
#[test]
fn activation_diagnostic_close_error_is_joined_with_manual_wait() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts {
        close_error: true,
        ..Default::default()
    }));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    let out = service.resume_with_activation(&id, &OperationControl::default());
    let error = out.error.unwrap().to_string();
    assert!(error.contains("manual_reload_required"));
    assert!(error.contains("fixture.close_failed"));
    assert_eq!(counts.borrow().inputs, 0);
}
#[test]
fn cancelled_activation_preserves_replacement_owner() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    assert!(
        service
            .disconnect_with_activation(&id, &OperationControl::default())
            .error
            .is_none()
    );
    let new = service
        .activate(&f.request(), "different-key", &OperationControl::default())
        .result
        .unwrap();
    assert_ne!(new.session, id);
    assert!(
        service
            .resume_with_activation(&id, &OperationControl::default())
            .error
            .is_none()
    );
    let owner = inspect_window_owner(
        &f.target.addon_parent(),
        &f.target.resource(),
        &OperationControl::default(),
    )
    .owner
    .unwrap();
    assert_eq!(owner.operation_id, new.session);
}
#[test]
fn activation_manual_observation_handoff_retains_con_and_starts_budget_once() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    clock.set(9_000_000);
    counts.borrow_mut().candidates = vec![identity('3')];
    let out = service.resume_with_activation(&id, &OperationControl::default());
    assert!(out.error.is_none(), "{:?}", out.error);
    assert_eq!(out.result.unwrap().session, id);
    let book = ConnectionBook::open(&f.root).unwrap();
    let state = book
        .status(&id, &OperationControl::default())
        .unwrap()
        .state;
    assert_eq!(state.connect_budget.unwrap().started_at_ms, 9_000_000);
    assert!(state.bound);
    assert!(state.operation.is_none());
    assert_eq!(counts.borrow().inputs, 1);
    assert_eq!(f.read(&id).phase, ActivationPhase::RuntimeSelected);
}

#[test]
fn explicit_reload_completed_history_is_read_only_after_later_reload() {
    use crate::go_mailbox_v1::{
        journal::save_state,
        state::{ReloadAttempt, ReloadPhase},
    };
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    counts.borrow_mut().candidates = vec![identity('3')];
    assert!(
        service
            .resume_with_activation(&id, &OperationControl::default())
            .error
            .is_none()
    );
    let book = ConnectionBook::open(&f.root).unwrap();
    let (mut record, mut journal) = book.acquire(&id, &OperationControl::default()).unwrap();
    for key in ["first-reload", "later-reload"] {
        record.state.reload = Some(ReloadAttempt {
            automatic: false,
            recovery_budget: Some(budget(1000, DEFAULT_RECOVERY_MILLIS).unwrap()),
            reconcile_pending: false,
            request: key.into(),
            from: "2".repeat(32),
            phase: ReloadPhase::Complete,
            input_step: 0,
            patch_transitions: 0,
        });
        save_state(
            &mut journal,
            &OperationControl::default(),
            &mut record.state,
            "fixture_reload_complete",
        )
        .unwrap();
    }
    journal.close().unwrap();
    let opens = counts.borrow().opens;
    let inputs = counts.borrow().inputs;
    let result = service.reload(&id, "first-reload", &OperationControl::default());
    assert!(result.error.is_none());
    let result = result.result.unwrap();
    assert!(result.complete);
    assert_eq!(result.continuation.goal, "reload");
    assert_eq!(result.continuation.request_id, "first-reload");
    assert_eq!(result.report_state, "unavailable");
    assert_eq!(counts.borrow().opens, opens);
    assert_eq!(counts.borrow().inputs, inputs);
    assert_eq!(
        book.status(&id, &OperationControl::default())
            .unwrap()
            .state
            .reload
            .unwrap()
            .request,
        "later-reload"
    );
}

struct EmptyDiagnostic;
impl crate::go_mailbox_v1::observations::RecordReader for EmptyDiagnostic {
    fn find(
        &mut self,
        _: &OperationControl,
        _: &crate::go_mailbox_v1::Selector,
        _: Option<&mut dyn FnMut(&crate::mailbox::Record) -> bool>,
        _: bool,
    ) -> ChannelResult<Vec<crate::mailbox::Record>> {
        Ok(Vec::new())
    }
}
impl crate::go_mailbox_v1::doctor::DoctorRuntime for EmptyDiagnostic {
    fn uptime_millis(&self) -> i64 {
        1000
    }
    fn checked_close(&mut self) -> ChannelResult<()> {
        Ok(())
    }
}
impl crate::go_mailbox_v1::connection::doctor_service::DoctorEnvironment for Env {
    type Diagnostic = EmptyDiagnostic;
    fn release(&self) -> &str {
        "3.2.0"
    }
    fn confirm_target(&mut self, _: &ConnectionTarget, c: &OperationControl) -> ChannelResult<()> {
        check_control(c)
    }
    fn inspect_deployment(
        &mut self,
        _: &ConnectionTarget,
        c: &OperationControl,
    ) -> ChannelResult<crate::go_mailbox_v1::doctor::DoctorDeployment> {
        assert!(c.remaining().unwrap() <= std::time::Duration::from_secs(10));
        Ok(crate::go_mailbox_v1::doctor::DoctorDeployment {
            state: "absent".into(),
            ..Default::default()
        })
    }
    fn open_diagnostic(
        &mut self,
        _: &ConnectionTarget,
        _: &OperationControl,
    ) -> ChannelResult<EmptyDiagnostic> {
        self.counts.borrow_mut().opens += 1;
        Ok(EmptyDiagnostic)
    }
}
impl crate::go_mailbox_v1::connection::doctor_repair::RepairEnvironment for Env {
    fn open_for_repair(
        &mut self,
        target: &ConnectionTarget,
        owner: &WindowOwner,
        _: &str,
        c: &OperationControl,
    ) -> ChannelResult<Model> {
        self.open(target, Some(owner), c)
    }
}
#[test]
fn top_level_doctor_requires_explicit_target_and_caps_original_control() {
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let mut service = service(&f, &counts, &clock);
    assert!(
        service
            .top_level_live_doctor(&TargetRequest::default(), "", &OperationControl::default())
            .is_none()
    );
    assert_eq!(counts.borrow().opens, 0);
    let report = service
        .top_level_live_doctor(&f.request(), "", &OperationControl::default())
        .unwrap();
    assert!(!report.healthy);
    assert_eq!(counts.borrow().opens, 1);
    assert!(!f.root.join(".lycheedev").exists());
    assert_eq!(counts.borrow().inputs, 0);
}
#[test]
fn doctor_fix_delivery_failure_stops_before_activation_and_foreign_owner_stops_before_delivery() {
    use crate::go_mailbox_v1::{
        connection::doctor_repair::{DoctorFixOptions, RepairDelivery},
        doctor::{DoctorDeployment, DoctorResult},
    };
    struct Delivery {
        calls: usize,
    }
    impl RepairDelivery for Delivery {
        fn apply(
            &mut self,
            _: &ConnectionTarget,
            _: &DoctorDeployment,
            _: &Path,
            _: Option<&Path>,
            _: &OperationControl,
        ) -> ChannelResult<()> {
            self.calls += 1;
            Err(error("fixture.delivery_failed"))
        }
    }
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let mut service = service(&f, &counts, &clock);
    let mut report = DoctorResult {
        target: Some(f.target.clone()),
        deployment: Some(DoctorDeployment {
            state: "absent".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let options = DoctorFixOptions {
        release: Some(Path::new("release")),
        ..Default::default()
    };
    let mut delivery = Delivery { calls: 0 };
    assert!(
        service
            .doctor_fix(
                &report,
                &options,
                &mut delivery,
                &OperationControl::default()
            )
            .error
            .is_some()
    );
    assert_eq!(delivery.calls, 1);
    assert_eq!(counts.borrow().opens, 0);
    assert_eq!(counts.borrow().inputs, 0);
    assert!(!f.root.join(".lycheedev").exists());
    report.owner = Some(WindowOwner {
        schema: "lycheedev.window-owner.v1".into(),
        workspace_id: "f".repeat(32),
        resource: f.target.resource(),
        operation_id: format!("CON-{}", "a".repeat(32)),
        intent_sha256: "b".repeat(64),
    });
    assert!(
        service
            .doctor_fix(
                &report,
                &options,
                &mut delivery,
                &OperationControl::default()
            )
            .error
            .is_some()
    );
    assert_eq!(delivery.calls, 1);
}

#[test]
fn repair_closes_original_prepared_business_before_delivery_without_preparing_it() {
    use crate::go_mailbox_v1::{
        connection::doctor_repair::{DoctorFixOptions, RepairDelivery},
        doctor::{DoctorDeployment, DoctorResult},
        journal::save_state,
        state::{Operation, OperationPolicy, OperationStage},
    };
    struct Delivery {
        calls: usize,
    }
    impl RepairDelivery for Delivery {
        fn apply(
            &mut self,
            target: &ConnectionTarget,
            _: &DoctorDeployment,
            _: &Path,
            _: Option<&Path>,
            c: &OperationControl,
        ) -> ChannelResult<()> {
            self.calls += 1;
            assert!(!inspect_window_owner(&target.addon_parent(), &target.resource(), c).occupied);
            Err(error("fixture.delivery_stop_after_close"))
        }
    }
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    counts.borrow_mut().candidates = vec![identity('3')];
    assert!(
        service
            .resume_with_activation(&id, &OperationControl::default())
            .error
            .is_none()
    );
    let book = ConnectionBook::open(&f.root).unwrap();
    let (mut record, mut journal) = book.acquire(&id, &OperationControl::default()).unwrap();
    let mut operation = Operation::prepared(
        format!("LMO-{}", "5".repeat(32)),
        "6".repeat(32),
        "error('must not execute')".into(),
        5,
        OperationPolicy::Opaque,
    );
    operation.request = "original-business".into();
    operation.origin = Some(record.state.identity.clone());
    record.state.operation = Some(operation);
    save_state(
        &mut journal,
        &OperationControl::default(),
        &mut record.state,
        "fixture_prepared",
    )
    .unwrap();
    journal.close().unwrap();
    let report = DoctorResult {
        target: Some(f.target.clone()),
        owner: Some(record.metadata.owner),
        deployment: Some(DoctorDeployment {
            state: "managed".into(),
            version: "older".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let mut delivery = Delivery { calls: 0 };
    let outcome = service.doctor_fix(
        &report,
        &DoctorFixOptions {
            session: &id,
            release: Some(Path::new("release")),
            ..Default::default()
        },
        &mut delivery,
        &OperationControl::default(),
    );
    assert!(
        outcome
            .error
            .unwrap()
            .to_string()
            .contains("fixture.delivery_stop_after_close")
    );
    assert_eq!(delivery.calls, 1);
    let state = book
        .status(&id, &OperationControl::default())
        .unwrap()
        .state;
    assert!(state.closed);
    assert_eq!(state.operation.unwrap().stage, OperationStage::Cancelled);
    // Peer.publish asserts only bind/unbind. Any implicit prepare/commit path
    // would fail before the test reaches Delivery.
}

#[test]
fn explicit_reload_torn_current_log_never_repairs_even_with_completed_history() {
    use crate::go_mailbox_v1::{
        journal::save_state,
        state::{ReloadAttempt, ReloadPhase},
    };
    use std::io::Write;
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    counts.borrow_mut().candidates = vec![identity('3')];
    assert!(
        service
            .resume_with_activation(&id, &OperationControl::default())
            .error
            .is_none()
    );
    let book = ConnectionBook::open(&f.root).unwrap();
    let (mut record, mut journal) = book.acquire(&id, &OperationControl::default()).unwrap();
    record.state.reload = Some(ReloadAttempt {
        automatic: false,
        recovery_budget: Some(budget(1000, DEFAULT_RECOVERY_MILLIS).unwrap()),
        reconcile_pending: false,
        request: "completed".into(),
        from: "2".repeat(32),
        phase: ReloadPhase::Complete,
        input_step: 0,
        patch_transitions: 0,
    });
    save_state(
        &mut journal,
        &OperationControl::default(),
        &mut record.state,
        "fixture_reload_complete",
    )
    .unwrap();
    journal.close().unwrap();
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(record.paths.log_path())
        .unwrap();
    log.write_all(b"{torn-current-log").unwrap();
    log.sync_all().unwrap();
    drop(log);
    fn snapshot(path: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        fn walk(
            root: &std::path::Path,
            path: &std::path::Path,
            result: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>,
        ) {
            for entry in fs::read_dir(path).unwrap() {
                let p = entry.unwrap().path();
                if p.is_dir() {
                    walk(root, &p, result);
                } else {
                    result.insert(p.strip_prefix(root).unwrap().into(), fs::read(p).unwrap());
                }
            }
        }
        let mut result = Default::default();
        walk(path, path, &mut result);
        result
    }
    let before = snapshot(&f.root);
    let opens = counts.borrow().opens;
    let inputs = counts.borrow().inputs;
    for request in ["completed", "unknown"] {
        assert!(
            service
                .reload(&id, request, &OperationControl::default())
                .error
                .is_some()
        );
        assert_eq!(snapshot(&f.root), before);
    }
    assert_eq!(counts.borrow().opens, opens);
    assert_eq!(counts.borrow().inputs, inputs);
}

#[test]
fn activation_index_failure_keeps_orphan_records_without_publishing_owner_or_log() {
    let f = Fixture::new();
    let mut book = ConnectionBook::open(&f.root).unwrap();
    let index = f.root.join(".lycheedev/live/activations");
    fs::create_dir_all(index.parent().unwrap()).unwrap();
    fs::write(&index, b"fixture-block-index-parent").unwrap();
    let control = OperationControl::default();
    let result = book.create_activation(
        f.target.clone(),
        &f.request(),
        "key",
        &FakeClock(Rc::new(Cell::new(1000))),
        &mut Tokens(0),
        &control,
    );
    assert!(result.is_err());
    let id = format!("CON-{:032x}", 1);
    assert_eq!(
        book.activation_status(&id, &control)
            .unwrap()
            .activation
            .request,
        "key"
    );
    assert!(book.status(&id, &control).is_err());
    let owner = inspect_window_owner(&f.target.addon_parent(), &f.target.resource(), &control);
    assert!(owner.error.is_none());
    assert!(owner.owner.is_none());
    assert_eq!(fs::read(index).unwrap(), b"fixture-block-index-parent");
    assert!(
        fs::read_dir(f.path(&id).parent().unwrap())
            .unwrap()
            .all(|entry| {
                let name = entry.unwrap().file_name().to_string_lossy().into_owned();
                name.ends_with(".target.json") || name.ends_with(".activation.json")
            })
    );
}

#[test]
fn repair_publication_authenticates_original_durable_close_and_exact_unbind() {
    use crate::go_mailbox_v1::{
        connection::doctor_repair::authenticated_repair_publication,
        journal::save_state,
        state::{Transaction, TransactionPhase},
    };
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    counts.borrow_mut().candidates = vec![identity('3')];
    let control = OperationControl::default();
    assert!(
        service
            .resume_with_activation(&id, &control)
            .error
            .is_none()
    );
    let book = ConnectionBook::open(&f.root).unwrap();
    let (mut record, mut journal) = book.acquire(&id, &control).unwrap();
    let envelope = SlotEnvelope {
        schema: crate::go_mailbox_v1::SCHEMA.into(),
        index: record.state.identity.next_slot,
        runtime: record.state.identity.runtime.clone(),
        owner: record.state.owner.clone(),
        fence: record.state.identity.fence,
        nonce: "b".repeat(32),
        ticket: "c".repeat(32),
        action: "unbind".into(),
        guid: record.state.identity.guid.clone(),
        build: record.state.identity.build.clone(),
        ..SlotEnvelope::default()
    };
    let release = record.state.identity.release.clone();
    let authorize = |e: &SlotEnvelope| {
        authenticated_repair_publication(
            &book,
            &f.target,
            &record.metadata.owner,
            &release,
            e,
            &control,
        )
    };
    assert!(authorize(&envelope).is_err()); // Memory intent alone is never enough.
    record.state.closing = true;
    record.state.close_budget = Some(budget(1000, DEFAULT_RECOVERY_MILLIS).unwrap());
    record.state.transaction = Some(Transaction {
        envelope: envelope.clone(),
        phase: TransactionPhase::Intent,
        receipt: None,
        escape_attempts: 0,
        input_observation: None,
        release_wait_started_at_ms: 0,
    });
    assert!(authorize(&envelope).is_err());
    save_state(
        &mut journal,
        &control,
        &mut record.state,
        "fixture_durable_close_unbind",
    )
    .unwrap();
    let cap = authorize(&envelope).unwrap();
    assert_eq!(cap.envelope(), &envelope);
    assert_eq!(cap.consumer(), "17/19");
    let mut changed = envelope.clone();
    changed.nonce = "d".repeat(32);
    assert!(authorize(&changed).is_err());
    changed = envelope.clone();
    changed.action = "prepare".into();
    assert!(authorize(&changed).is_err());
    assert!(
        authenticated_repair_publication(
            &book,
            &f.target,
            &record.metadata.owner,
            "other-release",
            &envelope,
            &control
        )
        .is_err()
    );
    let mut other = f.target.clone();
    other.window.process_started_at += 1;
    assert!(
        authenticated_repair_publication(
            &book,
            &other,
            &record.metadata.owner,
            &release,
            &envelope,
            &control
        )
        .is_err()
    );
    // Removing authority in a later durable record invalidates a prior proof.
    record.state.closing = false;
    save_state(
        &mut journal,
        &control,
        &mut record.state,
        "fixture_close_removed",
    )
    .unwrap();
    assert!(authorize(&envelope).is_err());
    journal.close().unwrap();
}

#[test]
fn repair_unknown_business_never_republishes_or_dispatches_before_delivery() {
    use crate::go_mailbox_v1::{
        connection::doctor_repair::{DoctorFixOptions, RepairDelivery},
        doctor::{DoctorDeployment, DoctorResult},
        journal::save_state,
        state::{InputKind, Operation, OperationPolicy, Transaction, TransactionPhase},
    };
    struct NeverDelivery;
    impl RepairDelivery for NeverDelivery {
        fn apply(
            &mut self,
            _: &ConnectionTarget,
            _: &DoctorDeployment,
            _: &Path,
            _: Option<&Path>,
            _: &OperationControl,
        ) -> ChannelResult<()> {
            panic!("unconfirmed original close cannot authorize Delivery");
        }
    }
    let f = Fixture::new();
    let counts = Rc::new(RefCell::new(Counts::default()));
    let clock = Rc::new(Cell::new(1000));
    let (mut service, id) = start(&f, &counts, &clock);
    counts.borrow_mut().candidates = vec![identity('3')];
    let control = OperationControl::default();
    assert!(
        service
            .resume_with_activation(&id, &control)
            .error
            .is_none()
    );
    let book = ConnectionBook::open(&f.root).unwrap();
    let (mut record, mut journal) = book.acquire(&id, &control).unwrap();
    let operation = Operation::prepared(
        format!("LMO-{}", "5".repeat(32)),
        "6".repeat(32),
        "error('must never replay')".into(),
        5,
        OperationPolicy::Opaque,
    );
    let envelope = SlotEnvelope {
        schema: crate::go_mailbox_v1::SCHEMA.into(),
        index: record.state.identity.next_slot,
        runtime: record.state.identity.runtime.clone(),
        owner: record.state.owner.clone(),
        fence: record.state.identity.fence,
        nonce: "b".repeat(32),
        ticket: operation.ticket.clone(),
        action: "prepare".into(),
        guid: record.state.identity.guid.clone(),
        build: record.state.identity.build.clone(),
        code: operation.code.clone(),
        code_bytes: operation.code.len() as i32,
        code_checksum: crate::go_mailbox_v1::checksum(operation.code.as_ref()),
        budget: 5,
        ..SlotEnvelope::default()
    };
    record.state.operation = Some(operation.clone());
    record.state.transaction = Some(Transaction {
        envelope: envelope.clone(),
        phase: TransactionPhase::InputAttempted,
        receipt: None,
        escape_attempts: 0,
        input_observation: None,
        release_wait_started_at_ms: 0,
    });
    save_state(
        &mut journal,
        &control,
        &mut record.state,
        "fixture_unknown_prepare",
    )
    .unwrap();
    journal.close().unwrap();
    let report = DoctorResult {
        target: Some(f.target.clone()),
        owner: Some(record.metadata.owner),
        deployment: Some(DoctorDeployment {
            state: "modified".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    let before = counts.borrow().actions.len();
    let result = service.doctor_fix(
        &report,
        &DoctorFixOptions {
            session: &id,
            release: Some(Path::new("fixture-release")),
            ..Default::default()
        },
        &mut NeverDelivery,
        &control,
    );
    assert!(result.error.is_some());
    assert!(
        counts.borrow().actions[before..]
            .iter()
            .all(|a| *a == InputKind::Reload)
    );
    let after = book.status(&id, &control).unwrap();
    assert!(after.state.closing);
    assert_eq!(after.state.operation.unwrap().id, operation.id);
    assert_eq!(after.state.transaction.unwrap().envelope, envelope);
}
