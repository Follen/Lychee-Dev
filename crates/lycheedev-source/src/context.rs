//! Bounded original-source snippets. Structural candidate sites are not runtime binding proof.
use crate::{SourceTools, index, relations};
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError, evidence, selection, storage};
use serde_json::json;
use std::{borrow::Cow, collections::{BTreeMap, BTreeSet}, sync::Arc};
pub fn dispatch(inv: &Invocation) -> Result<Outcome> {
    dispatch_with_tools(inv, SourceTools::default())
}
pub fn dispatch_with_tools(inv: &Invocation, tools: SourceTools<'_>) -> Result<Outcome> {
    dispatch_with_services(inv, tools, None)
}
pub fn dispatch_with_services(
    inv: &Invocation,
    tools: SourceTools<'_>,
    broker: Option<&crate::session::BrokerIdentity>,
) -> Result<Outcome> {
    let reading = evaluate(inv, &tools, broker, None, None, &mut BTreeMap::new())?;
    let source = reading.pin.source.as_ref().expect("resolved source");
    let complete = reading.data["complete"] == true;
    let bytes = crate::research_material::measured("source.context.serialize_capture", || {
        Ok(serde_json::to_vec(&reading.data)?)
    })?;
    let capture = crate::research_material::measured("source.context.commit_capture", || evidence::commit_capture_bounded(
        &inv.home,
        lycheedev_core::evidence::CaptureInput {
            bytes: &bytes,
            maximum_bytes: lycheedev_core::evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":"source-context","locator":inv.flag("--symbol-id").or(inv.flag("--symbol")).unwrap(),"snapshot":reading.pin.id,"sourceCommit":source.exact_commit}),
            complete: complete,
            truncated: reading.data["truncated"] == true,
        },
        &inv.control,
    ))?;
    drop(bytes);
    let mut result = Outcome::new(reading.data);
    result.complete = complete;
    result.coverage = result.data["coverage"].clone();
    result.selection = json!({"snapshot":reading.pin.id});
    result.provenance.push(capture);
    Ok(result)
}
/// Capture-free evaluator used by a batch's single evidence transaction.
pub(crate) fn evaluate(
    inv: &Invocation,
    tools: &SourceTools<'_>,
    broker: Option<&crate::session::BrokerIdentity>,
    prepared: Option<&relations::PreparedResearch>,
    native_response: Option<serde_json::Value>,
    documents: &mut BTreeMap<String, Vec<u8>>,
) -> Result<relations::Reading> {
    let limit = inv.usize_flag("--limit", 50, 1, 200)?;
    let max_lines = inv
        .usize_flag("--max-lines", 120, 1, 2000)
        .map_err(|_| ToolkitError::arguments("--max-lines must be between 1 and 2000"))?;
    let max_bytes = inv
        .usize_flag("--max-bytes", 16 << 10, 1, 1 << 20)
        .map_err(|_| {
            ToolkitError::arguments("source context --max-bytes must be between 1 and 1048576")
        })?;
    let depth = inv
        .usize_flag("--depth", 1, 0, 4)
        .map_err(|_| ToolkitError::arguments("--depth must be between 0 and 4"))?;
    let pin = match prepared { Some(shared) => shared.pin.clone(), None => selection::resolve_pinned(inv)? };
    let source = pin.source.as_ref().ok_or_else(|| {
        ToolkitError::new("selection.source_missing", "selected pin has no source", 3)
    })?;
    let hash = cursor_key(inv, tools, source, limit, max_bytes, max_lines, depth);
    let key = &hash[..16];
    let cursor = Cursor::decode(inv.flag("--cursor").unwrap_or(""), key)?;
    let mut related_inv = inv.clone();
    related_inv.flags.remove("--cursor");
    related_inv
        .flags
        .insert("--direction".into(), "both".into());
    if !cursor.relation.is_empty() {
        related_inv
            .flags
            .insert("--cursor".into(), cursor.relation.clone());
    }
    let mut reading = if inv.flag_bool("--flow") {
        related_inv.flags.remove("--flow");related_inv.flags.remove("--release");related_inv.flags.remove("--session-reuse");
        related_inv.flags.insert("--static-only".into(),"true".into());
        if let Some(shared) = prepared {
            relations::with_prepared(&related_inv, tools, shared, None)?
        } else { relations::with_services(&related_inv,None,None)? }
    } else if let Some(shared) = prepared {
        relations::with_prepared(&related_inv, tools, shared, native_response)?
    } else {relations::with_native_services(&related_inv, tools, broker)?};
    let source = reading.pin.source.as_ref().unwrap();
    let relation_next = reading.data["nextCursor"].as_str().unwrap_or("").to_owned();
    reading.data.as_object_mut().unwrap().remove("nextCursor");
    let mut sites = vec![];
    if let Some(path) = reading.data["symbol"]["path"].as_str() {
        let first = reading.data["symbol"]["line"].as_u64().unwrap_or(1) as usize;
        let last = reading.data["symbol"]["endLine"]
            .as_u64()
            .unwrap_or(first as u64) as usize;
        sites.push((
            path.to_owned(),
            first,
            last.max(first),
            "definition".to_owned(),
        ));
        if reading.data["symbol"]["category"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("api-"))
            && let Some(env) = &reading.environment
        {
            let found = env
                .facts
                .iter()
                .filter(|fact| {
                    fact.name == reading.data["symbol"]["name"]
                        && fact.path == path
                        && fact.line >= first
                        && fact.line <= last.max(first)
                })
                .collect::<Vec<_>>();
            if found.len() == 1 {
                reading.data["apiFact"] = serde_json::to_value(found[0])?;
            } else if found.len() > 1 {
                reading.data["coverage"]["reasons"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!("API fact identity ambiguous"));
            }
        }
    }
    if depth > 0 {
        for relation in reading.data["relations"].as_array().unwrap() {
            if let (Some(path), Some(line)) = (relation["path"].as_str(), relation["line"].as_u64())
            {
                sites.push((
                    path.into(),
                    (line as usize).saturating_sub(2).max(1),
                    line as usize + 2,
                    format!("relation:{}", relation["kind"].as_str().unwrap_or("")),
                ));
            }
        }
        let names = reading.data["relations"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["kind"] == "call")
            .filter_map(|row| row["target"].as_str())
            .filter(|name| !name.is_empty() && *name != "<dynamic>")
            .map(str::to_owned)
            .collect::<BTreeSet<_>>();
        for name in names {
            let candidates = reading
                .index
                .symbols
                .iter()
                .filter(|symbol| {
                    symbol.kind == "declaration"
                        && symbol.name == name
                        && ["function", "local-function"].contains(&symbol.category.as_str())
                })
                .collect::<Vec<_>>();
            if candidates.len() == 1 {
                let symbol = candidates[0];
                sites.push((
                    symbol.path.clone(),
                    symbol.line,
                    symbol.end_line.max(symbol.line),
                    format!("candidate-callee:{name}"),
                ));
            } else if candidates.len() > 1 {
                reading.data["coverage"]["reasons"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(format!("callee candidate ambiguous: {name}")));
            }
        }
    }
    if depth > 1 {
        let mut frontier = BTreeSet::new();
        for row in reading.data["relations"].as_array().unwrap() {
            for field in ["source", "target"] {
                if let Some(name) = row[field].as_str() {
                    frontier.insert(name.to_owned());
                }
            }
        }
        let mut visited = BTreeSet::new();
        for level in 2..=depth {
            let mut next = BTreeSet::new();
            for symbol in &reading.index.symbols {
                inv.control.check()?;
                if symbol.kind != "relationship"
                    || !(frontier.contains(&symbol.name) || frontier.contains(&symbol.target))
                {
                    continue;
                }
                if sites.len() >= 2000 {
                    reading.data["truncated"] = json!(true);
                    continue;
                }
                if !visited.insert((symbol.path.clone(), symbol.line, symbol.category.clone())) {
                    continue;
                }
                sites.push((
                    symbol.path.clone(),
                    symbol.line.saturating_sub(2).max(1),
                    symbol.line + 2,
                    format!("static-depth:{level}"),
                ));
                next.insert(symbol.name.clone());
                next.insert(symbol.target.clone());
            }
            frontier = next;
            if frontier.is_empty() {
                break;
            }
        }
    }
    let mut snippets = vec![];
    let mut seen = BTreeSet::new();
    sites.retain(|(path, first, last, _)| {
        reading
            .index
            .documents
            .iter()
            .any(|document| document.path == *path)
            && seen.insert((path.clone(), *first, *last))
    });
    if cursor.site > sites.len() {
        return Err(invalid_cursor());
    }
    let (mut lines, mut bytes) = (0usize, 0usize);
    let mut truncated = reading.data["truncated"] == true;
    let mut next_cursor = None;
    let mut originals = reading.index.original_reader();
    // Cold bytes already belong to this immutable index. Memoize references
    // within the context request; the batch's existing map owns warm reads.
    let mut borrowed_documents = BTreeMap::<&str, &[u8]>::new();
    let snippets_result = crate::research_material::measured("source.context.snippets", || {
    for (i, (path, first, last, basis)) in sites.iter().enumerate().skip(cursor.site) {
        inv.control.check()?;
        let start = if i == cursor.site && cursor.line > 0 {
            cursor.line
        } else {
            *first
        };
        let start_byte = if i == cursor.site { cursor.byte } else { 0 };
        if snippets.len() == limit || lines == max_lines {
            truncated = true;
            next_cursor = Some(
                Cursor {
                    relation: cursor.relation.clone(),
                    site: i,
                    line: start,
                    byte: start_byte,
                }
                .encode(key),
            );
            break;
        }
        if !documents.contains_key(path) && !borrowed_documents.contains_key(path.as_str()) {
            let document = reading.index.documents.iter().find(|document| document.path == *path)
                .expect("site admitted from shared immutable index");
            match originals.read(document, &inv.control)? {
                Cow::Borrowed(raw) => { borrowed_documents.insert(&document.path, raw); }
                Cow::Owned(raw) => { documents.insert(path.clone(), raw); }
            }
        }
        let raw = borrowed_documents.get(path.as_str()).copied().unwrap_or_else(|| documents[path].as_slice());
        let excerpt = context_excerpt(
            raw,
            start,
            *last,
            start_byte,
            max_bytes - bytes,
            max_lines - lines,
        )?;
        if excerpt.text.is_empty() {
            continue;
        }
        bytes += excerpt.text.len();
        lines += excerpt.last - start + 1;
        snippets.push(json!({"path":path,"firstLine":start,"lastLine":excerpt.last,"contentHash":storage::sha256(&raw),"text":excerpt.text,"basis":basis}));
        if !excerpt.done {
            truncated = true;
            next_cursor = Some(
                Cursor {
                    relation: cursor.relation.clone(),
                    site: i,
                    line: excerpt.next_line,
                    byte: excerpt.next_byte,
                }
                .encode(key),
            );
            break;
        }
        if lines == max_lines || bytes >= max_bytes {
            if i + 1 < sites.len() {
                truncated = true;
                next_cursor = Some(
                    Cursor {
                        relation: cursor.relation.clone(),
                        site: i + 1,
                        line: 0,
                        byte: 0,
                    }
                    .encode(key),
                );
            }
            break;
        }
    }
    Ok(())
    });
    drop(borrowed_documents);
    originals.finish(snippets_result, &inv.control)?;
    if next_cursor.is_none() && !relation_next.is_empty() {
        next_cursor = Some(
            Cursor {
                relation: relation_next,
                site: 0,
                line: 0,
                byte: 0,
            }
            .encode(key),
        );
    }
    if let Some(next) = next_cursor {
        reading.data["nextCursor"] = json!(next);
    }
    reading.data["snippets"] = json!(snippets);
    let mut load_evidence = Vec::new();
    let load_facts=crate::research_material::measured("source.context.load_facts", || {
        match reading.load_facts.take() {
            Some(facts)=>Ok(facts),
            None=>index::load_facts_with_control(&inv.home, source, &reading.index, &inv.control).map(Arc::new),
        }
    })?;
    if let Some(target) = reading.data["symbol"]["path"].as_str() {
        let limit = inv.usize_flag("--limit", 50, 1, 200)?;
        for fact in load_facts.iter() {
            if resolve_load(&fact.path, &fact.target).as_deref() != Some(target) {
                continue;
            }
            if load_evidence.len() == limit {
                truncated = true;
                break;
            }
            load_evidence.push(json!({"source":fact.path,"target":fact.target,"kind":"load","state":"structural","path":fact.path,"line":fact.line,"basis":["TOC/XML literal load"],"reason":"load declaration; runtime execution not proven","sourceBlob":fact.source_blob}));
        }
    }
    reading.data["loadEvidence"] = json!(load_evidence);
    if inv.flag_bool("--flow") {
        #[cfg(feature="emmy-prototype")]
        {
            if let Some(reason) = prepared.and_then(|shared| shared.native_unavailable.as_ref()).filter(|_| tools.luac.is_none()) {
                reading.data["flow"] = json!({"rules_version":crate::native::FLOW_RULES,"findings":{"rows":[],"has_more":false},"guarded":[],
                    "gaps":[{"code":"flow.compiler_unavailable","detail":reason}],"complete":false,"input_complete":false,"analysis_complete":false,"output_complete":true,"truncated":false});
                reading.data["coverage"]["flow"] = json!("unavailable");
            } else { reading.data["flow"]=crate::flow::public::context_flow(inv,&reading,tools,&load_facts)?; }
        }
        #[cfg(not(feature="emmy-prototype"))]
        {return Err(ToolkitError::new("flow.unavailable","this build does not contain the native Source engine",3));}
    }
    reading.data["truncated"] = json!(truncated);
    let complete = reading.data["complete"] == true && !truncated && (!inv.flag_bool("--flow")||reading.data["flow"]["complete"]==true);
    reading.data["complete"] = json!(complete);
    if truncated {
        reading.data["coverage"]["state"] = json!("partial");
        reading.data["coverage"]["reasons"]
            .as_array_mut()
            .unwrap()
            .push(json!(
                "context output exceeded the selected line, byte or relation budget"
            ));
    }
    if depth == 0 || cursor.site > 0 {
        reading.data["relations"] = json!([]);
    }
    Ok(reading)
}

