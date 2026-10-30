//! Included as native_backend::activation_backend. Reads the same authenticated
//! reader already held by this backend; no extra locator scan or input path.
#![forbid(unsafe_code)]
use super::*;

pub(crate) fn activation_input_capability<R: RecordReader + ?Sized>(
    reader: &mut R,
    identity: &Identity,
    now: i64,
    control: &OperationControl,
) -> ChannelResult<bool> {
    check_control(control)?;
    if identity.input_state != PREVIOUS_SCHEMA {
        return Ok(identity.input_state == SCHEMA);
    }
    let records = reader.find(control, &input_selector(&identity.runtime)?, None, true)?;
    check_control(control)?;
    let Some(record) = records.first().filter(|_| records.len() == 1) else {
        return Ok(false);
    };
    let envelope = SlotEnvelope {
        action: "reload".into(),
        runtime: identity.runtime.clone(),
        owner: identity.owner.clone(),
        fence: identity.fence,
        index: identity.next_slot,
        guid: identity.guid.clone(),
        build: identity.build.clone(),
        ..SlotEnvelope::default()
    };
    // A stale valid two-key profile proves upgrade capability only. Every
    // actual reload independently requires fresh INPUT and native guards.
    Ok(record.contract().ok().is_some_and(|record| {
        inspect_input(&record, &envelope.expected(), 0, now).is_ok_and(|sample| {
            sample
                .observation
                .bindings
                .as_ref()
                .is_some_and(|b| !b.reload.is_empty())
        })
    }))
}

impl NativeBackend {
    pub fn activation_capability(
        &mut self,
        identity: &Identity,
        control: &OperationControl,
    ) -> ChannelResult<bool> {
        self.core.check(control)?;
        activation_input_capability(
            &mut self.core.reader,
            identity,
            self.core.clock.uptime(),
            control,
        )
    }
}
