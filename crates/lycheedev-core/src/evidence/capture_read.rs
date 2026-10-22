//! Read the current immutable capture and its authenticated original bytes.
use super::*;
use crate::control::OperationControl;
pub fn fetch_capture_controlled(home:&Path,id:&str,maximum:usize,control:&OperationControl)->Result<(CaptureRef,Vec<u8>)> {
    let raw=Metadata::read_raw_controlled(home,&format!("capture/{id}"),control)?;
    let capture=dependencies::inspect_capture_ref(&raw,id,control)?;
    let bytes=read_blob_bounded(home,&serde_json::to_value(&capture.blob)?,maximum,control)?;
    control.check()?;Ok((capture,bytes))
}
