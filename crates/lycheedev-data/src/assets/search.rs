//! Public asset-name modes. Result null/ordering/provenance follow the frozen
//! filename use cases; no semantic claim is inferred from a community filename.
use super::{listfile, source};
use lycheedev_core::{Outcome, Result, ToolkitError};
use crate::request::DataContext;
use super::request::{SearchMode, SearchRequest};
use serde_json::{Value, json};

fn listings(entries: &[listfile::Entry]) -> Result<Value> {
    if entries.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::to_value(entries)?)
    }
}
pub fn run(ctx: &DataContext, request: &SearchRequest) -> Result<Outcome> {
    ctx.control.check()?;
    // Keep optional project resolution before listfile preparation. Explicit
    // snapshots are resolved only after the requested listfile is prepared.
    let implicit = if ctx.source.snapshot.is_none() { Some(ctx.resolve_pinned()?) } else { None };
    let kind = request.listfile.as_str();
    let limit = request.limit;
    let maximum = request.maximum_bytes;
    let reading = source::prepare(
        &ctx.home,
        kind,
        ctx.source.offline,
        false,
        listfile::Limits {
            bytes: maximum,
            ..Default::default()
        },
        &ctx.control,
        &source::PublicFetcher,
    )?;
    let pin = match implicit {
        Some(pin) => pin,
        None => {
            ctx.resolve_pinned()?
        }
    };
    let data = pin.data.as_ref().ok_or_else(|| {
        ToolkitError::new("records.data_pin_required", "records.data_pin_required", 2)
    })?;
    let context = json!({"snapshot":pin.id,"pin":data,"source":"listfile","listfile":serde_json::to_value(&reading.provenance)?});
    let mut truncated = false;
    let result = match &request.mode {
        SearchMode::Query(query) => {
        let (total, files) = reading.index.search(query, limit, false, &ctx.control)?;
        truncated = total > files.len();
        json!({"context":context,"query":query,"total":total,"returned":files.len(),"truncated":truncated,"files":listings(&files)?})
    }, SearchMode::Extension(extension) => {
        let (total, files) = reading.index.search(extension, limit, true, &ctx.control)?;
        truncated = total > files.len();
        json!({"context":context,"extension":extension,"total":total,"returned":files.len(),"truncated":truncated,"files":listings(&files)?})
    }, SearchMode::Name(name) => {
        let matches = reading.index.resolve(name, &ctx.control)?;
        json!({"context":context,"fileName":name,"normalized":listfile::normalize(name),"resolved":!matches.is_empty(),"matches":listings(&matches)?})
    }, SearchMode::FileId(id) => {
        let id = *id;
        let names = reading.index.names(id, &ctx.control)?;
        let mut entry = json!({"fileDataID":id,"fileName":names.first().map_or("unknown", |entry| entry.file_name.as_str())});
        if !names.is_empty() {
            entry["names"] = serde_json::to_value(&names)?;
        }
        json!({"context":context,"entries":[entry]})
    }};
    ctx.control.check()?;
    let mut outcome = Outcome::new(result);
    outcome.selection = json!({"snapshot":pin.id});
    if truncated {
        outcome
            .warnings
            .push(json!("File-name search was truncated by --limit."));
    }
    Ok(outcome)
}
