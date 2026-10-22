//! Pure in-memory C-slice fixtures. No native process, filesystem durability,
//! window input or real runtime replacement is claimed by these tests.
use super::super::driver::{Clock, Driver, TokenSource, error};
use super::super::journal::StateJournal;
use super::super::observations::*;
use super::super::reload::ContinuationScheduler;
use super::super::runtime_retirement::{RetirementTarget, RuntimeRetirementBackend};
use super::super::state::*;
use super::super::*;
use lycheedev_core::control::{OperationControl, TrackedCancellation};
use std::{cell::Cell, rc::Rc, time::Duration};

#[derive(Clone)]
pub(crate) struct FakeClock(pub Rc<Cell<i64>>);
impl Clock for FakeClock {
    fn unix_millis(&self) -> i64 {
        self.0.get()
    }
}
pub(crate) struct Tokens(pub u64);
impl TokenSource for Tokens {
    fn token(&mut self) -> ChannelResult<String> {
        self.0 += 1;
        Ok(format!("{:032x}", self.0))
    }
}
#[derive(Default)]
pub(crate) struct Journal {
    pub events: Vec<(String, State)>,
    pub results: Vec<Operation>,
    pub fail: Option<&'static str>,
    pub fact_flush_delay: Option<Duration>,
}
impl StateJournal for Journal {
    fn append(&mut self, c: &OperationControl, event: &str, s: &State) -> ChannelResult<()> {
        check_control(c)?;
        if self.fail == Some(event) {
            return Err(error("fixture.save_failed"));
        }
        if event == "input_observed" {
            if let Some(delay) = self.fact_flush_delay {
                std::thread::sleep(delay);
            }
        }
        // Simulate reload hydration and loss of host observed_at_millis.
        self.events.push((
            event.into(),
            serde_json::from_slice(&serde_json::to_vec(s).unwrap()).unwrap(),
        ));
        Ok(())
    }
    fn historical_request(
        &mut self,
        _: &OperationControl,
        _: &str,
    ) -> ChannelResult<Option<State>> {
        Ok(None)
    }
    fn write_result(&mut self, c: &OperationControl, op: &Operation) -> ChannelResult<()> {
        check_control(c)?;
        if self.fail == Some("write_result") {
            return Err(error("fixture.result_failed"));
        }
        self.results.push(op.clone());
        Ok(())
    }
    fn checkpoint(&mut self, c: &OperationControl, _: &mut State, _: bool) -> ChannelResult<()> {
        check_control(c)
    }
}
#[derive(Default)]
pub(crate) struct Peer {
    pub actions: Vec<String>,
    pub input_kinds: Vec<InputKind>,
    pub superseded: Vec<SlotEnvelope>,
    pub retired: Vec<SlotEnvelope>,
    pub candidate: Option<Identity>,
    pub proof: Option<RuntimeReplacementProof>,
    pub observe_error: Option<ChannelError>,
    pub retirement_error: Option<ChannelError>,
    pub supersede_error: Option<ChannelError>,
    pub proof_reads: usize,
    pub candidate_reads: usize,
    pub pending_bind: bool,
    pub expire_after_input: bool,
    pub cancel_after_input: bool,
    pub cancel_parent_on_input: Option<TrackedCancellation>,
    pub cancel_parent_after_deadline: Option<TrackedCancellation>,
    pub reject_capacity: bool,
    pub input_reads: usize,
}
impl Backend for Peer {
    fn observe(&mut self, c: &OperationControl, q: &ObservationQuery) -> ObservationDispatch {
        if let Err(err) = check_control(c) {
            return Err(err).into();
        }
        if let Some(err) = &self.observe_error {
            return Err(err.clone()).into();
        }
        if self.pending_bind
            || !(matches!(q.envelope.action.as_str(), "bind" | "unbind")
                || self.reject_capacity && q.envelope.action == "prepare")
        {
            return Ok(Observation::Pending { diagnostic: None }).into();
        }
        let e = &q.envelope;
        let mut identity = q.identity.clone();
        identity.owner = e.owner.clone();
        identity.fence = e.fence;
        identity.next_slot = e.index + 1;
        Ok(Observation::Receipt(Receipt {
            identity,
            nonce: e.nonce.clone(),
            ticket: e.ticket.clone(),
            action: e.action.clone(),
            state: if e.action == "bind" {
                "bound"
            } else if e.action == "prepare" {
                "rejected"
            } else {
                "unbound"
            }
            .into(),
            reason: if e.action == "prepare" {
                "slot_capacity"
            } else {
                ""
            }
            .into(),
            ..Receipt::default()
        }))
        .into()
    }
    fn publish(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        check_control(c)?;
        // Recovery tests must never silently execute business code.
        assert!(
            matches!(e.action.as_str(), "bind" | "unbind")
                || self.reject_capacity && e.action == "prepare"
        );
        self.actions.push(e.action.clone());
        Ok(())
    }
    fn observe_input(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        after: i64,
        _: &str,
    ) -> ChannelResult<InputObservation> {
        check_control(c)?;
        self.input_reads += 1;
        Ok(input(
            e.runtime.clone(),
            e.owner.clone(),
            e.fence,
            e.index,
            after + 200,
        ))
    }
    fn input(&mut self, c: &OperationControl, a: &InputAction) -> InputDispatch {
        let mut error = check_control(c).err();
        if error.is_none() {
            self.input_kinds.push(a.kind);
            if let Some(signal) = self.cancel_parent_on_input.take() {
                signal.cancel();
                error = Some(ChannelError::Cancelled);
            } else if self.cancel_after_input {
                c.tracked_cancel_handle().unwrap().cancel();
                error = Some(ChannelError::Cancelled);
            }
            if let Some(signal) = self.cancel_parent_after_deadline.take() {
                let deadline = c.deadline();
                // Actual events, not a supplied cause: goal at 200ms, parent
                // cancellation at 250ms, blocked backend returns at 300ms.
                std::thread::scope(|scope| {
                    let cancel = scope.spawn(move || {
                        std::thread::sleep(
                            deadline.saturating_duration_since(std::time::Instant::now())
                                + Duration::from_millis(50),
                        );
                        signal.cancel();
                    });
                    std::thread::sleep(
                        deadline.saturating_duration_since(std::time::Instant::now())
                            + Duration::from_millis(100),
                    );
                    cancel.join().unwrap();
                });
            }
            if self.expire_after_input {
                // No OS/game interaction: model queued input completing at the
                // invocation's actual monotonic deadline while wall time is frozen.
                std::thread::sleep(
                    c.deadline()
                        .saturating_duration_since(std::time::Instant::now())
                        + Duration::from_millis(1),
                );
            }
        }
        InputDispatch {
            outcome: InputOutcome {
                disposition: InputDisposition::Submitted,
                messages_queued: 1,
                at_millis: a.observation.as_ref().map_or(0, |o| o.observed_at_millis),
                reason: String::new(),
                retryable: false,
            },
            error,
        }
    }
    fn runtime_candidate(
        &mut self,
        c: &OperationControl,
        _: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        check_control(c)?;
        self.candidate_reads += 1;
        Ok(self.candidate.clone())
    }
    fn supersede(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        _: &Identity,
    ) -> ChannelResult<()> {
        check_control(c)?;
        if let Some(err) = self.supersede_error.take() {
            return Err(err);
        }
        self.superseded.push(e.clone());
        Ok(())
    }
    fn consumed(&mut self, c: &OperationControl, _: &SlotEnvelope) -> ChannelResult<()> {
        check_control(c)
    }
}
impl RuntimeRetirementBackend for Peer {
    fn observe_runtime_replacement(
        &mut self,
        c: &OperationControl,
        _: &Identity,
    ) -> ChannelResult<Option<RuntimeReplacementProof>> {
        check_control(c)?;
        self.proof_reads += 1;
        if let Some(err) = &self.observe_error {
            Err(err.clone())
        } else {
            Ok(self.proof.clone())
        }
    }
    fn retire_replaced_runtime(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        _: &RuntimeReplacementProof,
    ) -> ChannelResult<()> {
        check_control(c)?;
        if let Some(err) = self.retirement_error.take() {
            return Err(err);
        }
        self.retired.push(e.clone());
        Ok(())
    }
}
pub(crate) struct Schedule {
    pub millis: u64,
    pub waits: usize,
    pub stop_after: usize,
}
impl Default for Schedule {
    fn default() -> Self {
        Self {
            millis: 0,
            waits: 0,
            stop_after: 1,
        }
    }
}
impl ContinuationScheduler for Schedule {
    fn monotonic_millis(&self) -> u64 {
        self.millis
    }
    fn wait(&mut self, c: &OperationControl, duration: Duration) -> ChannelResult<()> {
        check_control(c)?;
        self.waits += 1;
        self.millis += duration.as_millis() as u64;
        if self.waits >= self.stop_after {
            Err(ChannelError::Cancelled)
        } else {
            Ok(())
        }
    }
}
pub(crate) type TestDriver = Driver<Peer, Journal, FakeClock, Tokens>;
pub(crate) fn identity(runtime: char) -> Identity {
    Identity {
        schema: SCHEMA.into(),
        runtime: runtime.to_string().repeat(32),
        next_slot: 5,
        slots: SLOT_COUNT,
        input_state: SCHEMA.into(),
        guid: "g".into(),
        character: "c".into(),
        realm: "r".into(),
        build: "b".into(),
        product: "retail".into(),
        release: "3.2.0".into(),
        ..Identity::default()
    }
}
pub(crate) fn driver(stage: OperationStage, policy: OperationPolicy) -> TestDriver {
    let mut d = Driver::new(
        identity('1'),
        Peer::default(),
        Journal::default(),
        FakeClock(Rc::new(Cell::new(1000))),
        Tokens(10),
    )
    .unwrap();
    d.state.bound = true;
    d.state.identity.owner = d.state.owner.clone();
    d.state.identity.fence = 1;
    d.prepare_request(
        &OperationControl::default(),
        "request-key",
        "return 1",
        5,
        policy,
    )
    .unwrap();
    let op = d.state.operation.as_mut().unwrap();
    op.stage = stage;
    if stage != OperationStage::Prepared {
        op.prepared_nonce = "a".repeat(32);
        op.challenge = "b".repeat(32);
    }
    if matches!(
        stage,
        OperationStage::ConfirmReady
            | OperationStage::ResultVerified
            | OperationStage::ReleaseReady
            | OperationStage::Complete
    ) {
        op.result_bytes.0 = br#"{"ok":true}"#.to_vec();
        op.report_bytes = op.result_bytes.0.len() as u32;
        op.report_checksum = checksum(&op.result_bytes.0);
    }
    d.state.validate().unwrap();
    d
}
pub(crate) fn transaction(d: &mut TestDriver, action: &str, phase: TransactionPhase) {
    let ticket = d.state.operation.as_ref().unwrap().ticket.clone();
    d.begin(&OperationControl::default(), action, ticket, |_| {})
        .unwrap();
    d.state.transaction.as_mut().unwrap().phase = phase;
}
pub(crate) fn restart(d: TestDriver) -> TestDriver {
    let state = d.journal.events.last().unwrap().1.clone();
    Driver::from_state(state, d.backend, d.journal, d.clock, d.tokens).unwrap()
}
pub(crate) fn input(
    runtime: String,
    owner: String,
    fence: u64,
    next_slot: i32,
    sample: i64,
) -> InputObservation {
    InputObservation {
        schema: SCHEMA.into(),
        runtime,
        owner,
        fence,
        next_slot,
        guid: "g".into(),
        build: "b".into(),
        sample_millis: sample,
        observed_at_millis: sample,
        input_blocked: Some(false),
        bindings: Some(InputBindings {
            wake: "ALT-CTRL-F12".into(),
            reload: "ALT-CTRL-F11".into(),
            ..InputBindings::default()
        }),
        ..InputObservation::default()
    }
}
pub(crate) fn target() -> RetirementTarget {
    RetirementTarget {
        process_id: 12,
        process_started_at: 34,
    }
}
pub(crate) fn proof() -> RuntimeReplacementProof {
    let current = identity('2');
    let first = input(
        current.runtime.clone(),
        String::new(),
        0,
        current.next_slot,
        1000,
    );
    let mut observation = first.clone();
    observation.sample_millis = 1200;
    RuntimeReplacementProof {
        schema: SCHEMA.into(),
        process_id: 12,
        process_started_at: 34,
        current,
        witness: RuntimeReplacementWitness {
            first,
            first_sequence: 1,
            address: 4096,
            length: 1024,
            before_sha256: "a".repeat(64),
            after_sha256: "b".repeat(64),
            sequence: 2,
            observation,
        },
    }
}
