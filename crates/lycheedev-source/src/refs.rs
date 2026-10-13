//! Bounded fixed-source reference research with preserved coverage and original evidence.
use crate::{SourceTools, relations};
use lycheedev_core::{Invocation, Outcome, Result, evidence};
use serde_json::json;
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    dispatch_with_tools(inv, SourceTools::default())
}
pub fn dispatch_with_tools(inv: &Invocation, tools: SourceTools<'_>) -> Result<Outcome> {
    dispatch_with_services(inv, tools, None)
}
pub fn dispatch_with_services(
    inv: &Invocation,
    tools: SourceTools<'_>,
    broker: Option<&crate::session::BrokerIdentity>,
) -> Result<Outcome> {
    let reading = relations::with_native_services(inv, &tools, broker)?;
    let complete = reading.data["complete"] == true;
    let source = reading.pin.source.as_ref().unwrap();
    let capture = evidence::commit_capture(
        &inv.home,
        &serde_json::to_vec(&reading.data)?,
        "application/json",
        json!({"kind":"source-refs","locator":inv.flag("--symbol-id").or(inv.flag("--symbol")).unwrap(),"snapshot":reading.pin.id,"sourceCommit":source.exact_commit}),
        complete,
        reading.data["truncated"] == true,
    )?;
    let mut result = Outcome::new(reading.data);
    result.complete = complete;
    result.coverage = result.data["coverage"].clone();
    result.selection = json!({"snapshot":reading.pin.id});
    result.provenance.push(capture);
    Ok(result)
}
