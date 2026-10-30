//! Included as connection::doctor_delivery. Real Delivery bridge; this keeps
//! the Live admission dependency out of Delivery and joins every close error.
#![forbid(unsafe_code)]
use super::doctor_repair::RepairDelivery;
use super::*;
use crate::mailbox::{admission::acquire_installation_maintenance, doctor::DoctorDeployment};

pub struct ManagedDoctorDelivery<V> {
    release: String,
    validator: V,
}
impl<V: lycheedev_delivery::formal_repair::AddonValidator> ManagedDoctorDelivery<V> {
    pub fn new(current_release: &str, validator: V) -> ChannelResult<Self> {
        if current_release.is_empty() {
            return Err(error("live.channel_release_required"));
        }
        Ok(Self {
            release: current_release.into(),
            validator,
        })
    }
}
impl<V: lycheedev_delivery::formal_repair::AddonValidator> RepairDelivery
    for ManagedDoctorDelivery<V>
{
    fn reset_slots(
        &mut self,
        target: &ConnectionTarget,
        archive: &Path,
        version: &str,
        control: &OperationControl,
    ) -> ChannelResult<bool> {
        // The diagnostic owner is only an observation. Recheck every shared
        // installation CON under the formal maintenance gate before Delivery.
        let mut admission = acquire_installation_maintenance(&target.addon_parent(), control)?;
        let reset = lycheedev_delivery::slot_reset::reset(
            Path::new(&target.client.directory),
            archive,
            version,
            control,
        )
        .map(|result| result.complete)
        .map_err(durable_journal::storage_error);
        match (reset, admission.close()) {
            (Ok(complete), Ok(())) => Ok(complete),
            (Err(a), Err(b)) => Err(ChannelError::Joined { causes: vec![a, b] }),
            (Err(error), _) | (_, Err(error)) => Err(error),
        }
    }
    fn apply(
        &mut self,
        target: &ConnectionTarget,
        _deployment: &DoctorDeployment,
        release: &Path,
        archive: Option<&Path>,
        control: &OperationControl,
    ) -> ChannelResult<()> {
        check_control(control)?;
        let client = &target.client;
        let selected = lycheedev_core::installation_selection::Client {
            directory: client.directory.clone(),
            product: client.product.clone(),
            product_code: client.product_code.clone(),
            catalog_product: client.catalog_product.clone(),
            full_build: client.full_build.clone(),
            interface: u32::try_from(client.interface)
                .map_err(|_| error("live.channel_target_mismatch"))?,
            toc: client.toc.clone(),
            identity_source: client.identity_source.clone(),
        };
        // Read-only report is not write authority. This exact shared gate checks
        // all current CON claims again after the original connection retired.
        let mut admission = acquire_installation_maintenance(&target.addon_parent(), control)?;
        let repaired = lycheedev_delivery::formal_repair::apply(
            &selected,
            release,
            archive,
            &self.release,
            &mut self.validator,
            control,
        )
        .map(|_| ())
        .map_err(durable_journal::storage_error);
        durable_journal::joined([repaired, admission.close()])
    }
}
