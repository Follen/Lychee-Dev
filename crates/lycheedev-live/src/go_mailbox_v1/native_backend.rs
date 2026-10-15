//! Fixed-Go 3.2.0 native adapter. All reads resolve the current named field;
//! neither a persisted INPUT DTO nor a historical address authorizes input.
//! The coordinator owns the exact CON lease, persists intent before `input`,
//! and durably records the exact receipt before `consumed`. This adapter never
//! retries a chord or implements a second business/protocol state machine.
#![forbid(unsafe_code)]

mod platform;
mod trace;

use super::native_reader::NativeReader;
use super::observations::{
    Backend, ChannelError, ChannelResult, InputAction, InputDispatch, Observation,
    ObservationDispatch, ObservationKind, ObservationQuery, RecordReader, check_control,
    observe_records,
};
use super::runtime_retirement::RuntimeRetirementBackend;
use super::state::{
    Blocker, InputKind, RuntimeReplacementProof, RuntimeReplacementWitness, SlotEnvelope,
};
use super::{
    ContractError, Identity, IdentityPublication, InputDisposition, InputFreshness,
    InputObservation, InputOutcome, Kind, PREVIOUS_SCHEMA, SCHEMA, SLOT_COUNT, Selector,
    ZERO_TOKEN, decode_identity, decode_input, inspect_input, parse_token,
};
use crate::mailbox::Record;
use lycheedev_core::{control::OperationControl, storage};
use lycheedev_delivery::{installation::VerifiedInstallation, slots::SlotPool};
use lycheedev_windows::mailbox_input::{
    InputError, InputFailure, InputReceipt, MailboxAction, MailboxBindings, MailboxControl,
    MailboxWindow,
};
use platform::{DiskPublications, NativeClock, NativeSender};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use trace::{ReaderEvidence, TracedReader};

const INPUT_GRACE: Duration = Duration::from_secs(2);
const REPLACEMENT_LIMIT: Duration = Duration::from_secs(15);
const REPLACEMENT_PAUSE: Duration = Duration::from_millis(1100);
const DIAGNOSTIC_FLUSH_LIMIT: Duration = Duration::from_secs(10);

fn error(code: &'static str) -> ChannelError {
    ContractError(code).into()
}
fn pending() -> ChannelError {
    ChannelError::Pending { causes: Vec::new() }
}
fn join(first: ChannelError, second: ChannelError) -> ChannelError {
    ChannelError::Joined {
        causes: vec![first, second],
    }
}
fn finish<T>(result: ChannelResult<T>, cleanup: ChannelResult<()>) -> ChannelResult<T> {
    match (result, cleanup) {
        (Err(a), Err(b)) => Err(join(a, b)),
        (Err(e), _) | (_, Err(e)) => Err(e),
        (Ok(value), Ok(())) => Ok(value),
    }
}
fn external(cause: impl Into<String>) -> ChannelError {
    ChannelError::External {
        causes: vec![cause.into()],
    }
}
fn toolkit_error(e: lycheedev_core::ToolkitError) -> ChannelError {
    e.into()
}

