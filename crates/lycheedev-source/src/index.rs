mod profile;
use crate::{
    facts::{self, Facts},
    git,
};
use lycheedev_core::{
    Result, ToolkitError, evidence, metadata::historical_write::finish_historical,
    selection::SourcePin, storage,
};
use profile::{Phase, Profile};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
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
    pub text: Vec<u8>,
    pub sha256: String,
    pub bytes: usize,
    #[serde(skip)]
    pub facts: Facts,
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
    let (parsed, _) = document_facts(home, &document.path, raw, &document.sha256, control)?;
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
pub struct Index {
    pub documents: Vec<Document>,
    pub assets: Vec<Value>,
    pub symbols: Vec<Symbol>,
    pub summary: Value,
    // At most six compatibility categories, derived from every diagnostic,
    // before the public sample is capped. Required when deserializing caches.
    #[serde(rename = "compatibilityGaps")]
    pub(crate) compatibility_gaps: BTreeSet<String>,
    #[serde(skip)]
    pub(crate) home: PathBuf,
}
pub fn stable_id(pin: &SourcePin, s: &Symbol) -> String {
    format!(
        "SYM-{}",
        &storage::sha256(
            format!(
                "{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
                pin.repository,
                pin.exact_commit,
                s.path,
                s.line,
                s.end_line,
                s.name,
                s.category,
                s.scope,
                s.ordinal
            )
            .as_bytes()
        )[..32]
    )
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
    storage::sha256(format!("rust-index-v9\0facts-layout-v8-go323-malformed\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}",pin.repository,pin.product,pin.exact_commit,pin.parser_revision).as_bytes())
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
                    let mut map: Index = serde_json::from_slice(&raw)?;
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
        let mut persisted = serde_json::to_value(&map)?;
        persisted["summary"].as_object_mut().unwrap().remove("io");
        let raw = serde_json::to_vec(&persisted)?;
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
    crate::persistence::finish_control(finish_historical(result, lock.close()), control.check())
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
fn facts_cache_path(home: &Path, path: &str, sha256: &str) -> PathBuf {
    let key = storage::sha256(
        format!(
            "facts-layout-v8-go323-malformed\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0{path}\0{sha256}"
        ).as_bytes(),
    );
    home.join("cache/source-facts-rust")
        .join(format!("{key}.json"))
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
    document_facts_profiled(home, path, data, sha256, control, None)
}
fn document_facts_profiled(
    home: &Path,
    path: &str,
    data: &[u8],
    sha256: &str,
    control: &lycheedev_core::control::OperationControl,
    profile: Option<&Profile>,
) -> Result<(Facts, bool)> {
    control.check()?;
    let cache = facts_cache_path(home, path, sha256);
    // A corrupt or obsolete rebuildable cache is never evidence. Read at most
    // the fixed Go 32 MiB limit plus one byte, then regenerate from authenticated
    // input bytes. Cancellation and owned-handle close failures still propagate.
    const MAX_FACT_CACHE_BYTES: usize = 32 << 20;
    if cache.exists() {
        let _phase = profile.map(|p| p.span(Phase::FactsRead));
        let mut file = std::fs::File::open(&cache)?;
        let loaded = (|| -> Result<Option<Value>> {
            let mut raw = Vec::new();
            let mut buffer = [0; 65536];
            while raw.len() <= MAX_FACT_CACHE_BYTES {
                control.check()?;
                let remaining = MAX_FACT_CACHE_BYTES + 1 - raw.len();
                let count = remaining.min(buffer.len());
                let n = file.read(&mut buffer[..count])?;
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buffer[..n]);
            }
            if raw.len() > MAX_FACT_CACHE_BYTES {
                return Ok(None);
            }
            Ok(serde_json::from_slice::<Value>(&raw).ok())
        })();
        if let Some(envelope) = storage::finish_file(file, loaded, control)?
            && envelope["schema"] == "lycheedev.source-facts.rust.v8"
            && envelope["inputSHA256"] == sha256
            && envelope["factsSHA256"] == storage::sha256(&serde_json::to_vec(&envelope["facts"])?)
            && let Ok(parsed) = serde_json::from_value(envelope["facts"].clone())
        {
            control.check()?;
            return Ok((parsed, true));
        }
    }
    let parse_phase = profile.map(|p| p.span(Phase::FactsParse));
    let lower_path = path.to_ascii_lowercase();
    let parsed = if lower_path.ends_with(".lua") {
        facts::lua(data, path.contains("/Blizzard_APIDocumentationGenerated/"))
    } else if lower_path.ends_with(".toc") {
        facts::toc(data)
    } else {
        facts::xml(data)
    };
    control.check()?;
    drop(parse_phase);
    let serialize_phase = profile.map(|p| p.span(Phase::FactsSerialize));
    let f = serde_json::to_value(&parsed)?;
    let envelope = serde_json::to_vec(
        &json!({"schema":"lycheedev.source-facts.rust.v8","inputSHA256":sha256,"factsSHA256":storage::sha256(&serde_json::to_vec(&f)?),"facts":f}),
    )?;
    if envelope.len() > MAX_FACT_CACHE_BYTES {
        return Err(ToolkitError::new(
            "codebase.fact_budget",
            "Source fact cache exceeds the 32 MiB limit",
            3,
        ));
    }
    drop(serialize_phase);
    let _publish_phase = profile.map(|p| p.span(Phase::FactsPublish));
    storage::atomic_write_controlled(&cache, &envelope, control)?;
    control.check().map_err(|mut error| {
        error.details = json!({"schema":"lycheedev.source-persistence.v1","path":cache,"mutation":"facts","state":"committed","cause":error.clone()});
        error
    })?;
    Ok((parsed, false))
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
    let deadline = control.deadline();
    let (mut cached, mut parsed_documents, mut read_bytes) = (0usize, 0usize, 0usize);
    let tree_phase = profile.span(Phase::GitTree);
    let files = git::tree_with_control(home, pin, control)?;
    drop(tree_phase);
    let mut index = Index {
        documents: vec![],
        assets: vec![],
        symbols: vec![],
        summary: Value::Null,
        compatibility_gaps: BTreeSet::new(),
        home: home.into(),
    };
    let (mut skipped, mut diagnostics, mut relationships) = (0, Vec::new(), 0usize);
    let mut retained = 0usize;
    for file in &files {
        if file.asset && file.bytes > 64 << 20 {
            index
                .assets
                .push(json!({"path":file.path,"contentHash":"","bytes":file.bytes,"local":null}));
        }
    }
    let visit_phase = profile.span(Phase::GitBlobs);
    let batches = git::visit_blobs_with_control(home, pin, &files, control, |file, data| {
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
            let ext = Path::new(&file.path)
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            index.assets.push(json!({"path":file.path,"contentHash":hash,"bytes":file.bytes,"extension":format!(".{ext}"),"mime":"application/octet-stream","format":ext,"width":0,"height":0,"local":local}));
            return Ok(());
        }
        retained = retained.checked_add(file.bytes).ok_or_else(|| {
            ToolkitError::new(
                "codebase.source_budget",
                "Source memory admission overflow",
                3,
            )
        })?;
        if retained > 256 << 20 {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "Source query memory budget exceeded",
                3,
            ));
        }
        if std::str::from_utf8(&data).is_err() {
            // Fixed Go AnalyzeDocument returns an untyped error here, which
            // aborts the index and maps to command.failed / dispatch / exit 5.
            // It is not a recoverable parse note or an oversized-file skip.
            return Err(ToolkitError::new(
                "command.failed",
                "codebase.analysis_encoding_or_budget",
                5,
            ));
        }
        let sha256 = storage::sha256(&data);
        let (facts, cache_hit) =
            document_facts_profiled(home, &file.path, &data, &sha256, control, Some(profile))?;
        if cache_hit {
            cached += 1;
        } else {
            parsed_documents += 1;
        }
        let publish_phase = profile.span(Phase::OriginalPublish);
        evidence::publish_blob_bounded(home, &data, git::MAX_SOURCE_BYTES, control)?;
        drop(publish_phase);
        let _symbols_phase = profile.span(Phase::Symbols);
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
            s.id = stable_id(pin, &s);
            index.symbols.push(s);
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
            text: data,
            sha256,
            bytes: file.bytes,
            facts,
        });
        Ok(())
    })?;
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
impl Index {
    fn read_document(&self, doc: &Document) -> Result<Vec<u8>> {
        self.read_document_controlled(doc, None)
    }
    fn read_document_controlled(
        &self,
        doc: &Document,
        control: Option<&lycheedev_core::control::OperationControl>,
    ) -> Result<Vec<u8>> {
        check_search(control)?;
        if !doc.text.is_empty() {
            let mut bytes = Vec::with_capacity(doc.text.len());
            for block in doc.text.chunks(64 << 10) {
                check_search(control)?;
                bytes.extend_from_slice(block);
            }
            check_search(control)?;
            return Ok(bytes);
        }
        let blob = json!({"sha256":doc.sha256,"bytes":doc.bytes});
        match control {
            Some(control) => {
                evidence::read_blob_bounded(&self.home, &blob, git::MAX_SOURCE_BYTES, control)
            }
            None => evidence::read_blob(&self.home, &blob, git::MAX_SOURCE_BYTES),
        }
    }
    pub fn coverage(&self) -> Value {
        let mut v = json!({"analyzedDocuments":self.documents.len(),"skippedDocuments":self.summary["skippedDocuments"],"diagnostics":self.summary["diagnostics"],"complete":self.summary["complete"]});
        if let Some(s) = self.summary.get("diagnosticSample") {
            v["diagnosticSample"] = s.clone();
        }
        v["io"] = self.summary["io"].clone();
        v
    }
    pub fn excerpt(&self, path: &str, start: usize, end: usize) -> Result<String> {
        let Some(doc) = self.documents.iter().find(|d| d.path == path) else {
            return Ok(path.into());
        };
        Ok(render_excerpt(&self.read_document(doc)?, start, end))
    }
    pub fn span(&self, path: &str, first: usize, last: usize) -> Result<Value> {
        let Some(doc) = self.documents.iter().find(|d| d.path == path) else {
            return Ok(Value::Null);
        };
        Ok(render_span(
            &self.read_document(doc)?,
            &doc.sha256,
            first,
            last,
        ))
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
        let mut loaded = BTreeMap::<String, Vec<u8>>::new();
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
            let normalized = text.replace('/', "\\").to_ascii_lowercase();
            for (number, a) in self.assets.iter().enumerate() {
                observer("asset", number);
                check_search(control)?;
                let p = a["path"].as_str().unwrap_or("");
                let n = p.replace('/', "\\").to_ascii_lowercase();
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
                let raw = self.read_document_controlled(doc, control)?;
                loaded.insert(doc.path.clone(), raw);
                for (i, l) in String::from_utf8_lossy(&loaded[&doc.path])
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
            if !loaded.contains_key(&s.path)
                && let Some(doc) = self.documents.iter().find(|d| d.path == s.path)
            {
                loaded.insert(s.path.clone(), self.read_document_controlled(doc, control)?);
            }
            let raw = loaded.get(&s.path).map(Vec::as_slice).unwrap_or(&[]);
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
            json!(loaded.values().map(Vec::len).sum::<usize>());
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

    #[test]
    fn malformed_contract_invalidates_previous_index_and_file_facts() {
        let pin = SourcePin {
            repository: "elvui".into(), product: "main".into(), requested_ref: "fixture".into(),
            exact_commit: "a".repeat(40), parser_revision: git::PARSER.into(),
        };
        let previous = storage::sha256(format!("rust-index-v8\0facts-layout-v7-go323-public\0TS0.26.3\0Lua0.5.0\0XML0.38\0generator-v6\0{}\0{}\0{}\0{}", pin.repository, pin.product, pin.exact_commit, pin.parser_revision).as_bytes());
        assert_ne!(fingerprint(&pin), previous);
        let input = storage::sha256(b"function goto() end");
        let previous_file = storage::sha256(format!("facts-layout-v7-go323-public\0tree-sitter-0.26.3\0lua-0.5.0\0xml-0.38\0generator-v6\0Example.lua\0{input}").as_bytes());
        let current = facts_cache_path(Path::new("fixture-home"), "Example.lua", &input);
        assert_ne!(current, Path::new("fixture-home").join("cache/source-facts-rust").join(format!("{previous_file}.json")));
    }

    struct Fixture {
        home: PathBuf,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.home);
        }
    }
    impl Fixture {
        fn new() -> Self {
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
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input)
                .unwrap();
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }
        fn pin(&self, documents: &BTreeMap<String, String>) -> SourcePin {
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
            let blobs = documents
                .iter()
                .map(|(path, raw)| {
                    (
                        path.clone(),
                        self.git(raw, &["hash-object", "-w", "--stdin"]),
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
        fs::remove_file(&cache).unwrap();
        let rebuilt =
            load_facts_from_documents_with_control(&fixture.home, &warm, &raw, &control).unwrap();
        assert_eq!(
            serde_json::to_value(&rebuilt).unwrap(),
            serde_json::to_value(&loads).unwrap()
        );
        assert_eq!(
            storage::read_json::<Value>(&cache).unwrap()["facts"],
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
        let original: Value = storage::read_json(&path).unwrap();
        for field in ["factsSHA256", "inputSHA256"] {
            let mut corrupt = original.clone();
            corrupt[field] = json!("0".repeat(64));
            corrupt["facts"]["loads"][0]["path"] = json!("Untrusted.lua");
            for consumer in 0..3 {
                storage::atomic_json(&path, &corrupt).unwrap();
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
                    _ => assert_eq!(
                        build_uncached(&fixture.home, &pin, &control)
                            .unwrap()
                            .summary["io"]["parsedDocuments"],
                        1
                    ),
                }
                assert_eq!(storage::read_json::<Value>(&path).unwrap(), original);
            }
        }
        // Unsupported cache schemas retain the existing parser-rebuild behavior.
        let mut obsolete = original.clone();
        obsolete["schema"] = json!("obsolete");
        obsolete["facts"] = Value::Null;
        storage::atomic_json(&path, &obsolete).unwrap();
        let loads =
            load_facts_from_documents_with_control(&fixture.home, &index, &raw, &control).unwrap();
        assert_eq!(loads.len(), 1);
        assert_eq!(loads[0].target, "Real.lua");
        assert_eq!(storage::read_json::<Value>(&path).unwrap(), original);
        // Invalid JSON and oversized cache files also regenerate from the
        // verified original document instead of unbounded reads or empty facts.
        fs::write(&path, b"{").unwrap();
        assert_eq!(
            load_facts_from_documents_with_control(&fixture.home, &index, &raw, &control).unwrap()
                [0]
            .target,
            "Real.lua"
        );
        assert_eq!(storage::read_json::<Value>(&path).unwrap(), original);
        let oversized = fs::File::create(&path).unwrap();
        oversized.set_len((32 << 20) + 1).unwrap();
        drop(oversized);
        assert_eq!(
            build_uncached(&fixture.home, &pin, &control)
                .unwrap()
                .summary["io"]["parsedDocuments"],
            1
        );
        assert_eq!(storage::read_json::<Value>(&path).unwrap(), original);
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
                fs::remove_file(facts_cache_path(
                    &fixture.home,
                    &expected.path,
                    &expected.sha256,
                ))
                .unwrap();
            }
        }
        let next = build_uncached(&fixture.home, &pin, &control).unwrap();
        assert_eq!(crate::xml_program::catalog_verifications(), before);
        assert_eq!(next.summary["io"]["parsedDocuments"], 2);
        assert_eq!(next.summary["io"]["cachedFacts"], 2);
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
            repository: "elvui".into(), product: "main".into(), requested_ref: "fixture".into(),
            exact_commit: fixture.git("invalid encoding fixture", &["commit-tree", &tree]),
            parser_revision: git::PARSER.into(),
        };
        let key = format!("source/rust-index/{}", fingerprint(&pin));
        // A retry may reuse the earlier good document's facts, but cannot turn
        // the invalid document into a successful partial index.
        for _ in 0..2 {
            let error = build_material_with_control(&fixture.home, &pin, &control).err().unwrap();
            assert_eq!(error.code, "command.failed");
            assert_eq!(error.message, "codebase.analysis_encoding_or_budget");
            assert_eq!(error.exit_code, 5);
            assert!(error.details.is_null());
            assert!(!is_ready_with_control(&fixture.home, &pin, &control).unwrap());
            let missing = crate::persistence::read::<Value>(&fixture.home, &key, &control).unwrap_err();
            assert_eq!(missing.code, "vault.record_missing");
            assert!(!facts_cache_path(&fixture.home, "ZZBad.lua", &storage::sha256(bad_raw)).exists());
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
}
