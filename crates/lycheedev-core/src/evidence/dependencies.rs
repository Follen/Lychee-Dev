//! Current typed capture identities and explicit dependency references.
use super::{CaptureRef, bounded};
use crate::{Result, ToolkitError, control::OperationControl};
use std::path::Path;

pub(super) fn finish_capture_checks<T>(result:Result<T>,control:&OperationControl,check:&mut impl FnMut()->Result<()>)->Result<T> {
    let result=crate::selection::selection_finish(result,control.check());
    crate::selection::selection_finish(result,check())
}
pub(super) fn decode_capture_ref(raw:&[u8],control:&OperationControl)->Result<CaptureRef> {
    control.check()?;
    let parsed=serde_json::from_slice(raw).map_err(Into::into);
    crate::selection::selection_finish(parsed,control.check())
}
pub(super) fn inspect_capture_ref(raw:&[u8],id:&str,control:&OperationControl)->Result<CaptureRef> {
    inspect_capture_ref_with_check(raw,id,control,&mut ||Ok(()))
}
pub(super) fn inspect_capture_ref_with_check(raw:&[u8],id:&str,control:&OperationControl,check:&mut impl FnMut()->Result<()>)->Result<CaptureRef> {
    control.check()?;check()?;
    let capture=decode_capture_ref(raw,control)?;
    super::validate_capture(&capture,id)?;
    finish_capture_checks(Ok(capture),control,check)
}
pub(super) fn references_capture_raw(home:&Path,key:&str,manifest:&[u8],target:&CaptureRef,control:&OperationControl)->Result<bool> {
    control.check()?;
    if let Some(id)=key.strip_prefix("capture/") {
        let capture=inspect_capture_ref(manifest,id,control)?;
        if capture.dependencies.contains(&target.id) {return Ok(true);}
        return current_manifest_references(home,&capture,target,control);
    }
    let value:serde_json::Value=serde_json::from_slice(manifest)?;
    let locator=format!("capture:{}",target.id);
    let mut pending=vec![&value];
    while let Some(value)=pending.pop() {
        control.check()?;
        match value {
            serde_json::Value::String(text) if text==&target.id || text==&locator=>return Ok(true),
            serde_json::Value::Array(items)=>pending.extend(items),
            serde_json::Value::Object(fields)=>pending.extend(fields.values()),
            _=>{}
        }
    }
    Ok(false)
}
/// Current typed manifest payloads may name a capture without listing it in the
/// outer capture document's `dependencies`. Demux keeps frame identities in its
/// bounded JSON body, and export repeats artifact/source identities there. A
/// manifest with explicit dependencies is authoritative and its payload is not
/// guessed at, except demux which always retains frame references in the body.
/// Scan only current schemas with the current strict JSON reader: no Go JSON
/// normalization or historical wire tolerance is reintroduced.
fn current_manifest_references(home:&Path,capture:&CaptureRef,target:&CaptureRef,control:&OperationControl)->Result<bool> {
    if capture.media_type!="application/json" {return Ok(false);}
    let kind=capture.provenance.kind.as_str();
    if kind!="asset-demux-manifest" && !capture.dependencies.is_empty() {return Ok(false);}
    let maximum=match (kind,target.provenance.kind.as_str()) {
        ("asset-demux-manifest","asset"|"asset-demux-source"|"asset-demux-frame")=>16usize<<20,
        ("asset-export-manifest","asset-image")=>1usize<<20,
        _=>return Ok(false),
    };
    control.check()?;
    let blob=serde_json::to_value(&capture.blob)?;
    let raw=bounded::read_blob_bounded_with_check(home,&blob,maximum,control,||Ok(()))?;
    control.check()?;
    let value:serde_json::Value=serde_json::from_slice(&raw)?;
    let found=match kind {
        "asset-demux-manifest"=>value["schema"]=="lycheedev.asset-demux.v1"
            && (value["sourceCapture"].as_str()==Some(target.id.as_str())
                || value["frames"].as_array().is_some_and(|frames|frames.iter().any(|frame|frame["capture"].as_str()==Some(target.id.as_str())))),
        "asset-export-manifest"=>value["schema"]=="lycheedev.asset-export.v1"
            && (value["sourceCapture"].as_str()==Some(target.id.as_str())
                || value["artifactCapture"]["id"].as_str()==Some(target.id.as_str())),
        _=>false,
    };
    control.check()?;
    Ok(found)
}
#[derive(serde::Serialize,serde::Deserialize)]
#[serde(rename_all="camelCase",deny_unknown_fields)]
struct WorkspaceIdentity { schema:String, workspace_id:String, created_at:String }
pub(crate) fn decode_workspace_identity(raw:&[u8])->Result<(serde_json::Value,bool)> {
    decode_workspace_identity_with_check(raw,||Ok(()))
}
pub(crate) fn decode_workspace_identity_with_check(raw:&[u8],mut check:impl FnMut()->Result<()>)->Result<(serde_json::Value,bool)> {
    check()?;
    let mut identity:WorkspaceIdentity=serde_json::from_slice(raw)?;
    let parsed=crate::storage::parse_timestamp(&identity.created_at).ok_or_else(||ToolkitError::invalid("invalid workspace timestamp"))?;
    // The current typed owner records one canonical spelling and never rewrites
    // the persisted manifest. Reading reports the canonical instant and tells
    // the caller whether it is the zero instant (Go time.IsZero predecessor).
    let zero_time=parsed.timestamp()==-62135596800;
    identity.created_at=crate::storage::timestamp_offset(parsed);
    check()?;
    Ok((serde_json::to_value(identity)?,zero_time))
}
pub(super) mod retention;
pub(super) use retention::decode_retention;
