//! Real exclusively-owned project files and separately constructed service
//! invocations exercise persistence. Peer models protocol only: none of these
//! tests claims native Windows input or process-lifetime acceptance.
use super::super::journal::StateJournal;
use super::super::{
    InputObservation,
    admission::{ClientRecord, WindowRecord, inspect_window_owner},
    observations::{Backend, InputAction, InputDispatch, ObservationDispatch, ObservationQuery},
    recovery::test_support::{FakeClock, Peer, Schedule, Tokens, identity},
    state::{Operation, ResultBytes, RuntimeReplacementProof, Transaction, TransactionPhase},
};
use super::*;
use std::{
    cell::{Cell, RefCell},
    fs,
    io::Write,
    path::PathBuf,
    rc::Rc,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    root: PathBuf,
    target: ConnectionTarget,
    id: String,
}
impl Fixture {
    fn new() -> Self {
        Self::for_process(17, 19)
    }
    fn for_process(pid: u32, created: u64) -> Self {
        let root = std::env::temp_dir().join(format!(
            "lycheedev-con-service-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let client = root.join("client");
        fs::create_dir_all(client.join("Interface/AddOns")).unwrap();
        let i = identity('2');
        let target = ConnectionTarget {
            client: ClientRecord {
                directory: client.display().to_string(),
                full_build: i.build.clone(),
                product: i.product.clone(),
                ..Default::default()
            },
            window: WindowRecord {
                process_id: pid,
                process_started_at: created,
                handle: 23,
                ..Default::default()
            },
        };
        let mut book = ConnectionBook::open(&root).unwrap();
        let record = book
            .create(
                target.clone(),
                i,
                &FakeClock(Rc::new(Cell::new(1000))),
                &mut Tokens(50),
                &control(),
            )
            .unwrap();
        Self {
            root,
            target,
            id: record.state.id,
        }
    }
    fn book(&self) -> ConnectionBook {
        ConnectionBook::open(&self.root).unwrap()
    }
    fn edit(&self, mutate: impl FnOnce(&mut State)) {
        let (mut record, mut journal) = self.book().acquire(&self.id, &control()).unwrap();
        mutate(&mut record.state);
        save_state(&mut journal, &control(), &mut record.state, "fixture_state").unwrap();
        journal.close().unwrap();
    }
    fn state(&self) -> State {
        self.book().status(&self.id, &control()).unwrap().state
    }
    fn log(&self) -> PathBuf {
        self.book()
            .status(&self.id, &control())
            .unwrap()
            .paths
            .log_path()
            .into()
    }
    fn service(&self) -> Service {
        service(&self.root, self.target.clone())
    }
    fn operation(&self, stage: OperationStage, request: &str) {
        self.edit(|state| {
            state.bound = true;
            let mut op = Operation::prepared(
                format!("LMO-{}", "4".repeat(32)),
                "5".repeat(32),
                "return 1".into(),
                5,
                OperationPolicy::Opaque,
            );
            op.request = request.into();
            op.stage = stage;
            op.attempt = 1;
            op.origin = Some(state.identity.clone());
            op.prepared_nonce = "6".repeat(32);
            op.challenge = "7".repeat(32);
            op.result_bytes =
                ResultBytes(br#"{ "ok":false,"large":9007199254740993,"x":1,"x":2 }"#.to_vec());
            op.report_bytes = op.result_bytes.0.len() as u32;
            op.report_checksum = super::super::checksum(&op.result_bytes.0);
            state.operation = Some(op);
        });
    }
}
fn control() -> OperationControl {
    OperationControl::uncancelable(Instant::now() + Duration::from_secs(30))
}
#[derive(Default)]
struct Counts {
    opens: usize,
    closes: usize,
    input: usize,
    ended: usize,
    retired: Vec<SlotEnvelope>,
}
struct Env {
    target: ConnectionTarget,
    counts: Rc<RefCell<Counts>>,
    open_error: Option<ChannelError>,
    close_error: Option<ChannelError>,
    end: ProcessEnd,
    retire_error: Option<ChannelError>,
    cancel_input: bool,
}
struct Model {
    peer: Peer,
    counts: Rc<RefCell<Counts>>,
    close_error: Option<ChannelError>,
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
        if let Some(e) = self.open_error.clone() {
            return Err(e);
        }
        Ok(Model {
            peer: Peer {
                cancel_after_input: self.cancel_input,
                ..Default::default()
            },
            counts: self.counts.clone(),
            close_error: self.close_error.take(),
        })
    }
    fn process_end(
        &mut self,
        _: &ConnectionTarget,
        c: &OperationControl,
    ) -> ChannelResult<ProcessEnd> {
        check_control(c)?;
        self.counts.borrow_mut().ended += 1;
        Ok(self.end)
    }
    fn retire_process(
        &mut self,
        _: &ConnectionTarget,
        _: &str,
        envelopes: &[SlotEnvelope],
        c: &OperationControl,
    ) -> ChannelResult<()> {
        check_control(c)?;
        self.counts
            .borrow_mut()
            .retired
            .extend_from_slice(envelopes);
        self.retire_error.clone().map_or(Ok(()), Err)
    }
}
impl ConnectionBackend for Model {
    fn discover(&mut self, _: &OperationControl, _: &str, _: &str) -> ChannelResult<Vec<Identity>> {
        Ok(vec![identity('2')])
    }
    fn checked_close(&mut self) -> ChannelResult<()> {
        self.counts.borrow_mut().closes += 1;
        self.close_error.take().map_or(Ok(()), Err)
    }
    fn scans(&self) -> &str {
        "fixture-scans"
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
    ) -> ChannelResult<InputObservation> {
        self.peer.observe_input(c, e, a, cap)
    }
    fn input(&mut self, c: &OperationControl, a: &InputAction) -> InputDispatch {
        self.counts.borrow_mut().input += 1;
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
fn service(root: &Path, target: ConnectionTarget) -> Service {
    ConnectionService::open(
        root,
        Env {
            target,
            counts: Default::default(),
            open_error: None,
            close_error: None,
            end: ProcessEnd::None,
            retire_error: None,
            cancel_input: false,
        },
        FakeClock(Rc::new(Cell::new(1000))),
        Tokens(100),
        Schedule::default(),
    )
    .unwrap()
}
fn request(key: &str) -> ExecuteRequest<'_> {
    ExecuteRequest {
        request: key,
        code: b"return 1",
        budget_seconds: 5,
        policy: OperationPolicy::Opaque,
    }
}
fn code(outcome: &ServiceOutcome, expected: &str) -> bool {
    outcome
        .error()
        .is_some_and(|e| e.to_string().contains(expected))
}

#[test]
fn completed_history_precedes_native_open_and_keeps_later_operation() {
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "first");
    f.edit(|s| {
        s.operation.as_mut().unwrap().request = "second".into();
        s.operation.as_mut().unwrap().id = format!("LMO-{}", "8".repeat(32));
    });
    let before = fs::read(f.log()).unwrap();
    let mut service = f.service();
    service.environment.open_error = Some(error("fixture.native_must_not_open"));
    let result = service.execute(&f.id, request("first"), &control());
    assert!(result.error().is_none(), "{:?}", result.error());
    let result = result.into_report().unwrap();
    assert!(result.complete);
    assert_eq!(result.operation, format!("LMO-{}", "4".repeat(32)));
    assert_eq!(
        result.report.unwrap().0,
        br#"{ "ok":false,"large":9007199254740993,"x":1,"x":2 }"#
    );
    assert_eq!(before, fs::read(f.log()).unwrap());
    assert_eq!(service.environment.counts.borrow().opens, 0);
}
#[test]
fn history_conflicts_are_readonly_for_code_budget_and_policy() {
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "same");
    let before = fs::read(f.log()).unwrap();
    for req in [
        ExecuteRequest {
            code: b"return 2",
            ..request("same")
        },
        ExecuteRequest {
            budget_seconds: 6,
            ..request("same")
        },
        ExecuteRequest {
            policy: OperationPolicy::Observation,
            ..request("same")
        },
    ] {
        let mut s = f.service();
        assert!(code(
            &s.execute(&f.id, req, &control()),
            "live.channel_request_conflict"
        ));
        assert_eq!(s.environment.counts.borrow().opens, 0);
    }
    assert_eq!(before, fs::read(f.log()).unwrap());
}
#[test]
fn report_and_status_do_not_need_a_claim_or_game_handle() {
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "report");
    f.edit(|s| {
        s.bound = false;
        s.closed = true;
    });
    f.book().retire_closed(&f.id, &control()).unwrap();
    let s = f.service();
    assert!(s.status(&f.id, &control()).error().is_none());
    assert!(
        s.report(&f.id, "report", &control())
            .into_report()
            .unwrap()
            .report
            .is_some()
    );
    assert_eq!(s.environment.counts.borrow().opens, 0);
}
#[test]
fn verified_report_and_failed_report_ok_remain_independent_of_cleanup() {
    let f = Fixture::new();
    f.operation(OperationStage::ResultVerified, "report");
    let result = f.service().status(&f.id, &control()).into_report().unwrap();
    assert!(!result.complete);
    assert_eq!(result.report_state, "verified");
    assert_eq!(result.cleanup, "pending");
    let raw = serde_json::to_string(&result).unwrap();
    assert!(raw.contains("9007199254740993"));
    assert!(raw.contains("\"ok\":false"));
    assert!(raw.contains("\"x\":1,\"x\":2"));
}
#[test]
fn closed_unknown_never_inherits_connection_completion() {
    let f = Fixture::new();
    f.operation(OperationStage::ExecutionUnknown, "unknown");
    f.edit(|s| {
        s.closed = true;
        s.bound = false;
    });
    let mut s = f.service();
    let result = s.disconnect(&f.id, &control());
    assert!(result.error().is_none());
    let result = result.into_report().unwrap();
    assert!(result.closed);
    assert!(!result.complete);
    assert_eq!(result.report_state, "unavailable");
    assert!(result.report.is_none());
    assert_eq!(s.environment.counts.borrow().opens, 0);
}
#[test]
fn unknown_retry_preserves_operation_ticket_and_never_sends_input() {
    let f = Fixture::new();
    f.operation(OperationStage::ExecutionUnknown, "unknown");
    let before = f.state();
    let mut s = f.service();
    let result = s.execute(&f.id, request("unknown"), &control());
    assert!(code(&result, "execution_unknown"));
    let after = f.state();
    assert_eq!(before.operation, after.operation);
    assert_eq!(s.environment.counts.borrow().input, 0);
    assert_eq!(
        s.environment.counts.borrow().opens,
        s.environment.counts.borrow().closes
    );
}
#[test]
fn active_execution_lease_blocks_second_service_before_native_open() {
    let f = Fixture::new();
    let (_, mut held) = f.book().acquire(&f.id, &control()).unwrap();
    let mut s = f.service();
    let result = s.resume(&f.id, &control());
    assert!(code(&result, "live.channel_pending"));
    assert_eq!(
        result.into_report().unwrap().continuation.kind,
        "wait_active_driver"
    );
    assert_eq!(s.environment.counts.borrow().opens, 0);
    held.close().unwrap();
}
#[test]
fn backend_open_failure_explicitly_releases_execution_lease() {
    let f = Fixture::new();
    let mut s = f.service();
    s.environment.open_error = Some(error("fixture.open_failed"));
    assert!(code(&s.resume(&f.id, &control()), "fixture.open_failed"));
    let (_, mut next) = f.book().acquire(&f.id, &control()).unwrap();
    next.close().unwrap();
}
#[test]
fn successful_turn_still_returns_checked_close_failure_and_releases_lease() {
    let f = Fixture::new();
    f.edit(|s| s.bound = true);
    let mut s = f.service();
    s.environment.close_error = Some(error("fixture.native_close_failed"));
    let result = s.resume(&f.id, &control());
    assert!(code(&result, "fixture.native_close_failed"));
    assert!(result.into_report().unwrap().bound);
    assert_eq!(s.environment.counts.borrow().closes, 1);
    let (_, mut next) = f.book().acquire(&f.id, &control()).unwrap();
    next.close().unwrap();
}

