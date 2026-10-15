//! Fixed-Go transaction and input journal ordering. Physical input and slot-file
//! durability remain Backend/StateJournal responsibilities; no unknown outcome
//! is inferred unsent. Oracle: channel/{driver,input,input_attempt,reload}.go at
//! 3057a9666ab078270b63c7f06de748069b0d1f4b.
use super::driver::{Clock, Driver, TokenSource, error, has_pending, pending, rejection_reason};
use super::journal::StateJournal;
use super::observations::{
    Backend, ChannelError, ChannelResult, InputAction, Observation, ObservationKind,
    ObservationQuery, check_control,
};
use super::slot_allocation::{may_reallocate, slot_payload};
use super::state::{
    InputAttempt, InputKind, OperationStage, RecoveryPhase, ReloadAttempt, ReloadPhase,
    SlotEnvelope, State, Transaction, TransactionPhase,
};
use super::{InputDiagnostic, InputDisposition, InputOutcome, Receipt, SCHEMA};
use lycheedev_core::control::OperationControl;
use std::{
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

pub(crate) fn join_errors(first: ChannelError, second: ChannelError) -> ChannelError {
    ChannelError::Joined {
        causes: vec![first, second],
    }
}
fn slot_destination(err: &ChannelError) -> Option<i32> {
    match err {
        ChannelError::SlotAvailable { index, .. } => Some(*index),
        ChannelError::Joined { causes } => causes.iter().find_map(slot_destination),
        _ => None,
    }
}
fn stale_observation(err: &ChannelError) -> bool {
    match err {
        ChannelError::Pending { causes } => {
            causes.contains(&"live.channel_input_observation_stale")
        }
        ChannelError::Joined { causes } => causes.iter().any(stale_observation),
        _ => false,
    }
}

impl<B: Backend, J: StateJournal, C: Clock, T: TokenSource> Driver<B, J, C, T> {
    pub(crate) fn begin(
        &mut self,
        control: &OperationControl,
        action: &str,
        ticket: String,
        configure: impl FnOnce(&mut SlotEnvelope),
    ) -> ChannelResult<()> {
        self.check(control)?;
        self.require_live_runtime()?;
        if self.state.transaction.is_some() {
            return Err(error("live.channel_transaction_pending"));
        }
        let nonce = self.next_token()?;
        let i = &self.state.identity;
        let mut envelope = SlotEnvelope {
            schema: SCHEMA.into(),
            index: i.next_slot,
            start_slot: i.next_slot,
            runtime: i.runtime.clone(),
            owner: self.state.owner.clone(),
            fence: 1,
            nonce,
            ticket,
            action: action.into(),
            guid: i.guid.clone(),
            build: i.build.clone(),
            ..SlotEnvelope::default()
        };
        configure(&mut envelope);
        // Same static bridge.SlotPayload gate as Go, before any journal write.
        slot_payload(&envelope)?;
        self.state.transaction = Some(Transaction {
            release_wait_started_at_ms: 0,
            envelope,
            phase: TransactionPhase::Intent,
            receipt: None,
            escape_attempts: 0,
            input_observation: None,
        });
        self.save(control, "slot_intent")
    }

    pub(crate) fn advance(&mut self, control: &OperationControl) -> ChannelResult<Receipt> {
        self.check(control)?;
        self.require_live_runtime()?;
        let tx = self
            .state
            .transaction
            .as_ref()
            .ok_or_else(|| error("live.channel_transaction_missing"))?;
        if let Some(receipt) = &tx.receipt {
            qualify_receipt(receipt, &tx.envelope, &self.state.identity)?;
            return receipt_result(receipt.clone());
        }
        let recovery_binding = self
            .state
            .recovery
            .as_ref()
            .is_some_and(|r| r.phase != RecoveryPhase::Complete)
            || self
                .state
                .reload
                .as_ref()
                .is_some_and(|r| r.phase == ReloadPhase::Binding);
        if self.state.closing
            && (matches!(tx.envelope.action.as_str(), "prepare" | "commit")
                || (tx.envelope.action == "bind" && !recovery_binding))
        {
            // Even intent can already exist on disk after a crash. Closing
            // observes the exact nonce, without publishing or waking business.
            self.waiting = "closing_exchange_unconfirmed".into();
            return self.receive(control);
        }
        while self.state.transaction.as_ref().unwrap().phase == TransactionPhase::Intent {
            self.check(control)?;
            let envelope = &self.state.transaction.as_ref().unwrap().envelope;
            match self.backend.publish(control, envelope) {
                Ok(()) => {
                    self.state.transaction.as_mut().unwrap().phase = TransactionPhase::Published;
                    self.save(control, "slot_published")?;
                }
                Err(err) => {
                    if let Some(index) = slot_destination(&err)
                        .filter(|n| may_reallocate(envelope, self.state.input.as_ref(), *n))
                    {
                        self.state.transaction.as_mut().unwrap().envelope.index = index;
                        self.save(control, "slot_reallocated")?;
                    } else {
                        return Err(err);
                    }
                }
            }
        }
        if self.state.transaction.as_ref().unwrap().phase == TransactionPhase::Published {
            let envelope = self.state.transaction.as_ref().unwrap().envelope.clone();
            self.submit_input(
                control,
                &envelope.nonce,
                &envelope.runtime,
                InputKind::Invoke,
                &envelope,
            )?;
            self.state.transaction.as_mut().unwrap().phase = TransactionPhase::InputAttempted;
            self.save(control, "input_attempted")?;
        }
        self.receive(control)
    }

    fn receive(&mut self, control: &OperationControl) -> ChannelResult<Receipt> {
        self.check(control)?;
        let envelope = self
            .state
            .transaction
            .as_ref()
            .ok_or_else(|| error("live.channel_transaction_missing"))?
            .envelope
            .clone();
        let query = ObservationQuery {
            kind: ObservationKind::Receipt,
            envelope: envelope.clone(),
            identity: self.state.identity.clone(),
        };
        let dispatch = self.backend.observe(control, &query);
        if let Err(cancel) = check_control(control) {
            return Err(match dispatch.error {
                Some(err) => join_errors(err, cancel),
                None => cancel,
            });
        }
        if let Some(err) = &dispatch.error {
            if rejection_reason(err).is_none() {
                if has_pending(err) {
                    let diagnostic = match &dispatch.observation {
                        Some(Observation::Pending { diagnostic }) => diagnostic.as_ref(),
                        _ => None,
                    };
                    if let Err(save_err) =
                        self.record_input_diagnostic(control, &envelope, diagnostic)
                    {
                        return Err(join_errors(err.clone(), save_err));
                    }
                }
                return Err(err.clone());
            }
        }
        let observation = dispatch
            .observation
            .ok_or_else(|| error("live.channel_observation_missing"))?;
        let receipt = match observation {
            Observation::Receipt(receipt) | Observation::Rejected { receipt } => receipt,
            Observation::Pending { diagnostic } => {
                if let Err(err) =
                    self.record_input_diagnostic(control, &envelope, diagnostic.as_ref())
                {
                    return Err(join_errors(pending(), err));
                }
                return Err(pending());
            }
            Observation::Reported { .. } => {
                return Err(error("live.channel_receipt_record_invalid"));
            }
        };
        qualify_receipt(&receipt, &envelope, &self.state.identity)?;
        let tx = self.state.transaction.as_mut().unwrap();
        tx.receipt = Some(receipt.clone());
        tx.phase = TransactionPhase::Received;
        // The exact receipt is durable before its shared reservation can retire.
        self.save(control, "receipt_verified")?;
        if let Some(err) = dispatch.error {
            Err(err)
        } else {
            receipt_result(receipt)
        }
    }

    pub(crate) fn finish_transaction(
        &mut self,
        control: &OperationControl,
        transition: impl FnOnce(&mut State),
    ) -> ChannelResult<()> {
        self.check(control)?;
        let tx = self
            .state
            .transaction
            .as_ref()
            .ok_or_else(|| error("live.channel_receipt_required"))?;
        let receipt = tx
            .receipt
            .as_ref()
            .ok_or_else(|| error("live.channel_receipt_required"))?;
        qualify_receipt(receipt, &tx.envelope, &self.state.identity)?;
        let mut identity = receipt.identity.clone();
        // If exact reservation retirement fails, neither logical state nor
        // operation phase advances. Consumed must be idempotent across restart.
        self.backend.consumed(control, &tx.envelope)?;
        transition(&mut self.state);
        identity.schema = SCHEMA.into();
        self.state.identity = identity;
        self.state.transaction = None;
        self.save(control, "slot_consumed")
    }

    pub(crate) fn submit_input(
        &mut self,
        control: &OperationControl,
        exchange: &str,
        runtime: &str,
        kind: InputKind,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()> {
        self.check(control)?;
        self.require_live_runtime()?;
        let mut after = 0;
        let mut recover_time = false;
        if let Some(previous) = &self.state.input {
            if previous.runtime == runtime {
                if previous.exchange == exchange
                    && previous.kind == kind
                    && previous
                        .outcome
                        .as_ref()
                        .is_none_or(|o| o.disposition != InputDisposition::NotSent)
                {
                    // Missing outcome, uncertain and submitted are all unknown
                    // delivery. Observe original receipt; never prepare again.
                    return Ok(());
                }
                after = previous.after_millis.max(previous.recovery_after_millis);
                if let Some(outcome) = &previous.outcome {
                    after = after.max(outcome.at_millis);
                }
                recover_time = previous.recovery_after_millis == 0
                    && previous.outcome.as_ref().is_none_or(|o| {
                        o.disposition != InputDisposition::NotSent && o.at_millis == 0
                    });
            }
        }
        let observation = self.observe_input(control, envelope, after)?;
        if observation.input_blocked == Some(true) {
            self.waiting = observation.reason;
            return Err(pending());
        }
        if recover_time {
            if observation.observed_at_millis <= 0 {
                self.waiting = "input_observation_unavailable".into();
                return Err(pending());
            }
            // Persist a current post-receipt upper bound once. A saved pre-input
            // sample and another exchange's watermark cannot bound this input.
            self.state.input.as_mut().unwrap().recovery_after_millis =
                observation.observed_at_millis;
            after = after.max(observation.observed_at_millis);
            self.save(control, "input_time_recovered")?;
            if i128::from(observation.sample_millis) < i128::from(after) + 100 {
                self.waiting = "input_observation_stale".into();
                return Err(ChannelError::Pending {
                    causes: vec!["live.channel_input_observation_stale"],
                });
            }
        }
        self.waiting.clear();
        // Unknown/submitted same-exchange input returned above, before this
        // flag. A reload never gates itself on the slot-activation request.
        if kind == InputKind::Invoke && observation.slot_reload_required {
            return self.request_slot_reload(control);
        }
        let action = InputAction {
            kind,
            envelope: envelope.clone(),
            observation: Some(observation),
            capability: self.state.identity.input_state.clone(),
            after_millis: after,
        };
        let (outcome, failure) = self.perform(control, exchange, runtime, action)?;
        if outcome.disposition == InputDisposition::NotSent {
            self.waiting = outcome.reason;
            if outcome.retryable {
                return Err(match failure {
                    Some(err) => join_errors(pending(), err),
                    None => pending(),
                });
            }
            return Err(failure.unwrap_or_else(|| error("live.channel_input_not_sent")));
        }
        if let Some(err) = failure {
            return Err(err);
        }
        Ok(())
    }

    pub(super) fn observe_input(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        after: i64,
    ) -> ChannelResult<super::InputObservation> {
        let observation = match self.backend.observe_input(
            control, envelope, after, &self.state.identity.input_state,
        ) {
            Ok(s) => s,
            Err(err) => {
                self.waiting = if stale_observation(&err) {
                    "input_observation_stale"
                } else {
                    "input_observation_unavailable"
                }.into();
                return Err(err);
            }
        };
        check_control(control)?;
        if !observation.valid_readiness() || !observation.valid_bindings() {
            self.waiting = "input_observation_unavailable".into();
            return Err(pending());
        }
        Ok(observation)
    }

    fn perform(
        &mut self,
        control: &OperationControl,
        exchange: &str,
        runtime: &str,
        action: InputAction,
    ) -> ChannelResult<(InputOutcome, Option<ChannelError>)> {
        self.check(control)?;
        if self.state.input.as_ref().is_some_and(|a| {
            a.exchange == exchange
                && a.runtime == runtime
                && a.kind == action.kind
                && a.outcome.as_ref().is_some_and(|o| {
                    o.disposition == InputDisposition::NotSent && o.messages_queued == 0
                })
        }) {
            self.checkpoint_log(control, true)?;
        }
        let id = self.next_token()?;
        self.state.input = Some(InputAttempt {
            id,
            exchange: exchange.into(),
            runtime: runtime.into(),
            kind: action.kind,
            after_millis: action.after_millis,
            recovery_after_millis: 0,
            escape_attempts: 0,
            observation: action.observation.clone(),
            outcome: None,
            after_diagnostic: None,
        });
        self.save(control, "input_intent")?;
        self.check(control)?;
        let dispatch = self.backend.input(control, &action);
        // Observe the original stop before the independent fact flush can
        // cross its deadline. Cancellation remains cancellation afterward.
        let ordering_error = self.capture_invocation_stop(control).err();
        let (outcome, failure) = if dispatch.outcome.validate().is_err() {
            (
                InputOutcome {
                    disposition: InputDisposition::Uncertain,
                    messages_queued: 0,
                    at_millis: 0,
                    reason: "invalid_transport_outcome".into(),
                    retryable: false,
                },
                Some(error("live.channel_input_outcome_invalid")),
            )
        } else {
            (dispatch.outcome, dispatch.error)
        };
        let failure = match (failure, ordering_error) {
            (Some(a), Some(b)) => Some(join_errors(a, b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        };
        self.state.input.as_mut().unwrap().outcome = Some(outcome.clone());
        // Cancellation stops new input, not the bounded persistence of facts.
        // This fresh 3s control is deliberately independent of caller cancellation.
        let flush = OperationControl::new(
            Instant::now() + Duration::from_secs(3),
            Arc::new(AtomicBool::new(false)),
        );
        self.save(&flush, "input_observed")?;
        Ok((outcome, failure))
    }

    fn record_input_diagnostic(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        diagnostic: Option<&InputDiagnostic>,
    ) -> ChannelResult<()> {
        let Some(diagnostic) = diagnostic else {
            return Ok(());
        };
        let Some(attempt) = &self.state.input else {
            return Ok(());
        };
        let Some(observation) = &attempt.observation else {
            return Ok(());
        };
        if !diagnostic.valid()
            || attempt.kind != InputKind::Invoke
            || attempt.exchange != envelope.nonce
            || attempt.runtime != diagnostic.runtime
            || attempt.runtime != envelope.runtime
            || diagnostic.slot < envelope.slot_start()
            || diagnostic.slot > envelope.index
            || diagnostic.sample_millis < observation.sample_millis
        {
            return Ok(());
        }
        let before = observation
            .input_attempt
            .as_ref()
            .map_or(0, |d| d.attempt_seq);
        let after = attempt
            .after_diagnostic
            .as_ref()
            .map_or(0, |d| d.attempt_seq);
        if diagnostic.attempt_seq <= before || diagnostic.attempt_seq <= after {
            return Ok(());
        }
        self.state.input.as_mut().unwrap().after_diagnostic = Some(diagnostic.clone());
        self.save(control, "input_attempt_diagnostic")
    }

    pub(super) fn recover_missing_release(
        &mut self,
        control: &OperationControl,
        original: ChannelError,
    ) -> ChannelResult<()> {
        match missing_release_decision(&self.state, self.clock.unix_millis())? {
            ReleaseRecoveryDecision::Pending => Err(original),
            ReleaseRecoveryDecision::StartWaiting { at_ms } => {
                self.state
                    .transaction
                    .as_mut()
                    .unwrap()
                    .release_wait_started_at_ms = at_ms;
                match self.save(control, "release_receipt_waiting") {
                    Ok(()) => Err(original),
                    Err(err) => Err(join_errors(original, err)),
                }
            }
            ReleaseRecoveryDecision::RequestReload => {
                let op = self.state.operation.as_ref().unwrap();
                self.state.reload = Some(ReloadAttempt {
                    request: format!("cleanup-{}", op.id),
                    from: self.state.identity.runtime.clone(),
                    phase: ReloadPhase::Intent,
                    recovery_budget: op.recovery_budget.clone(),
                    automatic: true,
                    slot_enablement: false, slot_budget_source: None,
                    reconcile_pending: true,
                    input_step: 0,
                    patch_transitions: 0,
                });
                self.save(control, "release_recovery_reload_intent")?;
                // The continuation layer owns actual reload and positive runtime
                // retirement. A durable report remains usable while cleanup waits.
                Err(error("live.channel_progress"))
            }
        }
    }
}

fn receipt_result(receipt: Receipt) -> ChannelResult<Receipt> {
    if receipt.state == "rejected" {
        Err(ChannelError::Rejected {
            reason: receipt.reason,
        })
    } else {
        Ok(receipt)
    }
}

fn qualify_receipt(
    receipt: &Receipt,
    envelope: &SlotEnvelope,
    identity: &super::Identity,
) -> ChannelResult<()> {
    receipt.validate()?;
    let i = &receipt.identity;
    if i.schema != envelope.schema
        || receipt.nonce != envelope.nonce
        || receipt.ticket != envelope.ticket
        || receipt.action != envelope.action
        || i.runtime != envelope.runtime
        || i.guid != envelope.guid
        || i.build != envelope.build
        || i.next_slot <= envelope.index
    {
        return Err(error("live.channel_receipt_mismatch"));
    }
    if i.owner != envelope.owner || i.fence != envelope.fence {
        return Err(error("live.channel_owner_mismatch"));
    }
    if i.product != identity.product || i.release != identity.release {
        return Err(error("live.channel_receipt_target_mismatch"));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseRecoveryDecision {
    Pending,
    StartWaiting { at_ms: i64 },
    RequestReload,
}

/// Pure B/C boundary: original exchange and operation deadline survive. No
/// reload is sent and no cleanup success is inferred by this decision alone.
pub fn missing_release_decision(
    state: &State,
    now_ms: i64,
) -> ChannelResult<ReleaseRecoveryDecision> {
    let Some(tx) = &state.transaction else {
        return Ok(ReleaseRecoveryDecision::Pending);
    };
    if tx.phase != TransactionPhase::InputAttempted
        || tx.envelope.action != "release"
        || state
            .operation
            .as_ref()
            .is_none_or(|o| o.stage != OperationStage::ReleaseReady)
    {
        return Ok(ReleaseRecoveryDecision::Pending);
    }
    if state
        .reload
        .as_ref()
        .is_some_and(|r| r.phase != ReloadPhase::Complete)
    {
        return Ok(ReleaseRecoveryDecision::Pending);
    }
    if tx.release_wait_started_at_ms == 0 {
        if now_ms <= 0 {
            return Err(error("live.channel_budget_invalid"));
        }
        return Ok(ReleaseRecoveryDecision::StartWaiting { at_ms: now_ms });
    }
    if i128::from(now_ms) - i128::from(tx.release_wait_started_at_ms) < 5000 {
        Ok(ReleaseRecoveryDecision::Pending)
    } else {
        Ok(ReleaseRecoveryDecision::RequestReload)
    }
}
