//! One connected scheduler for reload, recovery, close and business continuation.
//! The original durable deadline bounds every resume. Waiting/discovery are
//! injected; this module neither scans process memory nor sends physical input.
//! Oracle: channel/{reload,wait}.go at fixed Go 3.2.0.
use super::driver::{Clock, Driver, TokenSource, budget, error, pending};
use super::journal::StateJournal;
use super::observations::{Backend, ChannelError, ChannelResult};
use super::recovery::{
    Goal, InvocationStop, budget_exhausted, find_blocker, goal, goal_budget, has_code, join_result,
    terminal_operation,
};
use super::runtime_retirement::{RetirementTarget, RuntimeRetirementBackend, retirement_pending};
use super::slot_allocation::operation_needs_reload;
use super::state::{
    CleanupMethod, DEFAULT_RECOVERY_MILLIS, InputKind, OperationStage, RecoveryPhase,
    ReloadAttempt, ReloadPhase, SlotBudgetSource, SlotEnvelope, State, validate_request,
};
use super::transactions::join_errors;
use lycheedev_core::control::OperationControl;
use sha2::{Digest, Sha256};
use std::time::Duration;

/// Monotonic invocation scheduling, independent of persisted wall-clock budgets
/// and backend same-machine INPUT uptime. Implement wait cooperatively against
/// the passed control; tests can advance time without any OS/game access.
pub trait ContinuationScheduler {
    fn monotonic_millis(&self) -> u64;
    fn wait(&mut self, control: &OperationControl, duration: Duration) -> ChannelResult<()>;
}
pub fn discovery_cooldown_millis(elapsed: u64) -> u64 {
    elapsed.clamp(1000, 30_000)
}

/// Only the current publication gap is retryable ordinarily. An already
/// attempted reload also permits transient null state/old-string-layout gaps.
/// The typed original error remains attached, including independent faults.
pub fn pending_observation(state: &State, err: ChannelError) -> ChannelError {
    if has_code(&err, "memory.mailbox_unavailable: path_changed")
        || state
            .reload
            .as_ref()
            .is_some_and(|r| r.phase == ReloadPhase::InputAttempted)
            && (has_code(&err, "memory.mailbox_unavailable: lua_state_null")
                || has_code(&err, "memory.mailbox_unavailable: string_layout"))
    {
        join_errors(pending(), err)
    } else {
        err
    }
}
fn retryable_pending(state: &State, err: &ChannelError) -> bool {
    err.is_pending()
        && err.all_causes(&|c| match c {
            ChannelError::Pending { causes } => !causes.iter().any(|c| {
                matches!(
                    *c,
                    "live.channel_budget_exhausted" | "live.channel_budget_clock_rollback"
                )
            }),
            ChannelError::Contract(e) => {
                e.0 == "memory.mailbox_unavailable: path_changed"
                    || state
                        .reload
                        .as_ref()
                        .is_some_and(|r| r.phase == ReloadPhase::InputAttempted)
                        && matches!(
                            e.0,
                            "memory.mailbox_unavailable: lua_state_null"
                                | "memory.mailbox_unavailable: string_layout"
                        )
            }
            _ => false,
        })
}
fn progress(err: &ChannelError) -> bool {
    err.all_causes(&|c| matches!(c, ChannelError::Contract(e) if e.0 == "live.channel_progress"))
}
fn known_input_blocker(reason: &str) -> bool {
    !matches!(reason, "" | "input_observation_unavailable")
}

