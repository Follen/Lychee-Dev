//! Narrow original-CON unbind publication. The ordinary NativeCore still owns
//! fresh INPUT, one-use witnesses, same-lease verification and the entire burst.
#![forbid(unsafe_code)]
use super::*;
use lycheedev_delivery::slot_publication::RepairPublication;
use std::rc::Rc;

type Authenticate = dyn Fn(&SlotEnvelope, &OperationControl) -> ChannelResult<RepairPublication>;
#[derive(Clone)]
pub(super) struct RepairAuthorization {
    pub(super) consumer: String,
    authenticate: Rc<Authenticate>,
}
impl RepairAuthorization {
    pub(super) fn authorize(
        &self,
        consumer: &str,
        envelope: &SlotEnvelope,
        control: &OperationControl,
    ) -> ChannelResult<RepairPublication> {
        check_control(control)?;
        if envelope.action != "unbind" || consumer != self.consumer {
            return Err(error("live.channel_repair_publication_invalid"));
        }
        // Re-read the original durable close intent and exact unbind while the
        // caller holds its CON execution lease. No cached proof widens authority.
        let capability = (self.authenticate)(envelope, control)?;
        if capability.consumer() != consumer || capability.envelope() != envelope {
            return Err(error("live.channel_repair_publication_invalid"));
        }
        check_control(control)?;
        Ok(capability)
    }
}
impl DiskPublications {
    pub(in super::super) fn for_repair(
        addon: PathBuf,
        release: &str,
        consumer: String,
        authenticate: impl Fn(&SlotEnvelope, &OperationControl) -> ChannelResult<RepairPublication>
        + 'static,
    ) -> ChannelResult<Self> {
        let mut store = Self::recorded(addon, release)?;
        store.repair = Some(RepairAuthorization {
            consumer,
            authenticate: Rc::new(authenticate),
        });
        Ok(store)
    }
}
impl NativeBackend {
    /// The caller must hold the original CON execution lease throughout this
    /// backend's life. `authenticate` must read its authenticated journal under
    /// the SAME control, require durable closing and exact current unbind, and
    /// derive the capability from that original target/release/consumer only.
    /// It is called at publication and pre-burst verification, not at open:
    /// RequestClose and the unbind transaction are persisted after native open.
    pub(crate) fn open_for_repair(
        target: MailboxWindow,
        authenticated_addon: PathBuf,
        release: &str,
        trace_directory: Option<PathBuf>,
        guard: impl FnMut(&OperationControl) -> ChannelResult<()> + 'static,
        authenticate: impl Fn(&SlotEnvelope, &OperationControl) -> ChannelResult<RepairPublication>
        + 'static,
        control: &OperationControl,
    ) -> ChannelResult<Self> {
        let window = target.identity();
        let store = DiskPublications::for_repair(
            authenticated_addon,
            release,
            format!("{}/{}", window.process.pid, window.process.created),
            authenticate,
        )?;
        Self::open_with_store(
            target,
            store,
            release,
            trace_directory,
            Box::new(guard),
            control,
        )
    }
}
