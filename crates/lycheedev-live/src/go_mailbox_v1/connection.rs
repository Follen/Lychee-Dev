//! Formal CON application service. All mutable turns own the original execution
//! lease, reload the journal under that lease, and close every acquired resource.
//! Oracle: Go 3.2.0 3057a9666ab078270b63c7f06de748069b0d1f4b,
//! channel/{project_windows,request,process_end_windows}.go.
//!
//! Activation/repair/doctor are deliberately separate entry points, not hidden
//! fallbacks. A terminal historical execute is answered before opening a native
//! backend. An unknown execute is never converted into a fresh request.
#![forbid(unsafe_code)]

use super::{
    Identity, SCHEMA, SLOT_COUNT,
    admission::{AdmissionError, ConnectionBook, ConnectionRecord, ConnectionTarget, TargetFilter},
    driver::{Clock, Driver, TokenSource, error},
    durable_journal::{self, WindowOwner},
    journal::save_state,
    observations::{ChannelError, ChannelResult, check_control},
    reload::ContinuationScheduler,
    runtime_retirement::{RetirementTarget, RuntimeRetirementBackend},
    state::{
        CleanupMethod, OperationPolicy, OperationStage, ProcessEnd, SlotEnvelope, State,
        validate_request,
    },
};
use lycheedev_core::control::OperationControl;
use std::path::Path;

mod errors;
mod native;
mod outcome;
mod projection;
mod session;
pub use errors::{PublicFailure, classify_error};
pub use native::{
    NativeEnvironment, ProcessLifetimeObserver, SystemClock, SystemScheduler, SystemTokens,
    WindowsProcessLifetime,
};
pub use outcome::ServiceOutcome;
use projection::present_owned;
pub use projection::{ProjectResult, RawReport, present};
use session::{LeaseOnly, SessionOwner};

/// Existing public target selectors; no new command or protocol is introduced.
#[derive(Debug, Clone, Default)]
pub struct TargetRequest {
    pub installation: String,
    pub pid: u32,
    pub filter: TargetFilter,
}
#[derive(Debug, Clone, Copy)]
pub struct ExecuteRequest<'a> {
    pub request: &'a str,
    pub code: &'a [u8],
    pub budget_seconds: i32,
    pub policy: OperationPolicy,
}

/// Native implementation owns process/trace resources. Drop is the unwind
/// fallback only; all ordinary service exits call checked_close explicitly.
pub trait ConnectionBackend: RuntimeRetirementBackend {
    fn discover(
        &mut self,
        control: &OperationControl,
        character: &str,
        realm: &str,
    ) -> ChannelResult<Vec<Identity>>;
    fn checked_close(&mut self) -> ChannelResult<()>;
    fn scans(&self) -> &str;
}
/// The only OS/deployment seam. Tests may model it, but model evidence is not
/// Windows/process evidence. Implementations must not infer an ended process
/// from an inaccessible process, missing HWND, reader failure, or a timeout.
pub trait ConnectionEnvironment {
    type Backend: ConnectionBackend;
    fn resolve(
        &mut self,
        request: &TargetRequest,
        control: &OperationControl,
    ) -> ChannelResult<ConnectionTarget>;
    fn open(
        &mut self,
        target: &ConnectionTarget,
        owner: Option<&WindowOwner>,
        control: &OperationControl,
    ) -> ChannelResult<Self::Backend>;
    fn process_end(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<ProcessEnd>;
    /// Called only after a second process-lifetime observation under the exact
    /// execution lease and durable process_end_observed. No game input allowed.
    fn retire_process(
        &mut self,
        target: &ConnectionTarget,
        release: &str,
        envelopes: &[SlotEnvelope],
        control: &OperationControl,
    ) -> ChannelResult<()>;
}

fn channel(result: ChannelResult<()>) -> Result<(), AdmissionError> {
    result.map_err(Into::into)
}
fn contract(code: &'static str) -> AdmissionError {
    error(code).into()
}
struct BorrowClock<'a, C>(&'a C);
impl<C: Clock> Clock for BorrowClock<'_, C> {
    fn unix_millis(&self) -> i64 {
        self.0.unix_millis()
    }
}
struct BorrowTokens<'a, T>(&'a mut T);
impl<T: TokenSource> TokenSource for BorrowTokens<'_, T> {
    fn token(&mut self) -> ChannelResult<String> {
        self.0.token()
    }
}

