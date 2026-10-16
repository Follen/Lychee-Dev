//! Original-byte acceleration is disposable. Durable capture sources continue
//! through evidence publication at the callers that actually expose BlobRefs.
use super::{Document, git};
use lycheedev_core::{
    Result, ToolkitError,
    cache::SourceOriginalCache,
    control::OperationControl,
    selection::{SourcePin, valid_digest},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

fn matches(raw: &[u8], doc: &Document, control: &OperationControl) -> Result<bool> {
    if raw.len() != doc.bytes || std::str::from_utf8(raw).is_err() {
        return Ok(false);
    }
    let mut hash = Sha256::new();
    for block in raw.chunks(65536) {
        control.check()?;
        hash.update(block);
    }
    control.check()?;
    Ok(format!("{:x}", hash.finalize()) == doc.sha256)
}
pub(super) fn read(
    home: &Path,
    summary: &Value,
    doc: &Document,
    control: &OperationControl,
) -> Result<Vec<u8>> {
    control.check()?;
    if doc.bytes > git::MAX_SOURCE_BYTES {
        return Err(ToolkitError::new(
            "vault.blob_limit",
            "blob exceeds read budget",
            4,
        ));
    }
    if !valid_digest(&doc.sha256, 64) || !git::source_path(&doc.path) {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Invalid indexed original identity",
            4,
        ));
    }
    let cache = SourceOriginalCache::open(home, control)?;
    let result = (|| {
        if let Some(raw) = cache.read(&doc.sha256, control)?
            && matches(&raw, doc, control)?
        {
            return Ok(raw);
        }
        // The manifest/query image authenticated these fields before creating
        // the Index. Requested ref is deliberately not used: recovery cannot
        // resolve a moving branch or silently adopt newer source bytes.
        let pin = SourcePin {
            repository: summary["repository"].as_str().unwrap_or("").into(),
            product: summary["product"].as_str().unwrap_or("").into(),
            exact_commit: summary["commit"].as_str().unwrap_or("").into(),
            parser_revision: summary["parser"].as_str().unwrap_or("").into(),
            requested_ref: String::new(),
        };
        let (raw, _) =
            git::blob_with_control(home, &pin, &doc.path, git::MAX_SOURCE_BYTES, control)?;
        if !matches(&raw, doc, control)? {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Indexed original differs from the fixed Git object",
                4,
            ));
        }
        cache.publish(&doc.sha256, &raw, control)?;
        Ok(raw)
    })();
    cache.finish(result, control)
}
