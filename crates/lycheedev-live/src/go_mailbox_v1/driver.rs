//! Restartable fixed-Go Mailbox v1 business driver. This is the pure orchestration
//! slice, not a Windows input adapter or a durable filesystem journal. Production
//! callers must own the connection lease and use the continuation/budget layer.
//! Oracle: channel/driver.go at 3057a9666ab078270b63c7f06de748069b0d1f4b.
use super::journal::{StateJournal, save_state};
use super::observations::{
    Backend, ChannelError, ChannelResult, Observation, ObservationKind, ObservationQuery,
    check_control,
};
use super::recovery::{InvocationStop, synchronize_automatic_budget};
use super::slot_allocation::operation_needs_reload;
use super::state::{
    Blocker, CHANNEL_SCHEMA, CleanupMethod, DEFAULT_CONNECT_MILLIS, DEFAULT_RECOVERY_MILLIS,
    DurableBudget, Operation, OperationPolicy, OperationStage, RecoveryPhase, ReloadAttempt,
    ReloadPhase, SlotEnvelope, State, validate_request,
};
use super::{ContractError, Identity, MAX_PAYLOAD, SCHEMA, checksum, parse_token};
use lycheedev_core::control::{OperationControl, StopCause};
use std::cell::Cell;

/// Wall-clock Unix milliseconds for durable budgets and the release wait. INPUT
/// uses same-machine uptime supplied by its backend, never this wall clock.
pub trait Clock {
    fn unix_millis(&self) -> i64;
}
/// Production supplies cryptographically random lowercase 128-bit tokens.
/// Tests inject deterministic tokens; this trait must not fall back to time/PID.
pub trait TokenSource {
    fn token(&mut self) -> ChannelResult<String>;
}

pub struct Driver<B, J, C, T> {
    pub state: State,
    pub backend: B,
    pub journal: J,
    pub clock: C,
    pub tokens: T,
    pub waiting: String,
    // An append/rotation failure can be ambiguous. Reusing this in-memory state
    // must never bypass the failed durable intent; reload the verified journal.
    pub(super) journal_failed: bool,
    // A control stop proved to occur before append is different: no backend
    // effect may proceed, but a fresh bounded save may persist known facts and
    // sticky budget exhaustion. Never clear an ambiguous append failure here.
    pub(super) save_pending: bool,
    pub(super) invocation_control: Option<OperationControl>,
    pub(super) invocation_stop: Cell<Option<InvocationStop>>,
}

/// Witness the exact boundary rather than guessing from an error code: a
/// journal may return Cancelled/Deadline after a partial or completed append.
struct AppendWitness<'a, J> {
    inner: &'a mut J,
    entered: &'a mut bool,
}
impl<J: StateJournal> StateJournal for AppendWitness<'_, J> {
    fn append(
        &mut self,
        control: &OperationControl,
        event: &str,
        state: &State,
    ) -> ChannelResult<()> {
        *self.entered = true;
        self.inner.append(control, event, state)
    }
    fn historical_request(
        &mut self,
        control: &OperationControl,
        request: &str,
    ) -> ChannelResult<Option<State>> {
        self.inner.historical_request(control, request)
    }
    fn write_result(
        &mut self,
        control: &OperationControl,
        operation: &Operation,
    ) -> ChannelResult<()> {
        self.inner.write_result(control, operation)
    }
    fn checkpoint(
        &mut self,
        control: &OperationControl,
        state: &mut State,
        known_zero: bool,
    ) -> ChannelResult<()> {
        self.inner.checkpoint(control, state, known_zero)
    }
}

pub(super) fn error(code: &'static str) -> ChannelError {
    ContractError(code).into()
}
pub(super) fn pending() -> ChannelError {
    ChannelError::Pending { causes: Vec::new() }
}
pub(super) fn budget(now: i64, duration: i64) -> ChannelResult<DurableBudget> {
    let deadline = now
        .checked_add(duration)
        .ok_or_else(|| error("live.channel_budget_invalid"))?;
    let b = DurableBudget {
        started_at_ms: now,
        deadline_ms: deadline,
        last_observed_ms: now,
        ..DurableBudget::default()
    };
    b.validate()?;
    Ok(b)
}

