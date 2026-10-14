//! Destruction-only runtime retirement. A candidate identity, missing receipt,
//! queued reload or timeout cannot establish this proof. Native implementations
//! must resolve two guarded fresh INPUT observations in the selected process.
//! Oracle: channel/runtime_retirement{,_windows}.go at fixed Go 3.2.0.
use super::Identity;
use super::driver::{Clock, Driver, TokenSource, error};
use super::journal::StateJournal;
use super::observations::{Backend, ChannelError, ChannelResult};
use super::recovery::{has_code, observe_budget};
use super::reload::{ContinuationScheduler, pending_observation};
use super::state::{
    Blocker, CleanupMethod, OperationStage, ReloadPhase, RuntimeReplacementProof, SlotEnvelope,
    State,
};
use lycheedev_core::control::OperationControl;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

/// Independent selected-process identity supplied by the owning coordinator.
/// Never derive this target from an observed or persisted replacement proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetirementTarget {
    pub process_id: u32,
    pub process_started_at: u64,
}

/// No default successful implementation: supported backends must actually
/// verify current-publication/process freshness and retire only exact claims.
/// Retirement must be idempotent because a crash can follow its external effect.
pub trait RuntimeRetirementBackend: Backend {
    fn observe_runtime_replacement(
        &mut self,
        control: &OperationControl,
        old: &Identity,
    ) -> ChannelResult<Option<RuntimeReplacementProof>>;
    fn retire_replaced_runtime(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        proof: &RuntimeReplacementProof,
    ) -> ChannelResult<()>;
}

pub(super) fn retirement_blocker() -> Blocker {
    Blocker {
        kind: "runtime_retirement_pending".into(),
        condition: "precise_resource_retirement".into(),
        ..Blocker::default()
    }
}
pub fn retirement_pending() -> ChannelError {
    ChannelError::Blocked(retirement_blocker())
}

/// Structural matching only. Freshness and the selected live process are the
/// backend's responsibility at proof construction, never inferred from a hash.
pub fn retirement_proof_matches_state(
    state: &State,
    proof: &RuntimeReplacementProof,
) -> ChannelResult<()> {
    let invalid = || error("live.channel_runtime_retirement_invalid");
    proof.validate(&state.identity).map_err(|_| invalid())?;
    if state
        .recovery
        .as_ref()
        .is_some_and(|r| r.from.runtime == proof.current.runtime)
    {
        return Err(invalid());
    }
    for tx in [
        state.transaction.as_ref(),
        state.recovery.as_ref().and_then(|r| r.transaction.as_ref()),
    ]
    .into_iter()
    .flatten()
    {
        if tx.envelope.runtime == proof.current.runtime || tx.envelope.build != proof.current.build
        {
            return Err(invalid());
        }
    }
    Ok(())
}
fn proof_target(proof: &RuntimeReplacementProof, target: RetirementTarget) -> ChannelResult<()> {
    if target.process_id == 0
        || target.process_started_at == 0
        || proof.process_id != target.process_id
        || proof.process_started_at != target.process_started_at
    {
        Err(error("live.channel_runtime_replacement_target_changed"))
    } else {
        Ok(())
    }
}

/// Private transition used only after durable verified retirement evidence and
/// successful exact-reservation cleanup. Candidate bytes remain candidate bytes.
fn close_ended_state(state: &mut State) {
    if let Some(op) = state.operation.as_mut().filter(|o| {
        !matches!(
            o.stage,
            OperationStage::Complete | OperationStage::Cancelled
        )
    }) {
        if matches!(
            op.stage,
            OperationStage::ResultVerified | OperationStage::ReleaseReady
        ) {
            op.stage = OperationStage::Complete;
            op.cleanup_method = CleanupMethod::RuntimeDestroyed;
        } else {
            op.stage = OperationStage::ExecutionUnknown;
            op.cleanup_method = CleanupMethod::None;
        }
    }
    state.bound = false;
    state.closed = true;
    state.closing = false;
    state.transaction = None;
    state.reload = None;
    state.recovery = None;
}

