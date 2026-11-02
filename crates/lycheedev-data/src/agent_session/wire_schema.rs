//! Complete public wire requests. Nested query, business and navigation shapes
//! come from their owning production interfaces, not a second command catalog.
use super::{ExportFormat, MAX_PAGE_ROWS, PROTOCOL, default_timeout};
use serde_json::{Value, json};

fn nested(mut schema: Value) -> Value {
    let object = schema.as_object_mut().unwrap();
    object.remove("$schema");
    object.remove("$defs");
    schema
}

pub(super) fn request_schema(query: &Value, business: &Value, navigate: &Value) -> Value {
    let mut definitions = query["$defs"].as_object().unwrap().clone();
    definitions.insert("QueryRequest".into(), nested(query.clone()));
    definitions.insert("BusinessTask".into(), nested(business.clone()));
    definitions.insert("DomainRequest".into(), navigate.clone());
    let table = json!({"$ref":"#/$defs/TableRequest"});
    let semantics_table = json!({"allOf":[table,{"properties":{"mode":{"const":"static"},"hotfix_captures":{"maxItems":0}}}]});
    let query = json!({"$ref":"#/$defs/QueryRequest"});
    let handle = json!({"type":"string","minLength":1,
        "description":"Exact handle returned by this worker's query, explain or business result. Replace RES_FROM_QUERY_RESPONSE in examples; never invent a handle or reuse one from another worker."});
    let offset = json!({"type":"integer","minimum":0,"maximum":usize::MAX,"default":0,
        "description":"Zero-based row offset within the same retained result. offset + limit must fit platform usize; no source query is rerun."});
    let limit = json!({"type":"integer","minimum":1,"maximum":MAX_PAGE_ROWS});
    let static_table = json!({"name":"Map","mode":"static"});
    let sample_query = json!({"kind":"plan","plan":{"op":"lookup","table":static_table,"ids":[1]}});
    let item = navigate["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|variant| variant["properties"]["kind"]["const"] == "item_get")
        .unwrap();
    let mut variants = Vec::new();
    let mut add = |task: &str,
                   fields: Value,
                   required_fields: &[&str],
                   sample: Value,
                   result: &str,
                   recovery: &str| {
        let mut properties = fields.as_object().unwrap().clone();
        properties.insert("protocol".into(), json!({"const":PROTOCOL}));
        properties.insert("id".into(), json!({"type":"integer","minimum":1,"maximum":u64::MAX,
            "description":"Strictly increases across every submitted line in this process, including page, release, cancel and close. IDs are not replay keys."}));
        properties.insert("timeout_ms".into(), json!({"type":"integer","minimum":1,"maximum":3_600_000,"default":default_timeout(),
            "description":"Includes queue wait, execution and output backpressure. Does not extend the enclosing worker lifetime."}));
        properties.insert("task".into(), json!({"const":task}));
        let mut required = vec!["protocol", "id", "task"];
        required.extend_from_slice(required_fields);
        let mut example = sample.as_object().unwrap().clone();
        example.insert("protocol".into(), json!(PROTOCOL));
        example.insert("id".into(), json!(variants.len() + 1));
        example.insert("task".into(), json!(task));
        variants.push(json!({"type":"object","properties":properties,"required":required,
            "additionalProperties":false,"examples":[example],"x-result":result,"x-recovery":recovery}));
    };
    let retain = "Retain the complete matching-ID response and its result handle. A handle is not a committed CAP. Worker exit, cancellation, deadline or TTL expiry invalidates it; never rerun uncertain work automatically.";
    let page_recovery = "Use the exact handle and offset in this worker. No query replay or new CAP is implied. A missing/expired handle is not permission to repeat the original query or capture.";
    add(
        "navigate",
        json!({"request":{"$ref":"#/$defs/DomainRequest"}}),
        &["request"],
        json!({"request":{"kind":"item_get","item_id":19019,"mode":"static"}}),
        "Domain report, completeness and automatically committed CAP, with original typed request/source evidence.",
        "Publication may have committed before an output failure. Inspect capturePublication/completedCaptures and the evidence catalog; never automatically replay uncertain publication.",
    );
    add(
        "item_get",
        json!({"item_id":item["properties"]["item_id"],"mode":item["properties"]["mode"],"hotfix_captures":item["properties"]["hotfix_captures"]}),
        &["item_id", "mode"],
        json!({"item_id":19019,"mode":"static"}),
        "Alias for navigate request kind=item_get; same domain report and automatic CAP.",
        "Preserve the original input and capture publication facts. static requires no Hotfix captures; effective requires an ordered authenticated history. Do not replay uncertain work.",
    );
    add(
        "semantics",
        json!({"table":semantics_table}),
        &["table"],
        json!({"table":static_table}),
        "Pin-bound original DBD flags, enums and condition metadata; static table with no captures only, not a physical row query.",
        "Keep the same pin/table and original metadata. No row availability or Hotfix coverage is established.",
    );
    add(
        "explain",
        json!({"query":query,"verbose":{"type":"boolean","default":false},"analyze":{"type":"boolean","default":false}}),
        &["query"],
        json!({"query":sample_query}),
        "Owned result handle for native plan output; analyze=true executes once and retains provenance. Metadata/source preparation may precede root metrics.",
        retain,
    );
    add(
        "catalog",
        json!({}),
        &[],
        json!({}),
        "Fixed definition-manifest enumeration, not installed table availability. availability remains not_checked.",
        "Use fields/discover and the first required query to establish actual content availability at the same pin.",
    );
    for task in ["discover", "fields"] {
        add(
            task,
            json!({"table":table}),
            &["table"],
            json!({"table":static_table}),
            "Authenticated DBD schema, physical layout and provenance for the selected table.",
            "Preserve the same pin, table mode and capture history. Metadata is not complete row coverage.",
        );
    }
    add(
        "business",
        json!({"request":{"$ref":"#/$defs/BusinessTask"}}),
        &["request"],
        json!({"request":{"kind":"batch_ids","table":static_table,"ids":[1]}}),
        "Named raw-record components, each with an owned handle and completeness/provenance. Components do not silently become domain navigation reports.",
        retain,
    );
    add(
        "query",
        json!({"query":query}),
        &["query"],
        json!({"query":sample_query}),
        "Owned result handle, row count and completeness/provenance; ordered columns and exact numeric values are preserved.",
        retain,
    );
    add(
        "page",
        json!({"handle":handle,"offset":offset,"limit":limit}),
        &["handle", "limit"],
        json!({"handle":"RES_FROM_QUERY_RESPONSE","limit":10}),
        "Bounded rows and ordered columns from the retained result. A page is not a new query or a committed CAP.",
        page_recovery,
    );
    add(
        "export",
        json!({"handle":handle,"offset":offset,"limit":limit,"format":{"enum":[ExportFormat::Json,ExportFormat::Csv]}}),
        &["handle", "limit", "format"],
        json!({"handle":"RES_FROM_QUERY_RESPONSE","limit":10,"format":"csv"}),
        "Bounded JSON page or CSV text on stdout, with the same page/provenance; no arbitrary filesystem write or implicit CAP.",
        page_recovery,
    );
    add(
        "capture",
        json!({"handle":handle}),
        &["handle"],
        json!({"handle":"RES_FROM_QUERY_RESPONSE"}),
        "Committed CAP for all retained rows, their original complete flag and typed source/request proof, not merely the current page. A CAP does not upgrade partial coverage. Maximum 16777216 bytes; no truncated CAP on limit failure.",
        "Failure leaves the handle usable while its worker and TTL survive. Inspect capturePublication/completedCaptures after uncertain or post-commit failures; absence of a receipt does not prove no commit.",
    );
    add(
        "release",
        json!({"handle":handle}),
        &["handle"],
        json!({"handle":"RES_FROM_QUERY_RESPONSE"}),
        "released names the dropped result handle. Other handles remain owned by this worker.",
        "The released handle cannot page/export/capture again. Releasing a missing, expired or already released handle reports data.session.result_missing, not a query replay.",
    );
    add(
        "cancel",
        json!({"request_id":{"type":"integer","minimum":0,"maximum":u64::MAX,
        "description":"ID of the earlier outstanding request to cancel. Envelope id is a new increasing ID. Zero or an already completed/unknown ID has no outstanding match."}}),
        &["request_id"],
        json!({"request_id":1}),
        "An outstanding match terminates the isolated worker and invalidates every handle; no match returns cancellationRequested=false without replay.",
        "Keep the target request ID, original input and any committed CAP facts. Never replay uncertain work or reuse handles after worker termination.",
    );
    add(
        "close",
        json!({}),
        &[],
        json!({}),
        "Drains submitted work and closes the session. A complete close response and normal process exit are distinct from each result's coverage.",
        "Close has its own deadline including drain time. A close deadline or abnormal exit invalidates all handles; no submitted request is replayed. stdin EOF also drains work.",
    );
    let alias = variants
        .iter_mut()
        .find(|variant| variant["properties"]["task"]["const"] == "item_get")
        .unwrap();
    alias["allOf"] = item["allOf"].clone();
    json!({"$schema":"https://json-schema.org/draft/2020-12/schema", "title":"Data Agent JSONL request",
        "description":"One complete JSON object per line. Submit increasing IDs and wait for matching-ID responses. Examples with RES_FROM_QUERY_RESPONSE require the exact handle returned by a prior query in this worker.",
        "oneOf":variants,"$defs":definitions,
        "x-transport":{"requestBytes":1048576,"queuedRequests":16,"unknownFields":"rejected","framing":"Incomplete response frames are not results; there is no ordinary CLI begin/end wrapper."}})
}