impl<B: Backend, J: StateJournal, C: Clock, T: TokenSource> Driver<B, J, C, T> {
    pub fn new(
        identity: Identity,
        backend: B,
        journal: J,
        clock: C,
        mut tokens: T,
    ) -> ChannelResult<Self> {
        identity.validate()?;
        let owner = tokens.token()?;
        parse_token(&owner)?;
        let state = State {
            schema: CHANNEL_SCHEMA.into(),
            id: format!("CON-{owner}"),
            owner,
            identity,
            connect_budget: Some(budget(clock.unix_millis(), DEFAULT_CONNECT_MILLIS)?),
            ..State::default()
        };
        Self::from_state(state, backend, journal, clock, tokens)
    }

    /// The caller must first verify and hydrate the complete journal/history.
    /// This constructor never invents a new identity, request, nonce or deadline.
    pub fn from_state(
        mut state: State,
        backend: B,
        journal: J,
        clock: C,
        tokens: T,
    ) -> ChannelResult<Self> {
        state.validate()?;
        synchronize_automatic_budget(&mut state)?;
        Ok(Self {
            state,
            backend,
            journal,
            clock,
            tokens,
            waiting: String::new(),
            journal_failed: false,
            save_pending: false,
            invocation_control: None,
            invocation_stop: Cell::new(None),
        })
    }

    pub(crate) fn check(&self, control: &OperationControl) -> ChannelResult<()> {
        self.capture_invocation_stop(control)?;
        if self.journal_failed || self.save_pending {
            return Err(error("live.channel_journal_reload_required"));
        }
        check_control(control)
    }

    /// Ask the ordered control for its immutable first cause, rather than
    /// infer event order from a token sampled after a blocked backend returns.
    pub(super) fn capture_invocation_stop(&self, control: &OperationControl) -> ChannelResult<()> {
        if self
            .invocation_control
            .as_ref()
            .is_none_or(|active| !active.same_scope(control))
        {
            return Ok(());
        }
        let cause = control
            .stop_cause()
            .map_err(|_| error("live.channel_cancellation_order_unavailable"))?;
        if self.invocation_stop.get().is_none() {
            self.invocation_stop.set(cause.map(|cause| match cause {
                StopCause::Cancelled => InvocationStop::Cancelled,
                StopCause::DeadlineExceeded => InvocationStop::DeadlineExceeded,
            }));
        }
        Ok(())
    }

    pub(crate) fn next_token(&mut self) -> ChannelResult<String> {
        let token = self.tokens.token()?;
        parse_token(&token)?;
        Ok(token)
    }

    pub(crate) fn save(&mut self, control: &OperationControl, event: &str) -> ChannelResult<()> {
        if self.journal_failed {
            return Err(error("live.channel_journal_reload_required"));
        }
        self.save_pending = true;
        self.capture_invocation_stop(control)?;
        if let Err(err) = synchronize_automatic_budget(&mut self.state) {
            self.journal_failed = true;
            return Err(err);
        }
        let mut entered = false;
        let mut journal = AppendWitness {
            inner: &mut self.journal,
            entered: &mut entered,
        };
        let result = save_state(&mut journal, control, &mut self.state, event);
        let observed = self.capture_invocation_stop(control);
        match &result {
            Ok(()) => self.save_pending = false,
            Err(ChannelError::Cancelled | ChannelError::DeadlineExceeded) if !entered => {
                // The outer continuation may flush known facts under its own
                // 3s control. check() still forbids all new external effects.
            }
            Err(_) => self.journal_failed = true,
        }
        super::recovery::join_result(result, observed)
    }

