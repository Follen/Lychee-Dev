//! The fixed Hotfix CSV manifest and typed JSONL stream, sharing the SQL pair lease.
use super::*;
use std::path::Path;
const COLUMNS: [&str; 9] = [
    "id",
    "push_id",
    "record_id",
    "table_name",
    "status",
    "build",
    "region_id",
    "locale",
    "payload_length",
];
fn row(raw: &mut Vec<u8>, cells: &[String], budget: &Budget) -> Result<()> {
    for (n, text) in cells.iter().enumerate() {
        budget.check()?;
        if n != 0 {
            raw.push(b',');
        }
        let quote = text == "\\."
            || text.contains([',', '"', '\r', '\n'])
            || text.chars().next().is_some_and(char::is_whitespace);
        if quote {
            raw.push(b'"');
        }
        for chunk in text.as_bytes().chunks(4096) {
            budget.charge(Cost::work(chunk.len()))?;
            for b in chunk {
                match *b {
                    b'"' if quote => raw.extend_from_slice(b"\"\""),
                    b'\n' => raw.extend_from_slice(b"\r\n"),
                    b'\r' => {}
                    b => raw.push(b),
                }
            }
        }
        if quote {
            raw.push(b'"');
        }
    }
    raw.extend_from_slice(b"\r\n");
    Ok(())
}
pub(super) fn encode(result: &RemoteResult, budget: &Budget) -> Result<(Vec<u8>, Value)> {
    if result.complete && result.truncated {
        return Err(fault(
            "records.export_encoding",
            4,
            "records.export_encoding: a result cannot be complete and truncated",
        ));
    }
    // Admit the worst-case quoted bytes before cloning any record strings.
    let maximum = result
        .records
        .iter()
        .try_fold(1024usize, |n, r| {
            n.checked_add(
                r.table_name
                    .len()
                    .checked_add(r.build.len())?
                    .checked_add(r.locale.len())?
                    .checked_mul(2)?
                    .checked_add(256)?,
            )
        })
        .ok_or_else(limit)?;
    let allocation = budget.reserve_retained(maximum)?;
    let mut raw = Vec::new();
    raw.try_reserve_exact(maximum).map_err(|_| limit())?;
    row(
        &mut raw,
        &COLUMNS.iter().map(|v| (*v).into()).collect::<Vec<String>>(),
        budget,
    )?;
    for r in &result.records {
        budget.check()?;
        let _cells =
            budget.reserve_retained(r.table_name.len() + r.build.len() + r.locale.len() + 512)?;
        let cells = [
            if r.id == 0 {
                String::new()
            } else {
                r.id.to_string()
            },
            r.push.to_string(),
            r.record_id.to_string(),
            r.table_name.clone(),
            r.status.to_string(),
            r.build.clone(),
            r.region_id.map_or_else(String::new, |v| v.to_string()),
            r.locale.clone(),
            r.payload_length.to_string(),
        ];
        row(&mut raw, &cells, budget)?;
    }
    allocation.commit();
    let mut manifest = json!({"schema":"lycheedev.csv-export.v1","columns":COLUMNS,"rowCount":result.records.len(),"source":{"provider":result.change.provider,"product":result.query.product,"fullBuild":result.query.full_build,"region":result.query.region,"locale":result.query.locale,"ref":result.change.sha256,"fetchedAt":result.change.fetched_at},"content":{"sha256":storage::sha256(&raw),"bytes":raw.len()},"complete":result.complete,"truncated":result.truncated});
    let reason = if result.coverage.truncation_reason.is_empty() && result.truncated {
        "limit"
    } else {
        &result.coverage.truncation_reason
    };
    if !reason.is_empty() {
        manifest["truncationReason"] = json!(reason);
    }
    Ok((raw, manifest))
}
#[derive(Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema: String,
    columns: Vec<String>,
    row_count: usize,
    source: CsvSource,
    content: evidence::BlobRef,
    complete: bool,
    truncated: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    truncation_reason: String,
}
#[derive(Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CsvSource {
    provider: String,
    product: String,
    full_build: String,
    region: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    locale: String,
    #[serde(rename = "ref")]
    reference: String,
    fetched_at: String,
}
pub(super) fn csv(inv: &Invocation, result: &RemoteResult, budget: &Budget) -> Result<Value> {
    let (raw, manifest) = encode(result, budget)?;
    let wire: Manifest = serde_json::from_value(manifest.clone())?;
    let mut sidecar = serde_json::to_string_pretty(&wire)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes();
    sidecar.push(b'\n');
    let path = inv.required("--output")?;
    // Go Hotfix intentionally overwrites; --overwrite belongs only to SQL.
    let (data, destination) = crate::assets::output::destination_pair(
        &inv.home,
        Path::new(path),
        Path::new(&format!("{path}.manifest.json")),
        true,
    )?;
    crate::local_query::query_csv::publish_pair_after_data(
        inv,
        &data,
        &destination,
        true,
        &raw,
        &sidecar,
        budget,
        || Ok(()),
    )?;
    Ok(manifest)
}
#[derive(Serialize)]
struct Context<'a> {
    query: &'a Query,
    coverage: &'a Coverage,
    change: &'a selection::ChangePin,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Frame<'a> {
    schema: &'static str,
    #[serde(rename = "type")]
    kind: &'static str,
    #[serde(skip_serializing_if = "str::is_empty")]
    operation_id: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    context: Option<Context<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    record: Option<&'a Record>,
    #[serde(skip_serializing_if = "Option::is_none")]
    complete: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    truncated: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
    #[serde(skip_serializing_if = "<[String]>::is_empty")]
    warnings: &'a [String],
}
pub(super) fn jsonl(
    result: &RemoteResult,
    control: &OperationControl,
    budget: &Budget,
    emit: &mut impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let frame = |kind| Frame {
        schema: "lycheedev.jsonl.v1",
        kind,
        operation_id: &result.snapshot,
        context: None,
        index: None,
        record: None,
        complete: None,
        truncated: None,
        count: None,
        warnings: &[],
    };
    let mut send = |value: Frame<'_>| -> Result<()> {
        control.check()?;
        let mut raw = go_bytes(&value, budget, usize::MAX)?;
        raw.push(b'\n');
        emit(&raw)
    };
    let mut begin = frame("begin");
    begin.context = Some(Context {
        query: &result.query,
        coverage: &result.coverage,
        change: &result.change,
    });
    send(begin)?;
    for (index, record) in result.records.iter().enumerate() {
        let mut row = frame("record");
        row.index = Some(index);
        row.record = Some(record);
        send(row)?;
    }
    let mut end = frame("end");
    end.complete = Some(result.complete);
    end.truncated = Some(result.truncated);
    end.count = Some(result.records.len());
    end.warnings = &result.warnings;
    send(end)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Envelope<'a> {
    schema: &'static str,
    ok: bool,
    operation_id: &'static str,
    context: Value,
    result: &'a RemoteResult,
    captures: &'a [Value],
    warnings: &'a [Value],
    error: Option<()>,
}
pub(super) fn render(
    inv: &Invocation,
    result: &RemoteResult,
    timeout: Option<u64>,
    out: &mut impl std::io::Write,
    budget: &Budget,
) -> Result<()> {
    if inv.flag("--format") == Some("jsonl") {
        return jsonl(result, &inv.control, budget, &mut |raw| {
            out.write_all(raw)?;
            out.flush()?;
            Ok(())
        });
    }
    inv.control.check()?;
    if inv.flag("--format") == Some("json") {
        let outcome = outcome(result, Value::Null);
        let mut context = outcome.selection;
        if let Some(seconds) = timeout {
            context["timeoutSeconds"] = json!(seconds);
        }
        let envelope = Envelope {
            schema: "lycheedev.result.v1",
            ok: true,
            operation_id: "",
            context,
            result,
            captures: &outcome.provenance,
            warnings: &outcome.warnings,
            error: None,
        };
        lycheedev_core::public_json::write_serializable_line(out, &envelope)?;
    } else {
        lycheedev_core::public_json::write_pretty_serializable_line(out, result)?;
    }
    out.flush()?;
    Ok(())
}
