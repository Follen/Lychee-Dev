//! Included as connection::doctor_delivery. Real Delivery bridge; this keeps
//! the Live admission dependency out of Delivery and joins every close error.
#![forbid(unsafe_code)]
use super::doctor_repair::RepairDelivery;
use super::*;
use crate::go_mailbox_v1::{admission::acquire_installation_maintenance, doctor::DoctorDeployment};

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