/// Checked-close native readers and test sources share this narrow interface.
trait Source: RecordReader {
    fn evidence(&self) -> ReaderEvidence;
    fn close(self) -> ChannelResult<()>;
}
trait Publication {
    fn inspect(&mut self) -> ChannelResult<SlotPool>;
    fn publish(&mut self, consumer: &str, envelope: &SlotEnvelope) -> ChannelResult<()>;
    fn verify(&mut self, consumer: &str, envelope: &SlotEnvelope) -> ChannelResult<()>;
    fn verify_route(&mut self, envelope: &SlotEnvelope) -> ChannelResult<()>;
    fn consumed(&mut self, consumer: &str, envelope: &SlotEnvelope) -> ChannelResult<()>;
    fn retire(
        &mut self,
        consumer: &str,
        envelope: &SlotEnvelope,
        runtime: &str,
    ) -> ChannelResult<()>;
    fn close(self: Box<Self>) -> ChannelResult<()>;
}
trait Publications {
    // The production lease borrows only the ORIGINAL control, never this store.
    fn acquire<'a>(
        &self,
        control: &'a OperationControl,
    ) -> ChannelResult<Box<dyn Publication + 'a>>;
    /// Action-aware acquisition lets repair retain ordinary clean-manifest
    /// admission for non-unbind cleanup, before taking the same short lease.
    fn acquire_for<'a>(
        &self,
        control: &'a OperationControl,
        _envelope: &SlotEnvelope,
    ) -> ChannelResult<Box<dyn Publication + 'a>> {
        self.acquire(control)
    }
}
trait Clock {
    fn now(&self) -> Instant;
    fn uptime(&self) -> i64;
    fn wait(&mut self, control: &OperationControl, duration: Duration) -> ChannelResult<()>;
}
trait Sender {
    /// Lazily start the passive monitor before taking a publication lease.
    fn prepare(&mut self, control: &InputControl) -> ChannelResult<()> {
        control.check()
    }
    fn close(&mut self) -> ChannelResult<()> {
        Ok(())
    }
    fn send(
        &mut self,
        control: &InputControl,
        bindings: MailboxBindings,
        action: MailboxAction,
        guard: &mut dyn FnMut(&InputControl) -> ChannelResult<()>,
    ) -> Result<InputReceipt, InputFailure<ChannelError>>;
}
/// Retains ordered cancellation ancestry. The input boundary cannot extend it.
struct InputControl(OperationControl);
impl MailboxControl for InputControl {
    type Error = ChannelError;
    fn deadline(&self) -> Instant {
        self.0.deadline()
    }
    fn ordered_child(&self, deadline: Instant) -> ChannelResult<Self> {
        self.0
            .ordered_child(deadline)
            .map(Self)
            .map_err(toolkit_error)
    }
    fn check(&self) -> ChannelResult<()> {
        check_control(&self.0)
    }
}

type Guard = Box<dyn FnMut(&OperationControl) -> ChannelResult<()>>;
/// Ephemeral one-use witness, deliberately private and not Serialize/Deserialize.
/// Its address exists only to compare two current-publication reads in this
/// invocation. The public DTO and journal contain no input address.
struct InputWitness {
    envelope: SlotEnvelope,
    after: i64,
    capability: String,
    sample: InputObservation,
    address: u64,
}
#[derive(Default)]
struct SampleWait {
    key: String,
    started: Option<Instant>,
    deadline: Option<Instant>,
}
impl SampleWait {
    fn stale(&mut self, key: String, now: Instant) -> bool {
        if self.key != key || self.started.is_none() {
            self.key = key;
            self.started = Some(now);
            self.deadline = now.checked_add(INPUT_GRACE);
        }
        self.started.is_some_and(|start| now >= start)
            && self.deadline.is_some_and(|deadline| now < deadline)
    }
}
struct NativeCore<R, P, S, C> {
    reader: TracedReader<R>,
    publications: P,
    sender: S,
    clock: C,
    guard: Guard,
    release: String,
    consumer: String,
    pid: u32,
    creation: u64,
    witness: Option<InputWitness>,
    sample_wait: SampleWait,
    diagnostic_exchange: String,
}

