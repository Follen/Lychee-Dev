//! One explicit public listfile source; cached bytes are authenticated before
//! parsing. No provider substitution, merged origins or implicit online fallback.
use super::{fault, listfile};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, evidence, metadata::DataCacheMetadata, storage,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path};

const SCHEMA: &str = "lycheedev.listfile-source.v1";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceFile {
    pub component: String,
    #[serde(rename = "url")]
    pub url: String,
    pub bytes: u64,
    #[serde(rename = "sha256")]
    pub sha256: String,
    pub blob: evidence::BlobRef,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    pub kind: String,
    pub files: Vec<SourceFile>,
    pub fetched_at: String,
    pub entry_count: usize,
    pub rejected: usize,
    pub total_bytes: u64,
    pub cache: String,
}
#[derive(Serialize, Deserialize)]
struct Document {
    schema: String,
    provenance: Provenance,
}
pub struct Prepared {
    pub index: listfile::Index,
    pub provenance: Provenance,
}
pub trait Fetcher {
    fn fetch(&self, url: &str, maximum: usize, control: &OperationControl) -> Result<Vec<u8>>;
}
pub struct PublicFetcher;
fn unavailable(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new(
        "records.listfile_unavailable",
        format!("records.listfile_unavailable: {}", message.into()),
        5,
    )
}
fn format(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new(
        "records.listfile_format",
        format!("records.listfile_format: {}", message.into()),
        4,
    )
}
impl Fetcher for PublicFetcher {
    fn fetch(&self, locator: &str, maximum: usize, control: &OperationControl) -> Result<Vec<u8>> {
        http::fetch(locator, maximum, control)
    }
}
mod http;
#[cfg(test)]
mod tests;

