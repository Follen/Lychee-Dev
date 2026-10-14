//! Fixed Go 3.2 recovery and continuation facts. A descriptor only selects a
//! candidate; a fresh exact bind is the authority for open-runtime replacement.
//! Oracle: channel/{recovery,budget,budget_driver,continuation}.go at
//! 3057a9666ab078270b63c7f06de748069b0d1f4b. No native adapter is opened here.
use super::Identity;
use super::driver::{Clock, Driver, TokenSource, budget, error, pending};
use super::journal::StateJournal;
use super::observations::{Backend, ChannelError, ChannelResult};
use super::state::{
    Blocker, CleanupMethod, DEFAULT_CLOSE_MILLIS, DEFAULT_CONNECT_MILLIS, DEFAULT_RECOVERY_MILLIS,
    DurableBudget, OperationPolicy, OperationStage, RecoveryPhase, ReloadPhase, ResultBytes,
    RuntimeRecovery, State,
};
use super::transactions::join_errors;
use lycheedev_core::control::OperationControl;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

pub(super) fn has_code(err: &ChannelError, code: &str) -> bool {
    err.any_cause(&|e| {
        matches!(e, ChannelError::Contract(c) if c.0 == code)
            || matches!(e, ChannelError::Pending { causes } if causes.contains(&code))
    })
}
pub(super) fn budget_exhausted(rollback: bool) -> ChannelError {
    let mut causes = vec!["live.channel_budget_exhausted"];
    if rollback {
        causes.push("live.channel_budget_clock_rollback");
    }
    ChannelError::Pending { causes }
}

/// Automatic cleanup/capacity reload borrows the operation's one durable goal.
/// JSON cannot preserve Go pointer aliasing, so reconcile the two persisted
/// views explicitly. Only this operation's named automatic reload is linked;
/// an older completed reload must not contaminate a later operation's budget.
pub(super) fn synchronize_automatic_budget(state: &mut State) -> ChannelResult<bool> {
    let (Some(op), Some(reload)) = (&state.operation, &state.reload) else {
        return Ok(false);
    };
    let capacity_hash = hex::encode(&Sha256::digest(op.id.as_bytes())[..16]);
    if !reload.automatic
        || ![
            format!("cleanup-{}", op.id),
            format!("capacity-{}", op.id),
            format!("capacity-{capacity_hash}"),
        ]
        .contains(&reload.request)
    {
        return Ok(false);
    }
    let merged = match (&op.recovery_budget, &reload.recovery_budget) {
        (None, None) => return Ok(false),
        (Some(b), None) | (None, Some(b)) => {
            b.validate()?;
            b.clone()
        }
        (Some(a), Some(b)) => {
            a.validate()?;
            b.validate()?;
            // A shared goal cannot acquire a different origin or deadline.
            if a.started_at_ms != b.started_at_ms || a.deadline_ms != b.deadline_ms {
                return Err(error("live.channel_budget_invalid"));
            }
            DurableBudget {
                started_at_ms: a.started_at_ms,
                deadline_ms: a.deadline_ms,
                last_observed_ms: a.last_observed_ms.max(b.last_observed_ms),
                exhausted: a.exhausted || b.exhausted,
                clock_rollback: a.clock_rollback || b.clock_rollback,
                legacy: a.legacy || b.legacy,
            }
        }
    };
    merged.validate()?;
    let changed = op.recovery_budget.as_ref() != Some(&merged)
        || reload.recovery_budget.as_ref() != Some(&merged);
    state.operation.as_mut().unwrap().recovery_budget = Some(merged.clone());
    state.reload.as_mut().unwrap().recovery_budget = Some(merged);
    Ok(changed)
}

/// First observed stop of the invocation's original bounded control. Separate
/// input-fact flushes must never replace cancellation with a later deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InvocationStop {
    Cancelled,
    DeadlineExceeded,
}

