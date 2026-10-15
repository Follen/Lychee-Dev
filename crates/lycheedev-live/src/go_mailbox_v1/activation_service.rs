//! Included as connection::activation_service; extends the same service/book.
#![forbid(unsafe_code)]
use super::*;
use crate::go_mailbox_v1::{
    activation::{ActivationPhase, manual_reload_required},
    admission::{
        activation_store::{ActivationLease, ActivationRecord},
        inspect_window_owner,
    },
    driver::{budget, pending},
    recovery::observe_budget,
    state::{CHANNEL_SCHEMA, DEFAULT_RECOVERY_MILLIS, ReloadAttempt, ReloadPhase},
};
use std::time::{Duration, Instant};

pub trait ActivationBackend: ConnectionBackend {
    /// Only capability evidence. Implementations must never emit keys here.
    /// Old-name two-key INPUT may be stale; the ordinary Driver later requires
    /// a new fresh sample and all guards before any reload input.
    fn activation_capability(
        &mut self,
        identity: &Identity,
        control: &OperationControl,
    ) -> ChannelResult<bool>;
}
pub trait ActivationEnvironment: ConnectionEnvironment {
    fn guard_activation(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<()>;
}
fn missing(e: &ChannelError) -> bool {
    e.all_causes(
        &|e| matches!(e, ChannelError::Contract(c) if c.0 == "live.channel_journal_missing"),
    )
}
fn incompatible_leaf(e: &ChannelError) -> bool {
    matches!(e, ChannelError::Contract(c) if matches!(c.0,
        "live.channel_mailbox_incompatible" | "live.channel_legacy_identity_requires_activation"))
        || matches!(e, ChannelError::External { causes } if !causes.is_empty() && causes.iter().all(|c|
        c == "memory.mailbox_incompatible" || c.starts_with("memory.mailbox_incompatible:")))
}
fn incompatible(e: &ChannelError) -> bool {
    e.all_causes(&incompatible_leaf)
}
fn status_record(record: &ActivationRecord, now: i64) -> ProjectResult {
    record
        .activation
        .present(record.paths.connection_id(), &record.activation_path(), now)
}
fn acquired(record: &ActivationRecord, now: i64, cause: AdmissionError) -> ServiceOutcome {
    let mut result = status_record(record, now);
    if cause.is_busy() {
        result.continuation.kind = "wait_active_driver".into();
        let blocker = super::super::state::Blocker {
            kind: "active_driver".into(),
            consumer: record.metadata.owner.operation_id.clone(),
            installation: record.metadata.target.addon_parent().display().to_string(),
            condition: "active_driver_released".into(),
            ..Default::default()
        };
        result.continuation.blocker = Some(blocker.clone());
        ServiceOutcome::with_result(
            result,
            Err(AdmissionError::Joined(vec![
                cause,
                ChannelError::Blocked(blocker).into(),
            ])),
        )
    } else {
        ServiceOutcome::with_result(result, Err(cause))
    }
}
impl<E: ActivationEnvironment, C: Clock, T: TokenSource, S: ContinuationScheduler>
    ConnectionService<E, C, T, S>
where
    E::Backend: ActivationBackend,
{
    /// Public status/resume/disconnect dispatch must use these three wrappers.
    /// Only the exact missing-log condition enters activation; corruption and
    /// unknown historic generations are not migrated or treated as fresh work.
    pub fn status_with_activation(&self, id: &str, control: &OperationControl) -> ServiceOutcome {
        match self.book.status(id, control) {
            Ok(record) => ServiceOutcome::result(self.project(&record, "", "")),
            Err(e) if missing(&e) => match self.book.activation_status(id, control) {
                Ok(record) => {
                    ServiceOutcome::result(status_record(&record, self.clock.unix_millis()))
                }
                Err(e) => ServiceOutcome::failure(e),
            },
            Err(e) => ServiceOutcome::failure(e),
        }
    }
    pub fn resume_with_activation(
        &mut self,
        id: &str,
        control: &OperationControl,
    ) -> ServiceOutcome {
        match self.book.status(id, control) {
            Err(e) if missing(&e) => self.resume_activation(id, control),
            _ => self.resume(id, control),
        }
    }
    pub fn disconnect_with_activation(
        &mut self,
        id: &str,
        control: &OperationControl,
    ) -> ServiceOutcome {
        match self.book.status(id, control) {
            Err(e) if missing(&e) => self.disconnect_activation(id, control),
            _ => self.disconnect(id, control),
        }
    }
    pub fn connect_with_activation(
        &mut self,
        request: &TargetRequest,
        control: &OperationControl,
    ) -> ServiceOutcome {
        let target = match self.environment.resolve(request, control) {
            Ok(t) => t,
            Err(e) => return ServiceOutcome::failure(e),
        };
        if (!request.filter.build.is_empty() && request.filter.build != target.client.full_build)
            || (!request.filter.product.is_empty()
                && request.filter.product != target.client.product)
        {
            return ServiceOutcome::failure(contract("live.channel_target_mismatch"));
        }
        let inspection = inspect_window_owner(&target.addon_parent(), &target.resource(), control);
        if let Some(e) = inspection.error {
            return ServiceOutcome::failure(e);
        }
        if let Some(owner) = inspection.owner {
            if owner.workspace_id != self.book.workspace_id() {
                return ServiceOutcome::failure(AdmissionError::Occupied {
                    owner,
                    foreign: true,
                });
            }
            let metadata = match self.book.metadata(&owner.operation_id, control) {
                Ok(m) => m,
                Err(e) => return ServiceOutcome::failure(e),
            };
            if metadata.owner != owner || !metadata.target.same_connection_target(&target) {
                return ServiceOutcome::failure(contract("live.channel_ownership_changed"));
            }
            let status = self.status_with_activation(&owner.operation_id, control);
            if status.error.is_some() {
                return status;
            }
            if let Some(result) = &status.result {
                if !result.identity.runtime.is_empty()
                    && ((!request.filter.character.is_empty()
                        && request.filter.character != result.identity.character)
                        || (!request.filter.realm.is_empty()
                            && request.filter.realm != result.identity.realm))
                {
                    return ServiceOutcome::with_result(
                        result.clone(),
                        Err(contract("live.channel_target_mismatch")),
                    );
                }
            }
            return self.resume_with_activation(&owner.operation_id, control);
        }
        // Keep the one resolved target throughout discovery and admission.
        // Re-running Connect's resolver here could silently select a new PID.
        let mut backend = match self.environment.open(&target, None, control) {
            Ok(backend) => backend,
            Err(e) => return ServiceOutcome::failure(e),
        };
        enum Selected {
            Connection(ConnectionRecord),
            Activation(&'static str),
        }
        let selected = (|| -> Result<Selected, AdmissionError> {
            let candidates =
                match backend.discover(control, &request.filter.character, &request.filter.realm) {
                    Ok(candidates) => candidates,
                    Err(e) if incompatible(&e) => Vec::new(),
                    Err(e) => return Err(e.into()),
                };
            let Some(identity) = candidates.into_iter().next() else {
                return Ok(Selected::Activation("connect"));
            };
            if identity.build != target.client.full_build
                || identity.product != target.client.product
            {
                return Err(contract("live.channel_runtime_build_mismatch"));
            }
            if identity.slots != SLOT_COUNT
                || super::super::upgrade_input_capability(&identity.input_state)
            {
                return Ok(Selected::Activation("connect-upgrade"));
            }
            if identity.inventory.is_some_and(|n| n != identity.slots) {
                return Err(contract("live.channel_slot_inventory_incomplete"));
            }
            if identity.input_state != SCHEMA {
                return Err(contract("live.channel_input_capability_unsupported"));
            }
            self.book
                .create(
                    target.clone(),
                    identity,
                    &self.clock,
                    &mut self.tokens,
                    control,
                )
                .map(Selected::Connection)
        })();
        let closed = backend.checked_close();
        match selected {
            Err(e) => {
                let mut out = ServiceOutcome::failure(e);
                out.join(channel(closed));
                out
            }
            Ok(Selected::Connection(record)) => {
                if let Err(e) = closed {
                    return ServiceOutcome::with_result(
                        self.project(&record, "", backend.scans()),
                        Err(e.into()),
                    );
                }
                self.resume(&record.state.id, control)
            }
            Ok(Selected::Activation(prefix)) => {
                if let Err(e) = closed {
                    return ServiceOutcome::failure(e);
                }
                let key = match self.tokens.token() {
                    Ok(key) => format!("{prefix}-{key}"),
                    Err(e) => return ServiceOutcome::failure(e),
                };
                self.activate_resolved(target, request, &key, control)
            }
        }
    }
    /// Existing `live reload fallback` selects this route. It is not a blind
    /// chat command or a distinct keystroke implementation.
    pub fn activate(
        &mut self,
        request: &TargetRequest,
        key: &str,
        control: &OperationControl,
    ) -> ServiceOutcome {
        if let Err(e) = validate_request(key) {
            return ServiceOutcome::failure(e);
        }
        if request.installation.is_empty() || request.pid == 0 {
            return ServiceOutcome::failure(contract("live.channel_explicit_target_required"));
        }
        let target = match self.environment.resolve(request, control) {
            Ok(t) => t,
            Err(e) => return ServiceOutcome::failure(e),
        };
        self.activate_resolved(target, request, key, control)
    }
    fn activate_resolved(
        &mut self,
        target: ConnectionTarget,
        request: &TargetRequest,
        key: &str,
        control: &OperationControl,
    ) -> ServiceOutcome {
        if let Err(e) = self.environment.guard_activation(&target, control) {
            return ServiceOutcome::failure(e);
        }
        match self.book.activation_reference(&target, key, control) {
            Ok(Some(id)) => return self.resume_with_activation(&id, control),
            Err(e) => return ServiceOutcome::failure(e),
            Ok(None) => (),
        }
        match self.book.create_activation(
            target,
            request,
            key,
            &self.clock,
            &mut self.tokens,
            control,
        ) {
            Ok(record) => self.resume_with_activation(record.paths.connection_id(), control),
            Err(e) => ServiceOutcome::failure(e),
        }
    }
    pub fn disconnect_activation(
        &mut self,
        id: &str,
        control: &OperationControl,
    ) -> ServiceOutcome {
        let before = match self.book.activation_status(id, control) {
            Ok(v) => v,
            Err(e) => return ServiceOutcome::failure(e),
        };
        if before.activation.phase == ActivationPhase::Cancelled {
            return self.retire_cancelled(before, control);
        }
        let mut lease = match self.book.acquire_activation(id, control) {
            Ok(v) => v,
            Err(e) => return acquired(&before, self.clock.unix_millis(), e),
        };
        match self.book.status(id, control) {
            Ok(_) => {
                if let Err(e) = lease.close() {
                    return ServiceOutcome::with_result(
                        status_record(&lease.record, self.clock.unix_millis()),
                        Err(e.into()),
                    );
                }
                return self.disconnect(id, control);
            }
            Err(e) if missing(&e) => (),
            Err(e) => {
                let mut o = ServiceOutcome::with_result(
                    status_record(&lease.record, self.clock.unix_millis()),
                    Err(e.into()),
                );
                o.join(channel(lease.close()));
                return o;
            }
        }
        let persisted = status_record(&lease.record, self.clock.unix_millis());
        let result = lease
            .record
            .activation
            .cancel()
            .and_then(|_| lease.save(control));
        let projected = if result.is_ok() {
            status_record(&lease.record, self.clock.unix_millis())
        } else {
            persisted
        };
        let mut outcome = ServiceOutcome::with_result(projected, channel(result));
        let closed = lease.close();
        let can_retire = outcome.error.is_none() && closed.is_ok();
        outcome.join(channel(closed));
        if can_retire {
            outcome.join(self.book.retire_activation(id, control).map(|_| ()));
        }
        outcome
    }
    fn retire_cancelled(
        &self,
        record: ActivationRecord,
        control: &OperationControl,
    ) -> ServiceOutcome {
        ServiceOutcome::with_result(
            status_record(&record, self.clock.unix_millis()),
            self.book
                .retire_activation(record.paths.connection_id(), control)
                .map(|_| ()),
        )
    }
    pub fn resume_activation(&mut self, id: &str, control: &OperationControl) -> ServiceOutcome {
        let before = match self.book.activation_status(id, control) {
            Ok(v) => v,
            Err(e) => return ServiceOutcome::failure(e),
        };
        if before.activation.phase == ActivationPhase::Cancelled {
            return self.retire_cancelled(before, control);
        }
        let mut lease = match self.book.acquire_activation(id, control) {
            Ok(v) => v,
            Err(e) => return acquired(&before, self.clock.unix_millis(), e),
        };
        match self.book.status(id, control) {
            Ok(_) => {
                if let Err(e) = lease.close() {
                    return ServiceOutcome::with_result(
                        status_record(&lease.record, self.clock.unix_millis()),
                        Err(e.into()),
                    );
                }
                return self.resume(id, control);
            }
            Err(e) if missing(&e) => (),
            Err(e) => {
                let mut o = ServiceOutcome::with_result(
                    status_record(&lease.record, self.clock.unix_millis()),
                    Err(e.into()),
                );
                o.join(channel(lease.close()));
                return o;
            }
        }
        if lease.record.activation.phase == ActivationPhase::Cancelled {
            let closed = lease.close();
            if let Err(e) = closed {
                return ServiceOutcome::with_result(
                    status_record(&lease.record, self.clock.unix_millis()),
                    Err(e.into()),
                );
            }
            return self.retire_cancelled(lease.record, control);
        }
        let remaining = if lease.record.activation.phase == ActivationPhase::WaitingManualReload {
            Ok(DEFAULT_RECOVERY_MILLIS)
        } else {
            observe_activation(&mut lease, self.clock.unix_millis(), control)
        };
        let remaining = match remaining {
            Ok(ms) => ms,
            Err(e) => {
                let mut o = ServiceOutcome::with_result(
                    status_record(&lease.record, self.clock.unix_millis()),
                    Err(e.into()),
                );
                o.join(channel(lease.close()));
                return o;
            }
        };
        let budget_deadline = Instant::now() + Duration::from_millis(remaining as u64);
        let bounded = match control.ordered_child(budget_deadline) {
            Ok(c) => c,
            Err(e) => {
                let mut o = ServiceOutcome::with_result(
                    status_record(&lease.record, self.clock.unix_millis()),
                    Err(ChannelError::from(e).into()),
                );
                o.join(channel(lease.close()));
                return o;
            }
        };
        let mut backend = None;
        let mut selected = false;
        let mut waiting_runtime = false;
        let result = (|| -> ChannelResult<()> {
            if lease.record.activation.phase == ActivationPhase::RuntimeSelected {
                return Err(error("live.channel_journal_missing"));
            }
            backend = Some(self.environment.open(
                &lease.record.metadata.target,
                Some(&lease.record.metadata.owner),
                &bounded,
            )?);
            let native = backend.as_mut().unwrap();
            let a = &mut lease.record.activation;
            if matches!(
                a.phase,
                ActivationPhase::Prepared | ActivationPhase::WaitingManualReload
            ) {
                let candidates =
                    match native.discover(&bounded, &a.selection.character, &a.selection.realm) {
                        Ok(c) => c,
                        Err(e) if incompatible(&e) => Vec::new(),
                        Err(e) => return Err(e),
                    };
                if let Some(candidate) = candidates.first() {
                    let can_reload = native.activation_capability(candidate, &bounded)?;
                    if can_reload
                        && (a.phase == ActivationPhase::Prepared
                            || candidate.input_state == super::super::PREVIOUS_SCHEMA)
                    {
                        if !candidate.owner.is_empty() {
                            return Err(error("live.channel_ownership_changed"));
                        }
                        if a.phase == ActivationPhase::WaitingManualReload {
                            a.budget =
                                Some(budget(self.clock.unix_millis(), DEFAULT_RECOVERY_MILLIS)?);
                        }
                        let mut state = selected_state(id, candidate.clone(), a.budget.clone());
                        state.reload = Some(ReloadAttempt {
                            automatic: false,
                            slot_enablement: false, slot_budget_source: None,
                            recovery_budget: a.budget.clone(),
                            reconcile_pending: false,
                            request: a.request.clone(),
                            from: candidate.runtime.clone(),
                            phase: ReloadPhase::Intent,
                            input_step: 0,
                            patch_transitions: 0,
                        });
                        lease.select_runtime(&state, &bounded)?;
                        selected = true;
                        return Ok(());
                    }
                    if a.phase == ActivationPhase::Prepared {
                        a.prior_runtime = candidate.runtime.clone();
                    }
                }
                if a.phase == ActivationPhase::Prepared {
                    a.phase = ActivationPhase::WaitingManualReload;
                    lease.save(&bounded)?;
                    return Err(manual_reload_required());
                }
            }
            loop {
                check_control(&bounded)?;
                let a = &lease.record.activation;
                let candidates =
                    match native.discover(&bounded, &a.selection.character, &a.selection.realm) {
                        Ok(c) => c,
                        Err(e)
                            if a.phase == ActivationPhase::WaitingManualReload
                                && incompatible(&e) =>
                        {
                            return Err(manual_reload_required());
                        }
                        Err(e) => return Err(e),
                    };
                for candidate in candidates {
                    if candidate.runtime <= a.prior_runtime
                        || !candidate.owner.is_empty()
                        || candidate.build != lease.record.metadata.target.client.full_build
                        || candidate.product != lease.record.metadata.target.client.product
                    {
                        continue;
                    }
                    if candidate.slots != SLOT_COUNT
                        || candidate.inventory.is_some_and(|n| n != candidate.slots)
                    {
                        return Err(error("live.channel_client_restart_required"));
                    }
                    if candidate.input_state != SCHEMA {
                        return Err(error("live.channel_input_capability_unsupported"));
                    }
                    if lease.record.activation.phase == ActivationPhase::WaitingManualReload {
                        lease.record.activation.budget =
                            Some(budget(self.clock.unix_millis(), DEFAULT_RECOVERY_MILLIS)?);
                    }
                    let state =
                        selected_state(id, candidate, lease.record.activation.budget.clone());
                    lease.select_runtime(&state, &bounded)?;
                    selected = true;
                    return Ok(());
                }
                if lease.record.activation.phase == ActivationPhase::WaitingManualReload {
                    return Err(manual_reload_required());
                }
                waiting_runtime = true;
                self.scheduler
                    .wait(&bounded, Duration::from_millis(250))
                    .map_err(|e| ChannelError::Joined {
                        causes: vec![pending(), e],
                    })?;
            }
        })();
        // A durable log may exist even when marking runtime_selected failed.
        // The ordinary channel now owns its budget; never finalize the old copy.
        selected |= lease.channel_seeded() || self.book.status(id, &bounded).is_ok();
        let mut outcome = ServiceOutcome::with_result(
            status_record(&lease.record, self.clock.unix_millis()),
            channel(result),
        );
        // Authority facts flush before releasing the execution lease; diagnostic
        // close cannot consume this last durable-budget observation.
        if !selected && lease.record.activation.phase != ActivationPhase::WaitingManualReload {
            let flush = OperationControl::uncancelable(Instant::now() + Duration::from_secs(3));
            if bounded.deadline() == budget_deadline
                && bounded.stop_cause().ok().flatten()
                    == Some(lycheedev_core::control::StopCause::DeadlineExceeded)
            {
                lease.record.activation.budget.as_mut().unwrap().exhausted = true;
                outcome.join(channel(lease.save(&flush)));
            }
            outcome.join(channel(
                observe_activation(&mut lease, self.clock.unix_millis(), &flush).map(|_| ()),
            ));
            outcome.result = Some(status_record(&lease.record, self.clock.unix_millis()));
        }
        if waiting_runtime && !selected {
            if let Some(result) = outcome.result.as_mut() {
                result.stage = "activation_waiting_runtime".into();
            }
        }
        outcome.join(channel(lease.close()));
        if let Some(native) = backend.as_mut() {
            outcome.join(channel(native.checked_close()));
        }
        if selected && outcome.error.is_none() {
            return self.resume(id, control);
        }
        // A failed activation phase write after the first channel log must still
        // expose that durable connection rather than invent an unbound success.
        if let Ok(record) = self.book.status(id, control) {
            outcome.result = Some(self.project(&record, "", ""));
        }
        outcome
    }
}
fn observe_activation(
    lease: &mut ActivationLease,
    now: i64,
    control: &OperationControl,
) -> ChannelResult<i64> {
    lease.record.activation.validate()?;
    let (changed, result) = observe_budget(lease.record.activation.budget.as_mut().unwrap(), now);
    if changed {
        lease.save(control)?;
    }
    result
}
fn selected_state(
    id: &str,
    identity: Identity,
    budget: Option<super::super::state::DurableBudget>,
) -> State {
    State {
        schema: CHANNEL_SCHEMA.into(),
        id: id.into(),
        owner: id.trim_start_matches("CON-").into(),
        identity,
        connect_budget: budget,
        progress_version: 1,
        ..State::default()
    }
}
