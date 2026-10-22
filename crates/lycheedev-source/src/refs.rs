//! Bounded fixed-source reference research with preserved coverage and original evidence.
use crate::{
    SourceTools, relations,
    request::{RelationRequest, ResearchMode, ResearchSelection, SourceContext},
};
use lycheedev_core::{Outcome, Result, evidence};
use serde_json::json;
pub fn dispatch(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    request: &RelationRequest,
    mode: ResearchMode,
) -> Result<Outcome> {
    dispatch_with_tools(ctx, selection, request, mode, SourceTools::default())
}
pub fn dispatch_with_tools(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    request: &RelationRequest,
    mode: ResearchMode,
    tools: SourceTools<'_>,
) -> Result<Outcome> {
    let reading = relations::with_tools(ctx, selection, request, mode, &tools)?;
    let complete = reading.data["complete"] == true;
    let source = reading.pin.source.as_ref().unwrap();
    let bytes = crate::research_material::measured("source.refs.serialize_capture", || {
        Ok(serde_json::to_vec(&reading.data)?)
    })?;
    let capture = crate::research_material::measured("source.refs.commit_capture", || {
        evidence::commit_capture_bounded(
            &ctx.home,
            lycheedev_core::evidence::CaptureInput {
                bytes: &bytes,
                maximum_bytes: lycheedev_core::evidence::DEFAULT_CAPTURE_BYTES,
                media_type: "application/json",
                provenance: json!({"kind":"source-refs","locator":request.selector.value(),"snapshot":reading.pin.id,"sourceCommit":source.exact_commit}),
                complete: complete,
                truncated: reading.data["truncated"] == true,
            },
            &ctx.control,
        )
    })?;
    drop(bytes);
    let mut result = Outcome::new(reading.data);
    result.complete = complete;
    result.coverage = result.data["coverage"].clone();
    result.selection = json!({"snapshot":reading.pin.id});
    result.provenance.push(capture);
    Ok(result)
}
