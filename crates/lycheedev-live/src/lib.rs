#![forbid(unsafe_code)]
//! Native Mailbox connections, durable session ownership and runtime-root reads.
#[cfg(test)]
extern crate self as lycheedev_live;

pub mod lua_layout;
pub mod mailbox;
pub mod passive;
pub mod probes;
pub mod request;
pub mod runtime_binding;
pub mod runtime_root;
use lycheedev_core::{Outcome, Result};
use request::{LiveContext, LiveRequest};
use serde_json::json;
pub fn capabilities() -> serde_json::Value {
    json!({"protocol":"Lycheedev mailbox protocol v1","slots":mailbox::SLOT_COUNT,"realClientAcceptance":"not_run","durableIntent":true,"unknownInputReplay":false})
}
pub fn dispatch(ctx: &LiveContext, request: &LiveRequest) -> Result<Outcome> {
    match request {
        LiveRequest::Instances(request) => {
            let report = passive::discover(ctx, request, &passive::WindowsSource)?;
            Ok(Outcome::new(serde_json::to_value(report)?))
        }
        LiveRequest::Probe(request) => probes::dispatch(ctx, request),
    }
}
