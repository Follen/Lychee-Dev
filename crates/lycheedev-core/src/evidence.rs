//! Content-addressed original bytes and immutable provenance, separate from rebuildable cache.
use crate::{
    Invocation, Outcome, Result, ToolkitError,
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
mod bounded;
mod budgeted;
pub use budgeted::{
    CaptureAccounting, CaptureHeader, commit_capture_budgeted, commit_verified_capture_budgeted,
};
pub use bounded::{
    CaptureInput, DEFAULT_CAPTURE_BYTES, MAX_CAPTURE_BYTES, commit_capture_bounded,
    publish_blob_bounded, read_blob_bounded, verify_blob_bounded,
    VerifiedBlob, open_verified_blob_bounded, publish_blob_reader_bounded, commit_verified_capture,
};
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
    if capture.schema != "lycheedev.capture.v1"
        || capture.id != id
        || capture_identity(&capture)? != id
    {
        return Err(ToolkitError::new(
            "evidence.capture_integrity",
            "capture manifest content differs from identity",
            4,
        ));
    }
    Ok(serde_json::to_value(capture)?)
}
pub fn read_blob(home: &Path, blob: &Value, max_bytes: usize) -> Result<Vec<u8>> {
    bounded::read_internal(home, blob, max_bytes, None)
}
pub fn commands() -> &'static [&'static str] {
    &[
        "evidence show",
        "evidence verify",
        "evidence list",
        "evidence keep",
        "evidence bundle",
        "evidence remove",
    ]
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    workspace::open(&inv.home)?;
    match inv.command.as_str() {
        "evidence remove" => Ok(Outcome::new(remove_capture(&inv.home, inv.argument()?)?)),
        "evidence show" => Ok(Outcome::new(read_capture(&inv.home, inv.argument()?)?)),
        "evidence verify" => {
            let value = read_capture(&inv.home, inv.argument()?)?;
            verify_blob_bounded(&inv.home, &value["blob"], MAX_CAPTURE_BYTES, &inv.control)?;
            Ok(Outcome::new(json!({"capture":value,"verified":true})))
        }
        "evidence list" => {
            let limit = inv.usize_flag("--limit", 100, 1, 1000)?;
            let values: Vec<Value> = Metadata::open(&inv.home, false)?.list("capture/", limit)?;
            Ok(Outcome::new(json!({"captures":values})))
        }
        "evidence keep" => {
            let value = read_capture(&inv.home, inv.argument()?)?;
            let mut metadata = Metadata::open(&inv.home, true)?;
            let key = format!("evidence/keep/{}", inv.argument()?);
            let existing = metadata.generation(&key)?;
            let retention = if existing.is_some() {
                metadata.read::<Value>(&key)?
            } else {
                let retained = json!({"schema":"lycheedev.evidence-retention.v1","captureId":inv.argument()?,"blob":value["blob"],"keptAt":storage::utc_now()});
                metadata.put(&key, &retained, None)?;
                retained
            };
            if retention["schema"] != "lycheedev.evidence-retention.v1"
                || retention["captureId"] != inv.argument()?
                || retention["blob"] != value["blob"]
            {
                return Err(ToolkitError::new(
                    "evidence.invalid_retention",
                    "retention identity differs",
                    4,
                ));
            }
            Ok(Outcome::new(
                json!({"capture":value,"retention":retention,"created":existing.is_none()}),
            ))
        }
        "evidence bundle" => bundle(inv),
        _ => Err(ToolkitError::new(
            "evidence.migration_incomplete",
            "reference-safe removal is not yet migrated",
            3,
        )),
    }
}
fn names_capture(value: &Value, id: &str) -> bool {
    match value {
        Value::String(text) => text == id,
        Value::Array(values) => values.iter().any(|v| names_capture(v, id)),
        Value::Object(fields) => fields
            .iter()
            .any(|(key, value)| key == id || names_capture(value, id)),
        _ => false,
    }
}
fn names_blob(value: &Value, blob: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(|value| names_blob(value, blob)),
        Value::Object(fields) => {
            value.get("sha256") == blob.get("sha256") && value.get("bytes") == blob.get("bytes")
                || fields.values().any(|value| names_blob(value, blob))
        }
        _ => false,
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Removal {
    schema: String,
    capture_id: String,
    capture: Option<CaptureRef>,
    blob_was_present: bool,
    result: Value,
}
fn read_removal(metadata: &Metadata, key: &str, id: &str) -> Result<Removal> {
    let removal: Removal = metadata.read(key)?;
    if removal.schema != "lycheedev.evidence-removal.v1"
        || removal.capture_id != id
        || removal.result["captureId"] != id
        || removal.result["manifestRemoved"] != true
        || removal.result.as_object().is_none_or(|result| {
            result.len() != 6
                || [
                    "manifestRemoved",
                    "retentionRemoved",
                    "blobRemoved",
                    "blobShared",
                    "blobMissing",
                ]
                .iter()
                .any(|field| !result[*field].is_boolean())
        })
    {
        return Err(ToolkitError::new(
            "evidence.removal_integrity",
            "removal record identity differs",
            4,
        ));
    }
    let removed = removal.result["blobRemoved"] == true;
    let missing = removal.result["blobMissing"] == true;
    let shared = removal.result["blobShared"] == true;
    if removed && missing
        || shared && (removed || missing)
        || removal.capture.is_some() && (removed || missing || shared)
        || removal.capture.is_none() && !(removed || missing || shared)
    {
        return Err(ToolkitError::new(
            "evidence.removal_integrity",
            "removal disposition is inconsistent",
            4,
        ));
    }
    if let Some(capture) = &removal.capture
        && (capture.schema != "lycheedev.capture.v1"
            || capture.id != id
            || capture_identity(capture)? != id)
    {
        return Err(ToolkitError::new(
            "evidence.removal_integrity",
            "pending original capture identity differs",
            4,
        ));
    }
    Ok(removal)
}
fn finish_removal(
    home: &Path,
    metadata: &mut Metadata,
    key: &str,
    mut removal: Removal,
) -> Result<Value> {
    let Some(capture) = removal.capture.take() else {
        return Ok(removal.result);
    };
    let blob = serde_json::to_value(&capture.blob)?;
    let generation = metadata
        .generation(key)?
        .ok_or_else(|| ToolkitError::invalid("removal record disappeared"))?;
    // Recheck any documents published since the manifest transaction. The content
    // lock also fences all capture publication until the exact disposition is durable.
    let shared = metadata.delete_checked(&[], &[(key.into(), generation)], |documents| {
        let mut shared = false;
        for document in documents {
            if document.key == key {
                continue;
            }
            if names_capture(&document.value, &capture.id) {
                return Err(ToolkitError::new(
                    "evidence.capture_referenced",
                    "capture was referenced after removal preparation; original bytes retained",
                    3,
                )
                .with_details(json!({"documentKey":document.key,"result":removal.result})));
            }
            shared |= names_blob(&document.value, &blob);
        }
        Ok(shared)
    })?;
    removal.result["blobShared"] = json!(shared);
    if !shared {
        let path = blob_path(home, &capture.blob.sha256)?;
        storage::guard_path(home, &path)?;
        if path.exists() {
            bounded::verify_internal(home, &blob, MAX_CAPTURE_BYTES, None)?;
            fs::remove_file(path).map_err(|error| {
                ToolkitError::from(error)
                    .with_details(json!({"result":removal.result,"resumeCaptureId":capture.id}))
            })?;
        }
        // A crash after exact deletion but before this checkpoint is recoverable.
        removal.result["blobRemoved"] = json!(removal.blob_was_present);
        removal.result["blobMissing"] = json!(!removal.blob_was_present);
    }
    metadata.put(key, &removal, Some(generation))?;
    Ok(removal.result)
}
pub fn remove_capture(home: &Path, id: &str) -> Result<Value> {
    if !id
        .strip_prefix("CAP-")
        .is_some_and(|digest| valid_digest(digest, 64))
    {
        return Err(ToolkitError::new(
            "evidence.invalid_capture",
            "invalid capture identity",
            4,
        ));
    }
    let removal_key = format!("evidence/remove/{id}");
    let mut metadata = Metadata::open(home, true)?;
    if metadata.generation(&removal_key)?.is_some() {
        let removal = read_removal(&metadata, &removal_key, id)?;
        let Some(capture) = &removal.capture else {
            if metadata.generation(&format!("capture/{id}"))?.is_some() {
                return Err(ToolkitError::new(
                    "evidence.removal_integrity",
                    "completed removal conflicts with a present capture",
                    4,
                ));
            }
            return Ok(removal.result);
        };
        let _guard = ExclusiveLock::resource(
            &home.join("locks"),
            &format!("blob:{}", capture.blob.sha256),
        )?;
        let removal = read_removal(&metadata, &removal_key, id)?;
        return finish_removal(home, &mut metadata, &removal_key, removal);
    }
    let capture = read_capture(home, id)?;
    let digest = capture["blob"]["sha256"]
        .as_str()
        .ok_or_else(|| ToolkitError::invalid("capture blob digest missing"))?;
    let _guard = ExclusiveLock::resource(&home.join("locks"), &format!("blob:{digest}"))?;
    if metadata.generation(&removal_key)?.is_some() {
        let removal = read_removal(&metadata, &removal_key, id)?;
        return finish_removal(home, &mut metadata, &removal_key, removal);
    }
    let key = format!("capture/{id}");
    let keep = format!("evidence/keep/{id}");
    let generation = metadata
        .generation(&key)?
        .ok_or_else(|| ToolkitError::new("vault.record_missing", "capture disappeared", 3))?;
    let retention_generation = metadata.generation(&keep)?;
    let retained = retention_generation.is_some();
    if retained {
        let retention: Value = metadata.read(&keep)?;
        if retention["schema"] != "lycheedev.evidence-retention.v1"
            || retention["captureId"] != id
            || retention["blob"] != capture["blob"]
        {
            return Err(ToolkitError::new(
                "evidence.invalid_retention",
                "retention identity differs",
                4,
            ));
        }
    }
    let path = blob_path(home, digest)?;
    storage::guard_path(home, &path)?;
    let present = path.exists();
    if present {
        bounded::verify_internal(home, &capture["blob"], MAX_CAPTURE_BYTES, None)?;
    }
    let deletions = vec![key.clone(), keep.clone()];
    let mut expected = vec![(key.clone(), generation)];
    if let Some(generation) = retention_generation {
        expected.push((keep.clone(), generation));
    }
    let removal =
        metadata.delete_checked_record(&deletions, &expected, |documents| {
            let mut shared = false;
            let current = documents.iter().find(|document| document.key == key);
            let current_keep = documents.iter().find(|document| document.key == keep);
            if current.is_none_or(|document| document.value != capture)
                || current_keep.map(|document| document.generation) != retention_generation
            {
                return Err(ToolkitError::new("vault.generation_conflict", "capture or retention changed during removal preparation", 4));
            }
            for doc in documents {
                if doc.key == key || doc.key == keep {
                    continue;
                }
                if names_capture(&doc.value, id) {
                    return Err(ToolkitError::new(
                        "evidence.capture_referenced",
                        "another metadata document still names this exact capture",
                        3,
                    )
                    .with_details(json!({"documentKey":doc.key})));
                }
                if names_blob(&doc.value, &capture["blob"]) {
                    shared = true;
                }
            }
            let removal = Removal {
                schema: "lycheedev.evidence-removal.v1".into(), capture_id: id.into(),
                capture: if shared { None } else { Some(serde_json::from_value(capture.clone())?) },
                blob_was_present: present,
                result: json!({"captureId":id,"manifestRemoved":true,"retentionRemoved":retained,"blobRemoved":false,"blobShared":shared,"blobMissing":false}),
            };
            let record = serde_json::to_value(&removal)?;
            Ok((removal, Some((removal_key.clone(), record))))
        })?;
    finish_removal(home, &mut metadata, &removal_key, removal)
}
fn bundle(inv: &Invocation) -> Result<Outcome> {
    let ids: std::collections::BTreeSet<_> = inv.required("--ids")?.split(',').collect();
    if ids.is_empty() || ids.len() > 100 {
        return Err(ToolkitError::arguments("bundle requires 1..100 captures"));
    }
    let output = Path::new(inv.required("--output")?);
    if output.exists() {
        return Err(ToolkitError::new(
            "evidence.output_exists",
            "output already exists",
            4,
        ));
    }
    let mut captures = vec![];
    let mut blobs = std::collections::BTreeMap::new();
    let mut total = 0usize;
    for id in &ids {
        let capture = read_capture(&inv.home, id)?;
        let digest = capture["blob"]["sha256"].as_str().unwrap().to_owned();
        if let std::collections::btree_map::Entry::Vacant(entry) = blobs.entry(digest) {
            let bytes = read_blob(&inv.home, &capture["blob"], 128 << 20)?;
            total += bytes.len();
            if total > 128 << 20 {
                return Err(ToolkitError::new(
                    "evidence.bundle_limit",
                    "unique bundle bytes exceed 128 MiB",
                    4,
                ));
            }
            entry.insert(bytes);
        }
        captures.push(capture);
    }
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let result = (|| -> Result<()> {
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for capture in &captures {
            zip.start_file(
                format!("captures/{}.json", capture["id"].as_str().unwrap()),
                opts,
            )
            .map_err(|e| ToolkitError::invalid(e.to_string()))?;
            zip.write_all(&serde_json::to_vec_pretty(capture)?)?;
        }
        for (digest, bytes) in &blobs {
            zip.start_file(format!("blobs/{digest}"), opts)
                .map_err(|e| ToolkitError::invalid(e.to_string()))?;
            zip.write_all(bytes)?;
        }
        zip.finish()
            .map_err(|e| ToolkitError::invalid(e.to_string()))?
            .sync_all()?;
        Ok(())
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(output);
        return Err(error);
    }
    Ok(Outcome::new(
        json!({"output":output,"captures":captures.len(),"blobBytes":total,"complete":true}),
    ))
}