/// Observe once against the persisted absolute deadline. Exhaustion is sticky;
/// a later clock restoration cannot revive a goal. A changed fact must be saved
/// before any further input. Does not initialize absent legacy budgets.
pub fn observe_budget(b: &mut DurableBudget, now_ms: i64) -> (bool, ChannelResult<i64>) {
    if let Err(err) = b.validate() {
        return (false, Err(err.into()));
    }
    if b.exhausted {
        return (false, Err(budget_exhausted(b.clock_rollback)));
    }
    if now_ms < b.last_observed_ms {
        b.exhausted = true;
        b.clock_rollback = true;
        return (true, Err(budget_exhausted(true)));
    }
    let changed = now_ms != b.last_observed_ms;
    b.last_observed_ms = now_ms;
    if now_ms >= b.deadline_ms {
        b.exhausted = true;
        return (true, Err(budget_exhausted(false)));
    }
    (changed, Ok(b.deadline_ms - now_ms))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Goal {
    Reload,
    Close,
    Operation,
    Connect,
}
impl Goal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reload => "reload",
            Self::Close => "close",
            Self::Operation => "operation",
            Self::Connect => "connect",
        }
    }
    fn duration(self) -> i64 {
        match self {
            Self::Connect => DEFAULT_CONNECT_MILLIS,
            Self::Close => DEFAULT_CLOSE_MILLIS,
            _ => DEFAULT_RECOVERY_MILLIS,
        }
    }
}
pub fn goal(state: &State) -> Option<Goal> {
    if state
        .reload
        .as_ref()
        .is_some_and(|r| !r.automatic && r.phase != ReloadPhase::Complete)
    {
        Some(Goal::Reload)
    } else if state.closing {
        Some(Goal::Close)
    } else if state
        .reload
        .as_ref()
        .is_some_and(|r| r.phase != ReloadPhase::Complete)
    {
        Some(Goal::Reload)
    } else if state
        .operation
        .as_ref()
        .is_some_and(|o| !terminal_operation(o.stage))
    {
        Some(Goal::Operation)
    } else if !state.bound && !state.closed {
        Some(Goal::Connect)
    } else {
        None
    }
}
pub(super) fn terminal_operation(stage: OperationStage) -> bool {
    matches!(
        stage,
        OperationStage::Complete | OperationStage::Cancelled | OperationStage::ExecutionUnknown
    )
}
fn goal_budget(state: &State, goal: Goal) -> Option<&DurableBudget> {
    match goal {
        Goal::Reload => state
            .reload
            .as_ref()
            .and_then(|r| r.recovery_budget.as_ref()),
        Goal::Close => state.close_budget.as_ref(),
        Goal::Operation => state
            .operation
            .as_ref()
            .and_then(|o| o.recovery_budget.as_ref()),
        Goal::Connect => state.connect_budget.as_ref(),
    }
}
fn goal_budget_mut(state: &mut State, goal: Goal) -> &mut Option<DurableBudget> {
    match goal {
        Goal::Reload => {
            &mut state
                .reload
                .as_mut()
                .expect("selected reload goal")
                .recovery_budget
        }
        Goal::Close => &mut state.close_budget,
        Goal::Operation => {
            &mut state
                .operation
                .as_mut()
                .expect("selected operation goal")
                .recovery_budget
        }
        Goal::Connect => &mut state.connect_budget,
    }
}

