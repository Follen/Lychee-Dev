//! Fixed-snapshot batch research. Input admission precedes preparation; item
//! evaluators never call public dispatch, and only the batch publishes evidence.
use crate::{
    SourceTools, context, environment, index, native_research, relations,
    request::{
        ContextRequest, RelationDirection, RelationRequest, ResearchMode, ResearchSelection,
        SourceContext, SymbolSelector,
    },
};
use lycheedev_core::{Outcome, Result, ToolkitError, evidence, selection};
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

#[derive(Clone, Default, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BatchRequest {
    pub requests: Vec<BatchItem>,
}
#[derive(Clone, Default, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BatchItem {
    pub id: String,
    #[serde(deserialize_with = "present_operation")]
    pub query: Option<SearchQuery>,
    #[serde(deserialize_with = "present_operation")]
    pub refs: Option<RelationQuery>,
    #[serde(deserialize_with = "present_operation")]
    pub context: Option<ContextQuery>,
}
fn present_operation<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    decoder: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(decoder).map(Some)
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SearchQuery {
    pub mode: String,
    pub text: String,
    pub topic: String,
    pub limit: i64,
    pub cursor: String,
}
impl Default for SearchQuery {
    fn default() -> Self {
        Self {
            mode: "precise".into(),
            text: String::new(),
            topic: String::new(),
            limit: 10,
            cursor: String::new(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct RelationQuery {
    pub symbol_id: String,
    pub symbol: String,
    pub direction: String,
    pub limit: i64,
    pub cursor: String,
}
impl Default for RelationQuery {
    fn default() -> Self {
        Self {
            symbol_id: String::new(),
            symbol: String::new(),
            direction: "both".into(),
            limit: 25,
            cursor: String::new(),
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ContextQuery {
    pub symbol_id: String,
    pub symbol: String,
    pub limit: i64,
    pub max_bytes: i64,
    pub max_lines: i64,
    pub depth: i64,
    pub cursor: String,
}
impl Default for ContextQuery {
    fn default() -> Self {
        Self {
            symbol_id: String::new(),
            symbol: String::new(),
            limit: 25,
            max_bytes: 16384,
            max_lines: 80,
            depth: 0,
            cursor: String::new(),
        }
    }
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
                    if !(1..=200).contains(&query.limit) {
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
                    if !(1..=1 << 20).contains(&query.max_bytes) {
                        return Err(invalid_query("codebase.invalid_context_bytes"));
                    }
                    if !(1..=2000).contains(&query.max_lines) {
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

pub fn parse(raw: &[u8]) -> Result<BatchRequest> {
    if raw.len() > FILE_BYTES {
        return Err(invalid_batch("request file exceeds 64 KiB"));
    }
    let request: BatchRequest =
        serde_json::from_slice(raw).map_err(|error| invalid_batch(error.to_string()))?;
    request.validate()?;
    Ok(request)
}

/// The file contract is owned alongside its typed admission and preparation.
pub fn schema() -> Value {
    let selector = json!([
        {"required":["symbol"],"properties":{"symbol":{"minLength":1},"symbolId":{"maxLength":0}}},
        {"required":["symbolId"],"properties":{"symbolId":{"minLength":1},"symbol":{"maxLength":0}}}
    ]);
    let limit = |default| json!({"type":"integer","minimum":1,"maximum":200,"default":default});
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema", "title":"Source batch file",
        "type":"object","additionalProperties":false,"required":["requests"],
        "description":"UTF-8 JSON, at most 65536 bytes; duplicate and unknown fields are rejected. Every item is validated before source preparation. Results retain request order and one fixed source selection.",
        "properties":{"requests":{"type":"array","minItems":1,"maxItems":MAX_ITEMS,"items":{"$ref":"#/$defs/item"}}},
        "$defs":{
            "item":{"type":"object","additionalProperties":false,"properties":{
                "id":{"type":"string","default":"","description":"Optional caller label, returned with this item."},
                "query":{"$ref":"#/$defs/query"},"refs":{"$ref":"#/$defs/refs"},"context":{"$ref":"#/$defs/context"}},
                "oneOf":[{"required":["query"]},{"required":["refs"]},{"required":["context"]}]},
            "query":{"type":"object","additionalProperties":false,"required":["text"],"properties":{
                "text":{"type":"string","minLength":1,"description":"Trimmed nonempty search text, at most 512 UTF-8 bytes."},
                "mode":{"type":"string","enum":["","precise","exploratory"],"default":"precise"},
                "topic":{"type":"string","enum":["","api","lua","xml","toc","asset"],"default":"","description":"Trimmed and case-normalized topic."},
                "limit":limit(10),"cursor":{"type":"string","default":"","description":"Use the cursor from the same fixed query; an empty value starts its first page."}}},
            "refs":{"type":"object","additionalProperties":false,"oneOf":selector.clone(),"properties":{
                "symbolId":{"type":"string","default":""},"symbol":{"type":"string","default":""},
                "direction":{"type":"string","enum":["","incoming","outgoing","both"],"default":"both"},
                "limit":limit(25),"cursor":{"type":"string","default":""}}},
            "context":{"type":"object","additionalProperties":false,"oneOf":selector,"properties":{
                "symbolId":{"type":"string","default":""},"symbol":{"type":"string","default":""},"limit":limit(25),
                "maxBytes":{"type":"integer","minimum":1,"maximum":1<<20,"default":16384},
                "maxLines":{"type":"integer","minimum":1,"maximum":2000,"default":80},
                "depth":{"type":"integer","minimum":0,"maximum":4,"default":0},
                "cursor":{"type":"string","default":""}}}
        },
        "examples":[{"requests":[{"id":"find","query":{"text":"CreateFrame"}},
            {"id":"relations","refs":{"symbol":"CreateFrame","direction":"incoming"}},
            {"id":"context","context":{"symbol":"CreateFrame","maxBytes":16384,"maxLines":80}}]}]
    })
}

pub fn read(
    path: &Path,
    control: &lycheedev_core::control::OperationControl,
) -> Result<BatchRequest> {
    control.check()?;
    let mut file = std::fs::File::open(path)?;
    let result = (|| {
        if !file.metadata()?.is_file() {
            return Err(invalid_batch("request file is not a regular file"));
        }
        let mut raw = Vec::new();
        (&mut file)
            .take(FILE_BYTES as u64 + 1)
            .read_to_end(&mut raw)?;
        control.check()?;
        parse(&raw)
    })();
    lycheedev_core::storage::finish_file(file, result, control)
}

#[derive(Clone, Debug)]
enum Operation {
    Query(SearchQuery),
    Refs(RelationRequest),
    Context(ContextRequest),
}
fn prepare_calls(request: &BatchRequest, flow: bool) -> Result<Vec<Operation>> {
    request.validate()?;
    Ok(request
        .requests
        .iter()
        .map(|item| {
            if let Some(query) = &item.query {
                let mut query = query.clone();
                query.text = query.text.trim().into();
                query.topic = query.topic.trim().to_lowercase();
                Operation::Query(query)
            } else if let Some(query) = &item.refs {
                Operation::Refs(RelationRequest {
                    selector: if query.symbol_id.is_empty() {
                        SymbolSelector::Name(query.symbol.clone())
                    } else {
                        SymbolSelector::Id(query.symbol_id.clone())
                    },
                    direction: match query.direction.as_str() {
                        "incoming" => RelationDirection::Incoming,
                        "outgoing" => RelationDirection::Outgoing,
                        _ => RelationDirection::Both,
                    },
                    limit: query.limit as usize,
                    cursor: query.cursor.clone(),
                })
            } else {
                let query = item.context.as_ref().expect("admitted batch operation");
                Operation::Context(ContextRequest {
                    selector: if query.symbol_id.is_empty() {
                        SymbolSelector::Name(query.symbol.clone())
                    } else {
                        SymbolSelector::Id(query.symbol_id.clone())
                    },
                    limit: query.limit as usize,
                    max_bytes: query.max_bytes as usize,
                    max_lines: query.max_lines as usize,
                    depth: query.depth as usize,
                    cursor: query.cursor.clone(),
                    flow,
                })
            }
        })
        .collect())
}
/// The entrypoint retains the admitted request across runtime preparation.
/// Evaluators share one fixed selection and never reconstruct command flags.
pub fn dispatch_request(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    request: BatchRequest,
    mode: ResearchMode,
    flow: bool,
    tools: SourceTools<'_>,
) -> Result<Outcome> {
    dispatch_observed(
        ctx,
        selection,
        request,
        mode,
        flow,
        tools,
        None,
        &mut |_, _| {},
    )
}
/// An unavailable implicit runtime produces explicitly partial static evidence.
pub fn dispatch_unavailable(
    ctx: &SourceContext,
    selection: &ResearchSelection,
    request: BatchRequest,
    flow: bool,
    error: ToolkitError,
) -> Result<Outcome> {
    if terminal_control_error(&error) {
        return Err(error);
    }
    dispatch_observed(
        ctx,
        selection,
        request,
        ResearchMode::OneShot,
        flow,
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
    ctx: &SourceContext,
    selection: &ResearchSelection,
    request: BatchRequest,
    mode: ResearchMode,
    flow: bool,
    tools: SourceTools<'_>,
    unavailable: Option<ToolkitError>,
    observer: &mut impl FnMut(&'static str, usize),
) -> Result<Outcome> {
    let calls = prepare_calls(&request, flow)?;
    let mut ctx = ctx.clone();
    ctx.control = ctx
        .control
        .ordered_child(Instant::now() + Duration::from_secs(90))?;
    ctx.control.check()?;
    let pin = &selection.pin;
    let source = selection.source()?;
    for (i, item) in request.requests.iter().enumerate() {
        if let Some(query) = &item.query {
            validate_query_cursor(source, query).map_err(|error| item_error(error, i, item))?;
        }
    }
    let native = request.needs_research() && mode.uses_native() && unavailable.is_none();
    if native && tools.luac.is_none() {
        return Err(ToolkitError::new(
            "source.native_unavailable",
            "source batch research requires the verified native Source runtime or explicit --static-only",
            3,
        ));
    }
    let (index, digest) = crate::research_material::measured("source.batch.index", || {
        index::build_material_with_control(&ctx.home, source, &ctx.control)
    })?;
    let mut queries = Vec::new();
    // Fixed-index selectors for every item are resolved before semantic work.
    for (i, call) in calls.iter().enumerate() {
        let (selector, direction, flow) = match call {
            Operation::Query(_) => continue,
            Operation::Refs(query) => (&query.selector, query.direction, false),
            Operation::Context(query) => (&query.selector, RelationDirection::Both, query.flow),
        };
        let declarations = index
            .symbols
            .iter()
            .filter(|symbol| symbol.kind == "declaration" && selector.matches(symbol))
            .collect::<Vec<_>>();
        if selector.id().is_some() && declarations.is_empty() {
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
        if native && !flow && declarations.len() == 1 {
            queries.push(NativeQuery {
                item: i,
                query: native_research::QueryRequest {
                    symbol: declarations[0].clone(),
                    direction: direction.as_str().into(),
                },
            });
        }
    }
    let environment_source = if request.needs_research() {
        selection.environment_source()?
    } else {
        None
    };
    let (environment, load_facts, snapshot) = if !queries.is_empty() {
        let material = crate::research_material::load_or_build(
            &ctx,
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
            .map(|pin| environment::from_pin(&ctx.home, pin, &ctx.control))
            .transpose()?;
        let facts = if request.requests.iter().any(|item| item.context.is_some()) {
            index::load_facts_with_control(&ctx.home, source, &index, &ctx.control)?
        } else {
            Vec::new()
        };
        (environment, facts, None)
    };
    let shared = relations::PreparedResearch {
        selection: selection.clone(),
        index: Arc::new(index),
        environment: environment.map(Arc::new),
        load_facts: Arc::new(load_facts),
        native_unavailable: unavailable.as_ref().map(ToString::to_string),
    };
    for (i, call) in calls.iter().enumerate() {
        let checked = match call {
            Operation::Refs(query) => {
                relations::validate_prepared_cursor(query, mode, &tools, &shared)
            }
            Operation::Context(query) => {
                context::validate_prepared_cursor(query, mode, &tools, &shared)
            }
            Operation::Query(_) => Ok(()),
        };
        checked.map_err(|error| item_error(error, i, &request.requests[i]))?;
    }
    let mut responses = if let Some(snapshot) = snapshot {
        analyze(&ctx, snapshot, queries, &request, mode)?
    } else {
        BTreeMap::new()
    };
    let mut rows = Vec::new();
    let mut complete = true;
    let mut truncated = false;
    let mut documents = BTreeMap::new();
    for (i, (item, call)) in request.requests.iter().zip(&calls).enumerate() {
        ctx.control.check()?;
        let result = match call {
            Operation::Query(query) => shared.index.search_observed(
                source,
                &pin.id,
                &query.text,
                &query.mode,
                &query.topic,
                query.limit as usize,
                &query.cursor,
                Some(&ctx.control),
                observer,
            ),
            Operation::Refs(query) => {
                relations::with_prepared(&ctx, query, mode, &tools, &shared, responses.remove(&i))
                    .map(|reading| reading.data)
            }
            Operation::Context(query) => context::evaluate(
                &ctx,
                selection,
                query,
                mode,
                &tools,
                Some(&shared),
                responses.remove(&i),
                &mut documents,
            )
            .map(|reading| reading.data),
        }
        .map_err(|error| item_error(error, i, item))?;
        ctx.control.check()?;
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
    ctx.control.check()?;
    let (_, raw) = admit_json(&data, &ctx.control, evidence::DEFAULT_CAPTURE_BYTES, true).map_err(
        |mut error| {
            if error.code == "source.native_budget" {
                error.code = "vault.blob_limit".into();
                error.exit_code = 4;
            }
            error
        },
    )?;
    ctx.control.check()?;
    let capture = evidence::commit_capture_bounded(
        &ctx.home,
        evidence::CaptureInput {
            bytes: &raw,
            maximum_bytes: evidence::DEFAULT_CAPTURE_BYTES,
            media_type: "application/json",
            provenance: json!({"kind":"source-batch","locator":"query/refs/context","snapshot":pin.id,"sourceCommit":source.exact_commit}),
            complete,
            truncated,
        },
        &ctx.control,
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
    ctx: &SourceContext,
    snapshot: native_research::SnapshotRequest,
    queries: Vec<NativeQuery>,
    batch: &BatchRequest,
    mode: ResearchMode,
) -> Result<BTreeMap<usize, Value>> {
    let expected = queries.iter().map(|query| query.item).collect::<Vec<_>>();
    let values = if mode.reuses_session() {
        collect_native_results(queries, &ctx.control, RESULT_BYTES, |query| {
            crate::native_session::analyze(
                ctx,
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
            remaining_ms: ctx.control.remaining()?.as_millis().min(90_000) as u64,
        };
        let (_, bytes) = admit_json(&request, &ctx.control, WORKER_BYTES, true)?;
        let mut command = std::process::Command::new(std::env::current_exe()?);
        command.arg("--source-native-batch-worker");
        let output = crate::process::run_with_operation_control(
            command,
            RESULT_BYTES,
            ctx.control.remaining()?,
            Some(bytes),
            &ctx.control,
        );
        ctx.control.check()?;
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
