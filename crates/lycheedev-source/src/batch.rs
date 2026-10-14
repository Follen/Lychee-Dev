//! Fixed-snapshot batch research. Input admission precedes preparation; item
//! evaluators never call public dispatch, and only the batch publishes evidence.
use crate::{SourceTools, context, environment, index, native_research, relations};
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError, evidence, selection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
const FILE_BYTES: usize = 64 << 10;
const MAX_ITEMS: usize = 32;
const WORKER_BYTES: usize = 768 << 20;
const RESULT_BYTES: usize = 32 << 20;
const WORKER_SCHEMA: &str = "lycheedev.source-native-batch.v1";

#[derive(Clone, Default, Debug)]
pub struct BatchRequest {
    pub requests: Vec<BatchItem>,
}
#[derive(Clone, Default, Debug)]
pub struct BatchItem {
    pub id: String,
    pub query: Option<SearchQuery>,
    pub refs: Option<RelationQuery>,
    pub context: Option<ContextQuery>,
}
#[derive(Clone, Default, Debug)]
pub struct SearchQuery {
    pub mode: String,
    pub text: String,
    pub topic: String,
    pub limit: i64,
    pub cursor: String,
}
#[derive(Clone, Default, Debug)]
pub struct RelationQuery {
    pub symbol_id: String,
    pub symbol: String,
    pub direction: String,
    pub limit: i64,
    pub cursor: String,
}
#[derive(Clone, Default, Debug)]
pub struct ContextQuery {
    pub symbol_id: String,
    pub symbol: String,
    pub limit: i64,
    pub max_bytes: i64,
    pub max_lines: i64,
    pub depth: i64,
    pub cursor: String,
}
fn invalid_batch(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("codebase.invalid_batch", message, 2)
}
fn invalid_query(message: impl Into<String>) -> ToolkitError {
    ToolkitError::new("codebase.invalid_query", message, 2)
}
fn invalid_cursor() -> ToolkitError {
    ToolkitError::new(
        "codebase.invalid_search_cursor",
        "Invalid source batch item cursor",
        2,
    )
}
fn item_error(mut error: ToolkitError, i: usize, item: &BatchItem) -> ToolkitError {
    error.message = format!(
        "source batch request {} ({}): {}",
        i + 1,
        item.id,
        error.message
    );
    error
}
impl BatchRequest {
    pub fn needs_research(&self) -> bool {
        self.requests
            .iter()
            .any(|item| item.refs.is_some() || item.context.is_some())
    }
    /// Validate all input-only rules before opening the source store, building
    /// an index, authenticating a runtime or publishing any evidence.
    pub fn validate(&self) -> Result<()> {
        if !(1..=MAX_ITEMS).contains(&self.requests.len()) {
            return Err(invalid_batch("expected 1-32 requests"));
        }
        for (i, item) in self.requests.iter().enumerate() {
            if [
                item.query.is_some(),
                item.refs.is_some(),
                item.context.is_some(),
            ]
            .iter()
            .filter(|set| **set)
            .count()
                != 1
            {
                return Err(invalid_batch(format!(
                    "request {} requires exactly one of query, refs, context",
                    i + 1
                )));
            }
            let validate = || -> Result<()> {
                if let Some(query) = &item.query {
                    let text = query.text.trim();
                    let topic = query.topic.trim().to_lowercase();
                    if text.is_empty() || text.len() > 512 {
                        return Err(invalid_query("codebase.query_required"));
                    }
                    if !["", "api", "lua", "xml", "toc", "asset"].contains(&topic.as_str()) {
                        return Err(invalid_query("codebase.invalid_topic"));
                    }
                    if query.limit > 200 {
                        return Err(invalid_query("codebase.invalid_search_limit"));
                    }
                    if !["", "precise", "exploratory"].contains(&query.mode.as_str()) {
                        return Err(invalid_query("codebase.invalid_search_mode"));
                    }
                    cursor_shape(&query.cursor, false)?;
                }
                if let Some(query) = &item.refs {
                    selector(&query.symbol_id, &query.symbol)?;
                    if !["", "incoming", "outgoing", "both"].contains(&query.direction.as_str()) {
                        return Err(invalid_query("codebase.invalid_relation_direction"));
                    }
                    research_limit(query.limit)?;
                    cursor_shape(&query.cursor, false)?;
                }
                if let Some(query) = &item.context {
                    if !(0..=4).contains(&query.depth) {
                        return Err(invalid_query("codebase.invalid_context_depth"));
                    }
                    if query.max_bytes < 0 || query.max_bytes > 1 << 20 {
                        return Err(invalid_query("codebase.invalid_context_bytes"));
                    }
                    if query.max_lines < 0 || query.max_lines > 2000 {
                        return Err(invalid_query("codebase.invalid_context_lines"));
                    }
                    research_limit(query.limit)?;
                    selector(&query.symbol_id, &query.symbol)?;
                    cursor_shape(&query.cursor, true)?;
                }
                Ok(())
            };
            validate().map_err(|error| item_error(error, i, item))?;
        }
        Ok(())
    }
}
fn selector(id: &str, symbol: &str) -> Result<()> {
    if id.is_empty() == symbol.is_empty() {
        return Err(ToolkitError::new(
            "codebase.invalid_symbol",
            "Exactly one of symbolId or symbol is required",
            2,
        ));
    }
    Ok(())
}
fn research_limit(value: i64) -> Result<usize> {
    match value {
        0 => Ok(25),
        1..=200 => Ok(value as usize),
        _ => Err(invalid_query("codebase.invalid_relation_limit")),
    }
}
fn cursor_shape(raw: &str, context: bool) -> Result<()> {
    if raw.is_empty() {
        return Ok(());
    }
    if context {
        return crate::context::validate_cursor_shape(raw);
    }
    let Some((key, number)) = raw.split_once(':') else {
        return Err(invalid_cursor());
    };
    if key.len() != 16
        || !key.bytes().all(|b| b.is_ascii_hexdigit())
        || number.parse::<usize>().ok().is_none_or(|n| n > 10000)
    {
        return Err(invalid_cursor());
    }
    Ok(())
}

