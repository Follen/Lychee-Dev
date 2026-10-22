//! Content-addressed original bytes and immutable provenance, separate from rebuildable cache.
use crate::{
    Outcome, Result, ToolkitError,
    metadata::Metadata,
    selection::valid_digest,
    storage::{self, ExclusiveLock},
    workspace,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
mod archive_commands;
mod bounded;
pub(crate) use bounded::{
    StreamPublicationSettlement, publish_blob_attempt_predeclared_with_check,
    publish_blob_attempt_with_check, publish_data_blob_stream_with_check,
};
mod budgeted;
mod dependencies;
mod capture_read;
mod read_commands;
mod remove_commands;
pub use bounded::{
    CaptureInput, DEFAULT_CAPTURE_BYTES, MAX_CAPTURE_BYTES, VerifiedBlob, commit_capture_bounded,
    commit_capture_bounded_with_check, commit_verified_capture, open_verified_blob_bounded,
    open_verified_blob_bounded_optional_with_check, open_verified_blob_bounded_with_check,
    publish_blob_bounded, publish_blob_bounded_with_check, publish_blob_reader_bounded,
    publish_blob_reader_bounded_with_check, publish_blob_stream_bounded_with_check,
    read_blob_bounded, read_blob_bounded_optional_with_check, read_blob_bounded_with_check,
    verify_blob_bounded,
};
pub use budgeted::{
    BlobStreamPhase, CaptureAccounting, CaptureHeader, commit_capture_budgeted,
    commit_capture_budgeted_referencing, commit_capture_budgeted_referencing_with_metadata,
    commit_capture_budgeted_with_dependencies,
    commit_capture_budgeted_with_dependencies_and_metadata,
    commit_capture_budgeted_with_references, commit_data_capture_bounded_with_check,
    commit_verified_capture_budgeted, commit_verified_capture_budgeted_with_metadata,
};
pub(crate) use dependencies::{decode_workspace_identity, decode_workspace_identity_with_check};
pub use capture_read::fetch_capture_controlled;
pub use read_commands::read_capture_controlled_with_check;
pub fn blob_path(home: &Path, digest: &str) -> Result<PathBuf> {
    if !valid_digest(digest, 64) {
        return Err(ToolkitError::new(
            "vault.blob_integrity",
            "invalid blob digest",
            4,
        ));
    }
    Ok(home.join("blobs").join(&digest[..2]).join(&digest[2..]))
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Provenance {
    pub kind: String,
    pub locator: String,
    #[serde(default)]
    pub snapshot: String,
    #[serde(default)]
    pub operation_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_commit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_snapshot: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub base_source_commit: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub data_build: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub session: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobRef {
    pub sha256: String,
    pub bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureRef {
    pub schema: String,
    pub id: String,
    pub blob: BlobRef,
    pub media_type: String,
    pub provenance: Provenance,
    pub complete: bool,
    pub truncated: bool,
    pub captured_at: String,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty"
    )]
    pub dependencies: Vec<String>,
}
impl CaptureRef {
    pub(crate) fn blob_identity(&self)->(&str,u64) { (&self.blob.sha256,self.blob.bytes) }
}
fn capture_identity(capture: &CaptureRef) -> Result<String> {
    let mut unsigned = capture.clone();
    unsigned.id.clear();
    Ok(format!(
        "CAP-{}",
        storage::sha256(&storage::identity_json(&unsigned)?)
    ))
}
pub fn publish_blob(home: &Path, bytes: &[u8], max_bytes: usize) -> Result<Value> {
    workspace::open(home)?;
    bounded::publish_internal(home, bytes, max_bytes, None)
}
pub fn commit_capture(
    home: &Path,
    bytes: &[u8],
    media_type: &str,
    provenance: Value,
    complete: bool,
    truncated: bool,
) -> Result<Value> {
    if media_type.is_empty()
        || provenance["kind"].as_str().unwrap_or("").is_empty()
        || provenance["locator"].as_str().unwrap_or("").is_empty()
        || complete && truncated
    {
        return Err(ToolkitError::new(
            "evidence.invalid_capture",
            "invalid media/provenance/completeness",
            4,
        ));
    }
    workspace::open(home)?;
    bounded::commit_internal(
        home,
        CaptureInput {
            bytes,
            maximum_bytes: DEFAULT_CAPTURE_BYTES,
            media_type,
            provenance,
            complete,
            truncated,
        },
        None,
    )
}
pub fn read_capture(home: &Path, id: &str) -> Result<Value> {
    workspace::open(home)?;
    let capture: CaptureRef = Metadata::open(home, false)?.read(&format!("capture/{id}"))?;
    validate_capture(&capture, id)?;
    Ok(serde_json::to_value(capture)?)
}
fn validate_capture(capture: &CaptureRef, id: &str) -> Result<()> {
    if capture.schema != "lycheedev.capture.v1"
        || capture.id != id
        || capture.complete && capture.truncated
        || storage::parse_timestamp(&capture.captured_at).is_none()
        || capture_identity(capture)? != id
    {
        return Err(ToolkitError::new(
            "evidence.capture_integrity",
            "capture manifest content differs from identity",
            4,
        ));
    }
    Ok(())
}
pub fn read_blob(home: &Path, blob: &Value, max_bytes: usize) -> Result<Vec<u8>> {
    bounded::read_internal(home, blob, max_bytes, None)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EvidenceRequest {
    Show { id: String },
    Verify { id: String },
    List { limit: usize },
    Keep { id: String },
    Bundle { ids: Vec<String>, output: PathBuf },
    Remove { id: String },
}
pub fn dispatch(
    context: &crate::request::RequestContext,
    request: EvidenceRequest,
) -> Result<Outcome> {
    use EvidenceRequest as E;
    match request {
        E::Show { id } => read_commands::dispatch(context, E::Show { id }),
        E::Verify { id } => read_commands::dispatch(context, E::Verify { id }),
        E::List { limit } => read_commands::dispatch(context, E::List { limit }),
        E::Keep { .. } | E::Bundle { .. } => archive_commands::dispatch(context, request),
        E::Remove { id } => remove_capture_controlled(&context.home, &id, &context.control)
            .map(Outcome::new),
    }
}
pub fn remove_capture(home: &Path, id: &str) -> Result<Value> {
    remove_capture_controlled(home, id, &crate::control::OperationControl::default())
}
pub fn remove_capture_controlled(
    home: &Path,
    id: &str,
    control: &crate::control::OperationControl,
) -> Result<Value> {
    remove_commands::run(home, id, control)
}

pub(crate) use bounded::io_error as stream_source_error;

mod storage_inventory;
pub(crate) use storage_inventory::report as data_storage_report;
