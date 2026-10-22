//! Included as native_backend::activation_backend. Reads the same authenticated
//! reader already held by this backend; no extra locator scan or input path.
#![forbid(unsafe_code)]
use super::*;
impl NativeBackend {
    pub fn activation_capability(
        &mut self,
        identity: &Identity,
        control: &OperationControl,
    ) -> ChannelResult<bool> {
        self.core.check(control)?;
        Ok(identity.input_state == SCHEMA)
    }
}
