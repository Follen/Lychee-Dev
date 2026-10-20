//! Reference-safe removal with the existing durable, resumable Rust receipt.
//! Metadata scan/authentication/CAS/recovery share the actual SQLite snapshot.
use super::*;
use crate::{
    control::OperationControl,
    metadata::{
        RawDocument,
        historical_write::{
            HistoricalWriter, finish_historical, historical_lease,
            removal::{RemovalCommit, RemovalTransaction},
        },
    },
};
use dependencies::GoCaptureDecode;
use serde_json::value::RawValue;
mod deletion;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    capture_id: String,
    manifest_removed: bool,
    retention_removed: bool,
    blob_removed: bool,
    blob_shared: bool,
    blob_missing: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Removal {
    schema: String,
    capture_id: String,
    capture: Option<Box<RawValue>>,
    blob_was_present: bool,
    result: Receipt,
}
#[derive(Default)]
struct Progress {
    receipt: Option<Receipt>,
    preparation: Option<&'static str>,
    disposition: Option<&'static str>,
    recovery_complete: bool,
}
fn integrity(message: &str) -> ToolkitError {
    ToolkitError::new("evidence.removal_integrity", message, 4)
}
fn expected(error: &ToolkitError, code: &str) -> bool {
    error.code == code && error.details.is_null()
}
fn optional(result: Result<RawDocument>) -> Result<Option<RawDocument>> {
    match result {
        Ok(row) => Ok(Some(row)),
        Err(error) if expected(&error, "vault.record_missing") => Ok(None),
        Err(error) => Err(error),
    }
}
fn decode(raw: &[u8], id: &str, control: &OperationControl) -> Result<Removal> {
    control.check()?;
    let decoded = (|| {
        if crate::raw_json::validate_with_check(raw, || control.check())?.is_err() {
            return Err(integrity("invalid removal recovery JSON"));
        }
        let record: Removal = serde_json::from_slice(raw).map_err(|error| {
            integrity("invalid removal record")
                .with_details(json!({"cause":ToolkitError::from(error)}))
        })?;
        let r = &record.result;
        if record.schema != "lycheedev.evidence-removal.v1"
            || record.capture_id != id
            || r.capture_id != id
            || !r.manifest_removed
            || r.blob_removed && r.blob_missing
            || r.blob_shared && (r.blob_removed || r.blob_missing)
            || record.capture.is_some() && (r.blob_removed || r.blob_missing || r.blob_shared)
            || record.capture.is_none() && !(r.blob_removed || r.blob_missing || r.blob_shared)
        {
            return Err(integrity("removal record identity or disposition differs"));
        }
        if let Some(raw) = &record.capture {
            authenticate_pending(raw, id, control)?;
        }
        Ok(record)
    })();
    crate::selection::selection_finish(decoded, control.check())
}
fn authenticate_pending(
    raw: &RawValue,
    id: &str,
    control: &OperationControl,
) -> Result<GoCaptureDecode> {
    dependencies::inspect_capture_projection(raw.get().as_bytes(), id, control).map_err(|error| {
        if matches!(
            error.code.as_str(),
            "command.cancelled" | "command.deadline"
        ) {
            error
        } else {
            integrity("pending original capture identity differs")
                .with_details(json!({"cause":error}))
        }
    })
}
fn annotate(mut error: ToolkitError, progress: &Progress, id: &str) -> ToolkitError {
    let Some(receipt) = &progress.receipt else {
        return error;
    };
    let primary = error.clone();
    let state = progress.preparation.unwrap_or("unknown");
    error.message = format!(
        "{}\nCapture {id} removal metadata is {state}; recovery remains addressable by this ID.",
        error.message
    );
    error.details = json!({"schema":"lycheedev.evidence-removal-error.v1","primary":primary,
        "result":receipt,"resumeCaptureId":id,"removalPublication":{"state":state,
        "blobDisposition":progress.disposition,"recoveryComplete":progress.recovery_complete}});
    error
}
pub(super) fn run(home: &Path, id: &str, control: &OperationControl) -> Result<Value> {
    let mut progress = Progress::default();
    let result = crate::metadata::with_evidence_writer(home, id, control, |writer| {
        writer.with_write_lease(|writer| {
            // Go resolves the manifest first; a malformed/missing selector is
            // not rejected by an unrelated digest preflight.
            let key = format!("evidence/remove/{id}");
            let pending = optional(writer.document(&key))?
                .map(|row| decode(&row.bytes, id, control))
                .transpose()?;
            let capture = if let Some(record) = pending {
                progress.receipt = Some(record.result.clone());
                progress.preparation = Some("committed");
                let Some(raw) = record.capture else {
                    if optional(writer.document(&format!("capture/{id}")))?.is_some() {
                        return Err(integrity(
                            "completed removal conflicts with a present capture",
                        ));
                    }
                    progress.recovery_complete = true;
                    return serde_json::to_value(record.result).map_err(Into::into);
                };
                authenticate_pending(&raw, id, control)?
            } else {
                let row = writer.document(&format!("capture/{id}"))?;
                dependencies::inspect_capture_projection(&row.bytes, id, control)?
            };
            let (digest, _) = capture.signed_blob();
            // A blob lease precedes BEGIN IMMEDIATE, matching capture publishers'
            // order. The metadata-write lease is not mistaken for a SQLite fence.
            let blob_key = format!("blob:{}", digest.to_ascii_lowercase());
            #[cfg(test)]
            tests::before_blob_lease(&blob_key, control);
            let lease = historical_lease(home, &blob_key, control)?;
            let result = (|| {
                if optional(writer.document(&key))?.is_none() {
                    prepare(home, writer, &key, id, &capture, control, &mut progress)?;
                }
                complete(home, writer, &key, id, control, &mut progress)
            })();
            crate::selection::selection_finish(
                finish_historical(result, lease.close()),
                control.check(),
            )
        })
    });
    result.map_err(|error| annotate(error, &progress, id))
}
fn prepare(
    home: &Path,
    writer: &HistoricalWriter<'_>,
    key: &str,
    id: &str,
    locked: &GoCaptureDecode,
    control: &OperationControl,
    progress: &mut Progress,
) -> Result<()> {
    let mut state = RemovalCommit::NotCommitted;
    let mut proposed = None;
    let result = writer.removal_transaction(&mut state, |tx| {
        let capture_key = format!("capture/{id}");
        let keep_key = format!("evidence/keep/{id}");
        let row = tx.document(&capture_key)?;
        let capture = dependencies::inspect_capture_projection(&row.bytes, id, control)?;
        if capture.signed_blob() != locked.signed_blob() {
            return Err(integrity("capture changed after blob lease acquisition"));
        }
        let keep = optional(tx.document(&keep_key))?;
        if let Some(keep) = &keep {
            dependencies::retention::validate_removal_retention(&keep.bytes, &capture, control)?;
        }
        references(home, tx, key, &capture, control)?;
        let shared = shared(home, tx, &capture, control)?;
        // Preserve Rust's corruption-before-metadata-removal protection for a
        // blob which will be deleted. Shared blobs are not consumed by Go.
        let present = if shared {
            false
        } else {
            verified_present(home, &capture, control)?
        };
        let receipt = Receipt {
            capture_id: id.into(),
            manifest_removed: true,
            retention_removed: keep.is_some(),
            blob_removed: false,
            blob_shared: shared,
            blob_missing: false,
        };
        let capture = if shared {
            None
        } else {
            Some(RawValue::from_string(
                String::from_utf8(storage::identity_json(&capture)?)
                    .map_err(|_| ToolkitError::invalid("capture JSON is not UTF-8"))?,
            )?)
        };
        let removal = Removal {
            schema: "lycheedev.evidence-removal.v1".into(),
            capture_id: id.into(),
            capture,
            blob_was_present: present,
            result: receipt.clone(),
        };
        let raw = storage::identity_json(&removal)?;
        tx.put_recovery(key, 0, &raw)?;
        tx.delete_manifest(&capture_key, row.generation)?;
        if let Some(keep) = keep {
            tx.delete_manifest(&keep_key, keep.generation)?;
        }
        proposed = Some(receipt);
        Ok(())
    });
    if state != RemovalCommit::NotCommitted {
        progress.receipt = proposed;
        progress.preparation = Some(if state == RemovalCommit::Committed {
            "committed"
        } else {
            "unknown"
        });
    }
    result
}
fn complete(
    home: &Path,
    writer: &HistoricalWriter<'_>,
    key: &str,
    id: &str,
    control: &OperationControl,
    progress: &mut Progress,
) -> Result<Value> {
    let mut state = RemovalCommit::NotCommitted;
    let result = writer.removal_transaction(&mut state, |tx| {
        let row = tx.document(key)?;
        let mut record = decode(&row.bytes, id, control)?;
        progress.receipt = Some(record.result.clone());
        progress.preparation = Some("committed");
        if optional(tx.document(&format!("capture/{id}")))?.is_some() {
            return Err(integrity("removal conflicts with a present capture"));
        }
        let Some(raw) = record.capture.take() else {
            progress.recovery_complete = true;
            return serde_json::to_value(record.result).map_err(Into::into);
        };
        let capture = authenticate_pending(&raw, id, control)?;
        references(home, tx, key, &capture, control)?;
        let shared = shared(home, tx, &capture, control)?;
        record.result.blob_shared = shared;
        if shared {
            progress.disposition = Some("shared");
        } else {
            let deleted = deletion::remove(home, &capture, control, &mut progress.disposition);
            // Keep proved disposition facts even when a later checked directory
            // close fails. A mere requested/uncertain disposition stays explicit.
            if matches!(progress.disposition, Some("deleted" | "missing")) {
                record.result.blob_removed = record.blob_was_present;
                record.result.blob_missing = !record.blob_was_present;
                progress.receipt = Some(record.result.clone());
            }
            deleted?;
            // A resumed request may observe absence after its previous exact
            // disposition. Preserve the original durable receipt, not this try.
            record.result.blob_removed = record.blob_was_present;
            record.result.blob_missing = !record.blob_was_present;
        }
        progress.receipt = Some(record.result.clone());
        tx.put_recovery(key, row.generation, &storage::identity_json(&record)?)?;
        serde_json::to_value(record.result).map_err(Into::into)
    });
    if state == RemovalCommit::Committed {
        progress.recovery_complete = true;
    }
    result
}
fn verified_present(
    home: &Path,
    capture: &GoCaptureDecode,
    control: &OperationControl,
) -> Result<bool> {
    let (digest, bytes) = capture.signed_blob();
    // Stream only: no production payload-size cap, no DELETE access before
    // committing the recovery intent, and every original owner closes checked.
    bounded::verify_historical_blob_optional(home, digest, bytes, control)
}
fn references(
    home: &Path,
    tx: &RemovalTransaction<'_>,
    removal_key: &str,
    target: &GoCaptureDecode,
    control: &OperationControl,
) -> Result<()> {
    let capture_key = format!("capture/{}", target.id());
    let keep_key = format!("evidence/keep/{}", target.id());
    let mut after = String::new();
    for _ in 0..100 {
        let (count, last) = tx.visit_raw_page("", &after, |key, _, raw| {
            if key == capture_key || key == keep_key || key == removal_key {
                return Ok(());
            }
            if dependencies::references_capture_raw(home, key, raw, target, control)? {
                return Err(ToolkitError::new(
                    "evidence.capture_referenced",
                    "another metadata document still names this exact capture",
                    3,
                )
                .with_details(json!({"documentKey":key})));
            }
            Ok(())
        })?;
        if count < 1000 {
            return Ok(());
        }
        after = last;
    }
    Err(ToolkitError::new(
        "evidence.reference_scan_limit",
        "evidence.reference_scan_limit",
        3,
    ))
}
fn shared(
    home: &Path,
    tx: &RemovalTransaction<'_>,
    target: &GoCaptureDecode,
    control: &OperationControl,
) -> Result<bool> {
    let key = format!("capture/{}", target.id());
    let mut after = String::from("capture/");
    for _ in 0..100 {
        let mut found = false;
        let (count, last) = tx.visit_raw_page("capture/", &after, |other, _, raw| {
            if other != key && !found {
                found = dependencies::decode_capture_projection(raw, control)?.signed_blob()
                    == target.signed_blob();
            }
            Ok(())
        })?;
        if found {
            return Ok(true);
        }
        if count < 1000 {
            let (digest, bytes) = target.signed_blob();
            // The same physical CAS body can have an independent Data owner.
            // This read stays under our existing blob lease; it never takes
            // quota (Data publishers use quota -> blob, not the reverse).
            return workspace::data_disk::protects_evidence_body(home, digest, bytes, control);
        }
        after = last;
    }
    Err(ToolkitError::new(
        "evidence.capture_scan_limit",
        "evidence.capture_scan_limit",
        3,
    ))
}
#[cfg(test)]
mod tests;