/// Projection only: neither a second action selector nor an input authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Continuation {
    pub kind: String,
    pub session: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub goal: String,
    pub progress_version: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_budget_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocker: Option<Blocker>,
}
pub fn continuation(state: &State, now_ms: i64) -> Continuation {
    let mut c = Continuation {
        kind: "continue".into(),
        session: state.id.clone(),
        request_id: String::new(),
        goal: String::new(),
        progress_version: state.progress_version,
        remaining_budget_ms: None,
        blocker: state.blocker.clone(),
    };
    if state.runtime_end.is_some() && !state.closed {
        c.goal = "close".into();
        c.request_id = format!("close:{}", state.id);
        c.blocker = Some(super::runtime_retirement::retirement_blocker());
        return c;
    }
    let selected = goal(state);
    if let Some(g) = selected {
        c.goal = g.as_str().into();
    }
    if let Some(op) = &state.operation {
        c.request_id = op.request.clone();
    }
    if selected == Some(Goal::Reload) {
        c.request_id = state.reload.as_ref().unwrap().request.clone();
    }
    if selected == Some(Goal::Close) {
        c.request_id = format!("close:{}", state.id);
    }
    // A stale persisted blocker cannot override a terminal durable transition.
    if !state.closing
        && selected.is_none()
        && state
            .operation
            .as_ref()
            .is_some_and(|o| o.stage == OperationStage::ExecutionUnknown)
    {
        c.kind = "needs_decision".into();
        c.blocker = None;
        return c;
    }
    if selected.is_none()
        && (state.closed
            || state.bound
                && state.transaction.is_none()
                && state.operation.as_ref().is_none_or(|o| {
                    matches!(
                        o.stage,
                        OperationStage::Complete | OperationStage::Cancelled
                    )
                })
                && state
                    .recovery
                    .as_ref()
                    .is_none_or(|r| r.phase == RecoveryPhase::Complete))
    {
        c.kind = "completed".into();
        c.blocker = None;
        return c;
    }
    if let Some(b) = selected.and_then(|g| goal_budget(state, g)) {
        let remaining = if b.exhausted {
            0
        } else {
            b.deadline_ms.saturating_sub(now_ms).max(0)
        };
        c.remaining_budget_ms = Some(remaining);
        if remaining == 0 {
            c.kind = "budget_exhausted".into();
            return c;
        }
    }
    if let Some(b) = &c.blocker {
        c.kind = match b.kind.as_str() {
            "active_driver" => "wait_active_driver",
            "budget_exhausted" | "budget_clock_rollback" => "budget_exhausted",
            "prepared_operation_requires_reload" | "runtime_changed_during_recovery" => {
                "needs_decision"
            }
            "receipt_pending" | "input_keyboard_focus" => "continue",
            _ => "wait_external",
        }
        .into();
    }
    c
}

impl<B: Backend, J: StateJournal, C: Clock, T: TokenSource> Driver<B, J, C, T> {
    pub fn continuation(&self) -> Continuation {
        continuation(&self.state, self.clock.unix_millis())
    }

    pub fn request_close(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.check(control)?;
        if self.state.closed {
            return Ok(());
        }
        if self.state.close_budget.is_none() {
            self.state.close_budget = Some(budget(self.clock.unix_millis(), DEFAULT_CLOSE_MILLIS)?);
        }
        if self.state.closing {
            return Ok(());
        }
        self.state.closing = true;
        self.save(control, "disconnect_intent")
    }

    pub(super) fn budget_control(
        &mut self,
        control: &OperationControl,
    ) -> ChannelResult<OperationControl> {
        self.check(control)?;
        control
            .stop_cause()
            .map_err(|_| error("live.channel_cancellation_order_unavailable"))?;
        let synchronized = synchronize_automatic_budget(&mut self.state)?;
        let Some(selected) = goal(&self.state) else {
            return Ok(control.clone());
        };
        let now = self.clock.unix_millis();
        let reference = goal_budget_mut(&mut self.state, selected);
        let created = reference.is_none();
        if created {
            let mut b = budget(now, selected.duration())?;
            b.legacy = true;
            *reference = Some(b);
        }
        let (changed, result) = observe_budget(reference.as_mut().unwrap(), now);
        let synchronized = synchronize_automatic_budget(&mut self.state)? || synchronized;
        if changed || created || synchronized {
            self.save(
                control,
                if created {
                    "budget_migrated"
                } else {
                    "budget_observed"
                },
            )?;
        }
        let remaining = result.inspect_err(|err| {
            self.waiting = if has_code(err, "live.channel_budget_clock_rollback") {
                "budget_clock_rollback"
            } else {
                "budget_exhausted"
            }
            .into();
        })?;
        let deadline = Instant::now()
            .checked_add(Duration::from_millis(remaining as u64))
            .ok_or_else(|| error("live.channel_budget_invalid"))?;
        control
            .ordered_child(deadline)
            .map_err(|_| error("live.channel_cancellation_order_unavailable"))
    }

    /// Persist timeout even if wall time later returns below the deadline.
    pub(super) fn finish_budget(
        &mut self,
        parent: &OperationControl,
        bounded: &OperationControl,
    ) -> ChannelResult<()> {
        if bounded.deadline() >= parent.deadline()
            || self.invocation_stop.get() != Some(InvocationStop::DeadlineExceeded)
        {
            return Ok(());
        }
        let Some(selected) = goal(&self.state) else {
            return Ok(());
        };
        let Some(b) = goal_budget_mut(&mut self.state, selected).as_mut() else {
            return Ok(());
        };
        if b.exhausted {
            return Ok(());
        }
        b.exhausted = true;
        synchronize_automatic_budget(&mut self.state)?;
        self.waiting = "budget_exhausted".into();
        self.save(&flush_control(), "budget_observed")
    }

