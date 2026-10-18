//! Immutable probe revisions and mutable selectors; public Go wire shapes are preserved.
use lycheedev_core::{
    Invocation, Outcome, Result, ToolkitError,
    metadata::{HistoricalMutation, Metadata},
    storage, workspace,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revision {
    pub schema: String,
    pub id: String,
    pub sha256: String,
    pub bytes: usize,
    pub code: String,
    pub created_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Name {
    pub schema: String,
    pub name: String,
    pub revision: String,
    pub active: bool,
    pub updated_at: String,
}
fn name_valid(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn revision_valid(r: &Revision) -> Result<Vec<u8>> {
    let code = decode(&r.code)?;
    if r.schema != "lycheedev.probe-revision.v1"
        || r.id != format!("PRB-{}", r.sha256)
        || r.sha256 != storage::sha256(&code)
        || r.bytes != code.len()
        || code.is_empty()
        || code.len() > 262144
        || chrono::DateTime::parse_from_rfc3339(&r.created_at).is_err()
    {
        return Err(ToolkitError::new(
            "live.probe_revision_corrupt",
            "probe revision identity or content changed",
            4,
        ));
    }
    Ok(code)
}
fn validate_name(n: &Name) -> Result<()> {
    if n.schema != "lycheedev.probe-name.v1"
        || !name_valid(&n.name)
        || !n.revision.starts_with("PRB-")
        || chrono::DateTime::parse_from_rfc3339(&n.updated_at).is_err()
    {
        return Err(ToolkitError::new(
            "live.probe_name_corrupt",
            "invalid probe selector",
            4,
        ));
    }
    Ok(())
}
pub fn code(inv: &Invocation, selector: &str) -> Result<Vec<u8>> {
    let shown = show(inv, selector)?;
    let r: Revision = serde_json::from_value(shown.data["revision"].clone())?;
    revision_valid(&r)
}
pub fn show(inv: &Invocation, selector: &str) -> Result<Outcome> {
    let meta = Metadata::open_controlled(&inv.home, false, &inv.control)?;
    let result = (|| {
        let mut named = None;
        let id = if let Some(stripped) = selector.strip_prefix("PRB-") {
            if selector.len() != 68 || !lycheedev_core::selection::valid_digest(stripped, 64) {
                return Err(ToolkitError::arguments("invalid probe revision"));
            }
            selector.to_owned()
        } else {
            if !name_valid(selector) {
                return Err(ToolkitError::arguments("invalid probe name"));
            }
            let n: Name = meta.read_controlled(&format!("probe/name/{selector}"), &inv.control)?;
            validate_name(&n)?;
            if n.name != selector {
                return Err(ToolkitError::invalid("probe name identity differs"));
            }
            if !n.active {
                return Err(ToolkitError::new(
                    "live.probe_removed",
                    "probe name was removed; explicit revision remains immutable",
                    3,
                ));
            }
            let id = n.revision.clone();
            named = Some(n);
            id
        };
        let r: Revision = meta.read_controlled(&format!("probe/revision/{id}"), &inv.control)?;
        revision_valid(&r)?;
        if r.id != id {
            return Err(ToolkitError::invalid("probe revision identity differs"));
        }
        let named=named.map(|n|serde_json::to_value(n).unwrap()).unwrap_or_else(||json!({"schema":"lycheedev.probe-name.v1","name":"","revision":r.id,"active":true,"updatedAt":"0001-01-01T00:00:00Z"}));
        Ok(Outcome::new(
            json!({"name":named,"revision":r,"changed":false}),
        ))
    })();
    finish_registry(finish_registry(result, meta.close()), inv.control.check())
}
fn selector_identity(n: &Name, name: &str) -> Result<()> {
    validate_name(n)?;
    if n.name != name {
        return Err(ToolkitError::new(
            "live.probe_name_corrupt",
            "probe name identity differs",
            4,
        ));
    }
    Ok(())
}
const MAX_CODE: usize = 262144;
fn source_bytes(inv: &Invocation) -> Result<Vec<u8>> {
    source_bytes_observed(inv, || {})
}
fn source_bytes_observed(inv: &Invocation, after_stat: impl FnOnce()) -> Result<Vec<u8>> {
    inv.control.check()?;
    let mut file = fs::File::open(inv.required("--file")?)?;
    let result = (|| {
        use std::io::Read;
        let stat = file.metadata()?;
        if !stat.is_file() || stat.len() == 0 || stat.len() > MAX_CODE as u64 {
            return Err(ToolkitError::arguments(
                "probe must be a non-empty regular file of at most 256 KiB",
            ));
        }
        after_stat();
        let mut bytes = Vec::with_capacity(stat.len() as usize);
        let mut chunk = [0; 8192];
        loop {
            inv.control.check()?;
            let allowed = chunk.len().min(MAX_CODE + 1 - bytes.len());
            let n = file.read(&mut chunk[..allowed])?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() > MAX_CODE {
                return Err(ToolkitError::arguments(
                    "probe must be a non-empty regular file of at most 256 KiB",
                ));
            }
        }
        if bytes.is_empty() {
            return Err(ToolkitError::arguments(
                "probe must be a non-empty regular file of at most 256 KiB",
            ));
        }
        Ok(bytes)
    })();
    storage::finish_file(file, result, &inv.control)
}
// Keep the first typed cause and avoid reporting the same stop twice.
// The Core selection helper is private to that crate; registry consumers use
// the public historical-write cleanup contract at this owned boundary.
fn finish_registry<T>(result: Result<T>, stopped: Result<()>) -> Result<T> {
    if let (Err(first), Err(stop)) = (&result, &stopped)
        && first.code == stop.code
        && first.message == stop.message
        && first.exit_code == stop.exit_code
        && first.details == stop.details
    {
        return result;
    }
    lycheedev_core::metadata::historical_write::finish_historical(result, stopped)
}
fn write_registry<T>(
    inv: &Invocation,
    run: impl FnOnce(&lycheedev_core::metadata::HistoricalWriter<'_>) -> Result<T>,
) -> Result<T> {
    use lycheedev_core::metadata::historical_write::{finish_historical, historical_lease};
    let lease = historical_lease(&inv.home, "probe-registry", &inv.control)?;
    let result = Metadata::with_historical_write(&inv.home, &inv.control, run);
    finish_registry(
        finish_historical(result, lease.close()),
        inv.control.check(),
    )
}
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    dispatch_observed(inv, || Ok(()))
}
fn dispatch_observed(
    inv: &Invocation,
    before_commit: impl FnOnce() -> Result<()>,
) -> Result<Outcome> {
    workspace::open_controlled(&inv.home, &inv.control)?;
    match inv.command.as_str() {
        "live probe put" => {
            let name = inv.required("--name")?;
            if !name_valid(name) {
                return Err(ToolkitError::arguments("invalid probe name"));
            }
            let bytes = source_bytes(inv)?;
            let digest = storage::sha256(&bytes);
            let id = format!("PRB-{digest}");
            write_registry(inv, |writer| {
                let key = format!("probe/revision/{id}");
                let (r, fresh) = match writer.document(&key) {
                    Ok(doc) => {
                        let existing: Revision = serde_json::from_slice(&doc.bytes)?;
                        if existing.id != id || revision_valid(&existing)? != bytes {
                            return Err(ToolkitError::invalid("probe revision collision"));
                        }
                        (existing, false)
                    }
                    Err(e) if e.code == "vault.record_missing" => (
                        Revision {
                            schema: "lycheedev.probe-revision.v1".into(),
                            id,
                            sha256: digest,
                            bytes: bytes.len(),
                            code: encode(&bytes),
                            created_at: now(),
                        },
                        true,
                    ),
                    Err(e) => return Err(e),
                };
                let nk = format!("probe/name/{name}");
                let generation = match writer.document(&nk) {
                    Ok(doc) => {
                        let n: Name = serde_json::from_slice(&doc.bytes)?;
                        selector_identity(&n, name)?;
                        if n.active && n.revision == r.id {
                            return Ok(Outcome::new(
                                json!({"name":n,"revision":r,"changed":false}),
                            ));
                        }
                        doc.generation
                    }
                    Err(e) if e.code == "vault.record_missing" => 0,
                    Err(e) => return Err(e),
                };
                let n = Name {
                    schema: "lycheedev.probe-name.v1".into(),
                    name: name.into(),
                    revision: r.id.clone(),
                    active: true,
                    updated_at: now(),
                };
                let revision_bytes = serde_json::to_vec(&r)?;
                let name_bytes = serde_json::to_vec(&n)?;
                let mut changes = Vec::with_capacity(2);
                changes.push(HistoricalMutation {
                    key: &nk,
                    expected_generation: generation,
                    bytes: &name_bytes,
                });
                if fresh {
                    changes.push(HistoricalMutation {
                        key: &key,
                        expected_generation: 0,
                        bytes: &revision_bytes,
                    });
                }
                before_commit()?;
                writer.commit(&changes)?;
                Ok(Outcome::new(json!({"name":n,"revision":r,"changed":true})))
            })
        }
        "live probe list" => {
            let limit = inv.usize_flag("--limit", 100, 1, 1000)?;
            if !inv.home.join("state/toolkit.sqlite").exists() {
                inv.control.check()?;
                return Ok(Outcome::new(json!({"items":[]})));
            }
            let meta = Metadata::open_controlled(&inv.home, false, &inv.control)?;
            let result = (|| {
                let mut items = Vec::new();
                let mut after = String::new();
                while items.len() < limit {
                    let page_size = limit - items.len();
                    let (count, last) = meta.visit_raw_page_controlled_with_check(
                        "probe/name/",
                        &after,
                        page_size,
                        &inv.control,
                        || Ok(()),
                        |key, _, bytes| {
                            let n: Name = serde_json::from_slice(bytes)?;
                            selector_identity(&n, key.strip_prefix("probe/name/").unwrap_or(""))?;
                            if n.active || inv.flag_bool("--include-removed") {
                                items.push(n);
                            }
                            Ok(())
                        },
                    )?;
                    after = last;
                    if count < page_size {
                        break;
                    }
                }
                Ok(Outcome::new(json!({"items":items})))
            })();
            finish_registry(finish_registry(result, meta.close()), inv.control.check())
        }
        "live probe show" => show(inv, inv.argument()?),
        "live probe remove" => {
            let name = inv.argument()?;
            if !name_valid(name) {
                return Err(ToolkitError::arguments("invalid probe name"));
            }
            write_registry(inv, |writer| {
                let key = format!("probe/name/{name}");
                let doc = writer.document(&key)?;
                let mut n: Name = serde_json::from_slice(&doc.bytes)?;
                selector_identity(&n, name)?;
                if n.active {
                    n.active = false;
                    n.updated_at = now();
                    let bytes = serde_json::to_vec(&n)?;
                    writer.commit(&[HistoricalMutation {
                        key: &key,
                        expected_generation: doc.generation,
                        bytes: &bytes,
                    }])?;
                }
                Ok(Outcome::new(serde_json::to_value(n)?))
            })
        }
        _ => Err(ToolkitError::arguments("unknown probe registry command")),
    }
}
fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
}
const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | u32::from(*c.get(1).unwrap_or(&0)) << 8
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            out.push(if i > c.len() {
                '='
            } else {
                B64[((n >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    out
}
fn decode(text: &str) -> Result<Vec<u8>> {
    if text.len() > 349528 || !text.len().is_multiple_of(4) {
        return Err(ToolkitError::invalid("invalid probe base64"));
    }
    let mut out = Vec::new();
    for c in text.as_bytes().chunks(4) {
        let mut n = 0;
        for &b in c {
            n = (n << 6)
                | if b == b'=' {
                    0
                } else {
                    B64.iter()
                        .position(|v| *v == b)
                        .ok_or_else(|| ToolkitError::invalid("invalid probe base64"))?
                        as u32
                };
        }
        out.push((n >> 16) as u8);
        if c[2] != b'=' {
            out.push((n >> 8) as u8)
        }
        if c[3] != b'=' {
            out.push(n as u8)
        }
    }
    if encode(&out) != text {
        return Err(ToolkitError::invalid("noncanonical probe base64"));
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_source_identities_preserved() {
        for bytes in [&b"return 7\n"[..], &[0, 255, 27][..], &[1][..], &[1, 2][..]] {
            assert_eq!(decode(&encode(bytes)).unwrap(), bytes)
        }
        assert!(!name_valid("../probe"));
    }
}

#[cfg(test)]
#[path = "probes/registry_tests.rs"]
mod registry_tests;
