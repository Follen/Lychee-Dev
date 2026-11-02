//! The fixed Hotfix CSV manifest, sharing the existing pair lease.
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
fn row(
    raw: &mut Vec<u8>,
    cells: &[String],
    budget: &Budget,
    check: &mut impl FnMut() -> Result<()>,
) -> Result<()> {
    for (n, text) in cells.iter().enumerate() {
        check()?;
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
            check()?;
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
fn encode_with_check(
    result: &RemoteResult,
    budget: &Budget,
    mut check: impl FnMut() -> Result<()>,
) -> Result<(Vec<u8>, Value)> {
    check()?;
    budget.check()?;
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
        &mut check,
    )?;
    for r in &result.records {
        check()?;
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
        row(&mut raw, &cells, budget, &mut check)?;
    }
    check()?;
    budget.check()?;
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
pub(super) fn csv(ctx: &DataContext, path: &Path, result: &RemoteResult, budget: &Budget) -> Result<Value> {
    let (raw, manifest) = encode_with_check(result, budget, || ctx.control.check())?;
    let wire: Manifest = serde_json::from_value(manifest.clone())?;
    let mut sidecar = serde_json::to_string_pretty(&wire)?
        .replace('&', "\\u0026")
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
        .into_bytes();
    sidecar.push(b'\n');
    let sidecar_path = std::path::PathBuf::from(format!("{}.manifest.json", path.display()));
    // Go Hotfix intentionally overwrites; --overwrite belongs only to SQL.
    let (data, destination) = crate::assets::output::destination_pair_scoped(
        ctx,
        path,
        &sidecar_path,
        true,
        budget,
    )?;
    let result = crate::assets::pair_output::publish_pair_after_data(
        ctx,
        &data,
        &destination,
        true,
        &raw,
        &sidecar,
        budget,
        || Ok(()),
    );
    let published = result.is_ok();
    crate::assets::output::finish_pair(ctx, data, destination, budget, result, published)?;
    Ok(manifest)
}
