//! Retention records use the same current typed blob identity as their capture.
use super::*;
use crate::storage;
use serde_json::Value;
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct Retention { schema:String,capture_id:String,blob:super::super::BlobRef,kept_at:String }
pub(in crate::evidence) fn new_record(id:&str,capture:&CaptureRef,control:&OperationControl)->Result<Vec<u8>> {
    control.check()?;
    storage::identity_json(&Retention{schema:"lycheedev.evidence-retention.v1".into(),capture_id:id.into(),blob:capture.blob.clone(),kept_at:storage::utc_now()})
}
pub(in crate::evidence) fn decode_retention(raw:&[u8],capture:&CaptureRef,control:&OperationControl)->Result<Value> {
    let record=checked_record(raw,capture,control)?;
    let result=serde_json::to_value(record).map_err(Into::into);
    crate::selection::selection_finish(result,control.check())
}
pub(in crate::evidence) fn validate_removal_retention(raw:&[u8],capture:&CaptureRef,control:&OperationControl)->Result<()> { checked_record(raw,capture,control).map(|_|()) }
fn checked_record(raw:&[u8],capture:&CaptureRef,control:&OperationControl)->Result<Retention> {
    control.check()?;
    let record:Retention=serde_json::from_slice(raw)?;
    if record.schema!="lycheedev.evidence-retention.v1" || record.capture_id!=capture.id || record.blob.sha256!=capture.blob.sha256 || record.blob.bytes!=capture.blob.bytes || storage::parse_timestamp(&record.kept_at).is_none() {
        return Err(ToolkitError::new("evidence.invalid_retention","Retention identity differs",4));
    }
    crate::selection::selection_finish(Ok(record),control.check())
}