impl<B: Backend, J: StateJournal, C: Clock, T: TokenSource> Driver<B, J, C, T> {
    /// Fixed Go 3.2.2 startup activation borrows the original goal deadline.
    /// Later unbound runtimes can request another reload without replacing the
    /// original explicit request, business evidence or recovery transaction.
    pub(super) fn request_slot_reload(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.check(control)?;
        let runtime = self.state.identity.runtime.clone();
        if let Some(reload) = self
            .state
            .reload
            .as_mut()
            .filter(|r| r.phase != ReloadPhase::Complete)
        {
            if reload.from == runtime || reload.phase != ReloadPhase::Binding {
                return Err(pending());
            }
            reload.from = runtime;
            reload.phase = ReloadPhase::Intent;
            reload.slot_enablement = true;
        } else {
            let selected = goal(&self.state).unwrap_or(Goal::Connect);
            let original_budget = goal_budget(&self.state, selected)
                .cloned()
                .ok_or_else(|| error("live.channel_budget_invalid"))?;
            let source = match selected {
                Goal::Connect => SlotBudgetSource::Connect,
                Goal::Close => SlotBudgetSource::Close,
                Goal::Operation => SlotBudgetSource::Operation {
                    operation_id: self.state.operation.as_ref().unwrap().id.clone(),
                },
                // An active reload was handled above; never invent another link.
                Goal::Reload => return Err(error("live.channel_budget_invalid")),
            };
            self.state.reload = Some(ReloadAttempt {
                request: format!("slots-{runtime}"),
                from: runtime,
                phase: ReloadPhase::Intent,
                automatic: true,
                slot_enablement: true,
                slot_budget_source: Some(source),
                reconcile_pending: true,
                recovery_budget: Some(original_budget),
                input_step: 0,
                patch_transitions: 0,
            });
        }
        self.waiting.clear();
        self.save(control, "slot_enable_reload_intent")?;
        Err(error("live.channel_progress"))
    }

    /// Explicit control intent preserves any uncertain exchange. Closing an
    /// unconfirmed initial bind is allowed, but never binds the replacement.
    pub fn request_reload(
        &mut self,
        control: &OperationControl,
        request: &str,
    ) -> ChannelResult<()> {
        self.check(control)?;
        if self.state.runtime_end.is_some() {
            return Err(retirement_pending());
        }
        validate_request(request)?;
        let closing_bind = self.state.closing
            && self
                .state
                .transaction
                .as_ref()
                .is_some_and(|tx| tx.envelope.action == "bind");
        if !self.state.bound && !closing_bind
            || self.state.closed
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
        {
            return Err(pending());
        }
        let reconcile = self.state.transaction.is_some()
            || self
                .state
                .operation
                .as_ref()
                .is_some_and(|o| o.stage != OperationStage::Complete);
        if !reconcile {
            self.checkpoint(control)?;
        }
        self.state.reload = Some(ReloadAttempt {
            automatic: false,
            slot_enablement: false,
            slot_budget_source: None,
            recovery_budget: Some(budget(self.clock.unix_millis(), DEFAULT_RECOVERY_MILLIS)?),
            reconcile_pending: reconcile,
            request: request.into(),
            from: self.state.identity.runtime.clone(),
            phase: ReloadPhase::Intent,
            input_step: 0,
            patch_transitions: 0,
        });
        self.save(control, "reload_intent")
    }
    pub(super) fn closing_explicit_reload(&self) -> bool {
        self.state.closing
            && self
                .state
                .reload
                .as_ref()
                .is_some_and(|r| !r.automatic && r.phase != ReloadPhase::Complete)
    }
}

impl<B: RuntimeRetirementBackend, J: StateJournal, C: Clock, T: TokenSource> Driver<B, J, C, T> {
    pub(super) fn step_reload(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
    ) -> ChannelResult<()> {
        self.check(control)?;
        let r = self
            .state
            .reload
            .as_ref()
            .ok_or_else(|| error("live.channel_reload_missing"))?;
        if r.phase == ReloadPhase::Intent {
            let exchange = format!("reload:{}", r.request);
            let from = r.from.clone();
            let i = &self.state.identity;
            let envelope = SlotEnvelope {
                runtime: i.runtime.clone(),
                owner: i.owner.clone(),
                fence: i.fence,
                guid: i.guid.clone(),
                build: i.build.clone(),
                index: i.next_slot,
                action: "reload".into(),
                ..SlotEnvelope::default()
            };
            self.submit_input(control, &exchange, &from, InputKind::Reload, &envelope)?;
            self.state.reload.as_mut().unwrap().phase = ReloadPhase::InputAttempted;
            self.save(control, "reload_input_attempted")?;
            return Err(error("live.channel_progress"));
        }
        if self.closing_explicit_reload() {
            return if self.close_replaced_runtime(control, target)? {
                Ok(())
            } else {
                Err(pending())
            };
        }
        if r.phase == ReloadPhase::InputAttempted {
            return Err(pending());
        }
        if r.phase == ReloadPhase::Complete {
            return Ok(());
        }
        if r.reconcile_pending {
            if let Err(err) = self.finish_recovery(control) {
                if !err.all_causes(&|c| matches!(c, ChannelError::Contract(e) if e.0 == "live.channel_execution_unknown")) { return Err(err); }
            }
        } else {
            self.connect(control)?;
            if let Some(op) = self.state.operation.as_mut() {
                if op.stage == OperationStage::Prepared && op.prepared_nonce.is_empty() {
                    op.origin = Some(self.state.identity.clone());
                }
                if op.stage == OperationStage::ReleaseReady
                    && op.cleanup_method == CleanupMethod::ReloadRequired
                {
                    op.stage = OperationStage::Complete;
                    op.cleanup_method = CleanupMethod::RuntimeDestroyed;
                }
            }
        }
        self.state.reload.as_mut().unwrap().phase = ReloadPhase::Complete;
        self.save(control, "reload_verified")?;
        let r = self.state.reload.as_ref().unwrap();
        if r.reconcile_pending && !r.automatic && !self.state.closing {
            Ok(())
        } else {
            Err(error("live.channel_progress"))
        }
    }