impl<B: RuntimeRetirementBackend, J: StateJournal, C: Clock, T: TokenSource> Driver<B, J, C, T> {
    /// Input-free cleanup under the exact connection lease. A saved proof is an
    /// irreversible historical fact and is reused even after another reload.
    pub fn close_replaced_runtime(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
    ) -> ChannelResult<bool> {
        self.check(control)?;
        if self.state.closed {
            return Ok(true);
        }
        if self.state.runtime_end.is_none() {
            let Some(proof) = self
                .backend
                .observe_runtime_replacement(control, &self.state.identity)?
            else {
                return Ok(false);
            };
            self.check(control)?;
            proof_target(&proof, target)?;
            retirement_proof_matches_state(&self.state, &proof)?;
            let previous = self.state.clone();
            self.state.runtime_end = Some(proof);
            if let Err(err) = self.save(control, "runtime_replacement_observed") {
                self.state = previous;
                return Err(err);
            }
        }
        let proof = self.state.runtime_end.as_ref().unwrap().clone();
        proof_target(&proof, target)?;
        retirement_proof_matches_state(&self.state, &proof)?;
        for tx in [
            self.state.transaction.as_ref(),
            self.state
                .recovery
                .as_ref()
                .and_then(|r| r.transaction.as_ref()),
        ]
        .into_iter()
        .flatten()
        {
            self.backend
                .retire_replaced_runtime(control, &tx.envelope, &proof)?;
            self.check(control)?;
        }
        let previous = self.state.clone();
        let completed_reload = if self.closing_explicit_reload() {
            self.state.reload.clone().map(|mut r| {
                r.phase = ReloadPhase::Complete;
                r
            })
        } else {
            None
        };
        close_ended_state(&mut self.state);
        if let Some(r) = completed_reload {
            self.state.reload = Some(r);
        }
        if let Err(err) = self.save(control, "runtime_replacement_closed") {
            self.state = previous;
            return Err(err);
        }
        Ok(true)
    }

    /// Fixed Go's outer input-free retirement exit. Exhausted business budgets
    /// permit only observing destruction and retiring original claims; they do
    /// not authorize a new binding, reload or business attempt.
    pub fn continue_or_retire<S: ContinuationScheduler>(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
        closing: bool,
        scheduler: &mut S,
    ) -> ChannelResult<()> {
        self.check(control)?;
        if self.state.runtime_end.is_none() && self.close_bookkeeping_only() {
            return self.continue_until(control, target, scheduler);
        }
        if self.closing_explicit_reload() && self.state.runtime_end.is_none() {
            let available = self
                .state
                .reload
                .as_ref()
                .and_then(|r| r.recovery_budget.clone())
                .is_none_or(|mut b| observe_budget(&mut b, self.clock.unix_millis()).1.is_ok());
            if available {
                return self.continue_until(control, target, scheduler);
            }
        }
        if closing
            || self.state.runtime_end.is_some()
            || self.state.closing
            || self.continuation().kind == "budget_exhausted"
        {
            let probe = control.child(
                Instant::now() + Duration::from_secs(30),
                Arc::new(AtomicBool::new(false)),
            );
            match self.close_replaced_runtime(&probe, target) {
                Ok(true) => return Ok(()),
                Ok(false) => {}
                Err(err) => {
                    let optional_timeout = control.check().is_ok()
                        && self.state.runtime_end.is_none()
                        && err.any_cause(&|c| matches!(c, ChannelError::DeadlineExceeded))
                        && err.optional_observation_timeout();
                    if !optional_timeout {
                        if self.state.runtime_end.is_none()
                            && control.check().is_ok()
                            && has_code(&err, "memory.mailbox_unavailable: path_changed")
                        {
                            return Err(pending_observation(&self.state, err));
                        }
                        return Err(err);
                    }
                }
            }
        }
        self.continue_until(control, target, scheduler)
    }
}

#[cfg(test)]
#[path = "runtime_retirement_tests.rs"]
mod tests;