/// Real production backend using only the formal named reader, typed delivery
/// lease, and the two-key Windows boundary. No old Transport is accepted.
pub struct NativeBackend {
    core: NativeCore<NativeReader, DiskPublications, NativeSender, NativeClock>,
}
impl NativeBackend {
    /// `guard` must revalidate the coordinator's original CON/window/ownership
    /// lease, without acquiring installation maintenance (publication holds it).
    pub fn open(
        target: MailboxWindow,
        installation: &VerifiedInstallation,
        release: &str,
        trace_directory: Option<PathBuf>,
        guard: impl FnMut(&OperationControl) -> ChannelResult<()> + 'static,
        control: &OperationControl,
    ) -> ChannelResult<Self> {
        Self::open_with_store(
            target,
            DiskPublications::verified(installation, release),
            release,
            trace_directory,
            Box::new(guard),
            control,
        )
    }
    /// Cleanup/recovery can retain the authenticated recorded installation even
    /// after unrelated addon drift. This path cannot publish or verify Invoke bytes: delivery requires a
    /// verified manifest for those operations. Reload still requires a valid
    /// pool, fresh two-key INPUT and the original coordinator guard.
    pub fn open_recorded(
        target: MailboxWindow,
        authenticated_addon: PathBuf,
        release: &str,
        trace_directory: Option<PathBuf>,
        guard: impl FnMut(&OperationControl) -> ChannelResult<()> + 'static,
        control: &OperationControl,
    ) -> ChannelResult<Self> {
        Self::open_with_store(
            target,
            DiskPublications::recorded(authenticated_addon, release)?,
            release,
            trace_directory,
            Box::new(guard),
            control,
        )
    }
    fn open_with_store(
        target: MailboxWindow,
        publications: DiskPublications,
        release: &str,
        trace_directory: Option<PathBuf>,
        mut guard: Guard,
        control: &OperationControl,
    ) -> ChannelResult<Self> {
        check_control(control)?;
        guard(control)?;
        let window = target.identity().clone();
        let reader = NativeReader::open(window.clone(), release, control)
            .map_err(|failure| platform::reader_error(&failure.cause))?;
        let checked = finish(guard(control), check_control(control));
        if let Err(cause) = checked {
            return finish(Err(cause), reader.close());
        }
        Ok(Self {
            core: NativeCore {
                reader: TracedReader::new(reader, trace_directory),
                publications,
                sender: NativeSender::new(target),
                clock: NativeClock,
                guard,
                release: release.into(),
                consumer: format!("{}/{}", window.process.pid, window.process.created),
                pid: window.process.pid,
                creation: window.process.created,
                witness: None,
                sample_wait: SampleWait::default(),
                diagnostic_exchange: String::new(),
            },
        })
    }
    /// Close native process ownership before any diagnostic filesystem write.
    /// Close may retry an unconfirmed monitor join or unflushed evidence, never
    /// input or a consumed OS handle. The original cancellation does not skip the 10s
    /// diagnostic-only flush. Every independent close/flush error is returned.
    pub fn close(&mut self) -> ChannelResult<()> {
        self.core.close()
    }
    pub fn finish<T>(&mut self, result: ChannelResult<T>) -> ChannelResult<T> {
        finish(result, self.close())
    }
    pub fn discover(
        &mut self,
        control: &OperationControl,
        character: &str,
        realm: &str,
    ) -> ChannelResult<Vec<Identity>> {
        self.core.discover(control, character, realm)
    }
}
impl<R: Source, P: Publications, S: Sender, C: Clock> NativeCore<R, P, S, C> {
    fn check(&mut self, control: &OperationControl) -> ChannelResult<()> {
        self.reader.require_open()?;
        check_control(control)?;
        finish((self.guard)(control), check_control(control))
    }
    fn close(&mut self) -> ChannelResult<()> {
        self.witness = None;
        let closed = finish(self.sender.close(), self.reader.close());
        let flush = OperationControl::uncancelable(Instant::now() + DIAGNOSTIC_FLUSH_LIMIT);
        finish(closed, self.reader.flush(&flush))
    }
    fn discover(
        &mut self,
        control: &OperationControl,
        character: &str,
        realm: &str,
    ) -> ChannelResult<Vec<Identity>> {
        self.reader.require_open()?;
        let records =
            self.reader
                .find(control, &selector(Kind::Identity, ZERO_TOKEN), None, false)?;
        let mut identities = Vec::new();
        for record in records {
            let record = record.contract().map_err(external)?;
            let identity = match decode_identity(&record, &self.release)? {
                IdentityPublication::Current(i) | IdentityPublication::UpgradeCapability(i) => i,
                IdentityPublication::ManualActivationRequired(_) => {
                    return Err(join(
                        error("live.channel_mailbox_incompatible"),
                        error("live.channel_legacy_identity_requires_activation"),
                    ));
                }
            };
            if (!character.is_empty() && identity.character != character)
                || (!realm.is_empty() && identity.realm != realm)
            {
                continue;
            }
            if !identities.iter().any(|i: &Identity| {
                i.runtime == identity.runtime && i.next_slot == identity.next_slot
            }) {
                identities.push(identity);
            }
        }
        identities.sort_by(|a, b| {
            b.runtime
                .cmp(&a.runtime)
                .then(b.next_slot.cmp(&a.next_slot))
        });
        check_control(control)?;
        Ok(identities)
    }
    fn replacement_read(
        &mut self,
        control: &OperationControl,
    ) -> ChannelResult<(Identity, Record)> {
        self.check(control)?;
        let mut candidates = self.discover(control, "", "")?;
        if candidates.len() != 1 {
            return Err(pending());
        }
        let current = candidates.remove(0);
        let records = self
            .reader
            .find(control, &input_selector(&current.runtime)?, None, true)?;
        self.check(control)?;
        if records.len() != 1 {
            return Err(pending());
        }
        Ok((current, records.into_iter().next().unwrap()))
    }
}
fn selector(kind: Kind, runtime: super::Token) -> Selector {
    Selector {
        kind,
        runtime,
        nonce: ZERO_TOKEN,
        ticket: ZERO_TOKEN,
        body: None,
    }
}
fn input_selector(runtime: &str) -> ChannelResult<Selector> {
    let runtime = parse_token(runtime)?;
    Ok(Selector {
        nonce: runtime,
        ..selector(Kind::Input, runtime)
    })
}
fn capability(capability: &str, action: &str) -> ChannelResult<()> {
    if capability == SCHEMA || (action == "reload" && capability == PREVIOUS_SCHEMA) {
        Ok(())
    } else {
        Err(error("live.channel_input_capability_unsupported"))
    }
}
fn sample_key(e: &SlotEnvelope, after: i64) -> String {
    format!(
        "{}/{}/{}/{}/{}/{}/{}/{}",
        e.runtime, e.owner, e.fence, e.index, e.guid, e.build, e.action, after
    )
}
fn not_sent(reason: &str, cause: ChannelError, retryable: bool) -> InputDispatch {
    InputDispatch {
        outcome: InputOutcome {
            disposition: InputDisposition::NotSent,
            messages_queued: 0,
            at_millis: 0,
            reason: reason.into(),
            retryable,
        },
        error: Some(cause),
    }
}
fn input_failure(failure: InputFailure<ChannelError>) -> (InputReceipt, ChannelError) {
    let mut causes = Vec::new();
    match failure.cause {
        Some(InputError::Guard(e) | InputError::Control(e)) => causes.push(e),
        Some(InputError::Platform(e)) => causes.push(external(e)),
        Some(InputError::CallbackPanicked) => causes.push(error("desktop.input_callback_panicked")),
        None => {}
    }
    causes.extend(failure.cleanup_errors.into_iter().map(external));
    let cause = if causes.len() == 1 {
        causes.remove(0)
    } else if causes.is_empty() {
        error("desktop.input_failure_missing")
    } else {
        ChannelError::Joined { causes }
    };
    (failure.receipt, cause)
}
fn retryable_input(error: &ChannelError) -> bool {
    error.any_cause(&|cause| matches!(cause, ChannelError::Pending { .. })
        || matches!(cause, ChannelError::Contract(e) if e.0 == "memory.mailbox_unavailable: path_changed")
        || matches!(cause, ChannelError::External { causes } if causes.iter().any(|c| matches!(c.as_str(), "desktop.key_already_held" | "desktop.keyboard_active"))))
}