pub struct ConnectionService<E, C, T, S> {
    book: ConnectionBook,
    environment: E,
    clock: C,
    tokens: T,
    scheduler: S,
    #[cfg(test)]
    journal_fault: Option<fn(&mut durable_journal::DurableChannelJournal)>,
}
impl<E: ConnectionEnvironment, C: Clock, T: TokenSource, S: ContinuationScheduler>
    ConnectionService<E, C, T, S>
{
    pub fn open(
        project: &Path,
        environment: E,
        clock: C,
        tokens: T,
        scheduler: S,
    ) -> ChannelResult<Self> {
        Ok(Self {
            book: ConnectionBook::open(project)?,
            environment,
            clock,
            tokens,
            scheduler,
            #[cfg(test)]
            journal_fault: None,
        })
    }
    pub fn project_root(&self) -> &Path {
        self.book.root()
    }
    /// Pure persisted projection. Does not open a backend, repair a tail,
    /// acquire an execution lease, retire claims, or change a durable deadline.
    pub fn status(&self, id: &str, control: &OperationControl) -> ServiceOutcome {
        match self.book.status(id, control) {
            Ok(record) => ServiceOutcome::Completed(self.project_owned(record, "", "")),
            Err(e) => ServiceOutcome::failure(e),
        }
    }
    /// Library report lookup, not an advertised `live report` CLI command.
    /// Unknown/candidate bytes remain unavailable in the public projection.
    pub fn report(&self, id: &str, request: &str, control: &OperationControl) -> ServiceOutcome {
        let record = match self.book.status(id, control) {
            Ok(record) => record,
            Err(e) => return ServiceOutcome::failure(e),
        };
        match durable_journal::historical_request(&record.paths, request, control) {
            Ok(Some(state)) => ServiceOutcome::Completed(present_owned(
                state,
                record.paths.log_path(),
                self.clock.unix_millis(),
                "",
                "",
            )),
            Ok(None) => ServiceOutcome::with_result(
                self.project(&record, "", ""),
                Err(contract("live.channel_operation_missing")),
            ),
            Err(e) => ServiceOutcome::with_result(self.project(&record, "", ""), Err(e.into())),
        }
    }
    pub fn connect(
        &mut self,
        request: &TargetRequest,
        control: &OperationControl,
    ) -> ServiceOutcome {
        let target = match self.environment.resolve(request, control) {
            Ok(target) => target,
            Err(e) => return ServiceOutcome::failure(e),
        };
        match self.book.find_selected(&target, &request.filter, control) {
            Ok(Some(record)) => return self.resume(&record.state.id, control),
            Ok(None) => (),
            Err(e) => return ServiceOutcome::failure(e),
        }
        let mut backend = match self.environment.open(&target, None, control) {
            Ok(backend) => backend,
            Err(e) => return ServiceOutcome::failure(e),
        };
        let created = (|| -> Result<ConnectionRecord, AdmissionError> {
            let identities =
                backend.discover(control, &request.filter.character, &request.filter.realm)?;
            // Native discovery authenticates completeness/uniqueness. Empty or
            // obsolete results require the separate activation implementation.
            let identity = identities
                .into_iter()
                .next()
                .ok_or_else(|| contract("live.channel_activation_required"))?;
            if identity.build != target.client.full_build
                || identity.product != target.client.product
            {
                return Err(contract("live.channel_runtime_build_mismatch"));
            }
            if identity.slots != SLOT_COUNT
                || super::upgrade_input_capability(&identity.input_state)
            {
                return Err(contract("live.channel_activation_required"));
            }
            if identity.inventory.is_some_and(|n| n != identity.slots) {
                return Err(contract("live.channel_slot_inventory_incomplete"));
            }
            if identity.input_state != SCHEMA {
                return Err(contract("live.channel_input_capability_unsupported"));
            }
            self.book
                .create(target, identity, &self.clock, &mut self.tokens, control)
        })();
        let closed = backend.checked_close();
        match created {
            Err(e) => {
                let mut result = ServiceOutcome::failure(e);
                result = result.join(channel(closed));
                result
            }
            Ok(record) => {
                if let Err(e) = closed {
                    return ServiceOutcome::with_result(
                        self.project(&record, "", backend.scans()),
                        Err(e.into()),
                    );
                }
                self.drive(&record.state.id, Action::Resume, control)
            }
        }
    }
    pub fn resume(&mut self, id: &str, control: &OperationControl) -> ServiceOutcome {
        let record = match self.read_recover(id, control) {
            Ok(record) => record,
            Err(e) => return ServiceOutcome::failure(e),
        };
        if record.state.closed {
            return self.retire(record, control);
        }
        // Original Go only shortcuts completed business when no close/reload/
        // runtime retirement remains. Binding completion alone is not enough.
        if record.state.runtime_end.is_none()
            && !record.state.closing
            && record
                .state
                .reload
                .as_ref()
                .is_none_or(|r| r.phase == super::state::ReloadPhase::Complete)
            && record.state.transaction.is_none()
            && record
                .state
                .recovery
                .as_ref()
                .is_none_or(|r| r.phase == super::state::RecoveryPhase::Complete)
            && record
                .state
                .operation
                .as_ref()
                .is_some_and(|o| o.stage == OperationStage::Complete)
        {
            return ServiceOutcome::Completed(self.project_owned(record, "", ""));
        }
        // Fixed Go retains the pre-close-budget unbind shape. Route it through
        // Disconnect so original-process retirement is checked before native
        // open. Explicit closing/reload/runtime-end work keeps its own route.
        if record.state.runtime_end.is_none()
            && !record.state.closing
            && record
                .state
                .reload
                .as_ref()
                .is_none_or(|r| r.phase == super::state::ReloadPhase::Complete)
            && record
                .state
                .transaction
                .as_ref()
                .is_some_and(|tx| tx.envelope.action == "unbind")
        {
            return self.disconnect(id, control);
        }
        self.drive(id, Action::Resume, control)
    }
    pub fn execute(
        &mut self,
        id: &str,
        request: ExecuteRequest<'_>,
        control: &OperationControl,
    ) -> ServiceOutcome {
        let record = match self.read_recover(id, control) {
            Ok(record) => record,
            Err(e) => return ServiceOutcome::failure(e),
        };
        match historical(&record, request, control) {
            Ok(Some(state)) if complete(&state) => {
                return ServiceOutcome::Completed(present_owned(
                    state,
                    record.paths.log_path(),
                    self.clock.unix_millis(),
                    "",
                    "",
                ));
            }
            Ok(_) => (),
            Err(e) => {
                return ServiceOutcome::with_result(self.project(&record, "", ""), Err(e.into()));
            }
        }
        self.drive(id, Action::Execute(request), control)
    }
    pub fn disconnect(&mut self, id: &str, control: &OperationControl) -> ServiceOutcome {
        let record = match self.read_recover(id, control) {
            Ok(record) => record,
            Err(e) => return ServiceOutcome::failure(e),
        };
        if record.state.closed {
            return self.retire(record, control);
        }
        match self
            .environment
            .process_end(&record.metadata.target, control)
        {
            Ok(ProcessEnd::None) => self.drive(id, Action::Disconnect, control),
            Ok(_) => self.disconnect_ended(id, control),
            Err(e) => ServiceOutcome::with_result(self.project(&record, "", ""), Err(e.into())),
        }
    }
    fn read_recover(
        &self,
        id: &str,
        control: &OperationControl,
    ) -> Result<ConnectionRecord, AdmissionError> {
        match self.book.status(id, control) {
            Ok(record) => Ok(record),
            Err(e) if e.any_cause(&|cause| matches!(cause, ChannelError::Contract(c) if c.0 == "journal.memory_torn_tail")) => {
                let metadata = self.book.metadata(id, control)?;
                let paths = durable_journal::ChannelPaths::new(self.book.root(), id)?;
                durable_journal::recover_tail(paths, &metadata.target.addon_parent(), metadata.owner, control)?;
                self.book.status(id, control).map_err(Into::into)
            }
            Err(e) => Err(e.into()),
        }
    }
    fn project(&self, record: &ConnectionRecord, waiting: &str, scans: &str) -> ProjectResult {
        present(
            &record.state,
            record.paths.log_path(),
            self.clock.unix_millis(),
            waiting,
            scans,
        )
    }
    fn project_owned(&self, record: ConnectionRecord, waiting: &str, scans: &str) -> ProjectResult {
        present_owned(
            record.state,
            record.paths.log_path(),
            self.clock.unix_millis(),
            waiting,
            scans,
        )
    }
    fn retire(&mut self, record: ConnectionRecord, control: &OperationControl) -> ServiceOutcome {
        let projected = self.project(&record, "", "");
        ServiceOutcome::with_result(
            projected,
            self.book
                .retire_closed(&record.state.id, control)
                .map(|_| ()),
        )
    }
    fn acquired(&self, record: &ConnectionRecord, error: AdmissionError) -> ServiceOutcome {
        let mut projected = self.project(record, "", "");
        if error.is_busy() {
            projected.continuation.kind = "wait_active_driver".into();
            projected.continuation.blocker = Some(super::state::Blocker {
                kind: "active_driver".into(),
                consumer: record.metadata.owner.operation_id.clone(),
                installation: record.metadata.target.addon_parent().display().to_string(),
                condition: "active_driver_released".into(),
                ..Default::default()
            });
            ServiceOutcome::with_result(
                projected,
                Err(AdmissionError::Joined(vec![
                    ChannelError::Pending { causes: vec![] }.into(),
                    error,
                ])),
            )
        } else {
            ServiceOutcome::with_result(projected, Err(error))
        }
    }
    fn drive(
        &mut self,
        id: &str,
        action: Action<'_>,
        control: &OperationControl,
    ) -> ServiceOutcome {
        let before = match self.book.status(id, control) {
            Ok(record) => record,
            Err(e) => return ServiceOutcome::failure(e),
        };
        if before.state.closed {
            return ServiceOutcome::with_result(
                self.project(&before, "", ""),
                Err(contract("live.channel_closed")),
            );
        }
        let (record, journal) = match self.book.acquire(id, control) {
            Ok(owned) => owned,
            Err(e) => return self.acquired(&before, e),
        };
        // Only the under-lease replay can drive this turn. Release the earlier
        // read-only projection rather than retaining another code/report owner.
        drop(before);
        let session = self.own_session(journal);
        // All predicates are rechecked on the under-lease replay before native
        // open, including a concurrent CLI's just-completed request.
        let check = (|| -> Result<Option<State>, AdmissionError> {
            if record.state.closed {
                return Err(contract("live.channel_closed"));
            }
            if let Action::Execute(request) = action {
                if let Some(prior) = historical(&record, request, control)? {
                    if complete(&prior) {
                        return Ok(Some(prior));
                    }
                    if record.state.operation.as_ref().map(|o| &o.id)
                        != prior.operation.as_ref().map(|o| &o.id)
                    {
                        return Err(contract("live.channel_request_history_invalid"));
                    }
                }
            }
            Ok(None)
        })();
        match check {
            Ok(None) => (),
            other => {
                let outcome = match other {
                    Ok(Some(state)) => ServiceOutcome::Completed(present_owned(
                        state,
                        record.paths.log_path(),
                        self.clock.unix_millis(),
                        "",
                        "",
                    )),
                    Err(e) => ServiceOutcome::with_result(self.project(&record, "", ""), Err(e)),
                    Ok(None) => unreachable!(),
                };
                return session.finish(outcome, false).into_outcome();
            }
        }
        let backend = match self.environment.open(
            &record.metadata.target,
            Some(&record.metadata.owner),
            control,
        ) {
            Ok(backend) => backend,
            Err(e) => {
                let outcome =
                    ServiceOutcome::with_result(self.project(&record, "", ""), Err(e.into()));
                return session.finish(outcome, false).into_outcome();
            }
        };
        let target = RetirementTarget {
            process_id: record.metadata.target.window.process_id,
            process_started_at: record.metadata.target.window.process_started_at,
        };
        session
            .attach(backend)
            .run(
                record.state,
                record.paths.log_path(),
                BorrowClock(&self.clock),
                BorrowTokens(&mut self.tokens),
                |driver| {
                    match action {
                        Action::Resume => (),
                        Action::Disconnect => driver.request_close(control)?,
                        Action::Execute(request) => {
                            let prior = driver.historical_request(
                                control,
                                request.request,
                                request.code,
                                request.budget_seconds,
                                request.policy,
                            )?;
                            if prior.is_none() {
                                driver.prepare_request(
                                    control,
                                    request.request,
                                    request.code,
                                    request.budget_seconds,
                                    request.policy,
                                )?;
                            }
                            // Existing unknown/cancelled work is continued exactly;
                            // Driver refuses replay and preserves its original ticket.
                        }
                    }
                    if matches!(action, Action::Execute(_)) {
                        driver.continue_until(control, target, &mut self.scheduler)
                    } else {
                        driver.continue_or_retire(
                            control,
                            target,
                            matches!(action, Action::Disconnect),
                            &mut self.scheduler,
                        )
                    }
                },
            )
            .retire(&mut self.book, id, control)
    }

    fn own_session(
        &mut self,
        journal: durable_journal::DurableChannelJournal,
    ) -> SessionOwner<LeaseOnly> {
        #[cfg(test)]
        let journal = {
            let mut journal = journal;
            if let Some(fault) = self.journal_fault.take() {
                fault(&mut journal);
            }
            journal
        };
        SessionOwner::leased(journal)
    }
    fn disconnect_ended(&mut self, id: &str, control: &OperationControl) -> ServiceOutcome {
        let before = match self.book.status(id, control) {
            Ok(record) => record,
            Err(e) => return ServiceOutcome::failure(e),
        };
        if before.metadata.owner.resource != before.metadata.target.resource() {
            return ServiceOutcome::with_result(
                self.project(&before, "", ""),
                Err(contract("desktop.identity_changed")),
            );
        }
        let (mut record, journal) = match self.book.acquire(id, control) {
            Ok(value) => value,
            Err(e) => return self.acquired(&before, e),
        };
        let mut session = self.own_session(journal);
        let result = (|| -> ChannelResult<()> {
            let ended = self
                .environment
                .process_end(&record.metadata.target, control)?;
            if ended == ProcessEnd::None {
                return Err(error("desktop.identity_changed"));
            }
            record.state.process_end = ended;
            save_state(
                session.journal_mut(),
                control,
                &mut record.state,
                "process_end_observed",
            )?;
            let envelopes: Vec<_> = [
                record.state.transaction.as_ref(),
                record
                    .state
                    .recovery
                    .as_ref()
                    .and_then(|r| r.transaction.as_ref()),
            ]
            .into_iter()
            .flatten()
            .map(|tx| tx.envelope.clone())
            .collect();
            if !envelopes.is_empty() {
                self.environment.retire_process(
                    &record.metadata.target,
                    &record.state.identity.release,
                    &envelopes,
                    control,
                )?;
            }
            close_ended_state(&mut record.state);
            save_state(
                session.journal_mut(),
                control,
                &mut record.state,
                "process_end_closed",
            )
        })();
        let completed = result.is_ok();
        let outcome = ServiceOutcome::with_result(self.project(&record, "", ""), channel(result));
        session
            .finish(outcome, completed)
            .retire(&mut self.book, id, control)
    }
}
#[derive(Clone, Copy)]
enum Action<'a> {
    Resume,
    Execute(ExecuteRequest<'a>),
    Disconnect,
}
fn complete(state: &State) -> bool {
    state
        .operation
        .as_ref()
        .is_some_and(|o| o.stage == OperationStage::Complete)
}
fn historical(
    record: &ConnectionRecord,
    request: ExecuteRequest<'_>,
    control: &OperationControl,
) -> ChannelResult<Option<State>> {
    validate_request(request.request)?;
    let prior = durable_journal::historical_request(&record.paths, request.request, control)?;
    if let Some(state) = &prior {
        let op = state
            .operation
            .as_ref()
            .ok_or_else(|| error("live.channel_request_history_invalid"))?;
        if op.code.as_bytes() != request.code
            || op.budget != request.budget_seconds
            || op.policy != request.policy
        {
            return Err(error("live.channel_request_conflict"));
        }
    }
    Ok(prior)
}
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

#[cfg(test)]
mod tests;

#[path = "activation_service.rs"]
pub mod activation_service;

#[path = "activation_reload.rs"]
mod activation_reload;

#[path = "doctor_service.rs"]
pub mod doctor_service;

#[path = "doctor_repair.rs"]
pub mod doctor_repair;

#[path = "doctor_delivery.rs"]
pub mod doctor_delivery;
