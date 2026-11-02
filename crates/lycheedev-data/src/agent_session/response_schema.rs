//! Serialized result shapes owned by Session/Engine, plus the existing CLI
//! JSONL transport envelopes. This describes output; it does not decode it or
//! introduce a second task inventory.
use super::PROTOCOL;
use serde_json::{Value, json};

const DRAFT: &str = "https://json-schema.org/draft/2020-12/schema";

fn object(properties: Value, required: &[&str]) -> Value {
    json!({"type":"object","properties":properties,"required":required,
        "additionalProperties":true})
}
fn array(items: Value) -> Value {
    json!({"type":"array","items":items})
}
fn text() -> Value {
    json!({"type":"string"})
}
fn count() -> Value {
    json!({"type":"integer","minimum":0})
}
fn dynamic(description: &str) -> Value {
    json!({"description":description})
}
fn provenance() -> Value {
    dynamic(
        "Original typed request and authenticated source/pin evidence. Its fields depend on the owning engine operation; retain the complete value, including extensions.",
    )
}
fn rows() -> Value {
    array(array(dynamic(
        "JSON scalar, nested array or structured value from the selected column. Preserve exact JSON number lexemes, including integers outside JavaScript's safe range; do not round through binary64.",
    )))
}
fn receipt() -> Value {
    let mut result = object(
        json!({"handle":text(),"rows":count(),"retainedBytes":count(),
        "expiresAfterMs":{"type":"integer","minimum":1}}),
        &["handle", "rows", "retainedBytes", "expiresAfterMs"],
    );
    result["description"] = json!(
        "Session-local result receipt, at result.result for query/explain frames, or result.components[i].result for business. The handle is not a CAP; use it only in this same living worker before expiry/release. rows counts all retained rows, not one page."
    );
    result
}
fn query() -> Value {
    object(
        json!({"result":receipt(),"complete":{"type":"boolean"},"provenance":provenance()}),
        &["result", "complete", "provenance"],
    )
}
fn page() -> Value {
    let mut result = object(
        json!({"columns":array(text()),"rows":rows(),"offset":count(),
        "totalRows":count(),"complete":{"type":"boolean"}}),
        &["columns", "rows", "offset", "totalRows", "complete"],
    );
    result["description"] = json!(
        "columns gives ordered names; each rows entry has values in that order. offset is the requested zero-based offset and totalRows counts all retained result rows. complete is source/result coverage, not whether this page exhausted totalRows. An empty or bounded page never upgrades a partial result. Paging does not rerun a query."
    );
    result
}
fn capture() -> Value {
    let mut result = object(
        json!({"schema":{"const":"lycheedev.capture.v1"},"id":text(),
        "blob":object(json!({"sha256":text(),"bytes":count()}), &["sha256","bytes"]),
        "mediaType":text(),"provenance":object(json!({"kind":text(),"locator":text(),
            "snapshot":text(),"operationId":text(),"sourceCommit":text(),"baseSnapshot":text(),
            "baseSourceCommit":text(),"dataBuild":text(),"session":text()}),
            &["kind","locator","snapshot","operationId"]),
        "complete":{"type":"boolean"},"truncated":{"type":"boolean"},"capturedAt":text(),
        "dependencies":array(text())}),
        &[
            "schema",
            "id",
            "blob",
            "mediaType",
            "provenance",
            "complete",
            "truncated",
            "capturedAt",
        ],
    );
    result["description"] = json!(
        "Committed immutable CAP receipt for the entire retained result, not just a page. complete retains original coverage; partial results remain partial. The receipt references original bytes through blob.sha256/bytes. dependencies is omitted when empty. A result handle alone does not prove publication. After uncertain or post-commit failure inspect error.details.capturePublication/completedCaptures and the evidence catalog before any intentional new action."
    );
    result
}
fn table() -> Value {
    object(
        json!({"name":text(),"mode":{"enum":["static","effective"]},"hotfix_captures":array(text())}),
        &["name", "mode"],
    )
}
fn result(task: &str) -> Value {
    match task {
        "query" | "explain" => query(),
        "page" => page(),
        "export" => json!({"oneOf":[
            object(json!({"format":{"const":"json"},"page":page()}), &["format","page"]),
            object(json!({"format":{"const":"csv"},"text":text(),"page":page()}), &["format","text","page"])
        ],"description":"Export returns a bounded page. CSV text includes the quoted header and CRLF rows; page retains ordered typed values and coverage. No arbitrary file write or implicit CAP occurs."}),
        "capture" => capture(),
        "business" => {
            let mut component = query();
            component["properties"]["name"] = text();
            component["required"]
                .as_array_mut()
                .unwrap()
                .push(json!("name"));
            object(json!({"components":array(component)}), &["components"])
        }
        "navigate" | "item_get" => object(
            json!({
            "report":dynamic("Task-specific domain report. Read the selected navigation task's report fields and explicit complete/partial facts; unknown extensions are retained. This schema does not claim one fixed report type for all domain tasks."),
            "sources":array(provenance()),"capture":capture()}),
            &["report", "sources", "capture"],
        ),
        "fields" | "discover" => object(
            json!({"table":table(),
            "columns":array(object(json!({"name":text(),"dataType":text(),"nullable":{"type":"boolean"}}),
                &["name","dataType","nullable"])),"rowCount":count(),"provenance":provenance()}),
            &["table", "columns", "rowCount", "provenance"],
        ),
        "catalog" => object(
            json!({"snapshotId":text(),"definitionCommit":text(),
            "scope":{"const":"exact_definition_manifest_not_installed_tables"},
            "complete":{"type":"boolean","description":"Definition-manifest enumeration completeness, not installed availability or readable row coverage."},
            "provenance":provenance(),"tables":array(object(json!({"name":text(),"tableHash":count(),
                "db2FileDataId":count(),"dbcFileDataId":count(),"availability":{"const":"not_checked"}}),
                &["name","tableHash","db2FileDataId","dbcFileDataId","availability"]))}),
            &[
                "snapshotId",
                "definitionCommit",
                "scope",
                "complete",
                "provenance",
                "tables",
            ],
        ),
        "semantics" => object(
            json!({"table":table(),"columns":array(text()),"rows":rows(),
            "provenance":dynamic("Pinned DBD semantic metadata or null; not row availability/Hotfix coverage."),
            "complete":{"type":"boolean"}}),
            &["table", "columns", "rows", "provenance", "complete"],
        ),
        "release" => object(json!({"released":text()}), &["released"]),
        "close" => object(
            json!({"closed":{"const":true},"handlesReleased":{"const":true}}),
            &["closed", "handlesReleased"],
        ),
        "cancel" => {
            let mut result = object(
                json!({"requestId":{"type":"integer","minimum":0,"maximum":u64::MAX},
                "cancellationRequested":{"const":false},"replayed":{"const":false}}),
                &["requestId", "cancellationRequested", "replayed"],
            );
            result["description"] = json!(
                "Public supervised CLI success applies only when no outstanding request matches. A matching outstanding cancel terminates the isolated worker and invalidates every handle; it produces a terminal stderr diagnostic with command.cancelled and details.requestId/cancelRequestId/sessionInvalidated/replayed, not a successful result. No work is replayed."
            );
            result
        }
        _ => unreachable!("request task has no result description: {task}"),
    }
}
pub(super) fn results(request_schema: &Value) -> Value {
    let entries = request_schema["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| {
            let task = variant["properties"]["task"]["const"].as_str().unwrap();
            let mut schema = result(task);
            schema["$schema"] = json!(DRAFT);
            schema["title"] = json!(format!("Data Agent {task} result"));
            (task.to_owned(), schema)
        })
        .collect();
    Value::Object(entries)
}
pub(super) fn frames() -> Value {
    let error = object(
        json!({"code":text(),"message":text(),"exit_code":{"type":"integer"},
        "details":dynamic("Original structured causes and operation-specific facts, including cleanup, partial/committed captures or session invalidation. Unknown fields are extensions; preserve them.")}),
        &["code", "message", "exit_code", "details"],
    );
    let id = json!({"type":["integer","null"],"minimum":0,"maximum":u64::MAX,
        "description":"Matching request envelope ID, or null when ingress cannot attribute an invalid frame."});
    let mut success = object(
        json!({"protocol":{"const":PROTOCOL},"id":id,"ok":{"const":true},
        "result":dynamic("Task-selected payload; use resultSchemas[request.task]. Query/explain handles are at frame.result.result.handle, page rows at frame.result.rows, CSV at frame.result.text, CAP receipt at frame.result.id.")}),
        &["protocol", "id", "ok", "result"],
    );
    success["not"] = json!({"required":["error"]});
    success["x-stream"] = json!("stdout");
    let mut failed = object(
        json!({"protocol":{"const":PROTOCOL},"id":id,"ok":{"const":false},
        "error":{"$ref":"#/$defs/ToolkitError"}}),
        &["protocol", "id", "ok", "error"],
    );
    failed["not"] = json!({"required":["result"]});
    failed["x-stream"] = json!("stdout");
    let mut terminal = object(
        json!({"protocol":{"enum":["lycheedev.data-agent-supervisor.v1","lycheedev.data-agent-worker.v1"]},
        "ok":{"const":false},"error":{"$ref":"#/$defs/ToolkitError"}}),
        &["protocol", "ok", "error"],
    );
    terminal["not"] = json!({"anyOf":[{"required":["id"]},{"required":["result"]}]});
    terminal["x-stream"] = json!("stderr");
    json!({"$schema":DRAFT,"title":"Data Agent response and terminal diagnostic frames",
        "description":"Complete JSON object plus newline. Ordinary Agent stdout has no CLI begin/end wrapper. Match IDs and inspect ok before result. Retain stderr terminal diagnostics and process status independently; a previously received CAP or partial result is not erased by later failure. Dynamic values and unknown extensions are intentionally open.",
        "oneOf":[{"$ref":"#/$defs/SuccessFrame"},{"$ref":"#/$defs/ErrorFrame"},{"$ref":"#/$defs/TerminalDiagnostic"}],
        "$defs":{"SuccessFrame":success,"ErrorFrame":failed,"TerminalDiagnostic":terminal,"ToolkitError":error}})
}
