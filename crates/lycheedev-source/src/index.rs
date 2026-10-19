mod cache_projection;
mod facts_cache;
#[cfg(test)]
mod facts_decode_tests;
#[cfg(test)]
mod facts_encode_tests;
#[cfg(test)]
mod symbol_identity_tests;
mod facts_pipeline;
mod original_cache;
mod persistence_wire;
#[cfg(test)]
pub(crate) use original_cache::take_read_observations;
mod profile;
use crate::{
    facts::{self, Facts},
    git,
};
use lycheedev_core::{
    Result, ToolkitError, evidence, metadata::historical_write::finish_historical,
    selection::SourcePin, storage,
};
pub(crate) use cache_projection::build_summary_with_control;
use profile::{Phase, Profile};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Symbol {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    pub kind: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    pub category: String,
    pub confidence: String,
    pub path: String,
    pub line: usize,
    pub end_line: usize,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub signature: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub ordinal: usize,
}
fn is_zero(v: &usize) -> bool {
    *v == 0
}
fn render_excerpt(raw: &[u8], first: usize, last: usize) -> String {
    String::from_utf8_lossy(raw)
        .replace("\r\n", "\n")
        .split('\n')
        .enumerate()
        .filter(|(i, _)| i + 1 >= first && *i < last.min(first + 79))
        .map(|(i, l)| format!("{}: {}", i + 1, l.trim_end_matches('\r')))
        .collect::<Vec<_>>()
        .join("\n")
}
fn render_span(raw: &[u8], hash: &str, first: usize, last: usize) -> Value {
    let mut starts = vec![0usize];
    for (i, b) in raw.iter().enumerate() {
        if *b == b'\n' && i + 1 < raw.len() {
            starts.push(i + 1);
        }
    }
    let end_line = last.max(first).min(starts.len());
    json!({"firstLine":first,"lastLine":end_line,"startByte":starts.get(first.saturating_sub(1)),"endByte":starts.get(end_line).copied().unwrap_or(raw.len()),"coordinate":"utf8-byte","contentHash":hash})
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Document {
    pub path: String,
    #[serde(skip)]
    // Some(empty) is an authenticated empty document; None needs recovery.
    pub text: Option<Vec<u8>>,
    pub sha256: String,
    pub bytes: usize,
    #[serde(skip)]
    pub facts: Facts,
    #[serde(default, rename = "factsLocator", skip_serializing_if = "Option::is_none")]
    pub(crate) facts_locator: Option<facts_cache::Locator>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadFact {
    pub path: String,
    pub line: usize,
    pub kind: String,
    /// Exact parser-observed relative load target; not a proved runtime load.
    pub target: String,
    pub source_blob: Value,
}
/// Authenticate the immutable XML/TOC subset, then expose existing parser load
/// facts. Warm Index::Document.facts is intentionally skipped in serialization;
/// its default empty value is never evidence that a document has no loads.
pub fn load_facts_with_control(
    home: &Path,
    pin: &SourcePin,
    index: &Index,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Vec<LoadFact>> {
    control.check()?;
    let documents = load_documents(index)?;
    let files = git::tree_with_control(home, pin, control)?
        .into_iter()
        .filter(|f| documents.contains_key(f.path.as_str()))
        .collect::<Vec<_>>();
    if files.len() != documents.len() {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Load index/tree document identity differs",
            4,
        ));
    }
    let mut total = 0usize;
    let mut result = Vec::new();
    git::visit_blobs_with_control(home, pin, &files, control, |file, raw| {
        append_load_facts(
            home,
            documents[file.path.as_str()],
            &raw,
            control,
            &mut total,
            &mut result,
        )
    })?;
    Ok(result)
}
/// Reuse bytes already read from the pinned tree. The index authenticates each
/// XML/TOC path, size and digest before cached facts or original-byte evidence
/// are exposed; unrelated source documents do not affect this subset.
pub(crate) fn load_facts_from_documents_with_control(
    home: &Path,
    index: &Index,
    raw_documents: &BTreeMap<String, String>,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Vec<LoadFact>> {
    control.check()?;
    let documents = load_documents(index)?;
    let mut total = 0usize;
    let mut result = Vec::new();
    for document in index
        .documents
        .iter()
        .filter(|d| documents.contains_key(d.path.as_str()))
    {
        let raw = raw_documents.get(&document.path).ok_or_else(|| {
            ToolkitError::new(
                "codebase.source_integrity",
                "Load index/tree document identity differs",
                4,
            )
        })?;
        append_load_facts(
            home,
            document,
            raw.as_bytes(),
            control,
            &mut total,
            &mut result,
        )?;
    }
    Ok(result)
}
fn load_documents(index: &Index) -> Result<BTreeMap<&str, &Document>> {
    let documents = index
        .documents
        .iter()
        .filter(|d| {
            let path = d.path.to_ascii_lowercase();
            path.ends_with(".xml") || path.ends_with(".toc")
        })
        .map(|d| (d.path.as_str(), d))
        .collect::<BTreeMap<_, _>>();
    if documents.len() > 20000 {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "Load evidence document count exceeds budget",
            3,
        ));
    }
    Ok(documents)
}
fn append_load_facts(
    home: &Path,
    document: &Document,
    raw: &[u8],
    control: &lycheedev_core::control::OperationControl,
    total: &mut usize,
    result: &mut Vec<LoadFact>,
) -> Result<()> {
    control.check()?;
    *total = total.checked_add(raw.len()).ok_or_else(|| {
        ToolkitError::new("codebase.source_budget", "Load source size overflow", 3)
    })?;
    if *total > 128 << 20 {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "Load source bytes exceed128MiB",
            3,
        ));
    }
    if document.bytes != raw.len() || document.sha256 != storage::sha256(raw) {
        return Err(ToolkitError::new(
            "codebase.source_integrity",
            "Load evidence blob differs from indexed source",
            4,
        ));
    }
    let packed = if let Some(locator) = &document.facts_locator {
        facts_cache::read_locator(home, locator, &document.path, &document.sha256, control)?
    } else { None };
    let parsed = match packed {
        Some(facts) => facts,
        None => document_facts(home, &document.path, raw, &document.sha256, control)?.0,
    };
    let blob = evidence::publish_blob_bounded(home, raw, git::MAX_SOURCE_BYTES, control)?;
    for load in parsed.loads {
        control.check()?;
        if result.len() >= 100000 {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Load fact count exceeds100000",
                3,
            ));
        }
        result.push(LoadFact {
            path: document.path.clone(),
            line: load.line,
            kind: load.kind,
            target: load.path,
            source_blob: blob.clone(),
        });
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
pub struct Index<D = Vec<Document>, A = Vec<Value>, S = Vec<Symbol>> {
    pub documents: D,
    pub assets: A,
    pub symbols: S,
    pub summary: Value,
    // At most six compatibility categories, derived from every diagnostic,
    // before the public sample is capped. Required when deserializing caches.
    #[serde(rename = "compatibilityGaps")]
    pub(crate) compatibility_gaps: BTreeSet<String>,
    #[serde(skip)]
    pub(crate) home: PathBuf,
}
pub fn stable_id(pin: &SourcePin, s: &Symbol) -> String {
    SymbolIdentity::for_document(pin, &s.path).stable_id(s)
}
/// The identity prefix is identical for every declaration in one document.
/// Reuse only SHA state, never a result or an unauthenticated source identity.
/// The public helper above still derives the prefix from its own Symbol path.
struct SymbolIdentity(sha2::Sha256);
struct SymbolHashWriter<'a>(&'a mut sha2::Sha256);
impl std::io::Write for SymbolHashWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest;
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl SymbolIdentity {
    fn for_document(pin: &SourcePin, path: &str) -> Self {
        use sha2::Digest;
        let mut hash = sha2::Sha256::new();
        for field in [pin.repository.as_str(), pin.exact_commit.as_str(), path] {
            hash.update(field.as_bytes());
            hash.update(b"\0");
        }
        Self(hash)
    }
    fn stable_id(&self, symbol: &Symbol) -> String {
        use sha2::Digest;
        use std::io::Write;
        let mut hash = self.0.clone();
        // Same Display decimal coordinates and exact string bytes as the old
        // complete format! input, without allocating that intermediate String.
        write!(
            SymbolHashWriter(&mut hash),
            "{}\0{}\0{}\0{}\0{}\0{}",
            symbol.line,
            symbol.end_line,
            symbol.name,
            symbol.category,
            symbol.scope,
            symbol.ordinal
        )
        .expect("SHA-256 writer accepts every formatted byte");
        let digest = hash.finalize();
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut result = String::with_capacity(36);
        result.push_str("SYM-");
        for byte in &digest[..16] {
            result.push(HEX[(byte >> 4) as usize] as char);
            result.push(HEX[(byte & 15) as usize] as char);
        }
        result
    }
}
pub fn role(path: &str) -> &'static str {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    if p.contains("apidocumentationgenerated") {
        "official-generated-api"
    } else if p.contains("locale") || p.contains("localization") {
        "locale"
    } else if p.contains("libs/") || p.contains("vendor/") {
        "vendor"
    } else if p.contains("modelpaths") || p.contains("generated") {
        "generated-data"
    } else if p.contains("tools/") || p.ends_with("babelfish.lua") {
        "tool"
    } else {
        "project"
    }
}
pub fn penalty(role: &str) -> i32 {
    match role {
        "vendor" => 15,
        "locale" | "generated-data" | "tool" => 20,
        _ => 0,
    }
}
pub(crate) fn fingerprint(pin: &SourcePin) -> String {
    storage::sha256(format!("rust-index-v17\0assets-v2-go323-metadata\0facts-pack-v2-path-radix512\0facts-layout-v13-postcard-1.1.3\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}",pin.repository,pin.product,pin.exact_commit,pin.parser_revision).as_bytes())
}
pub fn build(home: &Path, pin: &SourcePin) -> Result<Index> {
    build_with_control(home, pin, &Default::default())
}
pub fn build_with_control(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Index> {
    build_material_with_control(home, pin, control).map(|(index, _)| index)
}
/// Return the digest of the bytes authenticated by this invocation, rather than
/// rereading a mutable metadata pointer when another consumer needs the identity.
pub(crate) fn build_material_with_control(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<(Index, String)> {
    build_material_projected_with_control(home, pin, control)
}
// Both consumers authenticate the same complete bytes and use the same serde
// schema, limits, error order and checked-close boundary. Only collection
// retention differs; the ordinary public Index keeps all three Vecs.
fn build_material_projected_with_control<D, A, S>(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<(Index<D, A, S>, String)>
where
    D: serde::de::DeserializeOwned + cache_projection::CollectionLength + From<Vec<Document>>,
    A: serde::de::DeserializeOwned + cache_projection::CollectionLength + From<Vec<Value>>,
    S: serde::de::DeserializeOwned + cache_projection::CollectionLength + From<Vec<Symbol>>,
{
    let profile = Profile::new();
    let metadata_phase = profile.span(Phase::Metadata);
    control.check()?;
    let identity = fingerprint(pin);
    let key = format!("source/rust-index/{identity}");
    let metadata = match lycheedev_core::metadata::Metadata::open_controlled(home, false, control) {
        Ok(m) => m,
        Err(e) if e.code == "vault.record_missing" && e.details.is_null() => {
            lycheedev_core::metadata::Metadata::open_controlled(home, true, control)?
        }
        Err(e) => return Err(e),
    };
    let manifest = metadata.read_controlled::<Value>(&key, control);
    let manifest = finish_historical(manifest, metadata.close());
    drop(metadata_phase);
    if let Ok(manifest) = &manifest {
        if manifest["schema"] == "lycheedev.source-index.rust.v6"
            && manifest["identity"] == identity
        {
            let digest = manifest["blob"]["sha256"].as_str().unwrap_or("");
            if !lycheedev_core::selection::valid_digest(digest, 64) {
                return Err(ToolkitError::new(
                    "codebase.source_integrity",
                    "Invalid cached index digest",
                    4,
                ));
            }
            let path = home
                .join("cache/objects")
                .join(&digest[..2])
                .join(&digest[2..]);
            if path.exists() {
                let _phase = profile.span(Phase::CachedIndex);
                storage::guard_path(home, &path)?;
                let started = std::time::Instant::now();
                let mut raw = vec![];
                let mut file = std::fs::File::open(&path)?.take((128 << 20) + 1);
                let loaded = (|| {
                    let mut chunk = [0; 65536];
                    loop {
                        control.check()?;
                        let n = file.read(&mut chunk)?;
                        if n == 0 {
                            break;
                        }
                        raw.extend_from_slice(&chunk[..n]);
                    }
                    if raw.len() > 128 << 20
                        || manifest["blob"]["bytes"] != raw.len()
                        || storage::sha256(&raw) != digest
                    {
                        return Err(ToolkitError::new(
                            "codebase.source_integrity",
                            "Cached index content differs",
                            4,
                        ));
                    }
                    let mut map: Index<D, A, S> = serde_json::from_slice(&raw)?;
                    control.check()?;
                    if map.documents.len() > 100000
                        || map.symbols.len() > 2000000
                        || map.assets.len() > 200000
                        || map.summary["commit"] != pin.exact_commit
                        || map.summary["repository"] != pin.repository
                        || map.summary["product"] != pin.product
                        || map.summary["parser"] != pin.parser_revision
                    {
                        return Err(ToolkitError::new(
                            "codebase.source_integrity",
                            "Cached index identity or budget differs",
                            4,
                        ));
                    }
                    map.home = home.into();
                    map.summary["io"] = json!({"gitTreeProcesses":0,"gitBlobProcesses":0,"cachedIndex":true,"indexBytesRead":raw.len(),"sourceBytesRead":0,"cachedFacts":map.documents.len(),"parsedDocuments":0,"elapsedMs":started.elapsed().as_millis(),"deadlineMs":180000});
                    Ok((map, digest.to_owned()))
                })();
                return storage::finish_file(file.into_inner(), loaded, control);
            }
        }
    } else if manifest.as_ref().unwrap_err().code != "vault.record_missing"
        || !manifest.as_ref().unwrap_err().details.is_null()
    {
        return Err(manifest.unwrap_err());
    }
    let lock_phase = profile.span(Phase::IndexLock);
    let lock = storage::ExclusiveLock::acquire_controlled(
        &home
            .join("locks")
            .join(format!("source-index-{identity}.lock")),
        control,
    )?;
    drop(lock_phase);
    let result = (|| {
        let map = build_uncached_profiled(home, pin, control, &profile)?;
        let serialize_phase = profile.span(Phase::IndexSerialize);
        let raw = persistence_wire::encode(&map)?;
        drop(serialize_phase);
        let publish_phase = profile.span(Phase::IndexPublish);
        let blob =
            lycheedev_core::cache::publish_derived_controlled(home, &raw, 128 << 20, control)?;
        drop(publish_phase);
        let _manifest_phase = profile.span(Phase::Manifest);
        crate::persistence::put_manifest(
            home,
            &key,
            &json!({"schema":"lycheedev.source-index.rust.v6","identity":identity,"blob":blob}),
            control,
        )?;
        Ok((map, blob["sha256"].as_str().unwrap().to_owned()))
    })();
    let (map, digest) =
        crate::persistence::finish_control(finish_historical(result, lock.close()), control.check())?;
    // The cold path still builds, serializes and publishes the complete index.
    // Move its fields only after the original lock close and final control.
    Ok((
        Index {
            documents: map.documents.into(),
            assets: map.assets.into(),
            symbols: map.symbols.into(),
            summary: map.summary,
            compatibility_gaps: map.compatibility_gaps,
            home: map.home,
        },
        digest,
    ))
}
/// Existing authenticated-index registration gate. Readiness never builds or
/// hashes a replacement: build() authenticates the cached content separately.
pub fn is_ready(home: &Path, pin: &SourcePin) -> Result<bool> {
    is_ready_with_control(home, pin, &Default::default())
}
pub fn is_ready_with_control(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<bool> {
    control.check()?;
    let identity = fingerprint(pin);
    let metadata = lycheedev_core::metadata::Metadata::open_controlled(home, false, control)?;
    let result = (|| {
        let manifest = match metadata
            .read_controlled::<Value>(&format!("source/rust-index/{identity}"), control)
        {
            Ok(v) => v,
            Err(e) if e.code == "vault.record_missing" && e.details.is_null() => return Ok(false),
            Err(e) => return Err(e),
        };
        if manifest["schema"] != "lycheedev.source-index.rust.v6"
            || manifest["identity"] != identity
        {
            return Ok(false);
        }
        let digest = manifest["blob"]["sha256"].as_str().unwrap_or("");
        if !lycheedev_core::selection::valid_digest(digest, 64) {
            return Err(ToolkitError::new(
                "codebase.source_integrity",
                "Invalid cached index identity",
                4,
            ));
        }
        let path = home
            .join("cache/objects")
            .join(&digest[..2])
            .join(&digest[2..]);
        storage::guard_path(home, &path)?;
        match std::fs::metadata(path) {
            Ok(m) => Ok(m.is_file()
                && m.len() <= 128 << 20
                && manifest["blob"]["bytes"].as_u64() == Some(m.len())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    })();
    crate::persistence::finish_control(finish_historical(result, metadata.close()), control.check())
}
fn facts_cache_key(path: &str, sha256: &str) -> String {
    storage::sha256(format!(
        "facts-layout-v13-postcard-1.1.3\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0{path}\0{sha256}"
    ).as_bytes())
}
fn facts_cache_path(home: &Path, path: &str, sha256: &str) -> PathBuf {
    home.join("cache/source-facts-rust").join(format!("{}.json", facts_cache_key(path, sha256)))
}
/// Return the complete observations and whether they came from the existing
/// authenticated per-document cache. Both indexing and load evidence use this
/// one parser/cache contract.
fn document_facts(
    home: &Path,
    path: &str,
    data: &[u8],
    sha256: &str,
    control: &lycheedev_core::control::OperationControl,
) -> Result<(Facts, bool)> {
    let cache = lycheedev_core::cache::SourceFactsCache::open(home, control)?;
    let result = document_facts_profiled(home, path, data, sha256, control, None, &cache, None);
    cache.finish(result, control)
}
fn document_facts_profiled(
    home: &Path,
    path: &str,
    data: &[u8],
    sha256: &str,
    control: &lycheedev_core::control::OperationControl,
    profile: Option<&Profile>,
    cache_owner: &lycheedev_core::cache::SourceFactsCache,
    prepared: Option<facts_pipeline::PreparedFacts>,
) -> Result<(Facts, bool)> {
    control.check()?;
    let cache = facts_cache_path(home, path, sha256);
    // A corrupt or obsolete rebuildable cache is never evidence. Read at most
    // the fixed Go 32 MiB limit plus one byte, then regenerate from authenticated
    // input bytes. Cancellation and owned-handle close failures still propagate.
    let key = cache
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("hashed cache name");
    {
        let _phase = profile.map(|p| p.span(Phase::FactsRead));
        if let Some(raw) = cache_owner.read(key, control)?
            && let Some(parsed) = decode_facts(&raw, sha256, control)?
        {
            return Ok((parsed, true));
        }
    }
    let parsed = parse_document_facts(path, data, control, profile, prepared)?;
    let envelope = encode_facts(&parsed, sha256, profile)?;
    let _publish_phase = profile.map(|p| p.span(Phase::FactsPublish));
    if let Some(profile) = profile.filter(|profile| profile.is_enabled()) {
        use lycheedev_core::cache::SourceCachePublicationPhase as Published;
        cache_owner.publish_profiled(key, &envelope, control, &mut |phase, elapsed| {
            profile.record(
                match phase {
                    Published::Lock => Phase::FactsPublishLock,
                    Published::Stage => Phase::FactsPublishStage,
                    Published::Write => Phase::FactsPublishWrite,
                    Published::Seal => Phase::FactsPublishSeal,
                    Published::Rename => Phase::FactsPublishRename,
                    Published::StageClose => Phase::FactsPublishStageClose,
                    Published::LockClose => Phase::FactsPublishLockClose,
                    Published::LockVerify => Phase::FactsPublishLockVerify,
                    Published::LockOpen => Phase::FactsPublishLockOpen,
                    Published::LockKernelAttempt => Phase::FactsPublishLockKernelAttempt,
                    Published::LockContentionBackoff => Phase::FactsPublishLockContentionBackoff,
                    Published::LockFailureClose => Phase::FactsPublishLockFailureClose,
                },
                elapsed,
            );
        })?;
    } else {
        cache_owner.publish(key, &envelope, control)?;
    }
    control.check().map_err(|mut error| {
        error.details = json!({"schema":"lycheedev.source-persistence.v1","path":cache,"mutation":"facts","state":"committed","cause":error.clone()});
        error
    })?;
    Ok((parsed, false))
}
fn parse_document_facts(
    path: &str, data: &[u8], control: &lycheedev_core::control::OperationControl,
    profile: Option<&Profile>, prepared: Option<facts_pipeline::PreparedFacts>,
) -> Result<Facts> {
    let mode = facts::ParseMode::for_path(path);
    let parsed = if let Some(prepared) = prepared.filter(|prepared| prepared.mode == mode) {
        if let Some(profile) = profile {
            profile.record(Phase::FactsParse, prepared.parse_elapsed);
        }
        prepared.facts
    } else {
        let _parse_phase = profile.map(|p| p.span(Phase::FactsParse));
        let _sequential_phase = profile.map(|p| p.span(Phase::FactsParseSequential));
        mode.parse(data)
    };
    // The original control decides stop/error ordering at the original stage;
    // speculative workers only observe its cancellation and absolute deadline.
    control.check()?;
    Ok(parsed)
}
fn encode_facts(parsed: &Facts, sha256: &str, profile: Option<&Profile>) -> Result<Vec<u8>> {
    let _phase = profile.map(|p| p.span(Phase::FactsSerialize));
    facts::binary::encode(parsed, sha256)
}
fn decode_facts(raw: &[u8], sha256: &str, control: &lycheedev_core::control::OperationControl) -> Result<Option<Facts>> {
    facts::binary::decode(raw, sha256, control)
}

#[cfg(test)]
fn build_uncached(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Index> {
    build_uncached_profiled(home, pin, control, &Profile::new())
}
fn build_uncached_profiled(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
    profile: &Profile,
) -> Result<Index> {
    let started = std::time::Instant::now();
    let tree_phase = profile.span(Phase::GitTree);
    let files = git::tree_with_control(home, pin, control)?;
    drop(tree_phase);
    let cache_owner = lycheedev_core::cache::SourceFactsCache::open(home, control)?;
    let result = build_uncached_cached(home, pin, control, profile, &files, started, &cache_owner);
    cache_owner.finish(result, control)
}
fn build_uncached_cached(
    home: &Path,
    pin: &SourcePin,
    control: &lycheedev_core::control::OperationControl,
    profile: &Profile,
    files: &[git::TreeFile],
    started: std::time::Instant,
    cache_owner: &lycheedev_core::cache::SourceFactsCache,
) -> Result<Index> {
    let deadline = control.deadline();
    let (mut cached, mut parsed_documents, mut read_bytes) = (0usize, 0usize, 0usize);
    let mut index = Index {
        documents: vec![],
        assets: vec![],
        symbols: vec![],
        summary: Value::Null,
        compatibility_gaps: BTreeSet::new(),
        home: home.into(),
    };
    let (mut skipped, mut diagnostics, mut relationships) = (0, Vec::new(), 0usize);
    let document_count = files.iter().filter(|file| !file.asset && !file.oversized).count();
    if document_count.checked_mul(facts_cache::LOCATOR_RETAINED_BYTES)
        .and_then(|n| n.checked_add(facts_cache::GROUP_RETAINED_BYTES))
        .is_none_or(|n| n > 256 << 20) {
        return Err(ToolkitError::new("codebase.source_budget", "Source document metadata exceeds budget", 3));
    }
    index.documents.try_reserve_exact(document_count).map_err(|_| ToolkitError::new(
        "codebase.source_budget", "Source document metadata was not admitted", 3,
    ))?;
    let metadata_retained = index.documents.capacity().checked_mul(facts_cache::LOCATOR_RETAINED_BYTES)
        .and_then(|n| n.checked_add(facts_cache::GROUP_RETAINED_BYTES))
        .filter(|n| *n <= 256 << 20).ok_or_else(|| ToolkitError::new(
            "codebase.source_budget", "Source document metadata exceeds budget", 3,
        ))?;
    let mut retained = metadata_retained;
    for file in files {
        if file.asset && file.bytes > 64 << 20 {
            index
                .assets
                .push(crate::assets::record(&file.path, file.bytes, &[], "", Value::Null));
        }
    }
    let mut grouped = facts_cache::Groups::new(cache_owner, files, control, profile);
    let visit_phase = profile.span(Phase::GitBlobs);
    let batches = facts_pipeline::visit(
        home,
        pin,
        files,
        control,
        profile,
        metadata_retained,
        grouped.consumer(|grouped, file, data, prepared| {
            control.check()?;
            if std::time::Instant::now() >= deadline {
                return Err(ToolkitError::new(
                    "codebase.source_budget",
                    "Source execution deadline exceeded",
                    3,
                ));
            }
            read_bytes += data.len();
            if file.asset {
                let hash = storage::sha256(&data);
                let publish_phase = profile.span(Phase::OriginalPublish);
                let local = evidence::publish_blob_bounded(home, &data, 64 << 20, control)?;
                drop(publish_phase);
                index.assets.push(crate::assets::record(
                    &file.path, file.bytes, &data, &hash, local,
                ));
                return Ok(());
            }
            let sha256 = document_input_sha(file, &data, &mut retained)?;
            let (facts, cache_hit, facts_locator) = grouped.document(
                file, &data, &sha256, prepared, &mut index.documents,
            )?;
            if cache_hit {
                cached += 1;
            } else {
                parsed_documents += 1;
            }
            let _symbols_phase = profile.span(Phase::Symbols);
            if !facts.declarations.is_empty() {
                let identity = SymbolIdentity::for_document(pin, &file.path);
                for (ordinal, d) in facts.declarations.iter().enumerate() {
                    let mut s = Symbol {
                        id: String::new(),
                        kind: "declaration".into(),
                        name: d.name.clone(),
                        target: String::new(),
                        category: d.category.clone(),
                        confidence: "exact".into(),
                        path: file.path.clone(),
                        line: d.line,
                        end_line: d.end_line,
                        signature: d.signature.clone(),
                        scope: d.scope.clone(),
                        ordinal,
                    };
                    s.id = identity.stable_id(&s);
                    index.symbols.push(s);
                }
            }
            for r in &facts.relationships {
                relationships += 1;
                index.symbols.push(Symbol {
                    id: String::new(),
                    kind: "relationship".into(),
                    name: r.from.clone(),
                    target: r.to.clone(),
                    category: r.category.clone(),
                    confidence: r.confidence.clone(),
                    path: file.path.clone(),
                    line: r.line,
                    end_line: r.line,
                    signature: String::new(),
                    scope: String::new(),
                    ordinal: 0,
                });
            }
            for load in &facts.loads {
                index.symbols.push(Symbol {
                    id: String::new(),
                    kind: "load".into(),
                    name: file.path.clone(),
                    target: load.path.clone(),
                    category: load.kind.clone(),
                    confidence: "exact".into(),
                    path: file.path.clone(),
                    line: load.line,
                    end_line: load.line,
                    signature: String::new(),
                    scope: String::new(),
                    ordinal: 0,
                });
            }
            for h in &facts.headers {
                index.symbols.push(Symbol {
                    id: String::new(),
                    kind: "header".into(),
                    name: h.key.clone(),
                    target: h.value.clone(),
                    category: "toc-field".into(),
                    confidence: "exact".into(),
                    path: file.path.clone(),
                    line: h.line,
                    end_line: h.line,
                    signature: String::new(),
                    scope: String::new(),
                    ordinal: 0,
                });
            }
            for n in &facts.diagnostics {
                diagnostics.push(json!({"path":file.path,"line":n.line,"message":n.message}));
            }
            index.documents.push(Document {
                path: file.path.clone(),
                text: Some(data),
                sha256,
                bytes: file.bytes,
                facts,
                facts_locator,
            });
            Ok(())
        }),
    );
    let batches = grouped.finish(batches)?;
    drop(visit_phase);
    // Go emits skipped oversized documents after all analyzed documents.
    for file in files.iter().filter(|file| file.oversized) {
        skipped += 1;
        diagnostics.push(json!({"path":file.path,"line":0,"message":format!("source file has {} bytes, exceeding the {}-byte analysis limit; skipped",file.bytes,git::MAX_SOURCE_BYTES)}));
    }
    for diagnostic in &diagnostics {
        if let Some(path) = diagnostic["path"].as_str() {
            record_compatibility_gap(&mut index.compatibility_gaps, path);
        }
    }
    let failed_paths: BTreeSet<_> = diagnostics
        .iter()
        .filter_map(|d| d["path"].as_str())
        .collect();
    let ast_files = index
        .documents
        .iter()
        .filter(|d| !failed_paths.contains(d.path.as_str()))
        .count();
    index.summary = json!({"schema":"lycheedev.source-map.v2","storage":"file-cache","repository":pin.repository,"product":pin.product,"commit":pin.exact_commit,"parser":pin.parser_revision,"documents":index.documents.len(),"astFiles":ast_files,"skippedDocuments":skipped,"declarations":index.symbols.iter().filter(|s|s.kind=="declaration").count(),"relationships":relationships,"assets":index.assets.len(),"diagnostics":diagnostics.len(),"complete":skipped==0&&diagnostics.is_empty()});
    index.summary["io"] = json!({"gitTreeProcesses":1,"gitBlobProcesses":batches,"cachedFacts":cached,"parsedDocuments":parsed_documents,"readBytes":read_bytes,"elapsedMs":started.elapsed().as_millis(),"deadlineMs":180000});
    if !diagnostics.is_empty() {
        index.summary["diagnosticSample"] =
            json!(diagnostics.into_iter().take(20).collect::<Vec<_>>());
    }
    Ok(index)
}

// The ordered consumer's original retention/encoding prelude remains before
// every cache error, including a deferred read-admission error for this row.
fn document_input_sha(file: &git::TreeFile, data: &[u8], retained: &mut usize) -> Result<String> {
    *retained = retained.checked_add(file.bytes).ok_or_else(|| {
        ToolkitError::new(
            "codebase.source_budget",
            "Source memory admission overflow",
            3,
        )
    })?;
    if *retained > 256 << 20 {
        return Err(ToolkitError::new(
            "codebase.source_budget",
            "Source query memory budget exceeded",
            3,
        ));
    }
    if std::str::from_utf8(data).is_err() {
        // Fixed Go AnalyzeDocument returns an untyped error here, which
        // aborts the index and maps to command.failed / dispatch / exit 5.
        // It is not a recoverable parse note or an oversized-file skip.
        return Err(ToolkitError::new(
            "command.failed",
            "codebase.analysis_encoding_or_budget",
            5,
        ));
    }
    Ok(storage::sha256(data))
}
fn record_compatibility_gap(gaps: &mut BTreeSet<String>, path: &str) {
    let path = path.to_ascii_lowercase();
    let categories: &[&str] = if path.ends_with(".lua") {
        if path.contains("/blizzard_apidocumentationgenerated/") {
            &["mixin", "api", "event"]
        } else {
            &["mixin"]
        }
    } else if path.ends_with(".xml") {
        &["template", "frame-type"]
    } else if path.ends_with(".toc") {
        &["interface"]
    } else {
        &[]
    };
    gaps.extend(categories.iter().map(|category| (*category).to_owned()));
}
fn check_search(control: Option<&lycheedev_core::control::OperationControl>) -> Result<()> {
    control.map_or(Ok(()), lycheedev_core::control::OperationControl::check)
}
fn coverage(summary: &Value, documents: usize) -> Value {
    let mut v = json!({"analyzedDocuments":documents,"skippedDocuments":summary["skippedDocuments"],"diagnostics":summary["diagnostics"],"complete":summary["complete"]});
    if let Some(s) = summary.get("diagnosticSample") {
        v["diagnosticSample"] = s.clone();
    }
    v["io"] = summary["io"].clone();
    v
}
impl Index {
    pub(crate) fn original_reader(&self) -> original_cache::OriginalReader<'_> {
        original_cache::OriginalReader::new(self)
    }
    #[cfg(test)]
    pub(crate) fn read_document_controlled(
        &self,
        doc: &Document,
        control: Option<&lycheedev_core::control::OperationControl>,
    ) -> Result<Vec<u8>> {
        let standalone = lycheedev_core::control::OperationControl::default();
        let control = control.unwrap_or(&standalone);
        let mut originals = self.original_reader();
        let result = originals.read(doc, control).map(Cow::into_owned);
        originals.finish(result, control)
    }
    pub fn coverage(&self) -> Value {
        coverage(&self.summary, self.documents.len())
    }
    pub fn excerpt(&self, path: &str, start: usize, end: usize) -> Result<String> {
        let Some(doc) = self.documents.iter().find(|d| d.path == path) else {
            return Ok(path.into());
        };
        let control = lycheedev_core::control::OperationControl::default();
        let mut originals = self.original_reader();
        let result = originals
            .read(doc, &control)
            .map(|raw| render_excerpt(&raw, start, end));
        originals.finish(result, &control)
    }
    pub fn span(&self, path: &str, first: usize, last: usize) -> Result<Value> {
        let Some(doc) = self.documents.iter().find(|d| d.path == path) else {
            return Ok(Value::Null);
        };
        let control = lycheedev_core::control::OperationControl::default();
        let mut originals = self.original_reader();
        let result = originals
            .read(doc, &control)
            .map(|raw| render_span(&raw, &doc.sha256, first, last));
        originals.finish(result, &control)
    }
    #[allow(clippy::too_many_arguments)] // Public legacy query contract has seven independent selectors.
    pub fn search(
        &self,
        pin: &SourcePin,
        snapshot: &str,
        text: &str,
        mode: &str,
        topic: &str,
        limit: usize,
        cursor: &str,
    ) -> Result<Value> {
        self.search_observed(
            pin,
            snapshot,
            text,
            mode,
            topic,
            limit,
            cursor,
            None,
            &mut |_, _| {},
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn search_with_control(
        &self,
        pin: &SourcePin,
        snapshot: &str,
        text: &str,
        mode: &str,
        topic: &str,
        limit: usize,
        cursor: &str,
        control: &lycheedev_core::control::OperationControl,
    ) -> Result<Value> {
        self.search_observed(
            pin,
            snapshot,
            text,
            mode,
            topic,
            limit,
            cursor,
            Some(control),
            &mut |_, _| {},
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn search_observed(
        &self,
        pin: &SourcePin,
        snapshot: &str,
        text: &str,
        mode: &str,
        topic: &str,
        limit: usize,
        cursor: &str,
        control: Option<&lycheedev_core::control::OperationControl>,
        observer: &mut impl FnMut(&'static str, usize),
    ) -> Result<Value> {
        check_search(control)?;
        let text = text.trim();
        if text.is_empty() || text.len() > 512 {
            return Err(ToolkitError::arguments(
                "Source query required (1..512 bytes)",
            ));
        }
        if !["", "precise", "exploratory"].contains(&mode)
            || !["", "api", "lua", "xml", "toc", "asset"].contains(&topic)
            || (limit == 0 || limit > 200)
        {
            return Err(ToolkitError::arguments(
                "Invalid source query mode/topic/limit",
            ));
        }
        let identity = storage::sha256(
            format!(
                "{}\0{}\0{}\0{mode}\0{text}\0{topic}",
                pin.repository, pin.product, pin.exact_commit
            )
            .as_bytes(),
        );
        let key = &identity[..16];
        let offset = if cursor.is_empty() {
            0
        } else {
            let Some((k, n)) = cursor.split_once(':') else {
                return Err(ToolkitError::new(
                    "codebase.invalid_search_cursor",
                    "Invalid source cursor",
                    2,
                ));
            };
            let n: usize = n.parse().map_err(|_| {
                ToolkitError::new("codebase.invalid_search_cursor", "Invalid source cursor", 2)
            })?;
            if k != key || n > 10000 {
                return Err(ToolkitError::new(
                    "codebase.invalid_search_cursor",
                    "Cursor does not match query",
                    2,
                ));
            }
            n
        };
        let broad = mode == "exploratory";
        let tokens: Vec<_> = text
            .split_whitespace()
            .map(str::to_ascii_lowercase)
            .collect();
        let standalone = lycheedev_core::control::OperationControl::default();
        let original_control = control.unwrap_or(&standalone);
        let mut originals = self.original_reader();
        let result = (|| {
        let mut loaded = BTreeMap::<&str, Cow<'_, [u8]>>::new();
        let mut exact = Vec::<(Symbol, &str, i32)>::new();
        let mut prefix = vec![];
        let mut full = vec![];
        let mut relations = Vec::new();
        let mut cut = false;
        for (number, s) in self.symbols.iter().enumerate() {
            observer("symbol", number);
            check_search(control)?;
            if s.kind == "relationship" {
                if s.name == text
                    || s.target == text
                    || (broad
                        && (s
                            .name
                            .to_ascii_lowercase()
                            .contains(&text.to_ascii_lowercase())
                            || s.target
                                .to_ascii_lowercase()
                                .contains(&text.to_ascii_lowercase())))
                {
                    relations.push(json!({"source":if s.name=="<file>"{&s.path}else{&s.name},"target":s.target,"kind":s.category,"confidence":s.confidence,"path":s.path,"line":s.line}));
                }
                continue;
            }
            if topic == "asset"
                || topic == "api" && !s.category.starts_with("api-")
                || ["lua", "xml", "toc"].contains(&topic)
                    && !s.path.to_ascii_lowercase().ends_with(&format!(".{topic}"))
            {
                continue;
            }
            if s.kind == "declaration"
                && (s.name == text || s.target == text || s.name.ends_with(&format!(".{text}")))
            {
                exact.push((s.clone(), "exact_symbol", 100));
            } else if s.kind == "header" && s.name == text {
                exact.push((s.clone(), "exact_fact", 90));
            }
            if s.name.starts_with(text) || s.name.contains(&format!(".{text}")) {
                prefix.push((
                    s.clone(),
                    if s.kind == "header" {
                        "name_prefix"
                    } else {
                        "symbol_prefix"
                    },
                    80,
                ));
            }
            let lower = format!("{} {} {}", s.name, s.target, s.signature).to_ascii_lowercase();
            let count = tokens.iter().filter(|t| lower.contains(t.as_str())).count();
            if count > 0 && (broad || count == tokens.len()) {
                full.push((s.clone(), "indexed_text", 70 + count.min(9) as i32));
            }
        }
        let mut candidates = exact;
        let exact_matches = candidates.len();
        if broad || candidates.is_empty() {
            candidates.extend(prefix);
        }
        if broad || candidates.is_empty() {
            candidates.extend(full);
        }
        if topic == "asset" {
            candidates.clear();
            let normalized = crate::assets::normalize_path(text);
            for (number, a) in self.assets.iter().enumerate() {
                observer("asset", number);
                check_search(control)?;
                let p = a["path"].as_str().unwrap_or("");
                let n = crate::assets::normalize_path(p);
                if n.contains(&normalized) {
                    candidates.push((
                        Symbol {
                            id: String::new(),
                            kind: "asset".into(),
                            name: p.into(),
                            target: String::new(),
                            category: "asset".into(),
                            confidence: "exact".into(),
                            path: p.into(),
                            line: 1,
                            end_line: 1,
                            signature: String::new(),
                            scope: String::new(),
                            ordinal: 0,
                        },
                        "asset_path",
                        if n == normalized { 100 } else { 70 },
                    ));
                }
            }
        } else if broad || candidates.is_empty() {
            for (number, doc) in self.documents.iter().enumerate() {
                observer("document", number);
                check_search(control)?;
                let lower = doc.path.to_ascii_lowercase();
                if topic == "api" && !lower.contains("/blizzard_apidocumentationgenerated/")
                    || ["lua", "xml", "toc"].contains(&topic)
                        && !lower.ends_with(&format!(".{topic}"))
                {
                    continue;
                }
                let raw = originals.read(doc, original_control)?;
                loaded.insert(&doc.path, raw);
                for (i, l) in String::from_utf8_lossy(&loaded[doc.path.as_str()])
                    .split('\n')
                    .enumerate()
                {
                    observer("line", i);
                    check_search(control)?;
                    if l.to_ascii_lowercase().contains(&text.to_ascii_lowercase()) {
                        candidates.push((
                            Symbol {
                                id: String::new(),
                                kind: "body".into(),
                                name: text.into(),
                                target: String::new(),
                                category: String::new(),
                                confidence: "exact".into(),
                                path: doc.path.clone(),
                                line: i + 1,
                                end_line: i + 1,
                                signature: l.into(),
                                scope: String::new(),
                                ordinal: 0,
                            },
                            "body_text",
                            60,
                        ));
                    }
                }
            }
        }
        if candidates.len() > 10000 {
            candidates.truncate(10000);
            cut = true;
        }
        check_search(control)?;
        candidates.sort_by(|(a, _, ar), (b, _, br)| {
            (br - penalty(role(&b.path)))
                .cmp(&(ar - penalty(role(&a.path))))
                .then(a.path.cmp(&b.path))
                .then(a.line.cmp(&b.line))
                .then(a.kind.cmp(&b.kind))
                .then(a.name.cmp(&b.name))
        });
        check_search(control)?;
        let mut seen = BTreeSet::new();
        candidates.retain(|(s, _, _)| {
            seen.insert(if s.id.is_empty() {
                format!("{}:{}\0{}\0{}", s.path, s.line, s.name, s.kind)
            } else {
                s.id.clone()
            })
        });
        check_search(control)?;
        let more = candidates.len() > offset + limit;
        let mut results = Vec::new();
        for (s, by, rank) in candidates.iter().skip(offset).take(limit) {
            observer("result", results.len());
            check_search(control)?;
            if !loaded.contains_key(s.path.as_str())
                && let Some(doc) = self.documents.iter().find(|d| d.path == s.path)
            {
                loaded.insert(&doc.path, originals.read(doc, original_control)?);
            }
            let raw = loaded.get(s.path.as_str()).map(|raw| raw.as_ref()).unwrap_or(&[]);
            let penalty = penalty(role(&s.path));
            let hash = self
                .documents
                .iter()
                .find(|d| d.path == s.path)
                .map(|d| d.sha256.clone())
                .or_else(|| {
                    self.assets
                        .iter()
                        .find(|a| a["path"] == s.path)
                        .and_then(|a| a["contentHash"].as_str())
                        .map(str::to_owned)
                })
                .unwrap_or_default();
            let (start, end) = if *by == "exact_symbol" {
                (s.line, s.end_line)
            } else {
                (s.line.saturating_sub(3).max(1), s.line + 3)
            };
            let mut row = json!({"kind":if s.kind=="header"{"toc"}else{&s.category},"name":s.name,"path":s.path,"matchedBy":by,"role":role(&s.path),"confidence":s.confidence,"excerpt":if s.kind=="asset"{s.path.clone()}else{render_excerpt(raw,start,end)},"contentHash":hash,"line":s.line,"score":rank-penalty,"scoreParts":{"match":rank,"rolePenalty":-penalty}});
            row["whyMatch"] = json!({"query":text,"strategy":by,"exactDeclaration":*by=="exact_symbol","confidenceBasis":if s.kind=="body"{"literal source text"}else{"structural source fact"}});
            row["sourceSpan"] = if s.kind == "asset" {
                Value::Null
            } else {
                render_span(raw, &hash, s.line, s.end_line)
            };
            row["excerptSpan"] = if s.kind == "asset" {
                Value::Null
            } else {
                render_span(raw, &hash, start, end.min(start + 79))
            };
            row["excerptTruncated"] = json!(end > start + 79);
            row["endLine"] = json!(s.end_line);
            if !s.scope.is_empty() {
                row["scope"] = json!(s.scope);
            }
            if !s.signature.is_empty() {
                row["signature"] = json!(s.signature);
            }
            if !s.id.is_empty() {
                row["symbolId"] = json!(s.id);
            }
            results.push(row);
        }
        let relation_cut = offset == 0 && relations.len() > limit;
        relations.truncate(limit);
        let truncated = more || cut || relation_cut;
        let mut response = json!({"sourceId":pin.repository,"product":pin.product,"requestedRef":pin.requested_ref,"matchedTag":null,"resolvedCommit":pin.exact_commit,"snapshotId":snapshot,"results":results,"complete":self.summary["complete"]==true&&!truncated,"truncated":truncated,"coverage":self.coverage()});
        response["coverage"]["io"]["sourceBytesRead"] =
            json!(loaded.values().map(|raw| raw.len()).sum::<usize>());
        response["coverage"]["io"]["sourceDocumentsRead"] = json!(loaded.len());
        response["matchCoverage"] = json!({"exactMatches":exact_matches,"candidateMatches":candidates.len(),"returned":results.len(),"offset":offset,"candidateBudget":10000,"candidateBudgetExceeded":cut,"relationPreviewTruncated":relation_cut,"strategy":if broad{"exploratory"}else{"exact-then-prefix-then-indexed-then-literal"},"fallbackReason":if exact_matches>0{Value::Null}else if results.is_empty(){json!("no_structural_or_literal_match")}else{json!("no_exact_declaration")}});
        response["analysis"] = json!({"structure":"tree-sitter-lua-0.5.0","semantic":"not_requested","syntaxCompilation":"not_requested","aliases":"lexical static inference"});
        if offset == 0 && !relations.is_empty() {
            response["relations"] = json!(relations);
        }
        if more && offset + limit <= 10000 {
            response["nextCursor"] = json!(format!("{key}:{}", offset + limit));
        }
        if results.is_empty() {
            response["suggestions"] = json!([
                "use explore for broader text and symbol matches",
                "check the topic, product and ref"
            ]);
        }
        check_search(control)?;
        Ok(response)
        })();
        originals.finish(result, original_control)
    }
}

#[cfg(test)]
mod facts_cache_tests {
    use super::*;
    use lycheedev_core::{control::OperationControl, workspace};
    use std::{
        fs,
        io::Write,
        process::{Command, Stdio},
    };

    fn evict_document_facts(home: &Path, document: &Document) {
        if let Some(locator) = &document.facts_locator {
            let key = locator.group.iter().map(|b| format!("{b:02x}")).collect::<String>();
            let pack = home.join("cache/source-facts-rust").join(format!("{key}.json"));
            if pack.exists() { fs::remove_file(pack).unwrap(); }
        }
        let old = facts_cache_path(home, &document.path, &document.sha256);
        if old.exists() { fs::remove_file(old).unwrap(); }
    }

    #[test]
    fn typed_xml_toc_preparation_preserves_authenticated_cache_precedence() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let cache = lycheedev_core::cache::SourceFactsCache::open(&fixture.home, &control).unwrap();
        for (path, raw) in [
            (
                "Prepared.xml",
                b"<Ui><Frame name=\"Prepared\"/><Script file=\"Full.lua\"/></Ui>".as_slice(),
            ),
            (
                "Prepared.toc",
                b"## Interface: 120100\nFull.lua\n".as_slice(),
            ),
        ] {
            let mode = facts::ParseMode::for_path(path);
            let expected = mode.parse(raw);
            let digest = storage::sha256(raw);
            let prepared = facts_pipeline::PreparedFacts {
                mode,
                facts: facts::for_worker(raw, mode, &control, 4 << 20).unwrap(),
                parse_elapsed: std::time::Duration::ZERO,
            };
            let profile = Profile::enabled(true);
            let (cold, hit) = document_facts_profiled(
                &fixture.home,
                path,
                raw,
                &digest,
                &control,
                Some(&profile),
                &cache,
                Some(prepared),
            )
            .unwrap();
            assert!(!hit);
            assert_eq!(profile.calls(Phase::FactsParseSequential), 0);
            assert_eq!(
                serde_json::to_value(&cold).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(
                cold.raw_relationship_targets,
                expected.raw_relationship_targets
            );
            let ignored = facts_pipeline::PreparedFacts {
                mode,
                facts: Facts::fresh(),
                parse_elapsed: std::time::Duration::ZERO,
            };
            let (warm, hit) = document_facts_profiled(
                &fixture.home,
                path,
                raw,
                &digest,
                &control,
                Some(&profile),
                &cache,
                Some(ignored),
            )
            .unwrap();
            assert!(hit, "authenticated persisted facts win over speculation");
            assert_eq!(
                serde_json::to_value(&warm).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(profile.calls(Phase::FactsParseSequential), 0);
        }
        cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn mismatched_preparation_mode_uses_the_original_complete_parser() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let cache = lycheedev_core::cache::SourceFactsCache::open(&fixture.home, &control).unwrap();
        let raw = b"<Ui><Frame name=\"Right\"/></Ui>";
        let prepared = facts_pipeline::PreparedFacts {
            mode: facts::ParseMode::LuaOrdinary,
            facts: facts::lua(b"function Wrong() end", false),
            parse_elapsed: std::time::Duration::ZERO,
        };
        let profile = Profile::enabled(true);
        let (actual, hit) = document_facts_profiled(
            &fixture.home,
            "Mismatch.xml",
            raw,
            &storage::sha256(raw),
            &control,
            Some(&profile),
            &cache,
            Some(prepared),
        )
        .unwrap();
        assert!(!hit);
        assert_eq!(profile.calls(Phase::FactsParseSequential), 1);
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(facts::xml(raw)).unwrap()
        );
        cache.finish(Ok(()), &control).unwrap();
    }
    #[test]
    fn typed_preparation_cannot_bypass_the_original_stop_checkpoint() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let signal = control.tracked_cancel_handle().unwrap();
        let cache = lycheedev_core::cache::SourceFactsCache::open(&fixture.home, &control).unwrap();
        let raw = b"<Ui/>";
        let prepared = facts_pipeline::PreparedFacts {
            mode: facts::ParseMode::Xml,
            facts: facts::xml(raw),
            parse_elapsed: std::time::Duration::ZERO,
        };
        signal.cancel();
        let profile = Profile::enabled(true);
        let result = document_facts_profiled(
            &fixture.home,
            "Stopped.xml",
            raw,
            &storage::sha256(raw),
            &control,
            Some(&profile),
            &cache,
            Some(prepared),
        );
        assert_eq!(
            cache.finish(result, &control).unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(profile.calls(Phase::FactsRead), 0);
        assert_eq!(profile.calls(Phase::FactsParse), 0);
    }

    // These two public rows are the actual 9a generated-20 observations retained
    // in PR #14 comment 6232293175. The fixture source/path bytes stay original.
    fn generated_contract_9a_cases() -> Vec<(String, Vec<u8>, Value)> {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/rust-parity/source-facts-generated/cases.json"
        ))
        .unwrap();
        [
            (
                "parenthesized_field_and_entry",
                json!({
                    "declarations":[
                        {"name":"Root","category":"api-system","line":1,"endLine":1,"signature":"Root"},
                        {"name":"Run","category":"api-function","line":1,"endLine":1,"signature":"Run(y: string)"}
                    ],
                    "relationships":[],"loads":[],"headers":[],"diagnostics":[]
                }),
            ),
            (
                "unsupported_clears_all",
                json!({
                    "declarations":[],"relationships":[],"loads":[],"headers":[],
                    "diagnostics":[{"message":"Syntax unavailable in fixed Go Lua parser","line":1}]
                }),
            ),
        ]
        .into_iter()
        .map(|(id, facts)| {
            let case = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["id"] == id)
                .unwrap();
            (
                case["path"].as_str().unwrap().to_owned(),
                case["source"].as_str().unwrap().as_bytes().to_vec(),
                facts,
            )
        })
        .collect()
    }
    fn generated_contract_9a_fingerprint(pin: &SourcePin) -> String {
        storage::sha256(format!("rust-index-v11\0facts-layout-v10-go323-xml-lexical\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}", pin.repository, pin.product, pin.exact_commit, pin.parser_revision).as_bytes())
    }
    fn generated_contract_9a_envelope(raw: &[u8], facts: &Value) -> Value {
        json!({
            "schema":"lycheedev.source-facts.rust.v10",
            "inputSHA256":storage::sha256(raw),
            "factsSHA256":storage::sha256(&serde_json::to_vec(facts).unwrap()),
            "facts":facts
        })
    }
    fn generated_contract_9a_file(home: &Path, path: &str, raw: &[u8]) -> PathBuf {
        let digest = storage::sha256(raw);
        let key = storage::sha256(format!("facts-layout-v10-go323-xml-lexical\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0{path}\0{digest}").as_bytes());
        home.join("cache/source-facts-rust")
            .join(format!("{key}.json"))
    }
    fn assert_corrected_generated_contract(path: &str, facts: &Facts) {
        if path.ends_with("parenthesized_field_and_entry.lua") {
            assert_eq!(
                facts
                    .declarations
                    .iter()
                    .map(|row| (row.name.as_str(), row.signature.as_str()))
                    .collect::<Vec<_>>(),
                [
                    ("Root", "Root"),
                    ("Ignored", "Ignored()"),
                    ("Run", "Run(x: number, y: string)")
                ]
            );
        } else {
            assert!(path.ends_with("unsupported_clears_all.lua"));
            assert_eq!(facts.diagnostics.len(), 1);
            assert_eq!(facts.diagnostics[0].line, 1);
            assert_eq!(
                facts.diagnostics[0].message,
                "source line:1(column:48) near '/':   syntax error\n"
            );
        }
    }

    #[test]
    fn generated_contract_rebuilds_valid_9a_file_caches_and_reuses_current_envelopes() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        for (path, raw, old_facts) in generated_contract_9a_cases() {
            let digest = storage::sha256(&raw);
            let old_path = generated_contract_9a_file(&fixture.home, &path, &raw);
            let old_envelope = generated_contract_9a_envelope(&raw, &old_facts);
            storage::atomic_json(&old_path, &old_envelope).unwrap();
            let old_bytes = fs::read(&old_path).unwrap();
            let current_path = facts_cache_path(&fixture.home, &path, &digest);
            assert_ne!(old_path, current_path);
            let expected = facts::lua(&raw, true);
            assert_corrected_generated_contract(&path, &expected);
            assert_ne!(serde_json::to_value(&expected).unwrap(), old_facts);
            let (rebuilt, hit) =
                document_facts(&fixture.home, &path, &raw, &digest, &control).unwrap();
            assert!(
                !hit,
                "the authenticated old layout cannot satisfy current facts"
            );
            assert_eq!(
                serde_json::to_value(&rebuilt).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            let current = fs::read(&current_path).unwrap();
            assert_eq!(&current[..8], facts::binary::MAGIC);
            assert_eq!(fs::read(&old_path).unwrap(), old_bytes);
            // Even valid old digests copied under the new key cannot bypass
            // the envelope schema gate. Rebuild, then prove a real warm hit.
            storage::atomic_json(&current_path, &old_envelope).unwrap();
            let (rebuilt, hit) =
                document_facts(&fixture.home, &path, &raw, &digest, &control).unwrap();
            assert!(!hit);
            assert_eq!(
                serde_json::to_value(&rebuilt).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(fs::read(&current_path).unwrap(), current);
            let (warm, hit) =
                document_facts(&fixture.home, &path, &raw, &digest, &control).unwrap();
            assert!(hit);
            assert_eq!(
                serde_json::to_value(&warm).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(fs::read(&old_path).unwrap(), old_bytes);
        }
    }

    #[test]
    fn generated_contract_rejects_9a_index_and_rekeys_query_readiness_and_warm_hits() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let cases = generated_contract_9a_cases();
        let raw = cases
            .iter()
            .map(|(path, raw, _)| (path.clone(), String::from_utf8(raw.clone()).unwrap()))
            .collect();
        let pin = fixture.pin(&raw);
        assert_eq!(pin.parser_revision, "source-v1");
        let old_identity = generated_contract_9a_fingerprint(&pin);
        let identity = fingerprint(&pin);
        assert_ne!(identity, old_identity);
        let current = build_uncached(&fixture.home, &pin, &control).unwrap();
        let without_io = |index: &Index| {
            let mut value = serde_json::to_value(index).unwrap();
            value["summary"].as_object_mut().unwrap().remove("io");
            value
        };
        let expected = without_io(&current);
        // Reconstruct the valid v6 index container from the sealed old public
        // rows. Recompute its declaration ordinals/IDs, content hash and manifest
        // exactly; rejection must be due to the new identity, not bad bytes.
        let mut old = current;
        old.symbols.retain(|symbol| symbol.name != "Ignored");
        for (ordinal, symbol) in old.symbols.iter_mut().enumerate() {
            symbol.ordinal = ordinal;
            if symbol.name == "Run" {
                symbol.signature = "Run(y: string)".into();
            }
            symbol.id = stable_id(&pin, symbol);
        }
        old.summary["declarations"] = json!(2);
        old.summary["diagnosticSample"][0]["message"] =
            json!("Syntax unavailable in fixed Go Lua parser");
        for (path, raw, facts) in &cases {
            old.documents
                .iter_mut()
                .find(|doc| doc.path == *path)
                .unwrap()
                .facts = serde_json::from_value(facts.clone()).unwrap();
            let doc = old.documents.iter_mut().find(|d| d.path == *path).unwrap();
            evict_document_facts(&fixture.home, doc);
            // The reconstructed 9a index never contained a packed locator.
            doc.facts_locator = None;
            storage::atomic_json(
                &generated_contract_9a_file(&fixture.home, path, raw),
                &generated_contract_9a_envelope(raw, facts),
            )
            .unwrap();
        }
        let old_raw = serde_json::to_vec(&without_io(&old)).unwrap();
        let old_blob = lycheedev_core::cache::publish_derived_controlled(
            &fixture.home,
            &old_raw,
            128 << 20,
            &control,
        )
        .unwrap();
        let old_key = format!("source/rust-index/{old_identity}");
        let old_manifest = json!({"schema":"lycheedev.source-index.rust.v6","identity":old_identity,"blob":old_blob});
        crate::persistence::put_manifest(&fixture.home, &old_key, &old_manifest, &control).unwrap();
        assert!(!is_ready_with_control(&fixture.home, &pin, &control).unwrap());
        assert_eq!(
            crate::query_store::status_with_control(&fixture.home, &pin, &control).unwrap()["ready"],
            false
        );
        let (rebuilt, _) = build_material_with_control(&fixture.home, &pin, &control).unwrap();
        assert_eq!(rebuilt.summary["io"]["parsedDocuments"], 2);
        assert_eq!(rebuilt.summary["io"]["cachedFacts"], 0);
        assert_eq!(without_io(&rebuilt), expected);
        for doc in &rebuilt.documents {
            assert_corrected_generated_contract(&doc.path, &doc.facts);
        }
        assert!(is_ready_with_control(&fixture.home, &pin, &control).unwrap());
        assert_eq!(
            crate::query_store::status_with_control(&fixture.home, &pin, &control).unwrap()["ready"],
            true
        );
        let (warm, _) = build_material_with_control(&fixture.home, &pin, &control).unwrap();
        assert_eq!(warm.summary["io"]["cachedIndex"], true);
        assert_eq!(without_io(&warm), expected);
        let query = crate::query_store::load_with_control(
            &fixture.home,
            &pin,
            "Ignored",
            "precise",
            "api",
            &control,
        )
        .unwrap();
        assert_eq!(query.summary["io"]["cachedIndex"], false);
        assert!(
            query
                .symbols
                .iter()
                .any(|symbol| symbol.name == "Ignored" && symbol.signature == "Ignored()")
        );
        let query_manifest: Value = crate::persistence::read(
            &fixture.home,
            &format!("source/rust-query-file/{identity}"),
            &control,
        )
        .unwrap();
        assert_eq!(query_manifest["identity"], identity);
        let query = crate::query_store::load_with_control(
            &fixture.home,
            &pin,
            "Ignored",
            "precise",
            "api",
            &control,
        )
        .unwrap();
        assert_eq!(query.summary["io"]["cachedIndex"], true);
        assert!(query.symbols.iter().any(|symbol| symbol.name == "Ignored"));
        assert_eq!(
            crate::persistence::read::<Value>(&fixture.home, &old_key, &control).unwrap(),
            old_manifest
        );
        let digest = old_blob["sha256"].as_str().unwrap();
        assert_eq!(
            fs::read(
                fixture
                    .home
                    .join("cache/objects")
                    .join(&digest[..2])
                    .join(&digest[2..])
            )
            .unwrap(),
            old_raw
        );
    }

    #[test]
    fn malformed_contract_invalidates_previous_index_and_file_facts() {
        let pin = SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            requested_ref: "fixture".into(),
            exact_commit: "a".repeat(40),
            parser_revision: git::PARSER.into(),
        };
        let previous = storage::sha256(format!("rust-index-v8\0facts-layout-v7-go323-public\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}", pin.repository, pin.product, pin.exact_commit, pin.parser_revision).as_bytes());
        assert_ne!(fingerprint(&pin), previous);
        let previous_observation = storage::sha256(format!("rust-index-v9\0facts-layout-v8-go323-malformed\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}", pin.repository, pin.product, pin.exact_commit, pin.parser_revision).as_bytes());
        assert_ne!(fingerprint(&pin), previous_observation);
        let input = storage::sha256(b"function goto() end");
        let previous_file = storage::sha256(format!("facts-layout-v7-go323-public\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0Example.lua\0{input}").as_bytes());
        let current = facts_cache_path(Path::new("fixture-home"), "Example.lua", &input);
        assert_ne!(
            current,
            Path::new("fixture-home")
                .join("cache/source-facts-rust")
                .join(format!("{previous_file}.json"))
        );
    }

    #[test]
    fn xml_lexical_contract_invalidates_previous_index_and_file_facts() {
        let pin = SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            requested_ref: "fixture".into(),
            exact_commit: "a".repeat(40),
            parser_revision: git::PARSER.into(),
        };
        let previous = storage::sha256(format!("rust-index-v10\0facts-layout-v9-go323-observed\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}", pin.repository, pin.product, pin.exact_commit, pin.parser_revision).as_bytes());
        assert_ne!(fingerprint(&pin), previous);
        let input = storage::sha256(b"<Ui></Ui junk>");
        let previous_file = storage::sha256(format!("facts-layout-v9-go323-observed\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0Example.xml\0{input}").as_bytes());
        assert_ne!(
            facts_cache_path(Path::new("fixture-home"), "Example.xml", &input),
            Path::new("fixture-home")
                .join("cache/source-facts-rust")
                .join(format!("{previous_file}.json"))
        );
    }

    #[test]
    fn xml_lexical_contract_rebuilds_previous_schema_even_with_valid_digests() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = b"<Ui><Frame name=\"Before\"/></Ui junk>";
        let digest = storage::sha256(raw);
        let (expected, hit) =
            document_facts(&fixture.home, "Example.xml", raw, &digest, &control).unwrap();
        assert!(!hit);
        assert_eq!(expected.declarations.len(), 1);
        assert_eq!(
            expected.diagnostics[0].message,
            "XML syntax error on line 1: invalid characters between </Ui and >"
        );
        let path = facts_cache_path(&fixture.home, "Example.xml", &digest);
        let original = fs::read(&path).unwrap();
        let mut obsolete: Value = serde_json::from_slice(
            &super::facts_encode_tests::historical_encode(&expected, &digest).unwrap(),
        )
        .unwrap();
        obsolete["schema"] = json!("lycheedev.source-facts.rust.v9");
        obsolete["facts"]["diagnostics"][0]["message"] =
            json!("XML syntax error on line 1: element <Ui> closed by </Ui junk>");
        obsolete["factsSHA256"] =
            json!(storage::sha256(&serde_json::to_vec(&obsolete["facts"]).unwrap()));
        storage::atomic_json(&path, &obsolete).unwrap();
        let (rebuilt, hit) =
            document_facts(&fixture.home, "Example.xml", raw, &digest, &control).unwrap();
        assert!(!hit);
        assert_eq!(
            serde_json::to_value(&rebuilt).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        let (warm, hit) =
            document_facts(&fixture.home, "Example.xml", raw, &digest, &control).unwrap();
        assert!(hit);
        assert_eq!(
            serde_json::to_value(&warm).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }

    pub(super) struct Fixture {
        pub(super) home: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }
    impl Fixture {
        pub(super) fn new() -> Self {
            let home = std::env::temp_dir().join(format!(
                "lycheedev-load-cache-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            workspace::initialize(&home).unwrap();
            let mirror = git::mirror(&home, "elvui");
            assert!(
                Command::new("git")
                    .args(["init", "--bare"])
                    .arg(&mirror)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
            Self { home }
        }
        fn git(&self, input: &str, args: &[&str]) -> String {
            self.git_bytes(input.as_bytes(), args)
        }
        fn git_bytes(&self, input: &[u8], args: &[&str]) -> String {
            let mut child = Command::new("git")
                .arg("--git-dir")
                .arg(git::mirror(&self.home, "elvui"))
                .args(args)
                .env("GIT_AUTHOR_NAME", "Facts Fixture")
                .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
                .env("GIT_COMMITTER_NAME", "Facts Fixture")
                .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            child.stdin.take().unwrap().write_all(input).unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }
        pub(super) fn pin(&self, documents: &BTreeMap<String, String>) -> SourcePin {
            fn tree(fixture: &Fixture, blobs: &BTreeMap<String, String>, prefix: &str) -> String {
                let mut entries = String::new();
                let mut directories = BTreeSet::new();
                for (path, blob) in blobs {
                    if let Some(relative) = path.strip_prefix(prefix) {
                        if let Some((directory, _)) = relative.split_once('/') {
                            directories.insert(directory);
                        } else {
                            entries.push_str(&format!("100644 blob {blob}\t{relative}\n"));
                        }
                    }
                }
                for directory in directories {
                    let child = tree(fixture, blobs, &format!("{prefix}{directory}/"));
                    entries.push_str(&format!("040000 tree {child}\t{directory}\n"));
                }
                fixture.git(&entries, &["mktree"])
            }
            // Large path-partition fixtures intentionally reuse a handful of
            // source bodies. Authenticate each distinct body once rather than
            // launching a redundant hash-object child for every path.
            let mut objects = BTreeMap::new();
            let blobs = documents
                .iter()
                .map(|(path, raw)| {
                    (
                        path.clone(),
                        objects.entry(raw.as_str()).or_insert_with(|| {
                            self.git(raw, &["hash-object", "-w", "--stdin"])
                        }).clone(),
                    )
                })
                .collect();
            let tree = tree(self, &blobs, "");
            SourcePin {
                repository: "elvui".into(),
                product: "main".into(),
                requested_ref: "fixture".into(),
                exact_commit: self.git("facts", &["commit-tree", &tree]),
                parser_revision: git::PARSER.into(),
            }
        }
    }
    fn assert_load_bytes(home: &Path, loads: &[LoadFact], raw: &BTreeMap<String, String>) {
        for load in loads {
            assert_eq!(
                evidence::read_blob(home, &load.source_blob, git::MAX_SOURCE_BYTES).unwrap(),
                raw[&load.path].as_bytes()
            );
        }
    }
    #[test]
    fn complete_cached_facts_feed_loads_across_cold_hit_and_byte_change() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let mut raw = BTreeMap::from([
            ("Main.toc".into(), "## Interface: 120100\nMain.xml\n".into()),
            ("Main.xml".into(), "<Ui>\n<Script file=\"First.lua\"/>\n<Frame name=\"FixtureFrame\"><Scripts><OnLoad>self:SetAlpha(1)</OnLoad></Scripts></Frame>\n</Ui>\n".into()),
        ]);
        let pin = fixture.pin(&raw);
        let cold = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(cold.summary["io"]["parsedDocuments"], 2);
        assert_eq!(cold.summary["io"]["cachedFacts"], 0);
        let hit = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(hit.summary["io"]["parsedDocuments"], 0);
        assert_eq!(hit.summary["io"]["cachedFacts"], 2);
        for (before, after) in cold.documents.iter().zip(&hit.documents) {
            assert_eq!(
                serde_json::to_value(&before.facts).unwrap(),
                serde_json::to_value(&after.facts).unwrap()
            );
        }
        assert!(
            !hit.documents
                .iter()
                .find(|d| d.path == "Main.xml")
                .unwrap()
                .facts
                .declarations
                .is_empty()
        );
        let warm: Index = serde_json::from_value(serde_json::to_value(&hit).unwrap()).unwrap();
        assert!(warm.documents.iter().all(|d| d.facts.loads.is_empty()));
        let loads = load_facts_with_control(&fixture.home, &pin, &warm, &control).unwrap();
        let borrowed =
            load_facts_from_documents_with_control(&fixture.home, &warm, &raw, &control).unwrap();
        assert_eq!(
            serde_json::to_value(&loads).unwrap(),
            serde_json::to_value(&borrowed).unwrap()
        );
        assert_eq!(loads.len(), 2);
        assert!(loads.iter().any(|f| f.path == "Main.toc"
            && f.line == 2
            && f.kind == "toc"
            && f.target == "Main.xml"));
        assert!(loads.iter().any(|f| f.path == "Main.xml"
            && f.line == 2
            && f.kind == "script"
            && f.target == "First.lua"));
        assert_load_bytes(&fixture.home, &loads, &raw);
        let xml = hit.documents.iter().find(|d| d.path == "Main.xml").unwrap();
        let cache = facts_cache_path(&fixture.home, &xml.path, &xml.sha256);
        evict_document_facts(&fixture.home, xml);
        let rebuilt =
            load_facts_from_documents_with_control(&fixture.home, &warm, &raw, &control).unwrap();
        assert_eq!(
            serde_json::to_value(&rebuilt).unwrap(),
            serde_json::to_value(&loads).unwrap()
        );
        assert_eq!(
            serde_json::to_value(
                decode_facts(&fs::read(&cache).unwrap(), &xml.sha256, &control)
                    .unwrap()
                    .unwrap()
            )
            .unwrap(),
            serde_json::to_value(&xml.facts).unwrap()
        );

        // A byte change cannot reuse the old path's facts, even with the same length.
        raw.get_mut("Main.xml").unwrap().replace_range(.., "<Ui>\n<Script file=\"Other.lua\"/>\n<Frame name=\"FixtureFrame\"><Scripts><OnLoad>self:SetAlpha(1)</OnLoad></Scripts></Frame>\n</Ui>\n");
        assert_eq!(
            load_facts_from_documents_with_control(&fixture.home, &warm, &raw, &control)
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
        let changed_pin = fixture.pin(&raw);
        let changed = build_uncached(&fixture.home, &changed_pin, &control).unwrap();
        assert_eq!(changed.summary["io"]["parsedDocuments"], 1);
        assert_eq!(changed.summary["io"]["cachedFacts"], 1);
        let loads = load_facts_from_documents_with_control(&fixture.home, &changed, &raw, &control)
            .unwrap();
        assert!(loads.iter().any(|f| f.target == "Other.lua"));
        assert!(!loads.iter().any(|f| f.target == "First.lua"));
        assert_load_bytes(&fixture.home, &loads, &raw);
    }
    #[test]
    fn corrupt_complete_facts_never_become_load_evidence() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([(
            "Main.xml".into(),
            "<Ui>\n<Script file=\"Real.lua\"/>\n</Ui>\n".into(),
        )]);
        let pin = fixture.pin(&raw);
        let index = build_uncached(&fixture.home, &pin, &control).unwrap();
        let doc = &index.documents[0];
        let path = facts_cache_path(&fixture.home, &doc.path, &doc.sha256);
        evict_document_facts(&fixture.home, doc);
        document_facts(&fixture.home, &doc.path, raw[&doc.path].as_bytes(), &doc.sha256, &control).unwrap();
        let original = fs::read(&path).unwrap();
        let original_facts = serde_json::to_value(&doc.facts).unwrap();
        for offset in [12, 44] {
            let mut untrusted = doc.facts.clone();
            untrusted.loads[0].path = "Untrusted.lua".into();
            let mut corrupt = encode_facts(&untrusted, &doc.sha256, None).unwrap();
            corrupt[offset..offset + 32].fill(0);
            for consumer in 0..3 {
                fs::write(&path, &corrupt).unwrap();
                match consumer {
                    0 => assert_eq!(
                        load_facts_with_control(&fixture.home, &pin, &index, &control).unwrap()[0]
                            .target,
                        "Real.lua"
                    ),
                    1 => assert_eq!(
                        load_facts_from_documents_with_control(
                            &fixture.home,
                            &index,
                            &raw,
                            &control
                        )
                        .unwrap()[0]
                            .target,
                        "Real.lua"
                    ),
                    _ => {
                        let rebuilt = build_uncached(&fixture.home, &pin, &control).unwrap();
                        assert_eq!(rebuilt.summary["io"]["parsedDocuments"], 1);
                        assert_eq!(
                            serde_json::to_value(&rebuilt.documents[0].facts).unwrap(),
                            original_facts
                        );
                    }
                }
                if consumer == 2 {
                    // A successful full rebuild now commits a cohort. Remove
                    // that reconstructible cohort before testing the next
                    // specifically per-file corruption; do not bypass it.
                    evict_document_facts(&fixture.home, doc);
                    document_facts(&fixture.home, &doc.path, raw[&doc.path].as_bytes(), &doc.sha256, &control).unwrap();
                }
                assert_eq!(fs::read(&path).unwrap(), original);
            }
        }
        // Unsupported cache schemas retain the existing parser-rebuild behavior.
        let mut obsolete = original.clone();
        obsolete[..8].copy_from_slice(b"LYCFCT12");
        fs::write(&path, &obsolete).unwrap();
        let loads =
            load_facts_from_documents_with_control(&fixture.home, &index, &raw, &control).unwrap();
        assert_eq!(loads.len(), 1);
        assert_eq!(loads[0].target, "Real.lua");
        assert_eq!(fs::read(&path).unwrap(), original);
        // Invalid JSON and oversized cache files also regenerate from the
        // verified original document instead of unbounded reads or empty facts.
        fs::write(&path, b"{").unwrap();
        assert_eq!(
            load_facts_from_documents_with_control(&fixture.home, &index, &raw, &control).unwrap()
                [0]
            .target,
            "Real.lua"
        );
        assert_eq!(fs::read(&path).unwrap(), original);
        let oversized = fs::File::create(&path).unwrap();
        oversized.set_len((32 << 20) + 1).unwrap();
        drop(oversized);
        let rebuilt = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(rebuilt.summary["io"]["parsedDocuments"], 1);
        assert_eq!(
            serde_json::to_value(&rebuilt.documents[0].facts).unwrap(),
            original_facts
        );
        evict_document_facts(&fixture.home, doc);
        document_facts(&fixture.home, &doc.path, raw[&doc.path].as_bytes(), &doc.sha256, &control).unwrap();
        assert_eq!(fs::read(&path).unwrap(), original);
        let mut missing = raw.clone();
        missing.clear();
        assert_eq!(
            load_facts_from_documents_with_control(&fixture.home, &index, &missing, &control)
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
    }
    #[test]
    fn public_index_cold_warm_and_rebuild_do_not_require_native_catalog() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([
            ("First.xml".into(), "<Ui>\n<Frame name=\"First\"/>\n</Ui>".into()),
            ("Second.xml".into(), "<Ui>\n<Frame name=\"Second\"><Scripts><OnLoad>self:Show()</OnLoad></Scripts></Frame>\n</Ui>".into()),
            ("Main.toc".into(), "First.xml\nSecond.xml\n".into()),
            ("Main.lua".into(), "function Run() end\n".into()),
        ]);
        let pin = fixture.pin(&raw);
        let before = crate::xml_program::catalog_verifications();
        let cold = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications(), before);
        assert_eq!(cold.summary["io"]["parsedDocuments"], 4);
        let hit = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications(), before);
        assert_eq!(hit.summary["io"]["cachedFacts"], 4);
        for (expected, cached) in cold.documents.iter().zip(&hit.documents) {
            assert_eq!(
                serde_json::to_value(&expected.facts).unwrap(),
                serde_json::to_value(&cached.facts).unwrap()
            );
            if expected.path.ends_with(".xml") {
                evict_document_facts(&fixture.home, expected);
            }
        }
        let next = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications(), before);
        assert_eq!(next.summary["io"]["parsedDocuments"], 4);
        assert_eq!(next.summary["io"]["cachedFacts"], 0);
        for (expected, fresh) in cold.documents.iter().zip(&next.documents) {
            assert_eq!(
                serde_json::to_value(&expected.facts).unwrap(),
                serde_json::to_value(&fresh.facts).unwrap()
            );
        }
        let without_io = |index: &Index| {
            let mut value = serde_json::to_value(index).unwrap();
            value["summary"].as_object_mut().unwrap().remove("io");
            value
        };
        assert_eq!(without_io(&cold), without_io(&hit));
        assert_eq!(without_io(&cold), without_io(&next));
    }
    #[test]
    fn static_facts_non_xml_and_stop_boundaries_never_initialize_catalog() {
        use std::{
            sync::{Arc, atomic::AtomicBool},
            time::{Duration, Instant},
        };
        let fixture = Fixture::new();
        let before = crate::xml_program::catalog_verifications();
        let control = OperationControl::default();
        for (path, raw) in [("Run.lua", "function Run() end"), ("Main.toc", "Run.lua\n")] {
            document_facts(
                &fixture.home,
                path,
                raw.as_bytes(),
                &storage::sha256(raw.as_bytes()),
                &control,
            )
            .unwrap();
        }
        assert_eq!(crate::xml_program::catalog_verifications(), before);
        let raw = b"<Ui><Frame name=\"Stopped\"/></Ui>";
        let digest = storage::sha256(raw);
        let cancelled = OperationControl::new(
            Instant::now() + Duration::from_secs(180),
            Arc::new(AtomicBool::new(true)),
        );
        let expired = OperationControl::uncancelable(Instant::now());
        for (stopped, code) in [
            (&cancelled, "command.cancelled"),
            (&expired, "command.deadline"),
        ] {
            assert_eq!(
                document_facts(&fixture.home, "Stopped.xml", raw, &digest, stopped)
                    .unwrap_err()
                    .code,
                code
            );
            assert_eq!(crate::xml_program::catalog_verifications(), before);
            assert!(!facts_cache_path(&fixture.home, "Stopped.xml", &digest).exists());
        }
        document_facts(&fixture.home, "First.xml", raw, &digest, &control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications(), before);
        // Reuse never bypasses the original entry check, even after initialization.
        assert_eq!(
            document_facts(&fixture.home, "Stopped.xml", raw, &digest, &cancelled)
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        assert!(!facts_cache_path(&fixture.home, "Stopped.xml", &digest).exists());
        assert_eq!(crate::xml_program::catalog_verifications(), before);
    }
    #[test]
    fn invalid_utf8_git_blob_aborts_index_without_publishing_a_ready_manifest() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let good = fixture.git("function Before() end\n", &["hash-object", "-w", "--stdin"]);
        let bad_raw = b"local broken = \xff\n";
        let bad = fixture.git_bytes(bad_raw, &["hash-object", "-w", "--stdin"]);
        let tree = fixture.git(
            &format!("100644 blob {good}\tAAGood.lua\n100644 blob {bad}\tZZBad.lua\n"),
            &["mktree"],
        );
        let pin = SourcePin {
            repository: "elvui".into(),
            product: "main".into(),
            requested_ref: "fixture".into(),
            exact_commit: fixture.git("invalid encoding fixture", &["commit-tree", &tree]),
            parser_revision: git::PARSER.into(),
        };
        let key = format!("source/rust-index/{}", fingerprint(&pin));
        // A failed partial cohort is discarded. A retry must still preserve
        // the invalid-document error and never publish a partial index.
        for _ in 0..2 {
            let error = build_material_with_control(&fixture.home, &pin, &control)
                .err()
                .unwrap();
            assert_eq!(error.code, "command.failed");
            assert_eq!(error.message, "codebase.analysis_encoding_or_budget");
            assert_eq!(error.exit_code, 5);
            assert!(error.details.is_null());
            assert!(!is_ready_with_control(&fixture.home, &pin, &control).unwrap());
            let missing =
                crate::persistence::read::<Value>(&fixture.home, &key, &control).unwrap_err();
            assert_eq!(missing.code, "vault.record_missing");
            assert!(
                !facts_cache_path(&fixture.home, "ZZBad.lua", &storage::sha256(bad_raw)).exists()
            );
        }
    }
    #[test]
    fn compatibility_gaps_cover_unsampled_diagnostics_and_survive_warm_index() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let mut raw = BTreeMap::new();
        for i in 0..25 {
            raw.insert(format!("Early/{i:02}.lua"), "function Broken(".to_owned());
        }
        raw.insert(
            "Late/Blizzard_APIDocumentationGenerated/Api.lua".into(),
            "function Broken(".into(),
        );
        raw.insert("Late/Frames.xml".into(), "<Ui><Frame>".into());
        let pin = fixture.pin(&raw);
        let cold = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(
            cold.summary["diagnosticSample"].as_array().unwrap().len(),
            20
        );
        assert_eq!(
            cold.compatibility_gaps,
            BTreeSet::from(["mixin", "api", "event", "template", "frame-type"].map(str::to_owned))
        );
        let serialized = serde_json::to_value(&cold).unwrap();
        let warm: Index = serde_json::from_value(serialized.clone()).unwrap();
        assert_eq!(warm.compatibility_gaps, cold.compatibility_gaps);
        let mut legacy = serialized;
        legacy.as_object_mut().unwrap().remove("compatibilityGaps");
        assert!(serde_json::from_value::<Index>(legacy).is_err());
        let mut all = cold.compatibility_gaps;
        record_compatibility_gap(&mut all, "Late/Main.TOC");
        assert_eq!(all.len(), 6);
        assert!(all.contains("interface"));
    }
    #[test]
    fn generated_mode_matches_exact_go_path_and_load_symbols_are_public() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let source = "local api={Name='System',Type='System'}";
        let raw = BTreeMap::from([
            (
                "Interface/Blizzard_APIDocumentationGenerated/Api.lua".into(),
                source.into(),
            ),
            (
                "Interface/blizzard_apidocumentationgenerated/Api.lua".into(),
                source.into(),
            ),
            (
                "OtherAPIDocumentationGenerated/Api.lua".into(),
                source.into(),
            ),
            ("Main.toc".into(), "Api.lua\n".into()),
        ]);
        let pin = fixture.pin(&raw);
        let cold = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(cold.summary["declarations"], 1);
        assert_eq!(
            cold.symbols
                .iter()
                .filter(|s| s.kind == "load")
                .map(|s| (s.name.as_str(), s.target.as_str(), s.category.as_str()))
                .collect::<Vec<_>>(),
            [("Main.toc", "Api.lua", "toc")]
        );
        let warm = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(warm.summary["io"]["cachedFacts"], 4);
        assert_eq!(
            serde_json::to_value(&cold.symbols).unwrap(),
            serde_json::to_value(&warm.symbols).unwrap()
        );
    }
    fn original_path(home: &Path, doc: &Document) -> PathBuf {
        home.join("cache/source-originals-rust").join(&doc.sha256)
    }
    fn without_query_io(mut value: Value) -> Value {
        value["coverage"].as_object_mut().unwrap().remove("io");
        value
    }
    #[test]
    fn source_original_cache_cold_warm_and_query_store_keep_full_results() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([
            (
                "Main.lua".into(),
                "function CacheProbe()\n  return 'literal-original-marker'\nend\n".into(),
            ),
            ("Empty.lua".into(), String::new()),
            (
                "Main.xml".into(),
                "<Ui><Script file=\"Main.lua\"/></Ui>".into(),
            ),
        ]);
        let pin = fixture.pin(&raw);
        let cold = build_with_control(&fixture.home, &pin, &control).unwrap();
        for doc in &cold.documents {
            assert!(!original_path(&fixture.home, doc).exists());
            assert_eq!(doc.text.as_deref(), Some(raw[&doc.path].as_bytes()));
            assert!(
                !evidence::blob_path(&fixture.home, &doc.sha256)
                    .unwrap()
                    .exists()
            );
        }
        let warm = build_with_control(&fixture.home, &pin, &control).unwrap();
        assert!(warm.documents.iter().all(|doc| doc.text.is_none()));
        for doc in &warm.documents {
            assert_eq!(
                warm.read_document_controlled(doc, Some(&control)).unwrap(),
                raw[&doc.path].as_bytes()
            );
        }
        for (text, mode) in [
            ("CacheProbe", "precise"),
            ("literal-original-marker", "exploratory"),
        ] {
            let expected = without_query_io(
                cold.search_with_control(&pin, "snapshot", text, mode, "", 20, "", &control)
                    .unwrap(),
            );
            for _ in 0..2 {
                let projected = crate::query_store::load_with_control(
                    &fixture.home,
                    &pin,
                    text,
                    mode,
                    "",
                    &control,
                )
                .unwrap();
                let actual = projected
                    .search_with_control(&pin, "snapshot", text, mode, "", 20, "", &control)
                    .unwrap();
                assert_eq!(without_query_io(actual), expected);
            }
        }
        assert_eq!(
            warm.excerpt("Main.lua", 1, 3).unwrap(),
            cold.excerpt("Main.lua", 1, 3).unwrap()
        );
        assert_eq!(
            warm.span("Main.lua", 1, 3).unwrap(),
            cold.span("Main.lua", 1, 3).unwrap()
        );
    }
    #[test]
    fn source_original_missing_corrupt_and_oversized_cache_recover_fixed_git_bytes() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([("Main.lua".into(), "function Original() end\n".into())]);
        let pin = fixture.pin(&raw);
        build_with_control(&fixture.home, &pin, &control).unwrap();
        let warm = build_with_control(&fixture.home, &pin, &control).unwrap();
        let doc = &warm.documents[0];
        let path = original_path(&fixture.home, doc);
        warm.read_document_controlled(doc, Some(&control)).unwrap();
        // A newer commit at the same source path must never become recovery input.
        let newer = fixture.pin(&BTreeMap::from([(
            "Main.lua".into(),
            "function Newer() end\n".into(),
        )]));
        assert_ne!(newer.exact_commit, pin.exact_commit);
        fixture.git("", &["update-ref", "refs/heads/main", &newer.exact_commit]);
        for damage in 0..5 {
            match damage {
                0 => fs::remove_file(&path).unwrap(),
                1 => fs::write(&path, b"partial").unwrap(),
                2 => fs::write(&path, vec![b'x'; doc.bytes]).unwrap(),
                3 => fs::write(&path, vec![0xff; doc.bytes]).unwrap(),
                _ => fs::OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .unwrap()
                    .set_len((32 << 20) + 1)
                    .unwrap(),
            }
            assert_eq!(
                warm.read_document_controlled(doc, Some(&control)).unwrap(),
                raw[&doc.path].as_bytes()
            );
            assert_eq!(fs::read(&path).unwrap(), raw[&doc.path].as_bytes());
            assert!(
                !evidence::blob_path(&fixture.home, &doc.sha256)
                    .unwrap()
                    .exists()
            );
        }
        fs::remove_dir_all(fixture.home.join("cache/source-originals-rust")).unwrap();
        assert_eq!(
            warm.read_document_controlled(doc, Some(&control)).unwrap(),
            raw[&doc.path].as_bytes()
        );
    }
    #[test]
    fn source_original_recovery_rejects_wrong_index_identity_and_unsafe_paths() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let pin = fixture.pin(&BTreeMap::from([(
            "Main.lua".into(),
            "function Real() end\n".into(),
        )]));
        build_with_control(&fixture.home, &pin, &control).unwrap();
        let warm = build_with_control(&fixture.home, &pin, &control).unwrap();
        let mut wrong = warm.documents[0].clone();
        wrong.bytes -= 1;
        assert_eq!(
            warm.read_document_controlled(&wrong, Some(&control)).unwrap_err().code,
            "codebase.source_integrity",
            "indexed-length mismatch retains integrity error, not frame budget"
        );
        let mut wrong = warm.documents[0].clone();
        wrong.sha256 = storage::sha256(b"different bytes");
        assert_eq!(
            warm.read_document_controlled(&wrong, Some(&control))
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
        assert!(!original_path(&fixture.home, &wrong).exists());
        wrong.bytes = git::MAX_SOURCE_BYTES + 1;
        assert_eq!(
            warm.read_document_controlled(&wrong, Some(&control))
                .unwrap_err()
                .code,
            "vault.blob_limit"
        );
        wrong.bytes = 0;
        wrong.path = "../outside.lua".into();
        assert_eq!(
            warm.read_document_controlled(&wrong, Some(&control))
                .unwrap_err()
                .code,
            "codebase.source_integrity"
        );
        let mut tampered = build_with_control(&fixture.home, &pin, &control).unwrap();
        tampered.summary["repository"] = json!("../uncontrolled");
        assert!(!original_path(&fixture.home, &warm.documents[0]).exists());
        assert!(
            tampered
                .read_document_controlled(&warm.documents[0], Some(&control))
                .is_err()
        );
    }
    #[test]
    fn source_original_recovery_cancellation_never_publishes_replacement() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let pin = fixture.pin(&BTreeMap::from([(
            "Main.lua".into(),
            "function Stop() end\n".into(),
        )]));
        build_with_control(&fixture.home, &pin, &control).unwrap();
        let warm = build_with_control(&fixture.home, &pin, &control).unwrap();
        let path = original_path(&fixture.home, &warm.documents[0]);
        assert!(!path.exists());
        let cancelled = OperationControl::default();
        cancelled
            .cancel_handle()
            .store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            warm.read_document_controlled(&warm.documents[0], Some(&cancelled))
                .unwrap_err()
                .code,
            "command.cancelled"
        );
        assert!(!path.exists());
        assert!(
            warm.read_document_controlled(&warm.documents[0], Some(&control))
                .is_ok()
        );
    }
    #[test]
    fn source_original_publication_failure_is_demanded_not_an_index_side_effect() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = "function Blocked() end\n";
        let pin = fixture.pin(&BTreeMap::from([("Main.lua".into(), raw.into())]));
        let path = fixture
            .home
            .join("cache/source-originals-rust")
            .join(storage::sha256(raw.as_bytes()));
        fs::create_dir_all(&path).unwrap();
        let cold = build_with_control(&fixture.home, &pin, &control).unwrap();
        assert!(is_ready_with_control(&fixture.home, &pin, &control).unwrap());
        let key = format!("source/rust-index/{}", fingerprint(&pin));
        let manifest = crate::persistence::read::<Value>(&fixture.home, &key, &control).unwrap();
        assert!(
            cold.search_with_control(&pin, "snapshot", "Blocked", "precise", "", 20, "", &control)
                .is_err()
        );
        assert_eq!(
            crate::persistence::read::<Value>(&fixture.home, &key, &control).unwrap(),
            manifest
        );
        fs::remove_dir(&path).unwrap();
        let recovered = build_with_control(&fixture.home, &pin, &control).unwrap();
        assert_eq!(recovered.documents.len(), 1);
        assert!(is_ready_with_control(&fixture.home, &pin, &control).unwrap());
        recovered
            .search_with_control(&pin, "snapshot", "Blocked", "precise", "", 20, "", &control)
            .unwrap();
        assert_eq!(fs::read(path).unwrap(), raw.as_bytes());
    }
    #[test]
    fn source_original_full_cache_loss_rebuilds_index_and_preserves_formal_evidence() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([
            (
                "Main.xml".into(),
                "<Ui><Script file=\"Main.lua\"/></Ui>".into(),
            ),
            ("Main.toc".into(), "Main.xml\n".into()),
        ]);
        let pin = fixture.pin(&raw);
        let index = build_with_control(&fixture.home, &pin, &control).unwrap();
        for doc in &index.documents {
            assert!(
                !evidence::blob_path(&fixture.home, &doc.sha256)
                    .unwrap()
                    .exists()
            );
        }
        let loads = load_facts_with_control(&fixture.home, &pin, &index, &control).unwrap();
        assert_eq!(loads.len(), 2);
        assert!(!fixture.home.join("cache/source-originals-rust").exists());
        fs::remove_dir_all(fixture.home.join("cache/source-facts-rust")).unwrap();
        assert_load_bytes(&fixture.home, &loads, &raw);
        for load in &loads {
            assert!(
                evidence::blob_path(&fixture.home, load.source_blob["sha256"].as_str().unwrap())
                    .unwrap()
                    .is_file()
            );
        }
        // A crash/prune can also remove final derived objects while their
        // durable metadata pointers remain. The fixed Git pin must reconstruct
        // the complete index rather than accept any missing/partial object.
        fs::remove_dir_all(fixture.home.join("cache")).unwrap();
        fs::create_dir(fixture.home.join("cache")).unwrap();
        let recovered = build_with_control(&fixture.home, &pin, &control).unwrap();
        assert_eq!(recovered.summary["io"]["parsedDocuments"], 2);
        let mut before = serde_json::to_value(&index).unwrap();
        let mut after = serde_json::to_value(&recovered).unwrap();
        before["summary"].as_object_mut().unwrap().remove("io");
        after["summary"].as_object_mut().unwrap().remove("io");
        assert_eq!(after, before);
        assert_load_bytes(&fixture.home, &loads, &raw);
    }
    #[test]
    fn source_cold_query_moves_text_and_precise_publishes_only_consumed_originals() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([
            (
                "Main.lua".into(),
                "function DemandProbe()\r\n return '你好'\r\nend\r\n".into(),
            ),
            ("Empty.lua".into(), String::new()),
            ("Unused.lua".into(), "function Unused() end\n".into()),
        ]);
        let pin = fixture.pin(&raw);
        let cold = crate::query_store::load_with_control(
            &fixture.home,
            &pin,
            "DemandProbe",
            "precise",
            "",
            &control,
        )
        .unwrap();
        assert!(cold.documents.iter().all(|doc| doc.text.is_some()));
        assert!(!fixture.home.join("cache/source-originals-rust").exists());
        // A genuinely cold query must own its authenticated bytes after full
        // index/image publication. No cache or Git reread can rescue this test.
        let mirror = git::mirror(&fixture.home, &pin.repository);
        fs::rename(&mirror, fixture.home.join("parked.git")).unwrap();
        let first = cold
            .search_with_control(
                &pin,
                "snapshot",
                "DemandProbe",
                "precise",
                "",
                20,
                "",
                &control,
            )
            .unwrap();
        let main = cold
            .documents
            .iter()
            .find(|doc| doc.path == "Main.lua")
            .unwrap();
        assert_eq!(
            fs::read(original_path(&fixture.home, main)).unwrap(),
            raw["Main.lua"].as_bytes()
        );
        for doc in cold.documents.iter().filter(|doc| doc.path != "Main.lua") {
            assert!(!original_path(&fixture.home, doc).exists());
}
        let (held, _) = storage::locked_sha256(
            &original_path(&fixture.home, main),
            main.bytes as u64,
            &control,
        )
        .unwrap();
        let warm = crate::query_store::load_with_control(
            &fixture.home,
            &pin,
            "DemandProbe",
            "precise",
            "",
            &control,
        )
        .unwrap();
        assert!(warm.documents.iter().all(|doc| doc.text.is_none()));
        let second = warm
            .search_with_control(
                &pin,
                "snapshot",
                "DemandProbe",
                "precise",
                "",
                20,
                "",
                &control,
            )
            .unwrap();
        assert_eq!(without_query_io(first), without_query_io(second));
        storage::finish_file(held, Ok(()), &control).unwrap();
        let empty = cold
            .documents
            .iter()
            .find(|doc| doc.path == "Empty.lua")
            .unwrap();
        let mut originals = cold.original_reader();
        let bytes = originals.read(empty, &control).unwrap();
        assert!(matches!(&bytes, Cow::Borrowed(_)));
        assert!(bytes.is_empty());
        assert!(!originals.git_started());
        originals.finish(Ok(()), &control).unwrap();
        assert_eq!(fs::read(original_path(&fixture.home, empty)).unwrap(), b"");
    }

    #[test]
    fn source_index_then_broad_default_fallback_and_warm_queries_keep_complete_results() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([
            (
                "A.lua".into(),
                "function A()\r\n return 'literal-你好'\r\nend\r\n".into(),
            ),
            (
                "Nested/B.lua".into(),
                "function B()\n return 'literal-你好'\nend\n".into(),
            ),
            ("Empty.lua".into(), String::new()),
            (
                "Main.xml".into(),
                "<Ui><Script file=\"A.lua\"/></Ui>".into(),
            ),
        ]);
        let pin = fixture.pin(&raw);
        let cold = build_with_control(&fixture.home, &pin, &control).unwrap();
        // A declaration signature retains its first source line. Keep this
        // token solely in the body so default mode really reaches fallback,
        // rather than returning a structural indexed_text match and reading
        // only the one document selected by the page limit.
        assert!(cold.symbols.iter().all(|symbol| {
            ![&symbol.name, &symbol.target, &symbol.signature]
                .iter()
                .any(|field| field.contains("literal-你好"))
        }));
        assert!(!fixture.home.join("cache/source-originals-rust").exists());
        let warm = build_with_control(&fixture.home, &pin, &control).unwrap();
        // Fresh originals use one demand reader including shared nested trees.
        let mut originals = warm.original_reader();
        for doc in &warm.documents {
            assert_eq!(
                originals.read(doc, &control).unwrap().as_ref(),
                raw[&doc.path].as_bytes()
            );
            assert!(originals.git_started());
        }
        originals.finish(Ok(()), &control).unwrap();
        fs::remove_dir_all(fixture.home.join("cache/source-originals-rust")).unwrap();
        for mode in ["", "exploratory"] {
            let actual = warm
                .search_with_control(&pin, "snapshot", "literal-你好", mode, "", 1, "", &control)
                .unwrap();
            let expected = cold
                .search_with_control(&pin, "snapshot", "literal-你好", mode, "", 1, "", &control)
                .unwrap();
            assert_eq!(without_query_io(actual.clone()), without_query_io(expected), "mode={mode:?}");
            assert_eq!(actual["results"][0]["matchedBy"], "body_text", "mode={mode:?}");
            assert_eq!(actual["coverage"]["io"]["sourceDocumentsRead"], 4, "mode={mode:?}");
            let cursor = actual["nextCursor"].as_str().unwrap();
            let next = warm
                .search_with_control(
                    &pin,
                    "snapshot",
                    "literal-你好",
                    mode,
                    "",
                    1,
                    cursor,
                    &control,
                )
                .unwrap();
            assert_ne!(actual["results"][0]["path"], next["results"][0]["path"]);
        }
        fs::rename(
            git::mirror(&fixture.home, &pin.repository),
            fixture.home.join("parked.git"),
        )
        .unwrap();
        let mut originals = warm.original_reader();
        for doc in &warm.documents {
            originals.read(doc, &control).unwrap();
        }
        assert!(!originals.git_started());
        originals.finish(Ok(()), &control).unwrap();
        assert!(
            warm.search_with_control(
                &pin,
                "snapshot",
                "literal-你好",
                "exploratory",
                "lua",
                20,
                "",
                &control
            )
            .is_ok()
        );
    }

    #[test]
    fn source_original_first_publication_error_precedes_later_missing_git_object() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let pin = fixture.pin(&BTreeMap::from([
            ("A.lua".into(), "function A() end\n".into()),
            ("B.lua".into(), "function B() end\n".into()),
        ]));
        build_with_control(&fixture.home, &pin, &control).unwrap();
        let warm = build_with_control(&fixture.home, &pin, &control).unwrap();
        let directory = fixture.home.join("cache/source-originals-rust");
        fs::create_dir_all(&directory).unwrap();
        for slot in 0..64 {
            fs::write(
                directory.join(format!(
                    ".lycheedev-cache-{}-{slot}",
                    warm.documents[0].sha256
                )),
                b"uncertain owner",
            )
            .unwrap();
        }
        let files = git::tree_with_control(&fixture.home, &pin, &control).unwrap();
        let later = &files
            .iter()
            .find(|file| file.path == "B.lua")
            .unwrap()
            .object;
        fs::remove_file(
            git::mirror(&fixture.home, &pin.repository)
                .join("objects")
                .join(&later[..2])
                .join(&later[2..]),
        )
        .unwrap();
        let first = warm
            .read_document_controlled(&warm.documents[0], Some(&control))
            .unwrap_err();
        assert_eq!(first.code, "vault.temporary_collision");
        let mut visited = Vec::new();
        let error = warm
            .search_observed(
                &pin,
                "snapshot",
                "missing literal",
                "exploratory",
                "",
                20,
                "",
                Some(&control),
                &mut |phase, number| {
                    if phase == "document" {
                        visited.push(number);
                    }
                },
            )
            .unwrap_err();
        assert_eq!((error.code, error.message), (first.code, first.message));
        assert_eq!(visited, [0]);
    }

    #[test]
    fn source_original_later_cancellation_does_not_prefetch_or_publish_next_document() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let pin = fixture.pin(&BTreeMap::from([
            ("A.lua".into(), "function A() end\n".into()),
            ("B.lua".into(), "function B() end\n".into()),
        ]));
        build_with_control(&fixture.home, &pin, &control).unwrap();
        let warm = build_with_control(&fixture.home, &pin, &control).unwrap();
        let cancelled = OperationControl::default();
        let cancel = cancelled.cancel_handle();
        let error = warm
            .search_observed(
                &pin,
                "snapshot",
                "missing literal",
                "exploratory",
                "",
                20,
                "",
                Some(&cancelled),
                &mut |phase, number| {
                    if phase == "document" && number == 1 {
                        cancel.store(true, std::sync::atomic::Ordering::Release);
                    }
                },
            )
            .unwrap_err();
        assert_eq!(error.code, "command.cancelled");
        assert!(original_path(&fixture.home, &warm.documents[0]).is_file());
        assert!(!original_path(&fixture.home, &warm.documents[1]).exists());
    }

    #[test]
    fn demand_git_reader_reuses_one_process_without_tree_memo_and_rejects_wrong_pin() {
        let fixture = Fixture::new();
        let control = OperationControl::default();
        let raw = BTreeMap::from([
            ("Nested/A.lua".into(), "function A() end\n".into()),
            ("Nested/B.lua".into(), "function B() end\n".into()),
        ]);
        let pin = fixture.pin(&raw);
        let mut reader = git::Originals::new(&fixture.home, pin.clone(), 0);
        let mut pid = None;
        for _ in 0..2 {
            for (path, expected) in &raw {
                assert_eq!(
                    reader.read(path, git::MAX_SOURCE_BYTES, &control).unwrap(),
                    expected.as_bytes()
                );
                match pid {
                    Some(pid) => assert_eq!(reader.process_id(), Some(pid)),
                    None => pid = reader.process_id(),
                }
            }
        }
        assert!(pid.is_some());
        reader.finish(Ok(()), &control).unwrap();
        for field in ["product", "parser", "commit"] {
            let mut wrong = pin.clone();
            match field {
                "product" => wrong.product = "not-a-product".into(),
                "parser" => wrong.parser_revision = "not-a-parser".into(),
                _ => wrong.exact_commit = "HEAD".into(),
            }
            let mut reader = git::Originals::new(&fixture.home, wrong, 1024);
            let error = reader
                .read("Nested/A.lua", git::MAX_SOURCE_BYTES, &control)
                .unwrap_err();
            assert_eq!(error.code, "codebase.invalid_source_pin");
            assert!(!reader.started());
            assert_eq!(
                reader.finish::<()>(Err(error), &control).unwrap_err().code,
                "codebase.invalid_source_pin"
            );
        }
    }
}