impl<R: Source, P: Publications, S: Sender, C: Clock> Backend for NativeCore<R, P, S, C> {
    fn observe(
        &mut self,
        control: &OperationControl,
        query: &ObservationQuery,
    ) -> ObservationDispatch {
        let mut dispatch: ObservationDispatch =
            observe_records(control, &mut self.reader, query).into();
        // Receipt diagnostic probe is explanatory only, at most once per exchange.
        // A new invocation may probe again, but never sends/replays input here.
        if let Err(cause) = check_control(control) {
            dispatch.error = Some(dispatch.error.map_or(cause.clone(), |e| join(e, cause)));
            return dispatch;
        }
        let is_pending = matches!(dispatch.observation, Some(Observation::Pending { .. }))
            || dispatch
                .error
                .as_ref()
                .is_some_and(ChannelError::is_pending);
        if query.kind != ObservationKind::Receipt
            || !is_pending
            || self.diagnostic_exchange == query.envelope.nonce
        {
            return dispatch;
        }
        self.diagnostic_exchange = query.envelope.nonce.clone();
        let Ok(selector) = input_selector(&query.envelope.runtime) else {
            return dispatch;
        };
        let records = self.reader.find(control, &selector, None, true);
        let records = finish(records, check_control(control));
        let records = match records {
            Ok(records) => records,
            Err(cause) => {
                dispatch.error = Some(join(dispatch.error.unwrap_or_else(pending), cause));
                return dispatch;
            }
        };
        if records.len() != 1 || records[0].payload.len() > 2048 {
            return dispatch;
        }
        let Ok(raw) = serde_json::from_slice::<InputObservation>(&records[0].payload) else {
            return dispatch;
        };
        if !(1..=SLOT_COUNT + 1).contains(&raw.next_slot) {
            return dispatch;
        }
        let mut expected = query.envelope.expected();
        expected.index = raw.next_slot;
        expected.start_slot = 0;
        if let Ok(record) = records[0].contract() {
            if let Ok(current) = decode_input(&record, &expected, 0, self.clock.uptime()) {
                dispatch.observation = Some(Observation::Pending {
                    diagnostic: current.input_attempt,
                });
            }
        }
        if let Err(cause) = check_control(control) {
            dispatch.error = Some(join(dispatch.error.unwrap_or_else(pending), cause));
        }
        dispatch
    }
    fn publish(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()> {
        self.check(control)?;
        let mut lease = self.publications.acquire_for(control, envelope)?;
        let result = (|| {
            self.check(control)?;
            lease.publish(&self.consumer, envelope)
        })();
        finish(result, lease.close())
    }
    fn observe_input(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        after_millis: i64,
        input_capability: &str,
    ) -> ChannelResult<InputObservation> {
        self.witness = None;
        capability(input_capability, &envelope.action)?;
        self.check(control)?;
        let records = self
            .reader
            .find(control, &input_selector(&envelope.runtime)?, None, true)?;
        check_control(control)?;
        let Some(record) = records.first() else {
            return Err(pending());
        };
        let inspected = record.contract().ok().and_then(|r| {
            inspect_input(&r, &envelope.expected(), after_millis, self.clock.uptime()).ok()
        });
        let Some(inspected) = inspected else {
            return Err(pending());
        };
        if inspected.freshness == InputFreshness::Stale {
            let within = self
                .sample_wait
                .stale(sample_key(envelope, after_millis), self.clock.now());
            // The special stale marker suppresses eager runtime discovery only
            // inside the fixed grace. Afterwards plain pending permits normal
            // discovery; newer-but-expired samples never restart that grace.
            return Err(if within {
                join(pending(), error("live.channel_input_observation_stale"))
            } else {
                pending()
            });
        }
        self.sample_wait = SampleWait::default();
        let sample = inspected.observation;
        // Zero/nonunique addresses cannot become an invocation witness.
        if records.len() == 1 && record.address != 0 {
            self.witness = Some(InputWitness {
                envelope: envelope.clone(),
                after: after_millis,
                capability: input_capability.into(),
                sample: sample.clone(),
                address: record.address,
            });
        }
        Ok(sample)
    }
    fn input(&mut self, control: &OperationControl, action: &InputAction) -> InputDispatch {
        // Consume before all failure paths: no retry of an unknown submission.
        let witness = self.witness.take();
        let kind = match action.kind {
            InputKind::Invoke => MailboxAction::Invoke,
            InputKind::Reload => MailboxAction::Reload,
            _ => {
                return not_sent(
                    "input_action_retired",
                    error("live.channel_input_action_invalid"),
                    false,
                );
            }
        };
        if let Err(cause) = capability(
            &action.capability,
            if kind == MailboxAction::Reload {
                "reload"
            } else {
                "invoke"
            },
        ) {
            return not_sent("input_capability_unsupported", cause, false);
        }
        if let Err(cause) = self.check(control) {
            return not_sent("guard_failed", cause, false);
        }
        let monitor = self.sender.prepare(&InputControl(control.clone()));
        if let Err(cause) = finish(monitor, check_control(control)) {
            return not_sent("keyboard_monitor", cause, false);
        }
        let acquired = if kind == MailboxAction::Invoke {
            self.publications.acquire_for(control, &action.envelope)
        } else {
            self.publications.acquire(control)
        };
        let mut lease = match acquired {
            Ok(lease) => Some(lease),
            Err(cause) => {
                let retryable = cause.is_pending();
                return not_sent("shared_publication", cause, retryable);
            }
        };
        let prepared = if kind == MailboxAction::Invoke {
            let lease = lease.as_mut().unwrap();
            lease
                .verify(&self.consumer, &action.envelope)
                .map_err(|e| ("slot_publication_changed", e))
                .and_then(|_| {
                    lease
                        .verify_route(&action.envelope)
                        .map_err(|e| ("slot_route_changed", e))
                })
        } else {
            let mut lease = lease.take().unwrap();
            let result = lease.inspect().map(|_| ());
            finish(result, lease.close()).map_err(|e| ("slot_pool_invalid", e))
        };
        let mut dispatch = if let Err((reason, cause)) = prepared {
            not_sent(reason, cause, false)
        } else {
            self.send_prepared(control, action, witness, kind)
        };
        if let Some(lease) = lease {
            if let Err(close) = lease.close() {
                dispatch.error = Some(dispatch.error.map_or(close.clone(), |e| join(e, close)));
            }
        }
        dispatch
    }
    fn runtime_candidate(
        &mut self,
        control: &OperationControl,
        from: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        Ok(self
            .discover(control, &from.character, &from.realm)?
            .into_iter()
            .find(|candidate| {
                candidate.runtime > from.runtime
                    && candidate.owner.is_empty()
                    && candidate.guid == from.guid
                    && candidate.build == from.build
                    && candidate.product == from.product
                    && candidate.release == from.release
            }))
    }
    fn supersede(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        replacement: &Identity,
    ) -> ChannelResult<()> {
        if replacement.owner != envelope.owner
            || replacement.guid != envelope.guid
            || replacement.build != envelope.build
            || replacement.runtime == envelope.runtime
        {
            return Err(error("live.channel_runtime_retirement_invalid"));
        }
        self.check(control)?;
        let mut lease = self.publications.acquire(control)?;
        let result = (|| {
            self.check(control)?;
            lease.retire(&self.consumer, envelope, &replacement.runtime)
        })();
        finish(result, lease.close())
    }
    fn consumed(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<()> {
        self.reader.require_open()?;
        let mut lease = self.publications.acquire(control)?;
        // The driver's receipt_observed journal precedes this call. No Input,
        // INPUT diagnostic, publication or timer calls this method internally.
        let result = lease.consumed(&self.consumer, envelope);
        finish(result, lease.close())
    }
}
impl<R: Source, P: Publications, S: Sender, C: Clock> NativeCore<R, P, S, C> {
    fn send_prepared(
        &mut self,
        control: &OperationControl,
        action: &InputAction,
        witness: Option<InputWitness>,
        kind: MailboxAction,
    ) -> InputDispatch {
        let Some(sample) = action.observation.as_ref().filter(|o| o.valid_bindings()) else {
            return not_sent("input_bindings_invalid", pending(), false);
        };
        let bindings = match sample
            .bindings
            .as_ref()
            .and_then(|b| MailboxBindings::new(&b.wake, &b.reload).ok())
        {
            Some(bindings) => bindings,
            None => return not_sent("input_bindings_invalid", pending(), false),
        };
        let Some(witness) = witness.filter(|w| {
            w.envelope == action.envelope
                && action.after_millis >= w.after
                && w.capability == action.capability
                && w.sample == *sample
        }) else {
            return not_sent("input_observation_unavailable", pending(), true);
        };
        let reader = &mut self.reader;
        let clock = &self.clock;
        let guard = &mut self.guard;
        let mut fresh_checked = false;
        let mut guard = |child: &InputControl| -> ChannelResult<()> {
            check_control(&child.0)?;
            finish(guard(&child.0), check_control(&child.0))?;
            if !fresh_checked {
                let records = reader.find(
                    &child.0,
                    &input_selector(&action.envelope.runtime)?,
                    None,
                    true,
                )?;
                if records.len() != 1 || records[0].address != witness.address {
                    return Err(pending());
                }
                let current = records[0].contract().ok().and_then(|r| {
                    decode_input(
                        &r,
                        &action.envelope.expected(),
                        action.after_millis,
                        clock.uptime(),
                    )
                    .ok()
                });
                if current.is_none_or(|s| {
                    s.sample_millis != sample.sample_millis
                        || !s.valid_bindings()
                        || s.bindings != sample.bindings
                        || s.input_blocked != Some(false)
                }) {
                    return Err(pending());
                }
                fresh_checked = true;
            }
            // Fixed Go checks the fresh publication immediately before the
            // burst; each following message checks the same CON/control/OS
            // identity. Requiring unchanged INPUT after our function key would
            // reject its own slot progression and obstruct ordinary key-up.
            check_control(&child.0)
        };
        let sent = self
            .sender
            .send(&InputControl(control.clone()), bindings, kind, &mut guard);
        let (receipt, cause) = match sent {
            Ok(r) => (r, None),
            Err(e) => {
                let (r, e) = input_failure(e);
                (r, Some(e))
            }
        };
        let disposition = if cause.is_none() && receipt.submission_complete {
            InputDisposition::Submitted
        } else if receipt.messages_queued == 0 {
            InputDisposition::NotSent
        } else {
            InputDisposition::Uncertain
        };
        let retryable =
            disposition == InputDisposition::NotSent && cause.as_ref().is_some_and(retryable_input);
        InputDispatch {
            outcome: InputOutcome {
                disposition,
                messages_queued: receipt.messages_queued as i32,
                at_millis: self.clock.uptime(),
                reason: cause.as_ref().map(ToString::to_string).unwrap_or_default(),
                retryable,
            },
            error: cause,
        }
    }
}

impl<R: Source, P: Publications, S: Sender, C: Clock> RuntimeRetirementBackend
    for NativeCore<R, P, S, C>
{
    fn observe_runtime_replacement(
        &mut self,
        control: &OperationControl,
        old: &Identity,
    ) -> ChannelResult<Option<RuntimeReplacementProof>> {
        old.validate()
            .map_err(|_| error("live.channel_runtime_replacement_invalid"))?;
        if self.pid == 0 || self.creation == 0 {
            return Err(error("live.channel_runtime_replacement_invalid"));
        }
        self.reader.require_open()?;
        let bounded = control
            .ordered_child(self.clock.now() + REPLACEMENT_LIMIT)
            .map_err(toolkit_error)?;
        let (current, before) = self.replacement_read(&bounded)?;
        let Some(first) = replacement_sample(old, &current, &before, self.clock.uptime()) else {
            return Ok(None);
        };
        self.clock
            .wait(&bounded, REPLACEMENT_PAUSE)
            .map_err(|e| join(pending(), e))?;
        let (next, after) = self.replacement_read(&bounded)?;
        let Some(latest) = replacement_sample(old, &next, &after, self.clock.uptime()) else {
            return Ok(None);
        };
        if next.runtime != current.runtime
            || next.guid != current.guid
            || next.owner != current.owner
            || next.fence != current.fence
            || next.next_slot != current.next_slot
            || latest.sample_millis <= first.sample_millis
            || after.sequence <= before.sequence
        {
            return Ok(None);
        }
        let before_wire = crate::mailbox::encode(&before).map_err(external)?;
        let after_wire = crate::mailbox::encode(&after).map_err(external)?;
        let proof = RuntimeReplacementProof {
            schema: SCHEMA.into(),
            process_id: self.pid,
            process_started_at: self.creation,
            current: next,
            witness: RuntimeReplacementWitness {
                first,
                first_sequence: before.sequence,
                address: after.address,
                length: after_wire.len() as u32,
                before_sha256: storage::sha256(&before_wire),
                after_sha256: storage::sha256(&after_wire),
                sequence: after.sequence,
                observation: latest,
            },
        };
        check_control(&bounded)?;
        proof.validate(old)?;
        Ok(Some(proof))
    }
    fn retire_replaced_runtime(
        &mut self,
        control: &OperationControl,
        envelope: &SlotEnvelope,
        proof: &RuntimeReplacementProof,
    ) -> ChannelResult<()> {
        if proof.process_id != self.pid
            || proof.process_started_at != self.creation
            || proof.current.runtime == envelope.runtime
            || proof.current.build != envelope.build
            || proof.current.release != self.release
        {
            return Err(error("live.channel_runtime_retirement_invalid"));
        }
        // proof.validate(old), exact target comparison and durable proof save are
        // the driver's boundary. This historical proof must survive later reload.
        self.check(control)?;
        let mut lease = self.publications.acquire(control)?;
        let result = (|| {
            self.check(control)?;
            lease.retire(&self.consumer, envelope, &proof.current.runtime)
        })();
        finish(result, lease.close())
    }
}
fn replacement_sample(
    old: &Identity,
    current: &Identity,
    record: &Record,
    now: i64,
) -> Option<InputObservation> {
    if current.validate().is_err()
        || current.input_state != SCHEMA
        || current.runtime == old.runtime
        || current.build != old.build
        || current.product != old.product
        || current.release != old.release
    {
        return None;
    }
    let envelope = SlotEnvelope {
        action: "reload".into(),
        runtime: current.runtime.clone(),
        owner: current.owner.clone(),
        fence: current.fence,
        index: current.next_slot,
        guid: current.guid.clone(),
        build: current.build.clone(),
        ..SlotEnvelope::default()
    };
    let observation =
        decode_input(&record.contract().ok()?, &envelope.expected(), -100, now).ok()?;
    if observation.schema != SCHEMA
        || observation.owner != current.owner
        || observation.fence != current.fence
        || observation.next_slot != current.next_slot
        || observation.sample_millis as u64 > super::MAX_EXACT_INTEGER
        || !(1..=SLOT_COUNT + 1).contains(&observation.next_slot)
        || observation.guid.is_empty()
        || observation.build.is_empty()
        || (if observation.owner.is_empty() {
            observation.fence != 0
        } else {
            parse_token(&observation.owner).map_or(true, |owner| owner == ZERO_TOKEN)
                || observation.fence == 0
                || observation.fence > super::MAX_EXACT_INTEGER
        })
    {
        return None;
    }
    Some(observation)
}

impl Backend for NativeBackend {
    fn observe(&mut self, c: &OperationControl, q: &ObservationQuery) -> ObservationDispatch {
        self.core.observe(c, q)
    }
    fn publish(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.core.publish(c, e)
    }
    fn observe_input(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        a: i64,
        cap: &str,
    ) -> ChannelResult<InputObservation> {
        self.core.observe_input(c, e, a, cap)
    }
    fn input(&mut self, c: &OperationControl, a: &InputAction) -> InputDispatch {
        self.core.input(c, a)
    }
    fn runtime_candidate(
        &mut self,
        c: &OperationControl,
        i: &Identity,
    ) -> ChannelResult<Option<Identity>> {
        self.core.runtime_candidate(c, i)
    }
    fn supersede(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        i: &Identity,
    ) -> ChannelResult<()> {
        self.core.supersede(c, e, i)
    }
    fn consumed(&mut self, c: &OperationControl, e: &SlotEnvelope) -> ChannelResult<()> {
        self.core.consumed(c, e)
    }
}
impl RuntimeRetirementBackend for NativeBackend {
    fn observe_runtime_replacement(
        &mut self,
        c: &OperationControl,
        old: &Identity,
    ) -> ChannelResult<Option<RuntimeReplacementProof>> {
        self.core.observe_runtime_replacement(c, old)
    }
    fn retire_replaced_runtime(
        &mut self,
        c: &OperationControl,
        e: &SlotEnvelope,
        p: &RuntimeReplacementProof,
    ) -> ChannelResult<()> {
        self.core.retire_replaced_runtime(c, e, p)
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
mod toolkit_error_tests {
    use super::*;
    #[test]
    fn application_error_keeps_exit_details_and_ordered_control() {
        let details = serde_json::json!({"installation":"selected","reason":"modified"});
        let mapped = toolkit_error(
            lycheedev_core::ToolkitError::new("delivery.invalid_payload", "bad digest", 4)
                .with_details(details.clone()),
        );
        assert!(
            matches!(mapped, ChannelError::Toolkit { code, exit_code:4, details:actual, .. } if code == "delivery.invalid_payload" && actual == details)
        );
        assert_eq!(
            toolkit_error(lycheedev_core::ToolkitError::new(
                "command.cancelled",
                "stop",
                7
            )),
            ChannelError::Cancelled
        );
    }
}

#[path = "activation_backend.rs"]
mod activation_backend;
