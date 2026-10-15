//! Original-controlled historical manifest and blob fetch. No evidence is created.
use super::*;
use crate::control::OperationControl;
pub fn fetch_capture_controlled(
    home: &Path,
    id: &str,
    maximum: usize,
    control: &OperationControl,
) -> Result<(CaptureRef, Vec<u8>)> {
    let raw = Metadata::read_raw_controlled(home, &format!("capture/{id}"), control)?;
    let (capture, signed_bytes) = dependencies::decode_historical_capture(&raw, id, control)?;
    // Fixed Go archive.go InspectCapture authenticates before ReadBlob checks
    // signed length/digest, then the maximum, then opens any blob file.
    if signed_bytes < 0 || !crate::selection::valid_digest(&capture.blob.sha256, 64) {
        return Err(ToolkitError::new(
            "vault.blob_integrity",
            "vault.blob_integrity",
            4,
        ));
    }
    let bytes = read_blob_bounded(
        home,
        &serde_json::to_value(&capture.blob)?,
        maximum,
        control,
    )
    .map_err(|mut error| {
        if matches!(
            error.code.as_str(),
            "vault.blob_integrity" | "vault.blob_limit"
        ) {
            error.message = error.code.clone();
        }
        error
    })?;
    control.check()?;
    Ok((capture, bytes))
}
#[cfg(test)]
#[path = "history_read_tests.rs"]
mod tests;