    pub(crate) fn checkpoint(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.check(control)?;
        if self.state.transaction.is_some()
            || self
                .state
                .reload
                .as_ref()
                .is_some_and(|r| r.phase != ReloadPhase::Complete)
            || self
                .state
                .recovery
                .as_ref()
                .is_some_and(|r| r.phase != RecoveryPhase::Complete)
            || self.state.operation.as_ref().is_some_and(|o| {
                !matches!(
                    o.stage,
                    OperationStage::Complete
                        | OperationStage::ExecutionUnknown
                        | OperationStage::Cancelled
                )
            })
        {
            return Err(pending());
        }
        self.checkpoint_log(control, false)
    }

    pub(super) fn checkpoint_log(
        &mut self,
        control: &OperationControl,
        known_zero: bool,
    ) -> ChannelResult<()> {
        self.check(control)?;
        let result = self
            .journal
            .checkpoint(control, &mut self.state, known_zero);
        if result.is_err() {
            self.journal_failed = true;
        }
        result
    }

    /// Read-only request lookup corresponding to Project.Execute's lookup and
    /// locked recheck. A completed historical request does not replace a newer
    /// operation. The coordinator returns the historic result without Run.
    pub fn historical_request(
        &mut self,
        control: &OperationControl,
        request: &str,
        code: impl AsRef<[u8]>,
        budget: i32,
        policy: OperationPolicy,
    ) -> ChannelResult<Option<State>> {
        self.check(control)?;
        validate_request(request)?;
        let code = code.as_ref();
        let previous = self.journal.historical_request(control, request)?;
        if let Some(state) = &previous {
            state.validate()?;
            let operation = state
                .operation
                .as_ref()
                .ok_or_else(|| error("live.channel_request_history_invalid"))?;
            if operation.request != request {
                return Err(error("live.channel_request_history_invalid"));
            }
            if operation.code.as_bytes() != code
                || operation.budget != budget
                || operation.policy != policy
            {
                return Err(error("live.channel_request_conflict"));
            }
        }
        Ok(previous)
    }

    pub fn connect(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.check(control)?;
        if self.state.bound {
            return Ok(());
        }
        if self.state.transaction.is_none() {
            self.begin(control, "bind", self.state.owner.clone(), |_| {})?;
        }
        let receipt = self.advance(control)?;
        if receipt.state != "bound" {
            return Err(error("live.channel_bind_not_confirmed"));
        }
        self.finish_transaction(control, |state| state.bound = true)
    }

    /// Like fixed-Go PrepareRequest, this is the low-level creation boundary.
    /// The owning coordinator must perform historical_request before this call,
    /// including a recheck under the connection lease.
    pub fn prepare_request(
        &mut self,
        control: &OperationControl,
        request: &str,
        code: impl AsRef<[u8]>,
        execution_budget: i32,
        policy: OperationPolicy,
    ) -> ChannelResult<()> {
        self.check(control)?;
        self.require_live_runtime()?;
        if self.state.closing || self.state.closed {
            return Err(error("live.channel_stopping"));
        }
        if !self.state.bound || self.state.transaction.is_some() {
            return Err(error("live.channel_not_idle"));
        }
        if self
            .state
            .operation
            .as_ref()
            .is_some_and(|o| o.stage != OperationStage::Complete)
        {
            return Err(pending());
        }
        let code = code.as_ref();
        if code.is_empty()
            || code.len() > 262144
            || code[0] == 27
            || !(1..=120).contains(&execution_budget)
        {
            return Err(error("live.channel_request_invalid"));
        }
        // Empty is retained for the Go low-level API; nonempty public keys are
        // validated before external state changes and again by State.validate.
        if !request.is_empty() {
            validate_request(request)?;
        }
        self.checkpoint(control)?;
        let id = self.next_token()?;
        let ticket = self.next_token()?;
        let mut operation = Operation::prepared_bytes(
            format!("LMO-{id}"),
            ticket,
            code.into(),
            execution_budget,
            policy,
        );
        operation.request = request.into();
        operation.origin = Some(self.state.identity.clone());
        operation.attempt = 1;
        operation.recovery_budget =
            Some(budget(self.clock.unix_millis(), DEFAULT_RECOVERY_MILLIS)?);
        self.state.operation = Some(operation);
        self.save(control, "operation_created")
    }

