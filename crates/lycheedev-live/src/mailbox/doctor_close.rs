//! Damaged slots cannot carry unbind. Keep the ordinary close intent, then use
//! the independent reload coordinator and positive runtime retirement proof.
use super::*;
use crate::mailbox::{
    journal::StateJournal,
    recovery::has_code,
    reload::pending_observation,
    state::{RecoveryPhase, ReloadPhase},
};
use std::time::{Duration, Instant};

impl<B: RuntimeRetirementBackend, J: StateJournal, C: Clock, T: TokenSource> Driver<B, J, C, T> {
    pub(super) fn close_slot_repair(
        &mut self,
        control: &OperationControl,
        target: RetirementTarget,
        scheduler: &mut impl ContinuationScheduler,
    ) -> ChannelResult<()> {
        if self.state.runtime_end.is_some() || self.close_bookkeeping_only() {
            return self.continue_or_retire(control, target, true, scheduler);
        }
        let probe = control
            .ordered_child(Instant::now() + Duration::from_secs(30))
            .map_err(ChannelError::from)?;
        match self.close_replaced_runtime(&probe, target) {
            Ok(true) => return Ok(()),
            Ok(false) => (),
            Err(err) => {
                let optional_timeout = control.check().is_ok()
                    && self.state.runtime_end.is_none()
                    && err.any_cause(&|cause| matches!(cause, ChannelError::DeadlineExceeded))
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
        if let Some(reload) = self
            .state
            .reload
            .as_ref()
            .filter(|r| r.phase != ReloadPhase::Complete)
        {
            if reload.automatic {
                return Err(crate::mailbox::driver::pending());
            }
            return self.continue_until(control, target, scheduler);
        }
        if self
            .state
            .recovery
            .as_ref()
            .is_some_and(|r| r.phase != RecoveryPhase::Complete)
        {
            return Err(crate::mailbox::driver::pending());
        }
        self.request_reload(control, &format!("doctor-slots-{}", self.state.id))?;
        self.continue_until(control, target, scheduler)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::{
        recovery::test_support::{Schedule, driver, proof, restart, target, transaction},
        state::{CleanupMethod, InputKind, OperationPolicy, OperationStage, TransactionPhase},
    };

    #[test]
    fn slot_repair_observed_destruction_preserves_verified_report_and_never_inputs() {
        for (stage, expected, cleanup) in [
            (
                OperationStage::ReleaseReady,
                OperationStage::Complete,
                CleanupMethod::RuntimeDestroyed,
            ),
            (
                OperationStage::ConfirmReady,
                OperationStage::ExecutionUnknown,
                CleanupMethod::None,
            ),
        ] {
            let mut d = driver(stage, OperationPolicy::Opaque);
            transaction(
                &mut d,
                if stage == OperationStage::ReleaseReady {
                    "release"
                } else {
                    "confirm"
                },
                TransactionPhase::InputAttempted,
            );
            let bytes = d.state.operation.as_ref().unwrap().result_bytes.clone();
            d.request_close(&OperationControl::default()).unwrap();
            d.backend.proof = Some(proof());
            d.close_slot_repair(
                &OperationControl::default(),
                target(),
                &mut Schedule::default(),
            )
            .unwrap();
            assert!(d.state.closed);
            let operation = d.state.operation.as_ref().unwrap();
            assert_eq!(operation.stage, expected);
            assert_eq!(operation.cleanup_method, cleanup);
            assert_eq!(operation.result_bytes, bytes);
            assert!(d.backend.input_kinds.is_empty());
            assert!(d.backend.actions.is_empty());
        }
    }

    #[test]
    fn slot_repair_reload_recovery_keeps_original_request_deadline_and_does_not_replay() {
        use crate::mailbox::InputDisposition;
        for lost in ["submitted", "uncertain", "missing"] {
            let mut d = driver(OperationStage::Running, OperationPolicy::Opaque);
            transaction(&mut d, "commit", TransactionPhase::InputAttempted);
            d.request_close(&OperationControl::default()).unwrap();
            assert!(
                d.close_slot_repair(
                    &OperationControl::default(),
                    target(),
                    &mut Schedule::default()
                )
                .is_err()
            );
            assert_eq!(d.backend.input_kinds, [InputKind::Reload]);
            assert!(d.backend.actions.is_empty());
            let input = d.state.input.as_mut().unwrap();
            match lost {
                "uncertain" => {
                    input.outcome.as_mut().unwrap().disposition = InputDisposition::Uncertain
                }
                "missing" => input.outcome = None,
                _ => (),
            }
            d.save(&OperationControl::default(), "fixture_reload_outcome_loss")
                .unwrap();
            let reload = d.state.reload.clone().unwrap();
            assert_eq!(reload.request, format!("doctor-slots-{}", d.state.id));
            let close_budget = d.state.close_budget.clone();
            let operation = d.state.operation.clone();
            let mut d = restart(d);
            assert!(
                d.close_slot_repair(
                    &OperationControl::default(),
                    target(),
                    &mut Schedule::default()
                )
                .is_err()
            );
            assert_eq!(d.backend.input_kinds, [InputKind::Reload], "{lost}");
            assert_eq!(d.state.reload.as_ref().unwrap().request, reload.request);
            assert_eq!(
                d.state.reload.as_ref().unwrap().recovery_budget,
                reload.recovery_budget
            );
            assert_eq!(d.state.close_budget, close_budget);
            assert_eq!(d.state.operation, operation);
            assert!(!d.state.closed);
            d.backend.proof = Some(proof());
            d.close_slot_repair(
                &OperationControl::default(),
                target(),
                &mut Schedule::default(),
            )
            .unwrap();
            assert!(d.state.closed);
            assert_eq!(d.backend.input_kinds, [InputKind::Reload], "{lost}");
            assert!(d.backend.actions.is_empty());
            assert_eq!(d.state.reload.as_ref().unwrap().request, reload.request);
            assert_eq!(
                d.state.reload.as_ref().unwrap().recovery_budget,
                reload.recovery_budget
            );
            assert_eq!(
                d.state.operation.as_ref().unwrap().stage,
                OperationStage::ExecutionUnknown
            );
        }
    }

    #[test]
    fn slot_repair_does_not_replace_existing_automatic_recovery_or_expired_reload() {
        use crate::mailbox::state::RuntimeRecovery;
        for kind in ["automatic", "recovery", "expired_explicit"] {
            let mut d = driver(OperationStage::Running, OperationPolicy::Opaque);
            d.request_close(&OperationControl::default()).unwrap();
            match kind {
                "automatic" => {
                    d.request_reload(&OperationControl::default(), "original-auto")
                        .unwrap();
                    d.state.reload.as_mut().unwrap().automatic = true;
                }
                "recovery" => {
                    d.state.recovery = Some(RuntimeRecovery {
                        from: d.state.identity.clone(),
                        transaction: None,
                        phase: RecoveryPhase::Binding,
                    });
                }
                _ => {
                    d.request_reload(&OperationControl::default(), "original-explicit")
                        .unwrap();
                    let reload = d.state.reload.as_mut().unwrap();
                    reload.recovery_budget.as_mut().unwrap().exhausted = true;
                }
            }
            let reload = d.state.reload.clone();
            let recovery = d.state.recovery.clone();
            let result = d.close_slot_repair(
                &OperationControl::default(),
                target(),
                &mut Schedule::default(),
            );
            assert!(result.is_err(), "{kind}");
            assert_eq!(d.state.reload, reload, "{kind}");
            assert_eq!(d.state.recovery, recovery, "{kind}");
            assert!(d.backend.input_kinds.is_empty(), "{kind}");
            assert!(d.backend.actions.is_empty(), "{kind}");
        }
    }
}
