//! Thin safe production adapters; all Windows FFI remains in Windows crate.
use super::*;
mod repair;
use lycheedev_delivery::slot_publication::{
    PublicationError, PublicationLease, RecordedInstallationIdentity, next_free_slot,
};
use lycheedev_windows::mailbox_input::{send_mailbox_action, uptime_millis};
use repair::RepairAuthorization;

pub(super) struct DiskPublications {
    identity: RecordedInstallationIdentity,
    addon: PathBuf,
    parent: String,
    release: String,
    repair: Option<RepairAuthorization>,
}
impl DiskPublications {
    pub(super) fn verified(installation: &VerifiedInstallation, release: &str) -> Self {
        Self {
            identity: installation.into(),
            addon: installation.canonical_path.clone(),
            parent: installation
                .canonical_path
                .parent()
                .unwrap_or(&installation.canonical_path)
                .display()
                .to_string(),
            release: release.into(),
            repair: None,
        }
    }
    pub(super) fn recorded(path: PathBuf, release: &str) -> ChannelResult<Self> {
        let parent = path.parent().unwrap_or(&path).display().to_string();
        Ok(Self {
            addon: path.clone(),
            identity: RecordedInstallationIdentity::from_authenticated_record(path)
                .map_err(publication_error)?,
            parent,
            release: release.into(),
            repair: None,
        })
    }
}
struct DiskLease<'a> {
    lease: PublicationLease<'a>,
    parent: String,
    repair: Option<RepairAuthorization>,
    control: &'a OperationControl,
}
impl Publications for DiskPublications {
    fn acquire<'a>(
        &self,
        control: &'a OperationControl,
    ) -> ChannelResult<Box<dyn Publication + 'a>> {
        self.acquire_identity(&self.identity, control)
    }
    fn acquire_for<'a>(
        &self,
        control: &'a OperationControl,
        envelope: &SlotEnvelope,
    ) -> ChannelResult<Box<dyn Publication + 'a>> {
        if self.repair.is_none() || envelope.action == "unbind" {
            return self.acquire(control);
        }
        // A recorded path alone cannot authorize ordinary publication. Obtain
        // the original release's clean receipt before the short lease; its
        // digest is rechecked under that SAME lease before publication/burst.
        let verified = lycheedev_delivery::installation::verify_main_addon_controlled(
            &self.addon,
            &self.release,
            control,
        )
        .map_err(toolkit_error)?;
        if verified.canonical_path != self.addon {
            return Err(error("live.channel_repair_publication_invalid"));
        }
        self.acquire_identity(&RecordedInstallationIdentity::from(&verified), control)
    }
}
impl DiskPublications {
    fn acquire_identity<'a>(
        &self,
        identity: &RecordedInstallationIdentity,
        control: &'a OperationControl,
    ) -> ChannelResult<Box<dyn Publication + 'a>> {
        match PublicationLease::acquire_recorded(identity, &self.release, control) {
            Ok(lease) => Ok(Box::new(DiskLease {
                lease,
                parent: self.parent.clone(),
                repair: self.repair.clone(),
                control,
            })),
            Err(cause) => {
                let cause = publication_error(cause);
                if let Err(stop) = check_control(control) {
                    let cause = if cause.any_cause(&|leaf| leaf == &stop) {
                        cause
                    } else {
                        join(cause, stop)
                    };
                    Err(join(
                        ChannelError::Blocked(Blocker {
                            kind: "publication_lock".into(),
                            installation: self.parent.clone(),
                            condition: "publication_lease_released".into(),
                            ..Blocker::default()
                        }),
                        cause,
                    ))
                } else {
                    Err(cause)
                }
            }
        }
    }
}
impl Publication for DiskLease<'_> {
    fn inspect(&mut self) -> ChannelResult<SlotPool> {
        self.lease.inspect().map_err(publication_error)
    }
    fn publish(&mut self, consumer: &str, envelope: &SlotEnvelope) -> ChannelResult<()> {
        let result = match &self.repair {
            Some(authority) if envelope.action == "unbind" => {
                let capability = authority.authorize(consumer, envelope, self.control)?;
                self.lease.publish_repair(&capability)
            }
            _ => self.lease.publish(consumer, envelope),
        };
        match result {
            Err(PublicationError::Reserved { reservation }) => {
                // Both exact reservation and forward hint remain under this
                // same lease; no post-unlock attribution to a later owner.
                let pool = self.lease.inspect().map_err(publication_error)?;
                let mut blocker = Blocker {
                    kind: "slot_reservation".into(),
                    installation: self.parent.clone(),
                    slot: envelope.index,
                    consumer: reservation.consumer,
                    runtime: reservation.runtime,
                    nonce: reservation.nonce,
                    condition: "exact_consumption_or_retirement_evidence".into(),
                    ..Blocker::default()
                };
                if envelope.schema == SCHEMA {
                    if let Some(index) = next_free_slot(&pool, envelope.index)
                        .filter(|next| envelope.action != "prepare" || *next <= SLOT_COUNT - 4)
                    {
                        return Err(ChannelError::SlotAvailable { index, blocker });
                    }
                    blocker.kind = "slot_pool_full".into();
                    blocker.condition = "forward_slot_available".into();
                }
                Err(ChannelError::Blocked(blocker))
            }
            result => result.map_err(publication_error),
        }
    }
    fn verify(&mut self, consumer: &str, envelope: &SlotEnvelope) -> ChannelResult<()> {
        if let Some(authority) = self.repair.as_ref().filter(|_| envelope.action == "unbind") {
            let capability = authority.authorize(consumer, envelope, self.control)?;
            self.lease
                .verify_repair(&capability)
                .map_err(publication_error)
        } else {
            self.lease
                .verify(consumer, envelope)
                .map_err(publication_error)
        }
    }
    fn verify_route(&mut self, envelope: &SlotEnvelope) -> ChannelResult<()> {
        if let Some(authority) = self.repair.as_ref().filter(|_| envelope.action == "unbind") {
            let capability = authority.authorize(&authority.consumer, envelope, self.control)?;
            self.lease
                .verify_repair_route(&capability)
                .map_err(publication_error)
        } else {
            self.lease.verify_route(envelope).map_err(publication_error)
        }
    }
    fn consumed(&mut self, consumer: &str, envelope: &SlotEnvelope) -> ChannelResult<()> {
        self.lease
            .confirm_consumed(consumer, &envelope.expected())
            .map_err(publication_error)
    }
    fn retire(
        &mut self,
        consumer: &str,
        envelope: &SlotEnvelope,
        runtime: &str,
    ) -> ChannelResult<()> {
        self.lease
            .retire_runtime(consumer, envelope, runtime)
            .map_err(publication_error)
    }
    fn close(self: Box<Self>) -> ChannelResult<()> {
        self.lease.close().map_err(publication_error)
    }
}
fn publication_error(error: PublicationError) -> ChannelError {
    match error {
        PublicationError::Failure(e) => toolkit_error(e),
        PublicationError::CloseFailures(errors) => ChannelError::Joined {
            causes: errors.into_iter().map(toolkit_error).collect(),
        },
        PublicationError::Reserved { .. } => {
            external("delivery.installation_conflict: delivery.slot_reserved")
        }
    }
}
pub(super) struct NativeSender(pub(super) MailboxWindow);
impl Sender for NativeSender {
    fn send(
        &mut self,
        control: &InputControl,
        bindings: MailboxBindings,
        action: MailboxAction,
        guard: &mut dyn FnMut(&InputControl) -> ChannelResult<()>,
    ) -> Result<InputReceipt, InputFailure<ChannelError>> {
        send_mailbox_action(&self.0, bindings, action, control, guard)
    }
}
pub(super) struct NativeClock;
impl Clock for NativeClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn uptime(&self) -> i64 {
        uptime_millis().min(i64::MAX as u64) as i64
    }
    fn wait(&mut self, control: &OperationControl, duration: Duration) -> ChannelResult<()> {
        let end = Instant::now() + duration;
        loop {
            check_control(control)?;
            let now = Instant::now();
            if now >= end {
                return Ok(());
            }
            std::thread::sleep(
                (end - now)
                    .min(Duration::from_millis(5))
                    .min(control.deadline().saturating_duration_since(now)),
            );
        }
    }
}
impl Source for NativeReader {
    fn evidence(&self) -> ReaderEvidence {
        let usage = self.usage();
        ReaderEvidence {
            usage: [
                usage.os_call_units,
                usage.query_calls,
                usage.read_calls,
                usage.requested_bytes,
                usage.allocation_bytes,
                usage.work_units,
            ],
            first_read_failure: self
                .last_failure()
                .and_then(|e| e.first_read_failure.clone()),
            root: Some(self.root_evidence().clone()),
        }
    }
    fn close(self) -> ChannelResult<()> {
        NativeReader::close(self)
    }
}
/// Same exact transient classifications as NativeReader, also on open failure.
/// Unknown strings never become retryable pending or runtime-destruction proof.
pub(super) fn reader_error(cause: &str) -> ChannelError {
    let mut causes: Vec<_> = cause
        .lines()
        .map(|line| match line {
            "memory.runtime_cancelled" | "command.cancelled" => ChannelError::Cancelled,
            "memory.runtime_deadline" | "command.deadline" => ChannelError::DeadlineExceeded,
            "mailbox.path_changed" => error("memory.mailbox_unavailable: path_changed"),
            "mailbox.lua_state_null" => error("memory.mailbox_unavailable: lua_state_null"),
            "mailbox.string_layout" => error("memory.mailbox_unavailable: string_layout"),
            _ => external(line),
        })
        .collect();
    match causes.len() {
        0 => error("live.channel_reader_error_missing"),
        1 => causes.remove(0),
        _ => ChannelError::Joined { causes },
    }
}