    fn next_step(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
    ) -> ChannelResult<()> {
        self.check(control)?;
        if self
            .state
            .reload
            .as_ref()
            .is_some_and(|r| r.phase != ReloadPhase::Complete)
        {
            return self.step_reload(control, target);
        }
        if self
            .state
            .recovery
            .as_ref()
            .is_some_and(|r| r.phase != RecoveryPhase::Complete)
        {
            self.finish_recovery(control)?;
            return Err(error("live.channel_progress"));
        }
        let closing = self.state.closing
            || self
                .state
                .transaction
                .as_ref()
                .is_some_and(|t| t.envelope.action == "unbind");
        let quiescent = self
            .state
            .transaction
            .as_ref()
            .is_none_or(|t| t.envelope.action == "unbind")
            && self
                .state
                .operation
                .as_ref()
                .is_none_or(|o| terminal_operation(o.stage));
        if closing && quiescent {
            self.disconnect(control)?;
            self.state.closing = false;
            self.state.closed = true;
            return self.save(control, "connection_closed");
        }
        if !self.state.bound {
            self.connect(control)?;
            return if closing {
                Err(error("live.channel_progress"))
            } else {
                Ok(())
            };
        }
        let Some(op) = self.state.operation.as_ref() else {
            return Ok(());
        };
        if self.state.transaction.is_none() {
            let request = if op.stage == OperationStage::ReleaseReady
                && op.cleanup_method == CleanupMethod::ReloadRequired
            {
                Some(format!("cleanup-{}", op.id))
            } else if !closing
                && op.stage == OperationStage::Prepared
                && operation_needs_reload(self.state.identity.next_slot, self.state.identity.slots)
            {
                Some(format!(
                    "capacity-{}",
                    hex::encode(&Sha256::digest(op.id.as_bytes())[..16])
                ))
            } else {
                None
            };
            if let Some(request) = request {
                self.state.reload = Some(ReloadAttempt {
                    automatic: true,
                    slot_enablement: false,
                    slot_budget_source: None,
                    recovery_budget: op.recovery_budget.clone(),
                    reconcile_pending: false,
                    request,
                    from: self.state.identity.runtime.clone(),
                    phase: ReloadPhase::Intent,
                    input_step: 0,
                    patch_transitions: 0,
                });
                self.save(control, "automatic_reload_intent")?;
                return Err(error("live.channel_progress"));
            }
        }
        match self.run(control) {
            Ok(()) if closing => Err(error("live.channel_progress")),
            Err(err) if err.all_causes(&|c| matches!(c, ChannelError::Contract(e) if matches!(e.0, "live.channel_reload_required" | "live.channel_cleanup_reload_required"))) => Err(error("live.channel_progress")),
            other => other,
        }
    }

    /// Public continuation boundary. `wait-seconds` belongs to `control` only;
    /// the saved goal deadline is never refreshed. The scheduler supplies the
    /// fixed 250ms poll/capped discovery cooldown without inventing authority.
    pub fn continue_until<S: ContinuationScheduler>(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
        scheduler: &mut S,
    ) -> ChannelResult<()> {
        self.waiting.clear();
        let result = self.continue_inner(control, target, scheduler);
        let result = result.map_err(|err| {
            if err.any_cause(&|c| {
                matches!(c, ChannelError::Cancelled | ChannelError::DeadlineExceeded)
            }) {
                join_errors(pending(), err)
            } else {
                err
            }
        });
        let recorded = self.record_blocker(&result);
        join_result(result, recorded)
    }