    pub fn recover_runtime(
        &mut self,
        control: &OperationControl,
        candidate: Identity,
    ) -> ChannelResult<()> {
        self.check(control)?;
        self.require_live_runtime()?;
        if candidate.inventory.is_some_and(|n| n != candidate.slots) {
            return Err(error("live.channel_slot_inventory_incomplete"));
        }
        if self
            .state
            .recovery
            .as_ref()
            .is_some_and(|r| r.phase != RecoveryPhase::Complete)
        {
            if candidate_matches(&self.state.identity, &candidate) {
                return Err(ChannelError::Blocked(Blocker {
                    kind: "runtime_changed_during_recovery".into(),
                    runtime: candidate.runtime,
                    condition: "original_process_end_or_protocol_recovery".into(),
                    ..Blocker::default()
                }));
            }
            return self.finish_recovery(control);
        }
        if !candidate_matches(&self.state.identity, &candidate) {
            return Err(pending());
        }
        if let Some(r) = self
            .state
            .reload
            .as_mut()
            .filter(|r| r.phase != ReloadPhase::Complete)
        {
            if !r.reconcile_pending
                && self.state.operation.as_ref().is_some_and(|o| {
                    o.stage == OperationStage::Prepared && o.prepared_nonce.is_empty()
                        || o.stage == OperationStage::ReleaseReady
                            && o.cleanup_method == CleanupMethod::ReloadRequired
                })
            {
                r.automatic = true;
            }
            r.phase = ReloadPhase::Binding;
            r.reconcile_pending = true;
        }
        self.state.recovery = Some(RuntimeRecovery {
            from: self.state.identity.clone(),
            transaction: self.state.transaction.take(),
            phase: RecoveryPhase::Binding,
        });
        self.state.identity = candidate;
        self.state.bound = false;
        self.save(control, "runtime_recovery_candidate")?;
        self.finish_recovery(control)
    }

    pub(super) fn finish_recovery(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.check(control)?;
        let Some(r) = self.state.recovery.as_ref() else {
            return Ok(());
        };
        if r.phase == RecoveryPhase::Complete {
            return Ok(());
        }
        if r.phase == RecoveryPhase::Binding {
            self.connect(control)?;
            self.state.recovery.as_mut().unwrap().phase = RecoveryPhase::Bound;
            self.save(control, "runtime_recovery_bound")?;
        }
        if let Some(tx) = &self.state.recovery.as_ref().unwrap().transaction {
            self.backend
                .supersede(control, &tx.envelope, &self.state.identity)?;
        }
        self.check(control)?;
        if let Some(op) = &self.state.operation {
            if !terminal_operation(op.stage) {
                // A durable commit intent may already be published and consumed
                // by a delayed key. An absent commit transaction is the proof.
                let safe = op.stage == OperationStage::Prepared
                    || op.stage == OperationStage::CommitReady
                        && self.state.recovery.as_ref().unwrap().transaction.is_none();
                if matches!(
                    op.stage,
                    OperationStage::ResultVerified | OperationStage::ReleaseReady
                ) {
                    self.journal.write_result(control, op)?;
                    let op = self.state.operation.as_mut().unwrap();
                    op.stage = OperationStage::Complete;
                    op.cleanup_method = CleanupMethod::RuntimeDestroyed;
                } else if self.state.closing && safe {
                    self.state.operation.as_mut().unwrap().stage = OperationStage::Cancelled;
                } else if !self.state.closing && (safe || op.policy == OperationPolicy::Observation)
                {
                    if op.attempt >= 3 {
                        return Err(error("live.channel_recovery_attempt_limit"));
                    }
                    let ticket = self.next_token()?;
                    let op = self.state.operation.as_mut().unwrap();
                    op.ticket = ticket;
                    op.attempt += 1;
                    op.origin = Some(self.state.identity.clone());
                    op.stage = OperationStage::Prepared;
                    op.prepared_nonce.clear();
                    op.challenge.clear();
                    op.result_bytes = ResultBytes::default();
                    op.report_bytes = 0;
                    op.report_checksum = 0;
                } else {
                    self.state.operation.as_mut().unwrap().stage = OperationStage::ExecutionUnknown;
                }
            }
        }
        self.state.recovery.as_mut().unwrap().phase = RecoveryPhase::Complete;
        self.save(control, "runtime_recovery_reconciled")?;
        if !self.state.closing
            && self
                .state
                .operation
                .as_ref()
                .is_some_and(|o| o.stage == OperationStage::ExecutionUnknown)
        {
            return Err(error("live.channel_execution_unknown"));
        }
        Ok(())
    }