fn cursor_key(inv: &Invocation, tools: &SourceTools<'_>, source: &selection::SourcePin, limit: usize, max_bytes: usize, max_lines: usize, depth: usize) -> String {
    let mode = if tools.luac.is_some()&&!inv.flag_bool("--static-only"){crate::native_research::RESEARCH_RULES}else{"static"};
    let identity = format!(
        "{}\0{}\0{}\0{}\0{}\0{limit}\0{max_bytes}\0{max_lines}\0{depth}\0{}\0{}\0{mode}",
        source.repository,
        source.product,
        source.exact_commit,
        inv.flag("--symbol-id").unwrap_or(""),
        inv.flag("--symbol").unwrap_or(""),
        inv.flag("--environment").unwrap_or(""),
        inv.flag_bool("--flow")
    );
    storage::sha256(identity.as_bytes())
}
pub(crate) fn validate_prepared_cursor(inv: &Invocation, tools: &SourceTools<'_>, prepared: &relations::PreparedResearch) -> Result<()> {
    let key = cursor_key(inv, tools, prepared.pin.source.as_ref().expect("prepared source pin"),
        inv.usize_flag("--limit",50,1,200)?, inv.usize_flag("--max-bytes",16<<10,1,1<<20)?,
        inv.usize_flag("--max-lines",120,1,2000)?, inv.usize_flag("--depth",1,0,4)?);
    let cursor = Cursor::decode(inv.flag("--cursor").unwrap_or(""), &key[..16])?;
    let mut relation = inv.clone(); relation.flags.remove("--cursor"); relation.flags.remove("--flow");
    relation.flags.insert("--direction".into(),"both".into());
    if inv.flag_bool("--flow") { relation.flags.insert("--static-only".into(),"true".into()); }
    if !cursor.relation.is_empty() { relation.flags.insert("--cursor".into(),cursor.relation); }
    relations::validate_prepared_cursor(&relation,tools,prepared)
}
pub(crate) fn validate_cursor_shape(raw: &str) -> Result<()> {
    if raw.is_empty() { return Ok(()); }
    let key = raw.split(':').next().ok_or_else(invalid_cursor)?;
    if key.len() != 16 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err(invalid_cursor()); }
    let cursor = Cursor::decode(raw, key)?;
    if !cursor.relation.is_empty() {
        let Some((key, offset)) = cursor.relation.split_once(':') else { return Err(invalid_cursor()); };
        if key.len() != 16 || !key.bytes().all(|byte| byte.is_ascii_hexdigit())
            || offset.parse::<usize>().ok().is_none_or(|value| value > 10000) { return Err(invalid_cursor()); }
    }
    Ok(())
}
fn invalid_cursor() -> ToolkitError {
    ToolkitError::new(
        "codebase.invalid_search_cursor",
        "context cursor belongs to a different query or exceeds source bounds",
        2,
    )
}
#[derive(Default)]
struct Cursor {
    relation: String,
    site: usize,
    line: usize,
    byte: usize,
}
impl Cursor {
    fn decode(raw: &str, key: &str) -> Result<Self> {
        if raw.is_empty() {
            return Ok(Self::default());
        }
        if raw.len() > 256 {
            return Err(invalid_cursor());
        }
        let parts = raw.split(':').collect::<Vec<_>>();
        if parts.len() != 5 || parts[0] != key {
            return Err(invalid_cursor());
        }
        let number = |part: &str, max: usize| {
            part.parse::<usize>()
                .ok()
                .filter(|n| *n <= max)
                .ok_or_else(invalid_cursor)
        };
        Ok(Self {
            relation: decode64(parts[1])?,
            site: number(parts[2], 2000)?,
            line: number(parts[3], 1_000_000)?,
            byte: number(parts[4], 1 << 20)?,
        })
    }
    fn encode(&self, key: &str) -> String {
        format!(
            "{key}:{}:{}:{}:{}",
            encode64(self.relation.as_bytes()),
            self.site,
            self.line,
            self.byte
        )
    }
}
const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
fn encode64(raw: &[u8]) -> String {
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let n = ((chunk[0] as u32) << 16)
            | ((chunk.get(1).copied().unwrap_or(0) as u32) << 8)
            | chunk.get(2).copied().unwrap_or(0) as u32;
        for i in 0..chunk.len() + 1 {
            out.push(ALPHABET[((n >> (18 - i * 6)) & 63) as usize] as char);
        }
    }
    out
}
fn decode64(raw: &str) -> Result<String> {
    if raw.len() > 172 || raw.len() % 4 == 1 {
        return Err(invalid_cursor());
    }
    let mut out = vec![];
    let (mut n, mut bits) = (0u32, 0);
    for byte in raw.bytes() {
        let digit = ALPHABET
            .iter()
            .position(|b| *b == byte)
            .ok_or_else(invalid_cursor)?;
        n = (n << 6) | digit as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((n >> bits) & 255) as u8);
            n &= (1 << bits) - 1;
        }
    }
    if n != 0 || out.len() > 128 {
        return Err(invalid_cursor());
    }
    String::from_utf8(out).map_err(|_| invalid_cursor())
}
struct Excerpt {
    text: String,
    last: usize,
    next_line: usize,
    next_byte: usize,
    done: bool,
}
fn context_excerpt(
    raw: &[u8],
    first: usize,
    last: usize,
    start_byte: usize,
    max_bytes: usize,
    max_lines: usize,
) -> Result<Excerpt> {
    let text = std::str::from_utf8(raw)
        .map_err(|_| ToolkitError::invalid("source context is not UTF-8"))?
        .replace("\r\n", "\n");
    let rows = text.split('\n').collect::<Vec<_>>();
    if first == 0 {
        return Err(invalid_cursor());
    }
    let (mut line, mut byte, mut out, mut final_line) =
        (first, start_byte, String::new(), first.saturating_sub(1));
    while line <= last && line <= rows.len() && line - first < max_lines {
        let whole = format!("{line}: {}", rows[line - 1].trim_end_matches('\r'));
        if byte > whole.len() || !whole.is_char_boundary(byte) {
            return Err(invalid_cursor());
        }
        let separator = usize::from(!out.is_empty());
        let available = max_bytes.saturating_sub(out.len() + separator);
        if available == 0 {
            break;
        }
        let part = &whole[byte..];
        let mut cut = part.len().min(available);
        while cut > 0 && !part.is_char_boundary(cut) {
            cut -= 1;
        }
        if cut == 0 && !part.is_empty() {
            return Err(ToolkitError::new(
                "codebase.source_budget",
                "UTF-8 codepoint exceeds context byte budget",
                3,
            ));
        }
        if separator > 0 {
            out.push('\n');
        }
        out.push_str(&part[..cut]);
        final_line = line;
        if cut < part.len() {
            return Ok(Excerpt {
                text: out,
                last: line,
                next_line: line,
                next_byte: byte + cut,
                done: false,
            });
        }
        line += 1;
        byte = 0;
    }
    Ok(Excerpt {
        text: out,
        last: final_line,
        next_line: line,
        next_byte: 0,
        done: line > last || line > rows.len(),
    })
}
fn resolve_load(path: &str, target: &str) -> Option<String> {
    let path = path.replace('\\', "/");
    let target = target.replace('\\', "/");
    if target.starts_with('/') || target.contains(':') {
        return None;
    }
    let mut parts: Vec<_> = path.split('/').collect();
    parts.pop();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}
