//! Fixed Go323 ReadWorkspace / InspectEvidence / ListCaptures default routes.
//! One read-only Metadata owner survives inspection and streaming verification.
use super::*;
use crate::{control::OperationControl, metadata::historical_write::finish_historical};
use dependencies::GoCaptureDecode;

fn finish<T>(metadata: Metadata, result: Result<T>, control: &OperationControl) -> Result<T> {
    let result = finish_historical(result, metadata.close());
    crate::selection::selection_finish(result, control.check())
}
fn read_metadata(home: &Path, control: &OperationControl) -> Result<Metadata> {
    let metadata = Metadata::open_controlled(home, false, control).map_err(|mut error| {
        if matches!(
            error.code.as_str(),
            "vault.schema_newer" | "vault.schema_invalid"
        ) {
            error.code = "vault.unsupported_workspace".into();
            if error.details.is_null() {
                error.message = "vault.unsupported_workspace".into();
            }
            error.exit_code = 4;
        }
        error
    })?;
    match metadata.require_historical_schema_with_check(control, || Ok(())) {
        Ok(()) => Ok(metadata),
        Err(error) => finish(metadata, Err(error), control),
    }
}
fn inspect(metadata: &Metadata, id: &str, control: &OperationControl) -> Result<GoCaptureDecode> {
    let raw =
        metadata.read_bytes_controlled_with_check(&format!("capture/{id}"), control, || Ok(()))?;
    dependencies::inspect_capture_projection(&raw, id, control)
}
fn inspect_evidence(
    metadata: &Metadata,
    home: &Path,
    id: &str,
    verify: bool,
    control: &OperationControl,
) -> Result<Value> {
    let capture = inspect(metadata, id, control)?;
    if verify {
        // Go InspectEvidence deliberately calls VerifyCapture, which re-reads
        // the manifest under the same Metadata owner. Do not silently bypass
        // the second read or verify a replaced row against the first value.
        let verified = inspect(metadata, id, control)?;
        let (digest, bytes) = verified.signed_blob();
        bounded::verify_historical_blob(home, digest, bytes, control)?;
    }
    let result = serde_json::to_value(capture).map_err(Into::into);
    crate::selection::selection_finish(result, control.check())
}
fn list(metadata: &Metadata, limit: usize, control: &OperationControl) -> Result<Value> {
    // Keep the exact bounded page until its SQLite cursor has closed. The
    // original row bytes move, rather than doubling a possible 1000 MiB page.
    // Go first decodes the complete row, then inspects the ID *inside* it.
    let documents = metadata.read_raw_page_controlled("capture/", "capture/", limit, control)?;
    let mut captures = Vec::new();
    for document in documents {
        control.check()?;
        let projected = dependencies::decode_capture_projection(&document.bytes, control)?;
        let capture = inspect(metadata, projected.id(), control)?;
        captures.try_reserve(1).map_err(|_| {
            ToolkitError::new("command.failed", "capture page allocation failed", 5)
        })?;
        captures.push(serde_json::to_value(capture)?);
    }
    control.check()?;
    Ok(json!({"captures":captures}))
}
pub(super) fn dispatch(inv: &Invocation) -> Result<Outcome> {
    let id = if inv.command == "evidence list" {
        None
    } else {
        Some(inv.argument()?)
    };
    let limit = if id.is_none() {
        Some(inv.usize_flag("--limit", 100, 1, 1000)?)
    } else {
        None
    };
    let metadata = read_metadata(&inv.home, &inv.control)?;
    let result = match id {
        Some(id) => inspect_evidence(
            &metadata,
            &inv.home,
            id,
            inv.command == "evidence verify",
            &inv.control,
        ),
        None => list(&metadata, limit.expect("list limit"), &inv.control),
    };
    // Keep the existing library verification receipt. The CLI's existing
    // evidence-verify adapter projects its capture as Go's public result.
    let result = result.map(|capture| {
        if inv.command == "evidence verify" {
            Outcome::new(json!({"capture":capture,"verified":true}))
        } else {
            Outcome::new(capture)
        }
    });
    finish(metadata, result, &inv.control)
}
#[cfg(test)]
mod tests;