    pub(crate) fn require_live_runtime(&self) -> ChannelResult<()> {
        if self.state.runtime_end.is_some() {
            Err(ChannelError::Blocked(Blocker {
                kind: "runtime_retirement_pending".into(),
                condition: "precise_resource_retirement".into(),
                ..Blocker::default()
            }))
        } else {
            Ok(())
        }
    }

    /// Restarts at durable boundaries; a submitted/unknown commit is only read.
    /// This does not implement the outer deadline/reload/replacement scheduler.
    pub fn run(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.check(control)?;
        if self.state.operation.is_none() {
            return Err(error("live.channel_operation_missing"));
        }
        loop {
            self.check(control)?;
            let stage = self
                .state
                .operation
                .as_ref()
                .expect("operation remains present")
                .stage;
            if self.state.closing && self.state.transaction.is_none() {
                if stage == OperationStage::Prepared {
                    self.state.operation.as_mut().unwrap().stage = OperationStage::Cancelled;
                    return self.save(control, "operation_cancelled_before_publication");
                }
                if stage == OperationStage::CommitReady {
                    self.waiting = "prepared_operation_requires_reload".into();
                    return Err(pending());
                }
            }
            match stage {
                OperationStage::ExecutionUnknown => {
                    return Err(error("live.channel_execution_unknown"));
                }
                OperationStage::Complete | OperationStage::Cancelled => return Ok(()),
                OperationStage::Prepared => {
                    if self.state.transaction.is_none()
                        && operation_needs_reload(
                            self.state.identity.next_slot,
                            self.state.identity.slots,
                        )
                    {
                        return Err(error("live.channel_reload_required"));
                    }
                    if self.state.transaction.is_none() {
                        let op = self.state.operation.as_ref().unwrap();
                        let (ticket, code, execution_budget) =
                            (op.ticket.clone(), op.code.clone(), op.budget);
                        self.begin(control, "prepare", ticket, |e| {
                            e.code_bytes = code.len() as i32;
                            e.code_checksum = checksum(code.as_bytes());
                            e.code = code;
                            e.budget = execution_budget;
                        })?;
                    }
                    let receipt = match self.advance(control) {
                        Ok(r) => r,
                        Err(err) => {
                            if rejection_reason(&err) == Some("slot_capacity") {
                                self.finish_transaction(control, |s| {
                                    let op = s.operation.as_ref().unwrap();
                                    s.reload = Some(ReloadAttempt {
                                        request: format!("capacity-{}", op.id),
                                        from: s.identity.runtime.clone(),
                                        phase: ReloadPhase::Intent,
                                        recovery_budget: op.recovery_budget.clone(),
                                        automatic: true,
                                        reconcile_pending: false,
                                        input_step: 0,
                                        patch_transitions: 0,
                                    });
                                })?;
                                return Err(error("live.channel_reload_required"));
                            }
                            return Err(err);
                        }
                    };
                    if !matches!(receipt.state.as_str(), "prepared" | "reported")
                        || receipt.challenge.len() != 32
                    {
                        return Err(error("live.channel_prepare_mismatch"));
                    }
                    self.finish_transaction(control, |s| {
                        let nonce = s.transaction.as_ref().unwrap().envelope.nonce.clone();
                        let op = s.operation.as_mut().unwrap();
                        op.prepared_nonce = nonce;
                        op.challenge = receipt.challenge;
                        op.stage = if receipt.state == "reported" {
                            OperationStage::Running
                        } else {
                            OperationStage::CommitReady
                        };
                    })?;
                }
                OperationStage::CommitReady => {
                    if self.state.transaction.is_none() {
                        let op = self.state.operation.as_ref().unwrap();
                        let (ticket, challenge, prepared_nonce) = (
                            op.ticket.clone(),
                            op.challenge.clone(),
                            op.prepared_nonce.clone(),
                        );
                        self.begin(control, "commit", ticket, |e| {
                            e.challenge = challenge;
                            e.prepared_nonce = prepared_nonce;
                        })?;
                    }
                    let receipt = self.advance(control)?;
                    if receipt.state != "accepted" {
                        return Err(error("live.channel_commit_not_accepted"));
                    }
                    self.finish_transaction(control, |s| {
                        s.operation.as_mut().unwrap().stage = OperationStage::Running
                    })?;
                }
                OperationStage::Running => {
                    let op = self.state.operation.as_ref().unwrap();
                    let i = &self.state.identity;
                    let envelope = SlotEnvelope {
                        nonce: op.prepared_nonce.clone(),
                        runtime: i.runtime.clone(),
                        ticket: op.ticket.clone(),
                        owner: self.state.owner.clone(),
                        fence: i.fence,
                        guid: i.guid.clone(),
                        build: i.build.clone(),
                        ..SlotEnvelope::default()
                    };
                    let query = ObservationQuery {
                        kind: ObservationKind::Result,
                        envelope,
                        identity: i.clone(),
                    };
                    let dispatch = self.backend.observe(control, &query);
                    if let Err(cancel) = check_control(control) {
                        return Err(match dispatch.error {
                            Some(err) => super::transactions::join_errors(err, cancel),
                            None => cancel,
                        });
                    }
                    if let Some(err) = dispatch.error {
                        return Err(err);
                    }
                    let observation = dispatch
                        .observation
                        .ok_or_else(|| error("live.channel_observation_missing"))?;
                    let (receipt, payload) = match observation {
                        Observation::Reported { receipt, payload } => (receipt, payload),
                        Observation::Pending { .. } => return Err(pending()),
                        _ => return Err(error("live.channel_report_record_invalid")),
                    };
                    validate_result(&query, &receipt, &payload)?;
                    let op = self.state.operation.as_mut().unwrap();
                    op.result_bytes.0 = payload;
                    op.report_bytes = receipt.report_bytes;
                    op.report_checksum = receipt.report_checksum;
                    op.stage = OperationStage::ConfirmReady;
                    self.save(control, "report_candidate")?;
                }
                OperationStage::ConfirmReady => {
                    if self.state.transaction.is_none() {
                        let ticket = self.state.operation.as_ref().unwrap().ticket.clone();
                        self.begin(control, "confirm", ticket, |_| {})?;
                    }
                    let receipt = self.advance(control)?;
                    let op = self.state.operation.as_ref().unwrap();
                    if receipt.state != "reported"
                        || receipt.prepared_nonce != op.prepared_nonce
                        || receipt.report_bytes != op.report_bytes
                        || receipt.report_checksum != op.report_checksum
                    {
                        return Err(error("live.channel_report_not_current"));
                    }
                    self.finish_transaction(control, |s| {
                        s.operation.as_mut().unwrap().stage = OperationStage::ResultVerified
                    })?;
                }
                OperationStage::ResultVerified => {
                    self.journal
                        .write_result(control, self.state.operation.as_ref().unwrap())?;
                    self.state.operation.as_mut().unwrap().stage = OperationStage::ReleaseReady;
                    self.save(control, "result_durable")?;
                }
                OperationStage::ReleaseReady => {
                    if self.state.operation.as_ref().unwrap().cleanup_method
                        == CleanupMethod::ReloadRequired
                    {
                        return Err(error("live.channel_cleanup_reload_required"));
                    }
                    if self.state.transaction.is_none() {
                        let op = self.state.operation.as_ref().unwrap();
                        let (ticket, bytes, sum) =
                            (op.ticket.clone(), op.report_bytes, op.report_checksum);
                        self.begin(control, "release", ticket, |e| {
                            e.report_bytes = bytes;
                            e.report_checksum = sum;
                        })?;
                    }
                    let receipt = match self.advance(control) {
                        Ok(r) => r,
                        Err(err) => {
                            if has_pending(&err) {
                                return self.recover_missing_release(control, err);
                            }
                            if rejection_reason(&err) == Some("slot_resources_pending") {
                                self.finish_transaction(control, |s| {
                                    s.operation.as_mut().unwrap().cleanup_method =
                                        CleanupMethod::ReloadRequired
                                })?;
                                return Err(error("live.channel_cleanup_reload_required"));
                            }
                            return Err(err);
                        }
                    };
                    if receipt.state != "released" {
                        return Err(error("live.channel_release_not_confirmed"));
                    }
                    self.finish_transaction(control, |s| {
                        let op = s.operation.as_mut().unwrap();
                        op.stage = OperationStage::Complete;
                        op.cleanup_method = CleanupMethod::Released;
                    })?;
                }
            }
        }
    }