#[test]
fn owned_terminal_projection_keeps_durable_raw_report_after_native_close_failure() {
    let f = Fixture::new();
    // The recovery-only Peer supports bind/unbind, not business release.
    // Start with completed business and drive the actual unbind/close path;
    // ResultVerified would correctly demand a release-capable business peer.
    f.operation(OperationStage::Complete, "owned-report");
    let original = f.state().operation.unwrap().result_bytes.0;
    let mut service = f.service();
    service.environment.close_error = Some(error("fixture.native_close_failed"));
    let outcome = service.disconnect(&f.id, &control());
    assert!(code(&outcome, "fixture.native_close_failed"));
    let ServiceOutcome::ReportedWithFailure { result, .. } = outcome else {
        panic!("checked close failure must preserve the completed report");
    };
    assert_eq!(result.report_state, "verified");
    assert!(result.complete && result.closed && !result.bound);
    assert_eq!(result.report.unwrap().0, original);
    let saved = f.state();
    assert!(saved.closed && !saved.bound && saved.transaction.is_none());
    let operation = saved.operation.unwrap();
    assert_eq!(operation.stage, OperationStage::Complete);
    assert_eq!(operation.result_bytes.0, original);
    assert_eq!(service.environment.counts.borrow().opens, 1);
    assert_eq!(service.environment.counts.borrow().closes, 1);
    // Retirement itself reacquires the released execution lease. A closed
    // connection no longer has a claim that book.acquire could reopen.
    let owner = inspect_window_owner(&f.target.addon_parent(), &f.target.resource(), &control());
    assert!(owner.error.is_none(), "{:?}", owner.error);
    assert!(!owner.occupied && owner.owner.is_none());
}