    pub(super) fn close_bookkeeping_only(&self) -> bool {
        let s = &self.state;
        s.closing
            && !s.bound
            && s.transaction.is_none()
            && s.operation
                .as_ref()
                .is_none_or(|o| terminal_operation(o.stage))
            && s.reload
                .as_ref()
                .is_none_or(|r| r.phase == ReloadPhase::Complete)
            && s.recovery
                .as_ref()
                .is_none_or(|r| r.phase == RecoveryPhase::Complete)
    }

    pub(super) fn record_blocker(&mut self, result: &ChannelResult<()>) -> ChannelResult<()> {
        let next = result.as_ref().err().and_then(|err| {
            if has_code(err, "live.channel_budget_exhausted") {
                Some(Blocker {
                    kind: if has_code(err, "live.channel_budget_clock_rollback") {
                        "budget_clock_rollback"
                    } else {
                        "budget_exhausted"
                    }
                    .into(),
                    condition: "explicit_budget_decision".into(),
                    ..Blocker::default()
                })
            } else if let Some(blocker) = find_blocker(err) {
                Some(blocker.clone())
            } else if err.is_pending() {
                let kind = if self.waiting.is_empty() {
                    "receipt_pending"
                } else {
                    &self.waiting
                };
                let condition = match kind {
                    "receipt_pending" => "exact_receipt_or_runtime_change",
                    "prepared_operation_requires_reload" => {
                        "authorized_reload_or_runtime_destruction"
                    }
                    "closing_exchange_unconfirmed" => "exact_receipt_or_runtime_destruction",
                    "budget_exhausted" | "budget_clock_rollback" => "explicit_budget_decision",
                    "shared_publication" => "publication_dependency_changed",
                    _ => "input_condition_changed",
                };
                let mut b = Blocker {
                    kind: kind.into(),
                    runtime: self.state.identity.runtime.clone(),
                    condition: condition.into(),
                    ..Blocker::default()
                };
                if let Some(tx) = &self.state.transaction {
                    b.slot = tx.envelope.index;
                    b.nonce = tx.envelope.nonce.clone();
                }
                Some(b)
            } else {
                None
            }
        });
        if self.state.blocker == next {
            return Ok(());
        }
        self.state.blocker = next;
        self.save(&flush_control(), "blocker_observed")
    }
}
fn candidate_matches(old: &Identity, candidate: &Identity) -> bool {
    candidate.validate().is_ok()
        && candidate.runtime > old.runtime
        && candidate.owner.is_empty()
        && candidate.guid == old.guid
        && candidate.build == old.build
        && candidate.product == old.product
        && candidate.release == old.release
}
pub(super) fn find_blocker(err: &ChannelError) -> Option<&Blocker> {
    match err {
        ChannelError::Blocked(b) | ChannelError::SlotAvailable { blocker: b, .. } => Some(b),
        ChannelError::Joined { causes } => causes.iter().find_map(find_blocker),
        _ => None,
    }
}
pub(super) fn flush_control() -> OperationControl {
    OperationControl::new(
        Instant::now() + Duration::from_secs(3),
        Arc::new(AtomicBool::new(false)),
    )
}
pub(super) fn join_result(
    result: ChannelResult<()>,
    later: ChannelResult<()>,
) -> ChannelResult<()> {
    match (result, later) {
        (Err(a), Err(b)) => Err(join_errors(a, b)),
        (Err(a), _) | (_, Err(a)) => Err(a),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "recovery_test_support.rs"]
pub(crate) mod test_support;
