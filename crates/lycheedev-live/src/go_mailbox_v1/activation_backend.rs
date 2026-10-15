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
        if identity.input_state != PREVIOUS_SCHEMA {
            return Ok(identity.input_state == SCHEMA);
        }
        self.core.check(control)?;
        let records =
            self.core
                .reader
                .find(control, &input_selector(&identity.runtime)?, None, true)?;
        self.core.check(control)?;
        if records.len() != 1 {
            return Ok(false);
        }
        let envelope = SlotEnvelope {
            runtime: identity.runtime.clone(),
            owner: identity.owner.clone(),
            fence: identity.fence,
            index: identity.next_slot,
            guid: identity.guid.clone(),
            build: identity.build.clone(),
            action: "reload".into(),
            ..Default::default()
        };
        let record = records[0].contract().map_err(external)?;
        // Invalid/three-key profiles cannot authorize automatic migration.
        Ok(
            inspect_input(&record, &envelope.expected(), 0, self.core.clock.uptime())
                .is_ok_and(|sample| sample.has_two_key_reload_capability()),
        )
    }
}
