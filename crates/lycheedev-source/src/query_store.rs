//! Bounded rebuildable file index. Exact names use sorted postings; other
//! strategies scan borrowed string slices and allocate only matched facts.
//! Original document bytes remain separately authenticated evidence blobs.
use crate::index::{self, Document, Index, Symbol};
use lycheedev_core::{
    Result, ToolkitError,
    metadata::Metadata,
    selection::{SourcePin, valid_digest},
    storage,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::Path,
};
const SCHEMA: &str = "lycheedev.source-query-file.rust.v2";
const MAGIC: &[u8; 8] = b"LYSPIDX2";
const MAX_BYTES: usize = 128 << 20;
const RECORD: usize = 92;
const DICTIONARY: usize = 16;
#[derive(Serialize, Deserialize)]
struct Meta {
    schema: String,
    identity: String,
    summary: Value,
    documents: Vec<Document>,
    assets: Vec<Value>,
}
fn invalid(message: &str) -> ToolkitError {
    ToolkitError::new("codebase.source_integrity", message, 4)
}
fn budget() -> ToolkitError {
    ToolkitError::new(
        "codebase.source_budget",
        "File query index budget exceeded",
        3,
    )
}
fn number(raw: &[u8], offset: usize) -> Result<usize> {
    let b = raw
        .get(
            offset
                ..offset
                    .checked_add(4)
                    .ok_or_else(|| invalid("Index offset overflow"))?,
        )
        .ok_or_else(|| invalid("Truncated query index"))?;
    Ok(u32::from_le_bytes(b.try_into().unwrap()) as usize)
}
fn put(raw: &mut Vec<u8>, n: usize) -> Result<()> {
    raw.extend_from_slice(&u32::try_from(n).map_err(|_| budget())?.to_le_bytes());
    Ok(())
}
struct Image<'a> {
    meta: Meta,
    records: &'a [u8],
    dictionary: &'a [u8],
    postings: &'a [u8],
    strings: &'a str,
}
impl<'a> Image<'a> {
    fn decode(raw: &'a [u8], identity: &str) -> Result<Self> {
        if raw.len() > MAX_BYTES || raw.get(..8) != Some(MAGIC.as_slice()) {
            return Err(invalid("Invalid file query index header"));
        }
        let (meta_len, record_count, dict_count, post_count, string_len) = (
            number(raw, 8)?,
            number(raw, 12)?,
            number(raw, 16)?,
            number(raw, 20)?,
            number(raw, 24)?,
        );
        if record_count > 2_000_000 || dict_count > 4_000_000 || post_count > 8_000_000 {
            return Err(budget());
        }
        let mut offset = 28usize;
        let mut section = |length: usize| -> Result<&'a [u8]> {
            let end = offset
                .checked_add(length)
                .ok_or_else(|| invalid("Index section overflow"))?;
            let bytes = raw
                .get(offset..end)
                .ok_or_else(|| invalid("Truncated query index section"))?;
            offset = end;
            Ok(bytes)
        };
        let meta: Meta = serde_json::from_slice(section(meta_len)?)?;
        if meta.schema != SCHEMA || meta.identity != identity {
            return Err(invalid("File query index identity differs"));
        }
        let records = section(record_count.checked_mul(RECORD).ok_or_else(budget)?)?;
        let dictionary = section(dict_count.checked_mul(DICTIONARY).ok_or_else(budget)?)?;
        let postings = section(post_count.checked_mul(4).ok_or_else(budget)?)?;
        let strings = std::str::from_utf8(section(string_len)?)
            .map_err(|_| invalid("Invalid query string pool UTF-8"))?;
        if offset != raw.len() || meta.documents.len() > 100_000 || meta.assets.len() > 200_000 {
            return Err(invalid("Query index section bounds differ"));
        }
        let image = Self {
            meta,
            records,
            dictionary,
            postings,
            strings,
        };
        // These are bounded derived data, but authenticated bytes still require
        // structural admission. Do not trust offsets just because SHA matches.
        for r in image.records.as_chunks::<RECORD>().0 {
            for field in 0..10 {
                image.string(r, field * 8)?;
            }
        }
        let mut previous = None;
        for d in image.dictionary.as_chunks::<DICTIONARY>().0 {
            let key = image.string(d, 0)?;
            if previous.is_some_and(|p: &str| p >= key) {
                return Err(invalid("Unsorted query dictionary"));
            }
            previous = Some(key);
            let start = number(d, 8)?;
            let count = number(d, 12)?;
            if start.checked_add(count).is_none_or(|n| n > post_count) {
                return Err(invalid("Posting range exceeds query index"));
            }
        }
        for p in image.postings.as_chunks::<4>().0 {
            if number(p, 0)? >= record_count {
                return Err(invalid("Posting refers to absent symbol"));
            }
        }
        Ok(image)
    }
    fn string(&self, raw: &[u8], offset: usize) -> Result<&'a str> {
        let start = number(raw, offset)?;
        let len = number(raw, offset + 4)?;
        self.strings
            .get(
                start
                    ..start
                        .checked_add(len)
                        .ok_or_else(|| invalid("String range overflow"))?,
            )
            .ok_or_else(|| invalid("Invalid query string range"))
    }
    fn row(&self, id: usize) -> &'a [u8] {
        &self.records[id * RECORD..(id + 1) * RECORD]
    }
    fn field(&self, id: usize, field: usize) -> &'a str {
        self.string(self.row(id), field * 8)
            .expect("admitted query strings")
    }
    fn symbol(&self, id: usize) -> Result<Symbol> {
        let r = self.row(id);
        Ok(Symbol {
            id: self.field(id, 0).into(),
            kind: self.field(id, 1).into(),
            name: self.field(id, 2).into(),
            target: self.field(id, 3).into(),
            category: self.field(id, 4).into(),
            confidence: self.field(id, 5).into(),
            path: self.field(id, 6).into(),
            signature: self.field(id, 7).into(),
            scope: self.field(id, 8).into(),
            line: number(r, 80)?,
            end_line: number(r, 84)?,
            ordinal: number(r, 88)?,
        })
    }
    fn exact_ids(&self, text: &str) -> Result<Vec<usize>> {
        let mut low = 0;
        let mut high = self.dictionary.len() / DICTIONARY;
        while low < high {
            let mid = low + (high - low) / 2;
            let d = &self.dictionary[mid * DICTIONARY..(mid + 1) * DICTIONARY];
            match self.string(d, 0)?.cmp(text) {
                std::cmp::Ordering::Less => low = mid + 1,
                std::cmp::Ordering::Greater => high = mid,
                std::cmp::Ordering::Equal => {
                    let start = number(d, 8)?;
                    let count = number(d, 12)?;
                    return (start..start + count)
                        .map(|i| number(self.postings, i * 4))
                        .collect();
                }
            }
        }
        Ok(vec![])
    }
    #[allow(clippy::too_many_arguments)] // Private projection combines query identity and measured IO context.
    fn project(
        mut self,
        home: &Path,
        pin: &SourcePin,
        text: &str,
        mode: &str,
        topic: &str,
        cached: bool,
        bytes: usize,
        started: std::time::Instant,
    ) -> Result<Index> {
        if self.meta.summary["commit"] != pin.exact_commit
            || self.meta.summary["repository"] != pin.repository
            || self.meta.summary["product"] != pin.product
            || self.meta.summary["parser"] != pin.parser_revision
        {
            return Err(invalid("Query index source identity differs"));
        }
        let permitted = |id: usize| {
            let path = self.field(id, 6);
            topic != "asset"
                && (topic != "api" || self.field(id, 4).starts_with("api-"))
                && (!["lua", "xml", "toc"].contains(&topic)
                    || path.to_ascii_lowercase().ends_with(&format!(".{topic}")))
        };
        let broad = mode == "exploratory";
        let exact = self.exact_ids(text)?;
        let mut ids = BTreeSet::new();
        let mut relations = BTreeSet::new();
        for id in exact {
            let kind = self.field(id, 1);
            if kind == "relationship" {
                relations.insert(id);
            } else if permitted(id)
                && ((kind == "declaration"
                    && (self.field(id, 2) == text
                        || self.field(id, 3) == text
                        || self.field(id, 2).ends_with(&format!(".{text}"))))
                    || (kind == "header" && self.field(id, 2) == text))
            {
                ids.insert(id);
            }
        }
        if broad || ids.is_empty() {
            for id in 0..self.records.len() / RECORD {
                if self.field(id, 1) != "relationship"
                    && permitted(id)
                    && (self.field(id, 2).starts_with(text)
                        || self.field(id, 2).contains(&format!(".{text}")))
                {
                    ids.insert(id);
                }
            }
        }
        if broad || ids.is_empty() {
            let tokens: Vec<_> = text
                .split_whitespace()
                .map(str::to_ascii_lowercase)
                .collect();
            for id in 0..self.records.len() / RECORD {
                if self.field(id, 1) == "relationship" || !permitted(id) {
                    continue;
                }
                let search = self.field(id, 9);
                let count = tokens
                    .iter()
                    .filter(|t| search.contains(t.as_str()))
                    .count();
                if count > 0 && (broad || count == tokens.len()) {
                    ids.insert(id);
                }
            }
        }
        if broad {
            let lower = text.to_ascii_lowercase();
            for id in 0..self.records.len() / RECORD {
                if self.field(id, 1) == "relationship"
                    && (self.field(id, 2).to_ascii_lowercase().contains(&lower)
                        || self.field(id, 3).to_ascii_lowercase().contains(&lower))
                {
                    relations.insert(id);
                }
            }
        }
        // Preserve canonical order BEFORE staged candidate truncation and ID
        // deduplication. Projecting only a top-N list would silently lose recall.
        ids.extend(relations);
        let symbols = ids
            .into_iter()
            .map(|id| self.symbol(id))
            .collect::<Result<Vec<_>>>()?;
        let assets = if topic == "asset" {
            let normalized = text.replace('/', "\\").to_ascii_lowercase();
            self.meta
                .assets
                .into_iter()
                .filter(|a| {
                    a["path"]
                        .as_str()
                        .unwrap_or("")
                        .replace('/', "\\")
                        .to_ascii_lowercase()
                        .contains(&normalized)
                })
                .collect()
        } else {
            vec![]
        };
        self.meta.summary["io"] = json!({"cachedIndex":cached,"projection":"file-posting","gitTreeProcesses":0,"gitBlobProcesses":0,"indexBytesRead":bytes,"projectedSymbols":symbols.len(),"parsedDocuments":0,"elapsedMs":started.elapsed().as_millis(),"deadlineMs":180000});
        Ok(Index {
            documents: self.meta.documents,
            assets,
            symbols,
            summary: self.meta.summary,
            home: home.into(),
        })
    }
}
pub fn load_with_control(
    home: &Path,
    pin: &SourcePin,
    text: &str,
    mode: &str,
    topic: &str,
    control: &lycheedev_core::control::OperationControl,
) -> Result<Index> {
    control.check()?;
    let text = text.trim();
    if text.is_empty()
        || text.len() > 512
        || !["", "precise", "exploratory"].contains(&mode)
        || !["", "api", "lua", "xml", "toc", "asset"].contains(&topic)
    {
        return Err(ToolkitError::arguments("Invalid source query"));
    }
    let started = std::time::Instant::now();
    let identity = index::fingerprint(pin);
    let key = format!("source/rust-query-file/{identity}");
    let mut metadata = Metadata::open(home, true)?;
    let existing = metadata.read::<Value>(&key);
    let raw = match existing {
        Ok(manifest) if manifest["schema"] == SCHEMA && manifest["identity"] == identity => {
            read_image(home, &manifest["blob"])?
        }
        Ok(_) => None,
        Err(e) if e.code == "vault.record_missing" => None,
        Err(e) => return Err(e),
    };
    let cached = raw.is_some();
    let raw = if let Some(raw) = raw {
        raw
    } else {
        let _lock = storage::ExclusiveLock::acquire(
            &home
                .join("locks")
                .join(format!("source-query-file-{identity}.lock")),
        )?;
        let full = index::build_with_control(home, pin, control)?;
        let raw = write_image(&full, &identity)?;
        let blob = lycheedev_core::cache::publish(home, &raw, MAX_BYTES, None)?;
        let generation = metadata.generation(&key)?;
        metadata.put(
            &key,
            &json!({"schema":SCHEMA,"identity":identity,"blob":blob}),
            generation,
        )?;
        raw
    };
    control.check()?;
    let projected = Image::decode(&raw, &identity)?.project(
        home,
        pin,
        text,
        mode,
        topic,
        cached,
        raw.len(),
        started,
    )?;
    control.check()?;
    Ok(projected)
}
/// Readiness is read-only: absent or obsolete derived cache is reported without
/// fetching, warming or mutating source state.
pub fn status(home: &Path, pin: &SourcePin) -> Result<Value> {
    let mut status = json!({"activeSnapshot":pin.exact_commit,"readySnapshots":0,"snapshotFiles":0,"astFiles":0,"parserSchema":pin.parser_revision,"indexSchema":"lycheedev.source-map.v2","storage":"file-cache","assets":0,"ready":false,"complete":false,"coverage":{"analyzedDocuments":0,"skippedDocuments":0,"diagnostics":0,"complete":false}});
    let metadata = match Metadata::open(home, false) {
        Ok(m) => m,
        Err(e) if e.code == "vault.record_missing" => return Ok(status),
        Err(e) => return Err(e),
    };
    let identity = index::fingerprint(pin);
    for (key, schema) in [
        (format!("source/rust-query-file/{identity}"), SCHEMA),
        (
            format!("source/rust-index/{identity}"),
            "lycheedev.source-index.rust.v5",
        ),
    ] {
        let manifest = match metadata.read::<Value>(&key) {
            Ok(m) => m,
            Err(e) if e.code == "vault.record_missing" => continue,
            Err(e) => return Err(e),
        };
        if manifest["schema"] != schema || manifest["identity"] != identity {
            continue;
        }
        let Some(raw) = read_image(home, &manifest["blob"])? else {
            continue;
        };
        let (summary, documents) = if schema == SCHEMA {
            let image = Image::decode(&raw, &identity)?;
            (image.meta.summary, image.meta.documents.len())
        } else {
            let full: Index = serde_json::from_slice(&raw)?;
            (full.summary, full.documents.len())
        };
        if summary["repository"] != pin.repository
            || summary["commit"] != pin.exact_commit
            || summary["product"] != pin.product
            || summary["parser"] != pin.parser_revision
        {
            return Err(invalid("Readiness cache source identity differs"));
        }
        status["ready"] = json!(true);
        status["readySnapshots"] = json!(1);
        status["snapshotFiles"] =
            json!(documents + summary["skippedDocuments"].as_u64().unwrap_or(0) as usize);
        status["assets"] = summary["assets"].clone();
        status["complete"] = summary["complete"].clone();
        status["coverage"] = json!({"analyzedDocuments":documents,"skippedDocuments":summary["skippedDocuments"],"diagnostics":summary["diagnostics"],"complete":summary["complete"]});
        status["astFiles"] = if let Some(count) = summary.get("astFiles") {
            count.clone()
        } else if summary["diagnostics"] == 0 {
            json!(documents)
        } else if summary["diagnostics"].as_u64().unwrap_or(u64::MAX) <= 25 {
            let failed: BTreeSet<_> = summary["diagnosticSample"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|d| d["path"].as_str())
                .collect();
            json!(documents.saturating_sub(failed.len()))
        } else {
            status["astFilesState"] = json!("unknown");
            status["astFilesReason"] =
                json!("older derived cache retained only sampled diagnostic paths");
            Value::Null
        };
        return Ok(status);
    }
    Ok(status)
}
fn read_image(home: &Path, blob: &Value) -> Result<Option<Vec<u8>>> {
    let digest = blob["sha256"].as_str().unwrap_or("");
    if !valid_digest(digest, 64) {
        return Err(invalid("Query index digest invalid"));
    }
    let path = home
        .join("cache/objects")
        .join(&digest[..2])
        .join(&digest[2..]);
    if !path.exists() {
        return Ok(None);
    }
    storage::guard_path(home, &path)?;
    let mut raw = vec![];
    fs::File::open(path)?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut raw)?;
    if raw.len() > MAX_BYTES || blob["bytes"] != raw.len() || storage::sha256(&raw) != digest {
        return Err(invalid("Query index content differs"));
    }
    Ok(Some(raw))
}
fn write_image(map: &Index, identity: &str) -> Result<Vec<u8>> {
    let mut summary = map.summary.clone();
    summary
        .as_object_mut()
        .ok_or_else(|| invalid("Invalid index summary"))?
        .remove("io");
    let meta = serde_json::to_vec(&Meta {
        schema: SCHEMA.into(),
        identity: identity.into(),
        summary,
        documents: map.documents.clone(),
        assets: map.assets.clone(),
    })?;
    let mut pool = vec![];
    let mut interned = BTreeMap::<String, (usize, usize)>::new();
    let mut records = vec![];
    let mut names = BTreeMap::<String, BTreeSet<usize>>::new();
    let mut intern = |value: &str, output: &mut Vec<u8>| -> Result<()> {
        let &(start, len) = if let Some(pair) = interned.get(value) {
            pair
        } else {
            let pair = (pool.len(), value.len());
            if pool
                .len()
                .checked_add(value.len())
                .is_none_or(|n| n > MAX_BYTES)
            {
                return Err(budget());
            }
            pool.extend_from_slice(value.as_bytes());
            interned.entry(value.into()).or_insert(pair)
        };
        put(output, start)?;
        put(output, len)
    };
    for (id, s) in map.symbols.iter().enumerate() {
        for field in [
            &s.id,
            &s.kind,
            &s.name,
            &s.target,
            &s.category,
            &s.confidence,
            &s.path,
            &s.signature,
            &s.scope,
            &format!("{} {} {}", s.name, s.target, s.signature).to_ascii_lowercase(),
        ] {
            intern(field, &mut records)?;
        }
        put(&mut records, s.line)?;
        put(&mut records, s.end_line)?;
        put(&mut records, s.ordinal)?;
        for name in [&s.name, &s.target] {
            if !name.is_empty() {
                names.entry(name.clone()).or_default().insert(id);
            }
        }
        if s.kind == "declaration" {
            for (at, _) in s.name.match_indices('.') {
                names.entry(s.name[at + 1..].into()).or_default().insert(id);
            }
        }
        if records.len() > MAX_BYTES || names.len() > 4_000_000 {
            return Err(budget());
        }
    }
    let mut dictionary = vec![];
    let mut postings = vec![];
    for (name, ids) in &names {
        intern(name, &mut dictionary)?;
        put(&mut dictionary, postings.len() / 4)?;
        put(&mut dictionary, ids.len())?;
        for &id in ids {
            put(&mut postings, id)?;
        }
    }
    let mut raw = MAGIC.to_vec();
    for n in [
        meta.len(),
        map.symbols.len(),
        names.len(),
        postings.len() / 4,
        pool.len(),
    ] {
        put(&mut raw, n)?;
    }
    for section in [&meta, &records, &dictionary, &postings, &pool] {
        if raw
            .len()
            .checked_add(section.len())
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(budget());
        }
        raw.extend_from_slice(section);
    }
    Ok(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::Facts;
    fn map() -> (Index, SourcePin) {
        let pin = SourcePin {
            repository: "fixture".into(),
            product: "retail".into(),
            exact_commit: "a".repeat(40),
            parser_revision: "source-v1".into(),
            ..Default::default()
        };
        let names = [
            ("declaration", "Alpha.Timer.After", "", "api-function"),
            ("declaration", "Timer.Afternoon", "", "api-function"),
            ("declaration", "Other", "", "lua-function"),
            ("relationship", "caller", "Alpha.Timer.After", "call"),
            ("header", "Interface", "120100", "toc-header"),
        ];
        let symbols = names
            .into_iter()
            .enumerate()
            .map(|(ordinal, (kind, name, target, category))| Symbol {
                id: format!("ID-{ordinal}"),
                kind: kind.into(),
                name: name.into(),
                target: target.into(),
                category: category.into(),
                confidence: "exact".into(),
                path: "Fixture.lua".into(),
                line: 1,
                end_line: 1,
                signature: "Timer input".into(),
                scope: "file".into(),
                ordinal,
            })
            .collect();
        (
            Index {
                documents: vec![Document {
                    path: "Fixture.lua".into(),
                    text: b"literal unique body\n".to_vec(),
                    bytes: 20,
                    sha256: storage::sha256(b"literal unique body\n"),
                    facts: Facts::default(),
                }],
                symbols,
                assets: vec![json!({"path":"Assets/Timer.blp"})],
                summary: json!({"repository":pin.repository,"product":pin.product,"commit":pin.exact_commit,"parser":pin.parser_revision,"complete":true,"skippedDocuments":0,"diagnostics":0}),
                home: Path::new(".").into(),
            },
            pin,
        )
    }
    #[test]
    fn file_projection_preserves_exact_suffix_prefix_indexed_body_assets_and_cursor() {
        let (full, pin) = map();
        let identity = index::fingerprint(&pin);
        let raw = write_image(&full, &identity).unwrap();
        assert_ne!(&raw[..8], b"SQLite f");
        for (text, mode, topic) in [
            ("Timer.After", "precise", "api"),
            ("Timer.A", "precise", "lua"),
            ("Timer input", "precise", ""),
            ("unique", "precise", ""),
            ("After", "exploratory", "api"),
            ("Timer", "precise", "asset"),
        ] {
            let mut projected = Image::decode(&raw, &identity)
                .unwrap()
                .project(
                    Path::new("."),
                    &pin,
                    text,
                    mode,
                    topic,
                    true,
                    raw.len(),
                    std::time::Instant::now(),
                )
                .unwrap();
            // The real store uses original content-addressed blobs. Keep this
            // purely projection test independent of disk/evidence behavior.
            projected.documents[0].text = full.documents[0].text.clone();
            let first = full.search(&pin, "PIN", text, mode, topic, 1, "").unwrap();
            let other = projected
                .search(&pin, "PIN", text, mode, topic, 1, "")
                .unwrap();
            for field in [
                "results",
                "relations",
                "truncated",
                "nextCursor",
                "matchCoverage",
            ] {
                assert_eq!(first[field], other[field], "{text}/{mode}/{topic}/{field}");
            }
            if let Some(cursor) = first["nextCursor"].as_str() {
                let next = full
                    .search(&pin, "PIN", text, mode, topic, 1, cursor)
                    .unwrap();
                let other = projected
                    .search(&pin, "PIN", text, mode, topic, 1, cursor)
                    .unwrap();
                assert_eq!(next["results"], other["results"]);
            }
        }
    }
    #[test]
    fn file_projection_rejects_truncated_offsets_identity_and_invalid_strings() {
        let (full, pin) = map();
        let identity = index::fingerprint(&pin);
        let raw = write_image(&full, &identity).unwrap();
        for len in [0, 7, 27, raw.len() - 1] {
            assert!(Image::decode(&raw[..len], &identity).is_err());
        }
        assert!(Image::decode(&raw, "other-identity").is_err());
        let mut altered = raw.clone();
        let record_start = 28 + number(&raw, 8).unwrap();
        altered[record_start..record_start + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(Image::decode(&altered, &identity).is_err());
        let mut altered = raw;
        *altered.last_mut().unwrap() = 0xff;
        assert!(Image::decode(&altered, &identity).is_err());
    }
}