fn load(
    home: &Path,
    kind: &str,
    limits: listfile::Limits,
    control: &OperationControl,
) -> Result<Prepared> {
    control.check()?;
    let raw = DataCacheMetadata::read_document_raw_controlled_with_check(
        home,
        &format!("listfile/{kind}"),
        control,
        || Ok(()),
    )?;
    let document: Document = serde_json::from_slice(&raw.bytes)
        .map_err(|_| format("invalid cached listfile document"))?;
    chrono::DateTime::parse_from_rfc3339(&document.provenance.fetched_at)
        .map_err(|_| format("invalid cached listfile timestamp"))?;
    control.check()?;
    if document.schema != SCHEMA || document.provenance.kind != kind {
        return Err(fault("records.listfile_format", 4));
    }
    let mut payloads = BTreeMap::new();
    let mut total = 0usize;
    for file in &document.provenance.files {
        control.check()?;
        let expected = if kind == "wowexport-binary" {
            // Go authenticates the template for every historical component;
            // only the downstream parser requires the eight known files.
            format!(
                "https://www.kruithne.net/wow.export/data/listfile/bin/{}",
                file.component
            )
        } else {
            listfile::source_url(kind, &file.component)?
        };
        if file.url != expected {
            return Err(unavailable("cached origin differs from requested source"));
        }
        if total >= limits.bytes || file.blob.bytes > (limits.bytes - total) as u64 {
            return Err(fault("records.listfile_limit", 3));
        }
        let blob = serde_json::to_value(&file.blob)?;
        let bytes = evidence::read_blob_bounded(home, &blob, limits.bytes - total, control)?;
        if file.bytes != bytes.len() as u64 || file.sha256 != file.blob.sha256 {
            return Err(format(format!(
                "cached payload {} failed digest check",
                file.component
            )));
        }
        total += bytes.len();
        payloads.insert(file.component.clone(), bytes);
    }
    if total as u64 != document.provenance.total_bytes {
        return Err(format("cached payload sizes changed"));
    }
    let index = listfile::parse(kind, &payloads, limits, control)?;
    if index.entries.len() != document.provenance.entry_count
        || index.rejected != document.provenance.rejected
    {
        return Err(format("cached entry counts changed"));
    }
    Ok(Prepared {
        index,
        provenance: document.provenance,
    })
}
fn wrapped_cache(kind: &str, error: ToolkitError) -> ToolkitError {
    if ["command.cancelled", "command.deadline"].contains(&error.code.as_str()) {
        return error;
    }
    if error.code == "vault.record_missing" && error.details.is_null() {
        return unavailable(format!("no {kind} listfile cached in this workspace"));
    }
    let message = format!(
        "records.listfile_unavailable: cached {kind} listfile is unusable: {}",
        error.message
    );
    if [
        "records.listfile_limit",
        "records.listfile_format",
        "records.listfile_query",
    ]
    .contains(&error.code.as_str())
    {
        ToolkitError { message, ..error }
    } else {
        ToolkitError {
            code: "records.listfile_unavailable".into(),
            message,
            exit_code: 5,
            ..error
        }
    }
}
pub fn prepare(
    home: &Path,
    kind: &str,
    offline: bool,
    refresh: bool,
    mut limits: listfile::Limits,
    control: &OperationControl,
    fetcher: &impl Fetcher,
) -> Result<Prepared> {
    control.check()?;
    if !listfile::valid_kind(kind) || offline && refresh {
        return Err(fault("records.listfile_query", 2));
    }
    // Validate explicit bounds before looking up a cache or opening a transport.
    let defaults = listfile::Limits::default();
    if limits.bytes == 0 {
        limits.bytes = defaults.bytes;
    }
    if limits.entries == 0 {
        limits.entries = defaults.entries;
    }
    if limits.line_bytes == 0 {
        limits.line_bytes = defaults.line_bytes;
    }
    if limits.bytes < 1024
        || limits.bytes > 1 << 30
        || limits.entries == 0
        || limits.entries > 16 << 20
        || limits.line_bytes < 128
        || limits.line_bytes > 16 << 20
    {
        return Err(fault("records.listfile_limit", 3));
    }
    let result = prepare_owned(home, kind, offline, refresh, limits, control, fetcher);
    finish_stop(result, control.check())
}
fn finish_stop<T>(result: Result<T>, cleanup: Result<()>) -> Result<T> {
    if let (Err(primary), Err(later)) = (&result, &cleanup) {
        if primary.code == later.code
            && primary.message == later.message
            && primary.exit_code == later.exit_code
            && primary.details == later.details
        {
            return result;
        }
    }
    lycheedev_core::metadata::owned_write::finish_owned(result, cleanup)
}
fn prepare_owned(
    home: &Path,
    kind: &str,
    offline: bool,
    refresh: bool,
    limits: listfile::Limits,
    control: &OperationControl,
    fetcher: &impl Fetcher,
) -> Result<Prepared> {
    if offline || !refresh {
        match load(home, kind, limits, control) {
            Ok(mut prepared) => {
                prepared.provenance.cache = if offline {
                    "offline-cache"
                } else {
                    "verified-cache"
                }
                .into();
                return Ok(prepared);
            }
            Err(error) if offline => return Err(wrapped_cache(kind, error)),
            Err(error)
                if ["command.cancelled", "command.deadline"].contains(&error.code.as_str()) =>
            {
                return Err(error);
            }
            // Cache validation may fall through, but a failed checked close or
            // timeout restoration is a real resource failure, not a cache miss.
            Err(error) if !error.details.is_null() => return Err(error),
            Err(error)
                if [
                    "vault.record_missing",
                    "vault.blob_integrity",
                    "records.listfile_format",
                    "records.listfile_unavailable",
                    "records.listfile_limit",
                ]
                .contains(&error.code.as_str()) => {}
            Err(error) => return Err(error),
        }
    }
    let key = format!("listfile/{kind}");
    let components: Vec<&str> = if kind == "wowexport-binary" {
        listfile::COMPONENTS.to_vec()
    } else {
        vec!["listfile"]
    };
    let mut payloads = BTreeMap::new();
    let mut files = Vec::new();
    let mut total = 0;
    for component in components {
        control.check()?;
        if total >= limits.bytes {
            return Err(fault("records.listfile_limit", 3));
        }
        let locator = listfile::source_url(kind, component)?;
        let body = fetcher
            .fetch(&locator, limits.bytes - total, control)
            .map_err(|error| {
                if ["command.cancelled", "command.deadline"].contains(&error.code.as_str()) {
                    return error;
                }
                let message = format!(
                    "records.listfile_unavailable: {kind} listfile component {:?}: {}",
                    if component == "listfile" {
                        ""
                    } else {
                        component
                    },
                    error.message
                );
                if [
                    "records.listfile_query",
                    "records.listfile_limit",
                    "records.listfile_format",
                ]
                .contains(&error.code.as_str())
                {
                    ToolkitError { message, ..error }
                } else {
                    ToolkitError {
                        code: "records.listfile_unavailable".into(),
                        message,
                        exit_code: 5,
                        ..error
                    }
                }
            })?;
        if body.len() > limits.bytes - total {
            return Err(fault("records.listfile_limit", 3));
        }
        let blob = lycheedev_core::workspace::data_disk::publish_shared_blob_with_check(
            home,
            &body,
            limits.bytes,
            control,
            &|| control.check(),
        )?;
        let sha256 = blob.sha256.clone();
        files.push(SourceFile {
            component: component.into(),
            url: locator,
            bytes: body.len() as u64,
            sha256,
            blob,
        });
        total += body.len();
        payloads.insert(component.into(), body);
    }
    let index = listfile::parse(kind, &payloads, limits, control)?;
    let provenance = Provenance {
        kind: kind.into(),
        files,
        fetched_at: storage::utc_now(),
        entry_count: index.entries.len(),
        rejected: index.rejected,
        total_bytes: total as u64,
        cache: "fetched".into(),
    };
    control.check()?;
    // Go reads the current generation only after the exact bytes are fetched,
    // authenticated and parsed. An intervening earlier cache write is replaceable.
    let expected_generation = match DataCacheMetadata::read_document_raw_controlled_with_check(
        home,
        &key,
        control,
        || Ok(()),
    ) {
        Ok(document) => Some(document.generation),
        Err(error) if error.code == "vault.record_missing" && error.details.is_null() => None,
        Err(error) => return Err(error),
    };
    let document = Document {
        schema: SCHEMA.into(),
        provenance: provenance.clone(),
    };
    // Absent means strict create; equal bytes cannot hide a lost create CAS.
    DataCacheMetadata::put_controlled_with_check(
        home,
        &key,
        &document,
        expected_generation,
        control,
        || Ok(()),
    )?;
    Ok(Prepared { index, provenance })
}
