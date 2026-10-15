//! Included as connection::native::activation_native. Uses the current native
//! reader/backend and existing deployment/target guard, never old Transport.
#![forbid(unsafe_code)]
use super::*;
use crate::go_mailbox_v1::connection::activation_service::{
    ActivationBackend, ActivationEnvironment,
};
impl<L: ProcessLifetimeObserver> ActivationEnvironment for NativeEnvironment<L> {
    fn guard_activation(
        &mut self,
        target: &ConnectionTarget,
        control: &OperationControl,
    ) -> ChannelResult<()> {
        deployment(target, &self.release, control).map(|_| ())
    }
}
impl ActivationBackend for NativeConnectionBackend {
    fn activation_capability(
        &mut self,
        identity: &Identity,
        control: &OperationControl,
    ) -> ChannelResult<bool> {
        self.inner.activation_capability(identity, control)
    }
}
