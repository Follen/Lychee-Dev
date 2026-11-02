//! Fixed Go323 ReadWorkspace / InspectEvidence / ListCaptures default routes.
//! One read-only Metadata owner survives inspection and streaming verification.
use super::*;
use crate::{control::OperationControl, metadata::owned_write::finish_owned};


fn finish<T>(metadata: Metadata, result: Result<T>, control: &OperationControl) -> Result<T> {
    let result = finish_owned(result, metadata.close());
    crate::selection::selection_finish(result, control.check())
}
pub(super) fn read_metadata(home: &Path, control: &OperationControl) -> Result<Metadata> {
    read_metadata_with_check(home, control, &mut || Ok(()))
}
fn read_metadata_with_check(
    home: &Path,
    control: &OperationControl,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<Metadata> {
    let metadata = Metadata::open_controlled_with_check(home, false, control, &mut *check)
        .map_err(|mut error| {
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
    match metadata.require_current_schema_with_check(control, &mut *check) {
        Ok(()) => Ok(metadata),
        Err(error) => {
            let result = finish_owned(Err(error), metadata.close());
            dependencies::finish_capture_checks(result, control, check)
        }
    }
}
pub(super) fn inspect(
    metadata: &Metadata,
    id: &str,
    control: &OperationControl,
) -> Result<CaptureRef> {
    let raw =
        metadata.read_bytes_controlled_with_check(&format!("capture/{id}"), control, || Ok(()))?;
    dependencies::inspect_capture_ref(&raw, id, control)
}
/// Inspect one authenticated historical manifest with the caller's original
/// stop scope and independent budget. No Data admission or fresh deadline.
pub fn read_capture_controlled_with_check(
    home: &Path,
    id: &str,
    control: &OperationControl,
    mut check: impl FnMut() -> Result<()>,
) -> Result<Value> {
    let metadata = match read_metadata_with_check(home, control, &mut check) {
        Ok(metadata) => metadata,
        Err(error) => return dependencies::finish_capture_checks(Err(error), control, &mut check),
    };
    let result = (|| {
        let raw = metadata.read_bytes_controlled_with_check(
            &format!("capture/{id}"),
            control,
            &mut check,
        )?;
        let value =
            dependencies::inspect_capture_ref_with_check(&raw, id, control, &mut check)?;
        control.check()?;
        check()?;
        serde_json::to_value(value).map_err(Into::into)
    })();
    let result = finish_owned(result, metadata.close());
    dependencies::finish_capture_checks(result, control, &mut check)
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
        let (digest, bytes) = verified.blob_identity();
        bounded::verify_capture_blob(home, digest, bytes, control)?;
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
        let projected = dependencies::decode_capture_ref(&document.bytes, control)?;
        let capture = inspect(metadata, &projected.id, control)?;
        captures.try_reserve(1).map_err(|_| {
            ToolkitError::new("command.failed", "capture page allocation failed", 5)
        })?;
        captures.push(serde_json::to_value(capture)?);
    }
    control.check()?;
    Ok(json!({"captures":captures}))
}
pub(super) fn dispatch(
    context: &crate::request::RequestContext,
    request: crate::evidence::EvidenceRequest,
) -> Result<Outcome> {
    use crate::evidence::EvidenceRequest as E;
    let (id, limit, verify) = match request {
        E::Show { id } => (Some(id), None, false),
        E::Verify { id } => (Some(id), None, true),
        E::List { limit } => (None, Some(limit), false),
        _ => unreachable!("read command request routed by evidence::dispatch"),
    };
    let metadata = read_metadata(&context.home, &context.control)?;
    let result = match id.as_deref() {
        Some(id) => inspect_evidence(
            &metadata,
            &context.home,
            id,
            verify,
            &context.control,
        ),
        None => list(&metadata, limit.expect("list request carries limit"), &context.control),
    };
    // Keep the existing library verification receipt. The CLI's existing
    // evidence-verify adapter projects its capture as Go's public result.
    let result = result.map(|capture| {
        if verify {
            Outcome::new(json!({"capture":capture,"verified":true}))
        } else {
            Outcome::new(capture)
        }
    });
    finish(metadata, result, &context.control)
}
#[cfg(test)]
mod tests;