    fn continue_inner<S: ContinuationScheduler>(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
        scheduler: &mut S,
    ) -> ChannelResult<()> {
        self.check(control)?;
        if self.state.runtime_end.is_some() && !self.state.closed {
            return Err(retirement_pending());
        }
        if self.state.closed {
            return Ok(());
        }
        let bounded = match self.budget_control(control) {
            Ok(b) => b,
            Err(err)
                if has_code(&err, "live.channel_budget_exhausted")
                    && self.close_bookkeeping_only() =>
            {
                self.check(control)?;
                self.waiting.clear();
                return self.next_step(control, target);
            }
            Err(err) => return Err(err),
        };
        self.invocation_control = Some(bounded.clone());
        self.invocation_stop.set(None);
        let result = self.continue_bounded(&bounded, target, scheduler);
        let result = join_result(result, self.capture_invocation_stop(&bounded));
        let timed_out = goal(&self.state).is_some()
            && bounded.deadline() < control.deadline()
            && self.invocation_stop.get() == Some(InvocationStop::DeadlineExceeded);
        let result = join_result(result, self.finish_budget(control, &bounded));
        self.invocation_control = None;
        if timed_out {
            join_result(result, Err(budget_exhausted(false)))
        } else {
            result
        }
    }

    fn continue_bounded<S: ContinuationScheduler>(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
        scheduler: &mut S,
    ) -> ChannelResult<()> {
        if self.closing_explicit_reload()
            && self.state.reload.as_ref().unwrap().phase == ReloadPhase::Intent
        {
            self.check(control)?;
            match self.close_replaced_runtime(control, target) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(err) => {
                    let err = pending_observation(&self.state, err);
                    let optional_timeout = control.check().is_ok()
                        && self.state.runtime_end.is_none()
                        && err.any_cause(&|c| matches!(c, ChannelError::DeadlineExceeded))
                        && err.optional_observation_timeout();
                    if !retryable_pending(&self.state, &err) && !optional_timeout {
                        return Err(err);
                    }
                }
            }
        }
        let mut next_discovery = 0;
        loop {
            self.check(control)?;
            self.waiting.clear();
            let err = match self.next_step(control, target) {
                Ok(()) => return Ok(()),
                Err(err) => pending_observation(&self.state, err),
            };
            if progress(&err) {
                continue;
            }
            if let Some(blocker) = find_blocker(&err) {
                self.waiting = if has_code(&err, "live.channel_shared_publication_pending") {
                    "shared_publication"
                } else {
                    &blocker.kind
                }
                .into();
                return Err(err);
            }
            if !retryable_pending(&self.state, &err) {
                return Err(err);
            }
            let known = known_input_blocker(&self.waiting);
            if !self.closing_explicit_reload()
                && (!known || self.state.closing && self.waiting != "input_observation_stale")
                && scheduler.monotonic_millis() >= next_discovery
            {
                self.check(control)?;
                let started = scheduler.monotonic_millis();
                let candidate = self
                    .backend
                    .runtime_candidate(control, &self.state.identity);
                let ended = scheduler.monotonic_millis();
                next_discovery =
                    ended.saturating_add(discovery_cooldown_millis(ended.saturating_sub(started)));
                self.check(control)?;
                match candidate {
                    Err(err) => {
                        let err = pending_observation(&self.state, err);
                        if !retryable_pending(&self.state, &err) {
                            return Err(err);
                        }
                    }
                    Ok(Some(candidate)) => match self.recover_runtime(control, candidate) {
                        Ok(()) => continue,
                        Err(err) => {
                            let err = pending_observation(&self.state, err);
                            if progress(&err) {
                                continue;
                            }
                            if has_code(&err, "live.channel_execution_unknown") && err.all_causes(&|c| matches!(c, ChannelError::Contract(e) if e.0 == "live.channel_execution_unknown"))
                                && self.state.reload.as_ref().is_some_and(|r| r.phase == ReloadPhase::Binding) { continue; }
                            if !retryable_pending(&self.state, &err) {
                                return Err(err);
                            }
                        }
                    },
                    Ok(None) => {}
                }
            }
            if matches!(
                self.waiting.as_str(),
                "prepared_operation_requires_reload" | "closing_exchange_unconfirmed"
            ) {
                return Err(pending());
            }
            scheduler.wait(control, Duration::from_millis(250))?;
        }
    }
}

#[cfg(test)]
#[path = "reload_tests.rs"]
mod tests;
