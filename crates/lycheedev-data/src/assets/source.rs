//! One explicit public listfile source; cached bytes are authenticated before
//! parsing. No provider substitution, merged origins or implicit online fallback.
use super::{fault, listfile};
use lycheedev_core::{
    Result, ToolkitError, control::OperationControl, evidence, metadata::Metadata, storage,
    workspace,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

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
fn url(value: &str) -> Result<ureq::http::Uri> {
    let parsed: ureq::http::Uri = value
        .parse()
        .map_err(|_| fault("records.listfile_query", 2))?;
    if parsed.scheme_str() != Some("https")
        || parsed
            .authority()
            .is_none_or(|authority| authority.as_str().contains('@') || authority.host().is_empty())
    {
        return Err(fault("records.listfile_query", 2));
    }
    Ok(parsed)
}
fn redirected(base: &ureq::http::Uri, location: &str) -> Result<String> {
    let absolute = if location.starts_with("https://") || location.contains("://") {
        location.to_owned()
    } else if location.starts_with("//") {
        format!("https:{location}")
    } else if location.starts_with('/') {
        format!(
            "https://{}{location}",
            base.authority().expect("validated authority")
        )
    } else {
        let parent = base
            .path()
            .rsplit_once('/')
            .map_or("", |(parent, _)| parent);
        format!(
            "https://{}{parent}/{location}",
            base.authority().expect("validated authority")
        )
    };
    url(&absolute)?;
    Ok(absolute)
}
impl Fetcher for PublicFetcher {
    fn fetch(&self, locator: &str, maximum: usize, control: &OperationControl) -> Result<Vec<u8>> {
        control.check()?;
        if maximum == 0 {
            return Err(fault("records.listfile_query", 2));
        }
        let end = Instant::now() + control.remaining()?.min(Duration::from_secs(120));
        let mut locator = locator.to_owned();
        for hop in 0..5 {
            control.check()?;
            let parsed = url(&locator)?;
            let remaining = end
                .checked_duration_since(Instant::now())
                .ok_or_else(|| unavailable("request deadline exceeded"))?;
            let agent = ureq::Agent::config_builder()
                .max_redirects(0)
                .http_status_as_error(false)
                .timeout_global(Some(remaining))
                .tls_config(
                    ureq::tls::TlsConfig::builder()
                        .provider(ureq::tls::TlsProvider::NativeTls)
                        .root_certs(ureq::tls::RootCerts::PlatformVerifier)
                        .build(),
                )
                .build()
                .new_agent();
            let mut response = agent
                .get(&locator)
                .header("Accept-Encoding", "identity")
                .call()
                .map_err(|error| {
                    control
                        .check()
                        .err()
                        .unwrap_or_else(|| unavailable(error.to_string()))
                })?;
            let status = response.status().as_u16();
            if [301, 302, 303, 307, 308].contains(&status) {
                if hop == 4 {
                    return Err(fault("records.listfile_query", 2));
                }
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|value| value.to_str().ok())
                    .ok_or_else(|| fault("records.listfile_query", 2))?;
                locator = redirected(&parsed, location)?;
                continue;
            }
            if status != 200 {
                return Err(unavailable(format!("HTTP {status}")));
            }
            if response
                .headers()
                .get("content-length")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<u64>().ok())
                .is_some_and(|length| length > maximum as u64)
            {
                return Err(fault("records.listfile_limit", 3));
            }
            let mut bytes = Vec::new();
            let mut reader = response.body_mut().as_reader();
            let mut buffer = [0u8; 65536];
            loop {
                control.check()?;
                let requested = buffer
                    .len()
                    .min(maximum.saturating_sub(bytes.len()).saturating_add(1));
                let n = reader.read(&mut buffer[..requested]).map_err(|error| {
                    control
                        .check()
                        .err()
                        .unwrap_or_else(|| unavailable(error.to_string()))
                })?;
                control.check()?;
                if n == 0 {
                    break;
                }
                if n > maximum - bytes.len() {
                    return Err(fault("records.listfile_limit", 3));
                }
                bytes
                    .try_reserve(n)
                    .map_err(|_| fault("records.listfile_limit", 3))?;
                bytes.extend_from_slice(&buffer[..n]);
            }
            if bytes.is_empty() {
                return Err(unavailable("empty listfile response"));
            }
            return Ok(bytes);
        }
        Err(fault("records.listfile_query", 2))
    }
}

fn load(
    home: &Path,
    kind: &str,
    limits: listfile::Limits,
    control: &OperationControl,
) -> Result<Prepared> {
    control.check()?;
    let raw: serde_json::Value = Metadata::open(home, false)?.read(&format!("listfile/{kind}"))?;
    let document: Document =
        serde_json::from_value(raw).map_err(|_| fault("records.listfile_format", 4))?;
    if document.schema != SCHEMA || document.provenance.kind != kind {
        return Err(fault("records.listfile_format", 4));
    }
    let mut payloads = BTreeMap::new();
    let mut total = 0usize;
    for file in &document.provenance.files {
        control.check()?;
        let expected = listfile::source_url(kind, &file.component)?;
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
    if error.code == "vault.record_missing" {
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
        ToolkitError::new("records.listfile_unavailable", message, 5)
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
    workspace::open(home)?;
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
            Err(_) => {}
        }
    }
    let key = format!("listfile/{kind}");
    let mut metadata = Metadata::open(home, true)?;
    let expected_generation = metadata.generation(&key)?;
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
                    ToolkitError::new("records.listfile_unavailable", message, 5)
                }
            })?;
        if body.len() > limits.bytes - total {
            return Err(fault("records.listfile_limit", 3));
        }
        let blob: evidence::BlobRef = serde_json::from_value(evidence::publish_blob_bounded(
            home,
            &body,
            limits.bytes,
            control,
        )?)?;
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
    metadata.put(
        &key,
        &Document {
            schema: SCHEMA.into(),
            provenance: provenance.clone(),
        },
        expected_generation,
    )?;
    Ok(Prepared { index, provenance })
}
