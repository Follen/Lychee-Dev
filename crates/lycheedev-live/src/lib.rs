//! Unified native lifecycle, bounded route ledger and independent static locator.
pub mod binding;
pub mod host;
pub mod journal;
pub mod lifecycle;
pub mod locator;
pub mod mailbox;
pub mod managed;
pub mod native;
pub mod optical;
pub mod passive;
pub mod probes;
pub mod replacement;
pub mod routes;
pub mod runtime_archive;
pub mod runtime_binding;
pub mod runtime_discovery;
pub mod runtime_observation;
pub mod runtime_root;
pub mod runtime_windows;
pub mod slots;
pub mod transport;
pub mod windows_channel;
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError};
use serde_json::json;
pub use runtime_observation::{observe_runtime, RuntimeObservationFailure, RuntimeTargetObservation};
pub const SLOT_COUNT: u16 = 500;
pub const SLOT_TRANSPORT: &str = "memory-slot-v4";
pub const ENVELOPE_SCHEMA: &str = "lycheedev.slot.v4";
pub const MAILBOX_SCHEMA: &str = "lycheedev.mailbox.v2";
pub fn commands() -> &'static [&'static str] {
    &[
        "live probe put",
        "live probe list",
        "live probe show",
        "live probe remove",
        "live instances",
        "live connect",
        "live execute",
        "live bugs",
        "live status",
        "live session",
        "live resume",
        "live disconnect",
        "live reload",
        "live probe load",
        "live run",
        "live ack",
        "live finish",
        "live cancel",
        "live abandon",
    ]
}
pub fn capabilities() -> serde_json::Value {
    json!({"protocol":ENVELOPE_SCHEMA,"slots":SLOT_COUNT,"maximumInstances":slots::MAX_INSTANCES,"bootstrapSlots":slots::BOOTSTRAP_SLOTS,"slotsPerInstance":slots::LANE_SLOTS,"normalTasksPerRuntime":28,"staticProductionFinalization":"not_finalized","realClientAcceptance":"not_run","reloadAdmission":"fixed selected lifecycle; unread results retain executionUnknown; real-client acceptance not_run"})
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    if inv.command == "live instances" {
        let report = passive::discover(inv, &passive::WindowsSource)?;
        return Ok(Outcome::new(serde_json::to_value(report)?));
    }
    if [
        "live probe put",
        "live probe list",
        "live probe show",
        "live probe remove",
    ]
    .contains(&inv.command.as_str())
    {
        return probes::dispatch(inv);
    }
    if commands().contains(&inv.command.as_str()) {
        return Err(ToolkitError::new(
            "live.services_required",
            "native command requires an injected managed installation and verified Windows services",
            3,
        ));
    }
    Err(ToolkitError {
        code: "live.migration_incomplete".into(),
        message: format!(
            "Rust lifecycle adapter for {} has not been integrated; no input was sent",
            inv.command
        ),
        exit_code: 3,
        details: json!({"state":"not_run","goFallback":false}),
    })
}