// Go accepts repeated known fields (merging pointed-to objects), null scalar
// fields and case-insensitive names. Preserve object order to retain those rules
// while rejecting unknown fields at every level and trailing JSON values.
#[derive(Debug)]
enum Json {
    Null,
    Scalar(Value),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}
impl<'de> Deserialize<'de> for Json {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Json;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("JSON value")
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Json, E> {
                Ok(Json::Null)
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Json, E> {
                Ok(Json::Scalar(json!(v)))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Json, E> {
                Ok(Json::Scalar(json!(v)))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Json, E> {
                Ok(Json::Scalar(json!(v)))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Json, E> {
                Ok(Json::Scalar(json!(v)))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Json, E> {
                Ok(Json::Scalar(json!(v)))
            }
            fn visit_string<E: serde::de::Error>(self, v: String) -> std::result::Result<Json, E> {
                Ok(Json::Scalar(Value::String(v)))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Json, A::Error> {
                let mut rows = Vec::new();
                while let Some(row) = seq.next_element()? {
                    rows.push(row);
                }
                Ok(Json::Array(rows))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Json, A::Error> {
                let mut fields = Vec::new();
                while let Some(field) = map.next_entry()? {
                    fields.push(field);
                }
                Ok(Json::Object(fields))
            }
        }
        decoder.deserialize_any(Visitor)
    }
}
fn object(value: Json) -> Result<Vec<(String, Json)>> {
    match value {
        Json::Object(fields) => Ok(fields),
        Json::Null => Ok(Vec::new()),
        _ => Err(invalid_batch("expected JSON object")),
    }
}
fn string(target: &mut String, value: Json) -> Result<()> {
    match value {
        Json::Null => Ok(()),
        Json::Scalar(Value::String(value)) => {
            *target = value;
            Ok(())
        }
        _ => Err(invalid_batch("expected JSON string")),
    }
}
fn integer(target: &mut i64, value: Json) -> Result<()> {
    match value {
        Json::Null => Ok(()),
        Json::Scalar(Value::Number(value)) if value.is_i64() => {
            *target = value.as_i64().unwrap();
            Ok(())
        }
        _ => Err(invalid_batch("expected signed 64-bit JSON integer")),
    }
}
fn fold_key(name: &str) -> String {
    name.chars()
        .map(|ch| match ch {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            _ => ch.to_ascii_lowercase(),
        })
        .collect()
}
fn unknown(name: &str) -> ToolkitError {
    invalid_batch(format!("unknown field {name:?}"))
}
fn decode_query(query: &mut Option<SearchQuery>, value: Json) -> Result<()> {
    if matches!(value, Json::Null) {
        *query = None;
        return Ok(());
    }
    let target = query.get_or_insert_with(Default::default);
    for (key, value) in object(value)? {
        match fold_key(&key).as_str() {
            "mode" => string(&mut target.mode, value)?,
            "text" => string(&mut target.text, value)?,
            "topic" => string(&mut target.topic, value)?,
            "limit" => integer(&mut target.limit, value)?,
            "cursor" => string(&mut target.cursor, value)?,
            _ => return Err(unknown(&key)),
        }
    }
    Ok(())
}
fn decode_refs(query: &mut Option<RelationQuery>, value: Json) -> Result<()> {
    if matches!(value, Json::Null) {
        *query = None;
        return Ok(());
    }
    let target = query.get_or_insert_with(Default::default);
    for (key, value) in object(value)? {
        match fold_key(&key).as_str() {
            "symbolid" => string(&mut target.symbol_id, value)?,
            "symbol" => string(&mut target.symbol, value)?,
            "direction" => string(&mut target.direction, value)?,
            "limit" => integer(&mut target.limit, value)?,
            "cursor" => string(&mut target.cursor, value)?,
            _ => return Err(unknown(&key)),
        }
    }
    Ok(())
}
fn decode_context(query: &mut Option<ContextQuery>, value: Json) -> Result<()> {
    if matches!(value, Json::Null) {
        *query = None;
        return Ok(());
    }
    let target = query.get_or_insert_with(Default::default);
    for (key, value) in object(value)? {
        match fold_key(&key).as_str() {
            "symbolid" => string(&mut target.symbol_id, value)?,
            "symbol" => string(&mut target.symbol, value)?,
            "limit" => integer(&mut target.limit, value)?,
            "maxbytes" => integer(&mut target.max_bytes, value)?,
            "maxlines" => integer(&mut target.max_lines, value)?,
            "depth" => integer(&mut target.depth, value)?,
            "cursor" => string(&mut target.cursor, value)?,
            _ => return Err(unknown(&key)),
        }
    }
    Ok(())
}
// encoding/json replaces invalid UTF-8 bytes and unpaired UTF-16 escapes
// inside strings. Normalize those representations before the strict JSON parser;
// malformed JSON syntax and raw control characters still fail normally.
fn go_json_text(raw: &[u8]) -> String {
    let mut utf8 = String::new();
    let mut rest = raw;
    while !rest.is_empty() {
        match std::str::from_utf8(rest) {
            Ok(text) => {
                utf8.push_str(text);
                break;
            }
            Err(error) => {
                let valid = error.valid_up_to();
                utf8.push_str(std::str::from_utf8(&rest[..valid]).expect("validated UTF-8 prefix"));
                utf8.push('\u{fffd}');
                rest = &rest[valid + 1..];
            }
        }
    }
    let bytes = utf8.as_bytes();
    let mut out = String::with_capacity(utf8.len());
    let mut i = 0;
    let mut quoted = false;
    let code = |at: usize| -> Option<u16> {
        let raw = bytes.get(at..at + 6)?;
        if raw[0] != b'\\' || raw[1] != b'u' {
            return None;
        }
        std::str::from_utf8(&raw[2..])
            .ok()
            .and_then(|hex| u16::from_str_radix(hex, 16).ok())
    };
    while i < bytes.len() {
        if bytes[i] == b'"' {
            quoted = !quoted;
            out.push('"');
            i += 1;
            continue;
        }
        if quoted && bytes[i] == b'\\' {
            if let Some(unit) = code(i) {
                if (0xd800..=0xdbff).contains(&unit) {
                    if code(i + 6).is_some_and(|low| (0xdc00..=0xdfff).contains(&low)) {
                        out.push_str(&utf8[i..i + 12]);
                        i += 12;
                        continue;
                    }
                    out.push_str("\\ufffd");
                    i += 6;
                    continue;
                }
                if (0xdc00..=0xdfff).contains(&unit) {
                    out.push_str("\\ufffd");
                    i += 6;
                    continue;
                }
            }
            out.push('\\');
            i += 1;
            if i == bytes.len() {
                break;
            }
            let next = utf8[i..].chars().next().expect("UTF-8 boundary");
            out.push(next);
            i += next.len_utf8();
            continue;
        }
        let next = utf8[i..].chars().next().expect("UTF-8 boundary");
        out.push(next);
        i += next.len_utf8();
    }
    out
}
pub fn parse(raw: &[u8]) -> Result<BatchRequest> {
    if raw.len() > FILE_BYTES {
        return Err(invalid_batch("request file exceeds 64 KiB"));
    }
    let tree: Json = serde_json::from_str(&go_json_text(raw))
        .map_err(|error| invalid_batch(error.to_string()))?;
    // Go reuses existing slice elements when a repeated requests array is
    // decoded. Omitted fields (including pointer objects) retain those values.
    let mut backing: Vec<BatchItem> = Vec::new();
    let mut length = 0;
    for (key, value) in object(tree)? {
        if fold_key(&key) != "requests" {
            return Err(unknown(&key));
        }
        let rows = match value {
            Json::Null => {
                backing.clear();
                length = 0;
                continue;
            }
            Json::Array(rows) => rows,
            _ => return Err(invalid_batch("requests must be an array")),
        };
        length = rows.len();
        // encoding/json replaces an explicitly empty slice with a fresh slice.
        if length == 0 {
            backing.clear();
        }
        for (i, row) in rows.into_iter().enumerate() {
            if i == backing.len() {
                backing.push(BatchItem::default());
            }
            let item = &mut backing[i];
            for (key, value) in object(row)? {
                match fold_key(&key).as_str() {
                    "id" => string(&mut item.id, value)?,
                    "query" => decode_query(&mut item.query, value)?,
                    "refs" => decode_refs(&mut item.refs, value)?,
                    "context" => decode_context(&mut item.context, value)?,
                    _ => return Err(unknown(&key)),
                }
            }
        }
    }
    backing.truncate(length);
    let request = BatchRequest { requests: backing };
    request.validate()?;
    Ok(request)
}
pub fn read(inv: &Invocation) -> Result<BatchRequest> {
    inv.control.check()?;
    let path = Path::new(inv.required("--file")?);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        inv.cwd.join(path)
    };
    let mut raw = Vec::new();
    std::fs::File::open(path)?
        .take(FILE_BYTES as u64 + 1)
        .read_to_end(&mut raw)?;
    inv.control.check()?;
    let request = parse(&raw)?;
    validate_options(inv)?;
    Ok(request)
}
fn validate_options(inv: &Invocation) -> Result<()> {
    if inv.flag_bool("--static-only")
        && (inv.flag("--release").is_some() || inv.flag_bool("--session-reuse"))
    {
        return Err(ToolkitError::arguments(
            "--static-only cannot be combined with --release or --session-reuse",
        ));
    }
    Ok(())
}
fn item_invocation(inv: &Invocation, item: &BatchItem) -> Result<Invocation> {
    let mut next = inv.clone();
    next.flags.remove("--file");
    next.positional = None;
    let set = |next: &mut Invocation, name: &str, value: &str| {
        if !value.is_empty() {
            next.flags.insert(name.into(), value.into());
        }
    };
    if let Some(query) = &item.query {
        next.command = "source query".into();
        next.positional = Some(query.text.trim().into());
        next.flags.remove("--flow");
        set(&mut next, "--mode", &query.mode);
        set(&mut next, "--topic", &query.topic.trim().to_lowercase());
        set(
            &mut next,
            "--limit",
            &if query.limit <= 0 { 10 } else { query.limit }.to_string(),
        );
        set(&mut next, "--cursor", &query.cursor);
    } else if let Some(query) = &item.refs {
        next.command = "source refs".into();
        next.flags.remove("--flow");
        set(&mut next, "--symbol-id", &query.symbol_id);
        set(&mut next, "--symbol", &query.symbol);
        set(&mut next, "--direction", &query.direction);
        set(
            &mut next,
            "--limit",
            &research_limit(query.limit)?.to_string(),
        );
        set(&mut next, "--cursor", &query.cursor);
    } else if let Some(query) = &item.context {
        next.command = "source context".into();
        set(&mut next, "--symbol-id", &query.symbol_id);
        set(&mut next, "--symbol", &query.symbol);
        set(
            &mut next,
            "--limit",
            &research_limit(query.limit)?.to_string(),
        );
        set(
            &mut next,
            "--max-bytes",
            &if query.max_bytes == 0 {
                16 << 10
            } else {
                query.max_bytes
            }
            .to_string(),
        );
        set(
            &mut next,
            "--max-lines",
            &if query.max_lines == 0 {
                80
            } else {
                query.max_lines
            }
            .to_string(),
        );
        set(&mut next, "--depth", &query.depth.to_string());
        set(&mut next, "--cursor", &query.cursor);
    }
    Ok(next)
}
pub fn dispatch(inv: &Invocation, tools: SourceTools<'_>) -> Result<Outcome> {
    dispatch_request(inv, read(inv)?, tools)
}
/// CLI retains the already-admitted request across runtime preparation: a
/// replaced file cannot change this operation, and the file is read only once.
pub fn dispatch_request(
    inv: &Invocation,
    request: BatchRequest,
    tools: SourceTools<'_>,
) -> Result<Outcome> {
    dispatch_observed(inv, request, tools, None, &mut |_, _| {})
}
/// Only the CLI's implicit-runtime admission calls this fallback. A missing or
/// invalid default runtime is never executed; static evidence stays partial.
pub fn dispatch_unavailable(
    inv: &Invocation,
    request: BatchRequest,
    error: ToolkitError,
) -> Result<Outcome> {
    inv.control.check()?;
    request.validate()?;
    validate_options(inv)?;
    if inv.flag("--release").is_some() || terminal_control_error(&error) {
        return Err(error);
    }
    let mut inv = inv.clone();
    // No persistent engine was admitted, just as Go leaves Analyze unset.
    inv.flags.remove("--session-reuse");
    dispatch_observed(
        &inv,
        request,
        SourceTools::default(),
        Some(error),
        &mut |_, _| {},
    )
}
pub fn terminal_control_error(error: &ToolkitError) -> bool {
    error.exit_code == 7
        || matches!(
            error.code.as_str(),
            "command.deadline" | "command.cancelled" | "operation.cancelled" | "process.cancelled"
        )
}
fn dispatch_observed(
    inv: &Invocation,
    request: BatchRequest,
    tools: SourceTools<'_>,
    unavailable: Option<ToolkitError>,
    observer: &mut impl FnMut(&'static str, usize),
) -> Result<Outcome> {
    request.validate()?;
    validate_options(inv)?;
    let mut inv = inv.clone();
    inv.control = inv.control.child(
        Instant::now() + Duration::from_secs(90),
        inv.control.cancel_handle(),
    );
    inv.control.check()?;
    let pin = selection::resolve_pinned(&inv)?;
    let source = pin.source.as_ref().ok_or_else(|| {
        ToolkitError::new("selection.source_missing", "selected pin has no source", 3)
    })?;
    inv.flags.insert("--snapshot".into(), pin.id.clone());
    let calls = request
        .requests
        .iter()
        .map(|item| item_invocation(&inv, item))
        .collect::<Result<Vec<_>>>()?;
    for (i, item) in request.requests.iter().enumerate() {
        if let Some(query) = &item.query {
            validate_query_cursor(source, query).map_err(|error| item_error(error, i, item))?;
        }
    }
    let native =
        request.needs_research() && !inv.flag_bool("--static-only") && unavailable.is_none();
    if native && tools.luac.is_none() {
        return Err(ToolkitError::new(
            "source.native_unavailable",
            "source batch research requires the verified native Source runtime or explicit --static-only",
            3,
        ));
    }
    let (index, digest) = crate::research_material::measured("source.batch.index", || {
        index::build_material_with_control(&inv.home, source, &inv.control)
    })?;
    let mut queries = Vec::new();
    // Fixed-index selectors for every item are resolved before semantic work.
    for (i, call) in calls
        .iter()
        .enumerate()
        .filter(|(_, call)| call.command != "source query")
    {
        let declarations = index
            .symbols
            .iter()
            .filter(|symbol| {
                symbol.kind == "declaration"
                    && (call.flag("--symbol-id").is_some_and(|id| id == symbol.id)
                        || call
                            .flag("--symbol")
                            .is_some_and(|name| name == symbol.name))
            })
            .collect::<Vec<_>>();
        if call.flag("--symbol-id").is_some() && declarations.is_empty() {
            return Err(item_error(
                ToolkitError::new(
                    "codebase.invalid_symbol",
                    "selected symbol ID is absent from the fixed index",
                    2,
                ),
                i,
                &request.requests[i],
            ));
        }
        if native && !call.flag_bool("--flow") && declarations.len() == 1 {
            queries.push(NativeQuery {
                item: i,
                query: native_research::QueryRequest {
                    symbol: declarations[0].clone(),
                    direction: call.flag("--direction").unwrap_or("both").into(),
                },
            });
        }
    }
    let selected_environment = if request.needs_research() {
        inv.flag("--environment")
            .map(|id| selection::load_pinned(&inv.home, id))
            .transpose()?
    } else {
        None
    };
    if selected_environment
        .as_ref()
        .is_some_and(|pin| pin.source.is_none())
    {
        return Err(ToolkitError::new(
            "selection.source_missing",
            "selected environment pin has no source",
            3,
        ));
    }
    let environment_source = selected_environment
        .as_ref()
        .and_then(|pin| pin.source.as_ref())
        .or_else(|| {
            (request.needs_research() && source.repository == "wow-ui-source").then_some(source)
        });
    let (environment, load_facts, snapshot) = if !queries.is_empty() {
        let material = crate::research_material::load_or_build(
            &inv,
            source,
            environment_source,
            &index,
            &digest,
            tools.luac.expect("admitted native runtime"),
        )?;
        (
            material.environment,
            material.load_facts,
            Some(material.snapshot),
        )
    } else {
        let environment = environment_source
            .map(|pin| environment::from_pin(&inv.home, pin, &inv.control))
            .transpose()?;
        let facts = if request.requests.iter().any(|item| item.context.is_some()) {
            index::load_facts_with_control(&inv.home, source, &index, &inv.control)?
        } else {
            Vec::new()
        };
        (environment, facts, None)
    };
    let shared = relations::PreparedResearch {
        pin: pin.clone(),
        index: Arc::new(index),
        environment: environment.map(Arc::new),
        load_facts: Arc::new(load_facts),
        native_unavailable: unavailable.as_ref().map(ToString::to_string),
    };
    for (i, call) in calls.iter().enumerate() {
        let checked = match call.command.as_str() {
            "source refs" => relations::validate_prepared_cursor(call, &tools, &shared),
            "source context" => context::validate_prepared_cursor(call, &tools, &shared),
            _ => Ok(()),
        };
        checked.map_err(|error| item_error(error, i, &request.requests[i]))?;
    }
    let mut responses = if let Some(snapshot) = snapshot {
        analyze(&inv, snapshot, queries, &request)?
    } else {
        BTreeMap::new()
    };
    let mut rows = Vec::new();
    let mut complete = true;
    let mut truncated = false;
    let mut documents = BTreeMap::new();
    for (i, (item, call)) in request.requests.iter().zip(&calls).enumerate() {
        inv.control.check()?;
        let result = if let Some(query) = &item.query {
            shared.index.search_observed(
                source,
                &pin.id,
                query.text.trim(),
                &query.mode,
                &query.topic.trim().to_lowercase(),
                if query.limit <= 0 {
                    10
                } else {
                    query.limit as usize
                },
                &query.cursor,
                Some(&inv.control),
                observer,
            )
        } else if item.refs.is_some() {
            relations::with_prepared(call, &tools, &shared, responses.remove(&i))
                .map(|reading| reading.data)
        } else {
            context::evaluate(
                call,
                &tools,
                None,
                Some(&shared),
                responses.remove(&i),
                &mut documents,
            )
            .map(|reading| reading.data)
        }
        .map_err(|error| item_error(error, i, item))?;
        inv.control.check()?;
        complete &= result["complete"] == true;
        truncated |= result["truncated"] == true;
        let mut row = serde_json::Map::new();
        if !item.id.is_empty() {
            row.insert("id".into(), json!(item.id));
        }
        row.insert(
            if item.query.is_some() {
                "query"
            } else if item.refs.is_some() {
                "refs"
            } else {
                "context"
            }
            .into(),
            result,
        );
        rows.push(Value::Object(row));
    }
    if !responses.is_empty() {
        return Err(ToolkitError::new(
            "source.native_worker_protocol",
            "Unconsumed batch native results",
            4,
        ));
    }
    let data = json!({"snapshot":pin.id,"results":rows,"complete":complete,"truncated":truncated});
    inv.control.check()?;
    let (_, raw) = admit_json(&data, &inv.control, evidence::DEFAULT_CAPTURE_BYTES, true).map_err(
        |mut error| {
            if error.code == "source.native_budget" {
                error.code = "vault.blob_limit".into();
                error.exit_code = 4;
            }
            error
        },
    )?;
    inv.control.check()?;
    let capture = evidence::commit_capture_bounded(
        &inv.home,
        evidence::CaptureInput {
            bytes: &raw,
            maximum_bytes: evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":"source-batch","locator":"query/refs/context","snapshot":pin.id,"sourceCommit":source.exact_commit}),
            complete,
            truncated,
        },
        &inv.control,
    )?;
    let mut result = Outcome::new(data);
    result.complete = complete;
    result.selection = json!({"snapshot":pin.id});
    result.provenance.push(capture);
    if let Some(error) = unavailable {
        result.warnings.push(json!(format!(
            "Verified native Emmy runtime unavailable; semantic coverage is incomplete: {error}"
        )));
    }
    Ok(result)
}
fn validate_query_cursor(source: &selection::SourcePin, query: &SearchQuery) -> Result<()> {
    if query.cursor.is_empty() {
        return Ok(());
    }
    let key = lycheedev_core::storage::sha256(
        format!(
            "{}\0{}\0{}\0{}\0{}\0{}",
            source.repository,
            source.product,
            source.exact_commit,
            query.mode,
            query.text.trim(),
            query.topic.trim().to_lowercase()
        )
        .as_bytes(),
    );
    if query
        .cursor
        .split_once(':')
        .is_none_or(|(found, _)| found != &key[..16])
    {
        return Err(invalid_cursor());
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeQuery {
    item: usize,
    query: native_research::QueryRequest,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeBatch {
    schema: String,
    snapshot: native_research::SnapshotRequest,
    queries: Vec<NativeQuery>,
    remaining_ms: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeReply {
    schema: String,
    result: std::result::Result<Vec<(usize, Value)>, NativeFailure>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct NativeFailure {
    item: Option<usize>,
    error: ToolkitError,
}
/// Count serialized JSON before retaining another result. Both native paths
/// use this same caller-controlled aggregate admission; counting allocates no
/// second JSON tree or full serialized body.
struct JsonAdmission<'a> {
    control: &'a lycheedev_core::control::OperationControl,
    maximum: usize,
    written: usize,
    bytes: Option<Vec<u8>>,
    exceeded: bool,
}
impl Write for JsonAdmission<'_> {
    fn write(&mut self, raw: &[u8]) -> std::io::Result<usize> {
        self.control.check().map_err(std::io::Error::other)?;
        if raw.len() > self.maximum.saturating_sub(self.written) {
            self.exceeded = true;
            return Err(std::io::Error::other("native batch JSON budget exceeded"));
        }
        for block in raw.chunks(64 << 10) {
            self.control.check().map_err(std::io::Error::other)?;
            if let Some(bytes) = &mut self.bytes {
                bytes.extend_from_slice(block);
            }
            self.written += block.len();
        }
        Ok(raw.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.control.check().map_err(std::io::Error::other)
    }
}
fn admit_json(
    value: &impl Serialize,
    control: &lycheedev_core::control::OperationControl,
    maximum: usize,
    collect: bool,
) -> Result<(usize, Vec<u8>)> {
    control.check()?;
    let mut writer = JsonAdmission {
        control,
        maximum,
        written: 0,
        bytes: collect.then(Vec::new),
        exceeded: false,
    };
    let serialized = serde_json::to_writer(&mut writer, value);
    control.check()?;
    if writer.exceeded {
        return Err(ToolkitError::new(
            "source.native_budget",
            "Native batch aggregate JSON exceeds transport budget",
            3,
        ));
    }
    serialized?;
    Ok((writer.written, writer.bytes.unwrap_or_default()))
}
fn collect_native_results(
    queries: Vec<NativeQuery>,
    control: &lycheedev_core::control::OperationControl,
    maximum: usize,
    mut evaluate: impl FnMut(NativeQuery) -> Result<Value>,
) -> std::result::Result<Vec<(usize, Value)>, NativeFailure> {
    let empty = NativeReply {
        schema: WORKER_SCHEMA.into(),
        result: Ok(Vec::new()),
    };
    let (mut total, _) = admit_json(&empty, control, maximum, false)
        .map_err(|error| NativeFailure { item: None, error })?;
    let mut values = Vec::new();
    for query in queries {
        let item = query.item;
        let failure = |error| NativeFailure {
            item: Some(item),
            error,
        };
        control.check().map_err(failure)?;
        let value = evaluate(query).map_err(failure)?;
        let separator = usize::from(!values.is_empty());
        let (bytes, _) = admit_json(
            &(item, &value),
            control,
            maximum.saturating_sub(total + separator),
            false,
        )
        .map_err(failure)?;
        total += bytes + separator;
        values.push((item, value));
    }
    control
        .check()
        .map_err(|error| NativeFailure { item: None, error })?;
    Ok(values)
}
fn native_failure(
    failure: NativeFailure,
    expected: &[usize],
    batch: &BatchRequest,
) -> ToolkitError {
    match failure.item {
        Some(i) if expected.contains(&i) => item_error(failure.error, i, &batch.requests[i]),
        None => failure.error,
        _ => ToolkitError::new(
            "source.native_worker_protocol",
            "Native batch error item differs",
            4,
        ),
    }
}
fn analyze(
    inv: &Invocation,
    snapshot: native_research::SnapshotRequest,
    queries: Vec<NativeQuery>,
    batch: &BatchRequest,
) -> Result<BTreeMap<usize, Value>> {
    let expected = queries.iter().map(|query| query.item).collect::<Vec<_>>();
    let values = if inv.flag_bool("--session-reuse") {
        collect_native_results(queries, &inv.control, RESULT_BYTES, |query| {
            crate::native_session::analyze(
                inv,
                native_research::WorkerRequest {
                    snapshot: snapshot.clone(),
                    query: query.query,
                },
            )
        })
        .map_err(|failure| native_failure(failure, &expected, batch))?
    } else {
        let request = NativeBatch {
            schema: WORKER_SCHEMA.into(),
            snapshot,
            queries,
            remaining_ms: inv.control.remaining()?.as_millis().min(90_000) as u64,
        };
        let (_, bytes) = admit_json(&request, &inv.control, WORKER_BYTES, true)?;
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.arg("--source-native-batch-worker");
        let output = crate::process::run_controlled(
            command,
            RESULT_BYTES,
            inv.control.remaining()?,
            Some(bytes),
            Some(inv.control.token()),
        );
        inv.control.check()?;
        let output = output?;
        if output.status != 0 {
            return Err(ToolkitError::new(
                "source.native_worker_failed",
                String::from_utf8_lossy(&output.stderr).into_owned(),
                5,
            ));
        }
        let reply: NativeReply = serde_json::from_slice(&output.stdout).map_err(|error| {
            ToolkitError::new("source.native_worker_protocol", error.to_string(), 4)
        })?;
        if reply.schema != WORKER_SCHEMA {
            return Err(ToolkitError::new(
                "source.native_worker_protocol",
                "Native batch response schema differs",
                4,
            ));
        }
        reply
            .result
            .map_err(|failure| native_failure(failure, &expected, batch))?
    };
    if values.iter().map(|(item, _)| *item).collect::<Vec<_>>() != expected
        || values
            .iter()
            .any(|(_, value)| value["schema"] != "lycheedev.source-native-research.v1")
    {
        return Err(ToolkitError::new(
            "source.native_worker_protocol",
            "Native batch result identities differ",
            4,
        ));
    }
    Ok(values.into_iter().collect())
}
/// Use the existing native ResearchSession, once. Existing process ownership
/// retires the worker on success, failure, cancellation and the shared deadline.
pub fn worker_stdio() -> Result<()> {
    let mut raw = Vec::new();
    std::io::stdin()
        .take(WORKER_BYTES as u64 + 1)
        .read_to_end(&mut raw)?;
    if raw.len() > WORKER_BYTES {
        return Err(ToolkitError::new(
            "source.native_budget",
            "Native batch input exceeds transport budget",
            3,
        ));
    }
    let request: NativeBatch = serde_json::from_slice(&raw)?;
    drop(raw);
    if request.schema != WORKER_SCHEMA
        || request.queries.is_empty()
        || request.queries.len() > MAX_ITEMS
        || request.remaining_ms == 0
        || request.remaining_ms > 90_000
        || request.queries.iter().enumerate().any(|(i, query)| {
            query.item >= MAX_ITEMS || i > 0 && request.queries[i - 1].item >= query.item
        })
    {
        return Err(ToolkitError::new(
            "source.native_worker_protocol",
            "Native batch admission failed",
            4,
        ));
    }
    let control = lycheedev_core::control::OperationControl::uncancelable(
        Instant::now() + Duration::from_millis(request.remaining_ms),
    );
    let result = run_native(request.snapshot, request.queries, &control);
    let (_, bytes) = admit_json(
        &NativeReply {
            schema: WORKER_SCHEMA.into(),
            result,
        },
        &control,
        RESULT_BYTES,
        true,
    )?;
    std::io::stdout().write_all(&bytes)?;
    Ok(())
}
fn run_native(
    snapshot: native_research::SnapshotRequest,
    queries: Vec<NativeQuery>,
    control: &lycheedev_core::control::OperationControl,
) -> std::result::Result<Vec<(usize, Value)>, NativeFailure> {
    run_native_observed(snapshot, queries, control, |_| {})
}
fn run_native_observed(
    snapshot: native_research::SnapshotRequest,
    queries: Vec<NativeQuery>,
    control: &lycheedev_core::control::OperationControl,
    mut observer: impl FnMut(&'static str),
) -> std::result::Result<Vec<(usize, Value)>, NativeFailure> {
    let mut session =
        native_research::ResearchSession::prepare_observed(snapshot, control, &mut observer)
            .map_err(|error| NativeFailure { item: None, error })?;
    collect_native_results(queries, control, RESULT_BYTES, |query| {
        let mut value = session.query_observed(query.query, control, &mut observer)?;
        value["lifecycle"] =
            json!({"owner":"source_owned_process","retired":true,"mode":"request_scoped_batch"});
        Ok(value)
    })
}
#[cfg(test)]
#[path = "batch_tests.rs"]
mod tests;