#[test]
fn owned_driver_budget_failure_keeps_original_projection_and_checked_resource_closes() {
    use crate::go_mailbox_v1::state::{DurableBudget, ReloadAttempt, ReloadPhase};
    let f = Fixture::new();
    f.operation(OperationStage::ReleaseReady, "owned-budget-error");
    f.edit(|s| {
        let operation = s.operation.as_mut().unwrap();
        operation.cleanup_method = CleanupMethod::ReloadRequired;
        let budget = DurableBudget {
            started_at_ms: 1000,
            deadline_ms: 9000,
            last_observed_ms: 1000,
            ..DurableBudget::default()
        };
        operation.recovery_budget = Some(budget.clone());
        s.reload = Some(ReloadAttempt {
            automatic: true,
            slot_enablement: false,
            slot_budget_source: None,
            recovery_budget: Some(DurableBudget { deadline_ms: 9001, ..budget }),
            reconcile_pending: false,
            request: format!("cleanup-{}", operation.id),
            from: s.identity.runtime.clone(),
            phase: ReloadPhase::Intent,
            input_step: 0,
            patch_transitions: 0,
        });
    });
    let original = f.state();
    let log = fs::read(f.log()).unwrap();
    let expected = present(&original, &f.log(), 1000, "", "fixture-scans");
    let mut service = f.service();
    service.environment.close_error = Some(error("fixture.native_close_failed"));
    let outcome = service.resume(&f.id, &control());
    assert!(code(&outcome, "live.channel_budget_invalid"));
    assert!(code(&outcome, "fixture.native_close_failed"));
    assert_eq!(
        serde_json::to_vec(&outcome.into_report().unwrap()).unwrap(),
        serde_json::to_vec(&expected).unwrap(),
    );
    assert_eq!(f.state(), original);
    assert_eq!(fs::read(f.log()).unwrap(), log);
    assert_eq!(service.environment.counts.borrow().opens, 1);
    assert_eq!(service.environment.counts.borrow().closes, 1);
    let (_, mut next) = f.book().acquire(&f.id, &control()).unwrap();
    next.close().unwrap();
}
#[test]
fn durable_deadline_is_not_restarted_by_another_invocation() {
    let f = Fixture::new();
    let deadline = f.state().connect_budget.unwrap().deadline_ms;
    let mut s = f.service();
    s.clock.0.set(deadline + 1);
    let result = s.resume(&f.id, &control());
    assert!(code(&result, "budget_exhausted"));
    assert_eq!(f.state().connect_budget.unwrap().deadline_ms, deadline);
    let mut second = f.service();
    second.clock.0.set(deadline + 1000);
    let result = second.resume(&f.id, &control());
    assert!(code(&result, "budget_exhausted"));
    assert_eq!(f.state().connect_budget.unwrap().deadline_ms, deadline);
}
#[test]
fn cancelled_invocation_does_not_restart_or_close_original_connection() {
    let f = Fixture::new();
    let original = fs::read(f.log()).unwrap();
    let c = OperationControl::default();
    c.tracked_cancel_handle().unwrap().cancel();
    let result = f.service().resume(&f.id, &c);
    assert!(code(&result, "command.cancelled"));
    assert_eq!(original, fs::read(f.log()).unwrap());
    assert!(!f.state().closed);
}
#[test]
fn process_end_model_rechecks_under_lease_and_preserves_unknown_evidence() {
    let f = Fixture::new();
    f.operation(OperationStage::Prepared, "interrupted");
    let before = f.state().operation.unwrap().result_bytes;
    let mut s = f.service();
    s.environment.end = ProcessEnd::PidReused;
    let result = s.disconnect(&f.id, &control());
    assert!(result.error().is_none(), "{:?}", result.error());
    let result = result.into_report().unwrap();
    assert!(result.closed);
    assert!(!result.complete);
    assert_eq!(result.process_end, "pid_reused");
    assert_eq!(s.environment.counts.borrow().ended, 2);
    assert_eq!(s.environment.counts.borrow().opens, 0);
    assert_eq!(f.state().operation.unwrap().result_bytes, before);
    assert!(
        !inspect_window_owner(&f.target.addon_parent(), &f.target.resource(), &control()).occupied
    );
    let before = fs::read(f.log()).unwrap();
    assert!(s.disconnect(&f.id, &control()).error().is_none());
    assert_eq!(before, fs::read(f.log()).unwrap());
}
#[test]
fn ended_process_promotes_only_already_verified_report() {
    for stage in [
        OperationStage::Prepared,
        OperationStage::CommitReady,
        OperationStage::Running,
        OperationStage::ConfirmReady,
        OperationStage::ExecutionUnknown,
        OperationStage::ResultVerified,
        OperationStage::ReleaseReady,
        OperationStage::Complete,
    ] {
        let f = Fixture::new();
        f.operation(stage, "ended");
        let mut s = f.service();
        s.environment.end = ProcessEnd::ProcessExited;
        let result = s.disconnect(&f.id, &control());
        assert!(result.error().is_none(), "{:?}", result.error());
        let r = result.into_report().unwrap();
        assert_eq!(
            r.complete,
            matches!(
                stage,
                OperationStage::ResultVerified
                    | OperationStage::ReleaseReady
                    | OperationStage::Complete
            )
        );
    }
}
#[test]
fn cleanup_failure_keeps_observed_end_and_original_transaction() {
    let f = Fixture::new();
    f.operation(OperationStage::Prepared, "interrupted");
    f.edit(|s| {
        s.transaction = Some(Transaction {
            envelope: SlotEnvelope {
                schema: SCHEMA.into(),
                runtime: s.identity.runtime.clone(),
                owner: s.owner.clone(),
                guid: s.identity.guid.clone(),
                build: s.identity.build.clone(),
                index: 1,
                nonce: "9".repeat(32),
                ticket: "8".repeat(32),
                fence: 1,
                action: "bind".into(),
                ..Default::default()
            },
            phase: TransactionPhase::Intent,
            release_wait_started_at_ms: 0,
            receipt: None,
            escape_attempts: 0,
            input_observation: None,
        })
    });
    let original = f.state().transaction;
    let mut s = f.service();
    s.environment.end = ProcessEnd::ProcessAbsent;
    s.environment.retire_error = Some(error("fixture.retirement_failed"));
    let result = s.disconnect(&f.id, &control());
    assert!(code(&result, "fixture.retirement_failed"));
    let state = f.state();
    assert_eq!(state.process_end, ProcessEnd::ProcessAbsent);
    assert!(!state.closed);
    assert_eq!(state.transaction, original);
    assert!(
        inspect_window_owner(&f.target.addon_parent(), &f.target.resource(), &control()).occupied
    );
}
#[test]
fn torn_tail_is_archived_before_terminal_retry_without_native_open() {
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "done");
    let path = f.log();
    let original = fs::read(&path).unwrap();
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"{\"sequence\":").unwrap();
    file.sync_all().unwrap();
    durable_journal::close_file(file).unwrap();
    let torn = fs::read(&path).unwrap();
    let mut s = f.service();
    let result = s.execute(&f.id, request("done"), &control());
    assert!(result.error().is_none(), "{:?}", result.error());
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(s.environment.counts.borrow().opens, 0);
    let backup = path.with_file_name(format!(
        "{}.torn-{}",
        path.file_name().unwrap().to_string_lossy(),
        &lycheedev_core::storage::sha256(&torn)[..16]
    ));
    assert_eq!(fs::read(backup).unwrap(), torn);
}
#[test]
fn malformed_complete_record_and_empty_prefix_are_never_repaired() {
    for bytes in [b"broken\n".as_slice(), b"{\"sequence\":".as_slice()] {
        let f = Fixture::new();
        let path = f.log();
        fs::write(&path, bytes).unwrap();
        let mut s = f.service();
        let result = s.resume(&f.id, &control());
        assert!(result.error().is_some());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(s.environment.counts.borrow().opens, 0);
        assert!(
            !fs::read_dir(path.parent().unwrap()).unwrap().any(|e| e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".torn-"))
        );
    }
}
#[test]
fn pending_close_projection_keeps_bound_separate_from_complete() {
    let f = Fixture::new();
    f.edit(|s| {
        s.bound = true;
        s.closing = true;
    });
    let r = f.service().status(&f.id, &control()).into_report().unwrap();
    assert!(r.bound);
    assert!(!r.complete);
    assert_eq!(r.stage, "closing");
}
#[test]
fn copied_project_metadata_is_not_ownership() {
    let f = Fixture::new();
    let other = Fixture::new();
    let path = other
        .root
        .join(".lycheedev/live/connections")
        .join(format!("{}.target.json", f.id));
    let original = f
        .root
        .join(".lycheedev/live/connections")
        .join(format!("{}.target.json", f.id));
    fs::copy(original, path).unwrap();
    assert!(code(
        &other.service().status(&f.id, &control()),
        "live.channel_project_owner_mismatch"
    ));
}
#[test]
fn completed_request_is_readable_in_a_fresh_process_model() {
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "done");
    let before = fs::read(f.log()).unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "go_mailbox_v1::connection::tests::fresh_process_history_helper",
            "--nocapture",
        ])
        .env("LYCHEEDEV_CON_HISTORY_ROOT", &f.root)
        .env("LYCHEEDEV_CON_HISTORY_ID", &f.id)
        .spawn()
        .unwrap();
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        if Instant::now() >= until {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("owned history helper timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(before, fs::read(f.log()).unwrap());
}
#[test]
fn fresh_process_history_helper() {
    let Some(root) = std::env::var_os("LYCHEEDEV_CON_HISTORY_ROOT") else {
        return;
    };
    let id = std::env::var("LYCHEEDEV_CON_HISTORY_ID").unwrap();
    let root = PathBuf::from(root);
    let record = ConnectionBook::open(&root)
        .unwrap()
        .status(&id, &control())
        .unwrap();
    let mut s = service(&root, record.metadata.target);
    s.environment.open_error = Some(error("fixture.no_native_in_subprocess"));
    let result = s.execute(&id, request("done"), &control());
    assert!(result.error().is_none());
    assert!(result.into_report().unwrap().complete);
    assert_eq!(s.environment.counts.borrow().opens, 0);
}

#[test]
fn public_error_mapping_preserves_trace_priority_and_full_causes() {
    let error = AdmissionError::Joined(vec![
        ChannelError::Pending { causes: vec![] }.into(),
        ChannelError::Cancelled.into(),
        ChannelError::External {
            causes: vec!["live.channel_trace_flush: fixture disk failure".into()],
        }
        .into(),
    ]);
    let mapped = classify_error(&error).unwrap();
    assert_eq!(mapped.exit_code, 5);
    assert_eq!(mapped.code, "live.channel_trace_flush");
    assert!(mapped.message.contains("command.cancelled"));
    assert!(mapped.message.contains("fixture disk failure"));
    assert_eq!(
        classify_error(&ChannelError::Cancelled.into())
            .unwrap()
            .exit_code,
        7
    );
    assert_eq!(
        classify_error(&ChannelError::Pending { causes: vec![] }.into())
            .unwrap()
            .exit_code,
        6
    );
    assert!(
        classify_error(
            &ChannelError::External {
                causes: vec!["os error 5".into()]
            }
            .into()
        )
        .is_none()
    );
}
#[test]
fn request_conflict_mapping_and_raw_report_keep_public_contract() {
    let mapped = classify_error(&error("live.channel_request_conflict").into()).unwrap();
    assert_eq!(mapped.exit_code, 2);
    let f = Fixture::new();
    f.operation(OperationStage::Cancelled, "cancelled");
    let r = f.service().status(&f.id, &control()).into_report().unwrap();
    assert!(!r.complete);
    assert_eq!(r.cleanup, "complete");
    assert_eq!(r.cleanup_method, "not_executed");
    assert!(r.report.is_none());
}
#[test]
fn terminal_history_survives_rotation_and_missing_archive_is_not_a_new_request() {
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "old");
    f.edit(|s| {
        let op = s.operation.as_mut().unwrap();
        op.request = "new".into();
        op.id = format!("LMO-{}", "8".repeat(32));
    });
    let (mut record, mut journal) = f.book().acquire(&f.id, &control()).unwrap();
    for _ in 0..256 {
        save_state(
            &mut journal,
            &control(),
            &mut record.state,
            "fixture_observation",
        )
        .unwrap();
    }
    journal
        .checkpoint(&control(), &mut record.state, false)
        .unwrap();
    journal.close().unwrap();
    assert!(!record.state.archive.is_empty());
    let mut s = f.service();
    assert!(
        s.execute(&f.id, request("old"), &control())
            .error()
            .is_none()
    );
    assert_eq!(s.environment.counts.borrow().opens, 0);
    let archive = record
        .paths
        .history_directory()
        .join(format!("{}.jsonl", record.state.archive));
    // Preserve evidence and simulate an unavailable required archive in this
    // exclusively owned fixture; never remove another test's tree.
    fs::rename(&archive, archive.with_extension("saved")).unwrap();
    let result = s.execute(&f.id, request("old"), &control());
    assert!(result.error().is_some());
    assert_eq!(s.environment.counts.borrow().opens, 0);
}
#[test]
fn main_addon_guard_never_inspects_shared_slots() {
    use lycheedev_delivery::installation::{self, Receipt, Resource};
    let f = Fixture::new();
    let addon = native::recorded_addon_directory(&f.target);
    fs::create_dir(&addon).unwrap();
    let code = b"-- no execution\n";
    fs::write(addon.join("Core.lua"), code).unwrap();
    let receipt = Receipt {
        schema: "lycheedev.installation.v1".into(),
        component: "addon".into(),
        version: "3.2.0".into(),
        commit: "a".repeat(40),
        resources: vec![Resource {
            path: "addon/Core.lua".into(),
            sha256: lycheedev_core::storage::sha256(code),
            bytes: code.len() as u64,
        }],
    };
    fs::write(
        addon.join(installation::MARKER),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
    // No slot pool exists. Main-addon qualification must still succeed.
    let verified = installation::verify_main_addon_controlled(&addon, "3.2.0", &control()).unwrap();
    assert_eq!(verified.canonical_path, fs::canonicalize(&addon).unwrap());
    fs::write(addon.join("Core.lua"), b"-- altered\n").unwrap();
    assert!(installation::verify_main_addon_controlled(&addon, "3.2.0", &control(),).is_err());
}

#[test]
fn real_owned_process_creation_mismatch_retires_con_without_any_game() {
    // Same fixed-Go fixture: a real owned test PID with impossible creation=1
    // proves PID reuse. It requires neither a game nor a valid HWND.
    let f = Fixture::for_process(std::process::id(), 1);
    f.operation(OperationStage::Prepared, "interrupted");
    let native = NativeEnvironment::new(&f.root, "3.2.0", WindowsProcessLifetime).unwrap();
    let mut service = ConnectionService::open(
        &f.root,
        native,
        SystemClock,
        SystemTokens,
        SystemScheduler::default(),
    )
    .unwrap();
    let result = service.disconnect(&f.id, &control());
    assert!(result.error().is_none(), "{:?}", result.error());
    let result = result.into_report().unwrap();
    assert!(result.closed);
    assert!(!result.complete);
    assert_eq!(result.process_end, "pid_reused");
    assert!(
        !inspect_window_owner(&f.target.addon_parent(), &f.target.resource(), &control()).occupied
    );
    let before = fs::read(f.log()).unwrap();
    assert!(service.disconnect(&f.id, &control()).error().is_none());
    assert_eq!(before, fs::read(f.log()).unwrap());
}

#[test]
fn connect_existing_selection_resumes_original_connection() {
    let f = Fixture::new();
    let mut s = f.service();
    let result = s.connect(&TargetRequest::default(), &control());
    assert!(result.error().is_none(), "{:?}", result.error());
    assert_eq!(result.into_report().unwrap().session, f.id);
    assert_eq!(s.environment.counts.borrow().opens, 1);
    assert_eq!(s.environment.counts.borrow().closes, 1);
}
#[test]
fn fresh_connect_persists_claim_then_binds_with_checked_resource_closes() {
    let f = Fixture::new();
    f.edit(|s| {
        s.closed = true;
        s.bound = false;
    });
    f.book().retire_closed(&f.id, &control()).unwrap();
    let mut s = f.service();
    let result = s.connect(&TargetRequest::default(), &control());
    assert!(result.error().is_none(), "{:?}", result.error());
    let result = result.into_report().unwrap();
    assert_ne!(result.session, f.id);
    assert!(result.bound);
    assert!(result.complete);
    assert_eq!(s.environment.counts.borrow().opens, 2);
    assert_eq!(s.environment.counts.borrow().closes, 2);
    let owner = inspect_window_owner(&f.target.addon_parent(), &f.target.resource(), &control())
        .owner
        .unwrap();
    assert_eq!(owner.operation_id, result.session);
    assert!(
        ConnectionBook::open(&f.root)
            .unwrap()
            .status(&result.session, &control())
            .unwrap()
            .state
            .bound
    );
}
#[test]
fn connect_filter_mismatch_never_opens_a_native_backend() {
    let f = Fixture::new();
    let mut s = f.service();
    let request = TargetRequest {
        filter: TargetFilter {
            character: "another".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(code(
        &s.connect(&request, &control()),
        "live.channel_target_mismatch"
    ));
    assert_eq!(s.environment.counts.borrow().opens, 0);
}

#[test]
fn toolkit_exit_and_details_survive_service_and_public_projection() {
    let f = Fixture::new();
    let mut service = f.service();
    let details = serde_json::json!({"__commandContext":{"installationSelection":{"candidates":[],"issues":[{"reason":"ambiguous"}]}}});
    let original =
        lycheedev_core::ToolkitError::new("selection.installation_ambiguous", "pick one", 2)
            .with_details(details.clone());
    service.environment.open_error = Some(durable_journal::storage_error(original));
    let result = service.resume(&f.id, &control());
    let mapped = classify_error(result.error().unwrap()).unwrap();
    assert_eq!(mapped.code, "selection.installation_ambiguous");
    assert_eq!(mapped.exit_code, 2);
    assert_eq!(mapped.details, details);
    let public = serde_json::to_value(&mapped).unwrap();
    assert_eq!(public["details"], details);
    assert_eq!(public["exit_code"], 2);
    let business = ChannelError::from(
        lycheedev_core::ToolkitError::new("live.channel_request_conflict", "conflict", 8)
            .with_details(details.clone()),
    );
    let mapped = classify_error(&business.into()).unwrap();
    assert_eq!(mapped.exit_code, 2);
    assert_eq!(mapped.details, details);
}
#[test]
fn toolkit_control_remains_typed_in_joined_service_failure() {
    let error = ChannelError::from(lycheedev_core::ToolkitError::new(
        "command.cancelled",
        "stop",
        7,
    ));
    assert_eq!(error, ChannelError::Cancelled);
    let joined = AdmissionError::Joined(vec![
        error.into(),
        ChannelError::from(lycheedev_core::ToolkitError::new(
            "external.io",
            "close failed",
            5,
        ))
        .into(),
    ]);
    let public = classify_error(&joined).unwrap();
    assert_eq!(public.exit_code, 7);
    assert!(public.message.contains("close failed"));
}

#[test]
fn legacy_unfinished_unbind_resume_checks_original_process_before_native_open() {
    let f = Fixture::new();
    f.edit(|s| {
        s.bound = true;
        s.closing = false;
        s.close_budget = None;
        s.transaction = Some(Transaction {
            envelope: SlotEnvelope {
                schema: SCHEMA.into(),
                runtime: s.identity.runtime.clone(),
                owner: s.owner.clone(),
                guid: s.identity.guid.clone(),
                build: s.identity.build.clone(),
                index: 1,
                fence: 1,
                nonce: "9".repeat(32),
                ticket: "8".repeat(32),
                action: "unbind".into(),
                ..Default::default()
            },
            phase: TransactionPhase::InputAttempted,
            release_wait_started_at_ms: 0,
            receipt: None,
            escape_attempts: 0,
            input_observation: None,
        });
    });
    let mut service = f.service();
    service.environment.end = ProcessEnd::PidReused;
    service.environment.open_error = Some(error("fixture.native_must_not_open"));
    let result = service.resume(&f.id, &control());
    assert!(result.error().is_none(), "{:?}", result.error());
    assert!(result.into_report().unwrap().closed);
    assert_eq!(service.environment.counts.borrow().ended, 2);
    assert_eq!(service.environment.counts.borrow().opens, 0);
    assert_eq!(service.environment.counts.borrow().retired.len(), 1);
    assert!(
        !inspect_window_owner(&f.target.addon_parent(), &f.target.resource(), &control()).occupied
    );
}
#[test]
fn discovery_empty_and_conflicting_branches_keep_core_code_exit_and_report() {
    for conflict in [false, true] {
        let f = Fixture::new();
        let directory = f.root.join(if conflict { "conflict" } else { "empty" });
        fs::create_dir(&directory).unwrap();
        if conflict {
            fs::write(directory.join(".flavor.info"), "invalid_product").unwrap();
            fs::write(directory.join("version.txt"), "12.1.0.70124").unwrap();
        }
        let err = native::discover_installations(&directory.display().to_string(), &control())
            .unwrap_err();
        let public = classify_error(&err.into()).unwrap();
        assert_eq!(
            public.code,
            if conflict {
                "selection.installation_metadata_conflict"
            } else {
                "selection.installation_missing"
            }
        );
        assert_eq!(public.exit_code, if conflict { 4 } else { 3 });
        let report = &public.details["__commandContext"]["installationSelection"];
        assert_eq!(report["candidates"], serde_json::json!([]));
        assert_eq!(report["issues"].as_array().unwrap().is_empty(), !conflict);
        assert_eq!(public.details["__commandContext"]["requestedProduct"], "");
        assert_eq!(
            report["directory"],
            lycheedev_core::client::display_path(&fs::canonicalize(&directory).unwrap())
        );
    }
}
#[test]
fn discovery_retains_multiple_candidates_for_later_pid_filtering() {
    let f = Fixture::new();
    let root = f.root.join("multi-client");
    fs::create_dir(&root).unwrap();
    for (folder, flavor, version) in [
        ("_retail_", "wow", "12.1.0.70124"),
        ("_classic_", "wow_classic", "5.5.4.70124"),
    ] {
        let client = root.join(folder);
        fs::create_dir(&client).unwrap();
        fs::write(client.join(".flavor.info"), flavor).unwrap();
        fs::write(client.join("version.txt"), version).unwrap();
    }
    let candidates = native::discover_installations(&root.display().to_string(), &control())
        .unwrap()
        .unwrap();
    assert_eq!(candidates.len(), 2);
}
fn file_cleanup_error(
    primary: lycheedev_core::ToolkitError,
    cleanup: Option<lycheedev_core::ToolkitError>,
    control: Option<lycheedev_core::ToolkitError>,
) -> lycheedev_core::ToolkitError {
    lycheedev_core::ToolkitError::new(primary.code.clone(),primary.message.clone(),primary.exit_code).with_details(serde_json::json!({
        "schema":lycheedev_core::storage::FILE_CLEANUP_SCHEMA,"primary":primary,"cleanup":cleanup,"control":control
    }))
}
#[test]
fn structured_cancel_close_and_deadline_close_keep_every_typed_cause_and_details() {
    for (name, exit) in [("command.cancelled", 7), ("command.deadline", 5)] {
        let primary = lycheedev_core::ToolkitError::new(name, "original stop", exit);
        let close = lycheedev_core::ToolkitError::new("external.io", "checked close failed", 5)
            .with_details(serde_json::json!({"handle":"owned"}));
        let error = file_cleanup_error(primary, Some(close), None);
        let details = error.details.clone();
        let mapped = durable_journal::storage_error(error);
        assert!(mapped.any_cause(&|cause| if name == "command.cancelled" {
            matches!(cause, ChannelError::Cancelled)
        } else {
            matches!(cause, ChannelError::DeadlineExceeded)
        }));
        assert!(mapped.any_cause(&|cause|matches!(cause,ChannelError::Toolkit{code,details,..} if code=="external.io"&&details["handle"]=="owned")));
        assert!(!mapped.optional_observation_timeout());
        assert!(mapped.to_string().contains("checked close failed"));
        let public = classify_error(&mapped.into()).unwrap();
        assert_eq!(public.details, details);
    }
}
#[test]
fn nested_file_cleanup_tree_cannot_be_misclassified_as_pure_timeout() {
    let deadline = lycheedev_core::ToolkitError::new("command.deadline", "deadline", 5);
    let close = lycheedev_core::ToolkitError::new("external.io", "first close failed", 5);
    let inner = file_cleanup_error(deadline, Some(close), None);
    let outer = file_cleanup_error(
        inner,
        Some(lycheedev_core::ToolkitError::new(
            "external.io",
            "second close failed",
            5,
        )),
        Some(lycheedev_core::ToolkitError::new(
            "command.cancelled",
            "later cancel",
            7,
        )),
    );
    let details = outer.details.clone();
    let mapped = ChannelError::from(outer);
    assert!(mapped.any_cause(&|e| matches!(e, ChannelError::DeadlineExceeded)));
    assert!(mapped.any_cause(&|e| matches!(e, ChannelError::Cancelled)));
    assert!(!mapped.optional_observation_timeout());
    assert!(mapped.to_string().contains("first close failed"));
    assert!(mapped.to_string().contains("second close failed"));
    let public = classify_error(&mapped.into()).unwrap();
    assert_eq!(public.details, details);
}
#[test]
fn opaque_control_details_never_disappear_or_authorize_optional_timeout_suppression() {
    let details = serde_json::json!({"schema":lycheedev_core::storage::FILE_CLEANUP_SCHEMA,"primary":"malformed","cleanup":null,"control":null});
    let mapped = ChannelError::from(
        lycheedev_core::ToolkitError::new("command.deadline", "stop", 5)
            .with_details(details.clone()),
    );
    assert!(mapped.any_cause(&|e| matches!(e, ChannelError::DeadlineExceeded)));
    assert!(!mapped.optional_observation_timeout());
    assert_eq!(classify_error(&mapped.into()).unwrap().details, details);
}

#[test]
fn structured_pure_control_aggregate_still_exposes_only_real_timeout_leaves() {
    let error = file_cleanup_error(
        lycheedev_core::ToolkitError::new("command.deadline", "first observation", 5),
        None,
        Some(lycheedev_core::ToolkitError::new(
            "command.deadline",
            "second observation",
            5,
        )),
    );
    let mapped = ChannelError::from(error);
    assert!(mapped.optional_observation_timeout());
    assert!(mapped.all_causes(&|cause| matches!(cause, ChannelError::DeadlineExceeded)));
}

#[test]
fn deployment_error_mapping_does_not_call_io_or_close_failure_a_dirty_installation() {
    for error in [
        lycheedev_core::ToolkitError::new("external.io", "access denied", 5),
        lycheedev_core::ToolkitError::new("delivery.invalid_installation", "malformed receipt", 4),
        file_cleanup_error(
            lycheedev_core::ToolkitError::new("external.io", "read failed", 5),
            Some(lycheedev_core::ToolkitError::new(
                "external.io",
                "close failed",
                5,
            )),
            None,
        ),
    ] {
        let expected_code = error.code.clone();
        let expected_exit = error.exit_code;
        let expected_details = error.details.clone();
        let mapped = native::deployment_error(error);
        let public = classify_error(&mapped.into()).unwrap();
        assert_eq!(public.code, expected_code);
        assert_eq!(public.exit_code, expected_exit);
        assert_eq!(public.details, expected_details);
    }
    for code in [
        "delivery.installation_not_clean",
        "delivery.invalid_payload",
    ] {
        let mapped = native::deployment_error(lycheedev_core::ToolkitError::new(
            code,
            "definite dirty state",
            3,
        ));
        assert_eq!(
            classify_error(&mapped.into()).unwrap().code,
            "live.channel_clean_current_addon_required"
        );
    }
}
#[test]
fn main_addon_absent_unmanaged_and_malformed_receipt_have_distinct_failures() {
    let f = Fixture::new();
    let path = f.root.join("main-addon-state");
    let inspect = |path: &Path| {
        lycheedev_delivery::installation::verify_main_addon_controlled(path, "3.2.0", &control())
            .unwrap_err()
    };
    assert_eq!(inspect(&path).code, "delivery.installation_not_clean");
    fs::create_dir(&path).unwrap();
    assert_eq!(inspect(&path).code, "delivery.installation_not_clean");
    fs::write(path.join(lycheedev_delivery::installation::MARKER), b"{").unwrap();
    let e = inspect(&path);
    assert_eq!(e.code, "delivery.invalid_installation");
    assert_eq!(e.exit_code, 4);
}

#[test]
fn recorded_addon_directory_matches_fixed_go_and_core_destination() {
    let f = Fixture::new();
    let client_root = Path::new(&f.target.client.directory);
    fs::write(client_root.join(".flavor.info"), "wow").unwrap();
    fs::write(client_root.join("version.txt"), "12.1.0.70124").unwrap();
    let client = lycheedev_core::client::inspect_with(
        client_root,
        &mut lycheedev_core::installation_selection::InvocationMetadata(&control()),
    )
    .unwrap();
    let path = native::recorded_addon_directory(&f.target);
    assert_eq!(
        path,
        lycheedev_core::client::addon_destination(&client).unwrap()
    );
    assert_eq!(path.file_name().unwrap(), "Lychee Dev");
    assert!(!path.exists());
    assert!(!f.target.addon_parent().join("LycheeDev").exists());
}

mod code_bytes;

#[test]
fn native_raw_byte_report_survives_service_projection_and_public_json_frames() {
    use lycheedev_core::public_json::{write_pretty_serializable_line, write_serializable_line};
    let f = Fixture::new();
    f.operation(OperationStage::Complete, "raw report");
    let raw = b" { \"s\":\"\xff\", \"x\":1e400, \"x\":-0 } \n".to_vec();
    f.edit(|s| {
        let op = s.operation.as_mut().unwrap();
        op.result_bytes = ResultBytes(raw.clone());
        op.report_bytes = raw.len() as u32;
        op.report_checksum = super::super::checksum(&raw);
    });
    let before = fs::read(f.log()).unwrap();
    for result in [
        f.service().status(&f.id, &control()),
        f.service().report(&f.id, "raw report", &control()),
    ] {
        assert!(result.error().is_none());
        let result = result.into_report().unwrap();
        assert_eq!(result.report.as_ref().unwrap().0, raw);
        assert!(serde_json::to_vec(&result).is_err());
        let mut output = Vec::new();
        write_serializable_line(&mut output, &result).unwrap();
        assert!(
            output
                .windows(b"\"report\":{\"s\":\"\xff\",\"x\":1e400,\"x\":-0}".len())
                .any(|b| b == b"\"report\":{\"s\":\"\xff\",\"x\":1e400,\"x\":-0}")
        );
        let mut pretty = Vec::new();
        write_pretty_serializable_line(&mut pretty, &result).unwrap();
        assert!(pretty.windows(b"\"report\": {\n    \"s\": \"\xff\",\n    \"x\": 1e400,\n    \"x\": -0\n  }".len()).any(|b| b == b"\"report\": {\n    \"s\": \"\xff\",\n    \"x\": 1e400,\n    \"x\": -0\n  }"));
    }
    assert_eq!(fs::read(f.log()).unwrap(), before);
    let invalid = RawReport(b"{\"s\":\"\xff\"} trailing".to_vec());
    let mut out = b"previous\n".to_vec();
    assert!(write_serializable_line(&mut out, &invalid).is_err());
    assert_eq!(out, b"previous\n");
}