    pub fn disconnect(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.check(control)?;
        if self
            .state
            .transaction
            .as_ref()
            .is_some_and(|t| t.envelope.action != "unbind")
        {
            return Err(pending());
        }
        if !self.state.bound {
            return Ok(());
        }
        if self.state.operation.as_ref().is_some_and(|o| {
            !matches!(
                o.stage,
                OperationStage::Complete
                    | OperationStage::ExecutionUnknown
                    | OperationStage::Cancelled
            )
        }) {
            return Err(pending());
        }
        if self.state.transaction.is_none() {
            self.checkpoint(control)?;
            self.begin(control, "unbind", self.state.owner.clone(), |_| {})?;
        }
        let receipt = self.advance(control)?;
        if receipt.state != "unbound" {
            return Err(error("live.channel_unbind_not_confirmed"));
        }
        self.finish_transaction(control, |s| s.bound = false)
    }
}

/// Backend result reads already perform guarded HEAD/BODY lookup. Requalifying
/// typed data also makes simulations obey the same checksum and target contract.
fn validate_result(q: &ObservationQuery, r: &super::Receipt, bytes: &[u8]) -> ChannelResult<()> {
    r.validate()?;
    let e = &q.envelope;
    let i = &r.identity;
    if i.schema != SCHEMA
        || r.state != "reported"
        || r.action != "prepare"
        || r.nonce != e.nonce
        || r.ticket != e.ticket
        || i.runtime != e.runtime
        || i.owner != e.owner
        || i.fence != e.fence
        || i.guid != e.guid
        || i.build != e.build
        || i.product != q.identity.product
        || i.release != q.identity.release
    {
        return Err(error("live.channel_receipt_mismatch"));
    }
    if r.report_bytes == 0 || r.report_bytes as usize > MAX_PAYLOAD {
        return Err(error("live.channel_report_length"));
    }
    if bytes.len() != r.report_bytes as usize || checksum(bytes) != r.report_checksum {
        return Err(error("live.channel_report_record_invalid"));
    }
    serde_json::from_slice::<Box<serde_json::value::RawValue>>(bytes)
        .map_err(|_| error("live.channel_report_json"))?;
    Ok(())
}

// These recursive helpers retain joined error categories. The backend must not
// flatten a pending/cancel/identity/IO pair into a success or an ordinary timeout.
pub(crate) fn has_pending(err: &ChannelError) -> bool {
    err.is_pending()
}
pub(crate) fn rejection_reason(err: &ChannelError) -> Option<&str> {
    match err {
        ChannelError::Rejected { reason } => Some(reason),
        ChannelError::Joined { causes } => causes.iter().find_map(rejection_reason),
        _ => None,
    }
}

#[cfg(test)]
#[path = "driver_tests.rs"]
mod tests;
