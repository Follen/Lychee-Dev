//! Fixed-Go keep/bundle consumers. Original owners remain held through their
//! actual metadata/filesystem mutations and are consumed with checked cleanup.
use super::*;
use crate::{
    control::OperationControl,
    metadata::owned_write::{OwnedMutation, finish_owned},
};
mod bundle;
fn expected(error: &ToolkitError, code: &str) -> bool {
    error.code == code && error.details.is_null()
}
pub(super) fn dispatch(
    context: &crate::request::RequestContext,
    request: crate::evidence::EvidenceRequest,
) -> Result<Outcome> {
    use crate::evidence::EvidenceRequest as E;
    match request {
        E::Keep { id } => keep(&context.home, &id, &context.control).map(Outcome::new),
        E::Bundle { ids, output } => bundle::run(context, &ids, &output).map(Outcome::new),
        _ => unreachable!("archive command request routed by evidence::dispatch"),
    }
}
fn keep(home: &Path, id: &str, control: &OperationControl) -> Result<Value> {
    crate::metadata::with_evidence_writer(home, id, control, |writer| {
        writer.with_write_lease(|writer| {
            let raw = writer.document(&format!("capture/{id}"))?;
            let capture = dependencies::inspect_capture_ref(&raw.bytes, id, control)?;
            // Go creates/marshals the proposed record even when a prior keep wins.
            let payload = dependencies::retention::new_record(id, &capture, control)?;
            let key = format!("evidence/keep/{id}");
            let (bytes, created) = match writer.document(&key) {
                Ok(document) => (document.bytes, false),
                Err(error) if expected(&error, "vault.record_missing") => {
                    match writer.commit(&[OwnedMutation {
                        key: &key,
                        expected_generation: 0,
                        bytes: &payload,
                    }]) {
                        Ok(()) => (payload, true),
                        Err(error) if expected(&error, "vault.generation_conflict") => {
                            (writer.document(&key)?.bytes, false)
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            };
            let retention = dependencies::decode_retention(&bytes, &capture, control)?;
            let result = Ok(json!({"capture":capture,"retention":retention,"created":created}));
            crate::selection::selection_finish(result, control.check())
        })
    })
}
#[cfg(test)]
mod tests;
