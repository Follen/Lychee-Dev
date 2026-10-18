//! The single fixed Go323 JSON boundary for frames, manifest and Outcome.
//! Keep the existing Value/public_json route: direct serde streaming would
//! change object-key ordering and some numeric token/escaping semantics.
use super::{StreamBegin, StreamEvent, StreamSummary, TableProof, WalkSummary};
use lycheedev_core::{Outcome, ToolkitError, selection::PinnedSet};
use serde_json::{Value, json};

fn proof(value: &TableProof) -> Value {
    let mut proof = json!({"contentVerified":value.content_verified,"keySource":value.key_source,"name":value.requested_name,"identity":value.identity,"layoutHash":value.layout_hash,"definitionSHA256":value.definition_sha256,"fileDataID":value.identity.db2_file_data_id,"contentKey":value.content_key,"logicalRows":value.logical_rows});
    if !value.content_verified {
        proof["partialContent"] = value.partial_content.clone();
        proof["missing"] = value.missing.clone();
        proof["unavailablePartitions"] = value.unavailable_partitions.clone();
    }
    proof
}
fn begin(value: &StreamBegin) -> Value {
    json!({"tables":[proof(&value.proof)],"snapshot":value.snapshot,"source":value.source,"table":value.proof.identity.name,"identity":value.proof.identity,"layoutHash":value.proof.layout_hash,"fields":value.projection.fields(),"filter":value.filter,"limit":value.limit.get()})
}
fn end(value: WalkSummary) -> Value {
    json!({"partial":value.partial,"partialReasons":if value.partial { json!(["encrypted_sections_unavailable"]) } else { Value::Null },"count":value.count,"complete":value.complete(),"truncated":value.truncated,"truncationReason":value.truncation_reason()})
}
pub fn event(value: StreamEvent<'_>) -> Value {
    match value {
        StreamEvent::Begin(value) => json!({"frame":"begin","begin":begin(value)}),
        StreamEvent::Record(value) => {
            json!({"frame":"record","record":{"index":value.index,"recordID":value.record_id,"row":value.row}})
        }
        StreamEvent::End(value) => json!({"frame":"end","end":end(value)}),
        StreamEvent::Error { code, message } => {
            json!({"frame":"error","error":{"code":code,"message":message}})
        }
    }
}
pub fn into_outcome(summary: StreamSummary) -> Outcome {
    let begin = summary.begin;
    let walk = summary.walk;
    let data = json!({"resources":summary.resources,"memoryBudget":summary.memory_budget,"snapshot":begin.snapshot,"pin":summary.pin,"source":begin.source,"tables":[proof(&begin.proof)],"relations":[],"complete":walk.complete(),"truncated":walk.truncated,"truncationReason":walk.truncation_reason(),"partial":walk.partial,"partialReasons":if walk.partial { json!(["encrypted_sections_unavailable"]) } else { Value::Null },"table":begin.proof.identity.name,"mode":"stream","limit":begin.limit.get(),"fields":begin.projection.fields(),"filter":begin.filter,"count":walk.count,"resourceAccounting":{"wholeQueryAccountingComplete":false,"used":summary.resource_usage}});
    let mut outcome = Outcome::new(data);
    outcome.complete = walk.complete();
    outcome.provenance = summary.captures;
    outcome
}
pub(super) fn manifest(begin_value: &StreamBegin, summary: WalkSummary) -> Value {
    json!({"begin":begin(begin_value),"end":end(summary)})
}
pub(super) fn capture_provenance(name: &str, pin: &PinnedSet) -> Value {
    json!({"kind":"data-stream-manifest","locator":format!("db2:{name}:stream"),"snapshot":pin.id,"dataBuild":pin.data.as_ref().expect("prepare checked DataPin").full_build})
}
pub(super) fn query_stage() -> Value {
    json!({"stage":"query"})
}
pub(super) fn with_committed_captures(
    mut error: ToolkitError,
    captures: Vec<Value>,
) -> ToolkitError {
    let primary = error.clone();
    let ids = captures
        .iter()
        .filter_map(|capture| capture["id"].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    error.message = format!(
        "{}\nstream manifest already committed: {ids}",
        error.message
    );
    error.details = json!({"schema":"lycheedev.stream-committed-error.v1", "primary":primary, "completedCaptures":captures});
    error
}
pub(super) fn with_marker_error(
    mut error: ToolkitError,
    frame_error: ToolkitError,
) -> ToolkitError {
    error.details =
        json!({"schema":"lycheedev.stream-errors.v1", "causes":[error.clone(), frame_error]});
    error
}
