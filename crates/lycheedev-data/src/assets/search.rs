//! Public asset-name modes. Result null/ordering/provenance follow the frozen
//! filename use cases; no semantic claim is inferred from a community filename.
use super::{listfile, source};
use lycheedev_core::{Invocation, Outcome, Result, ToolkitError, selection};
use serde_json::{Value, json};

fn listings(entries: &[listfile::Entry]) -> Result<Value> {
    if entries.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::to_value(entries)?)
    }
}
pub fn run(inv: &Invocation) -> Result<Outcome> {
    inv.control.check()?;
    // Go resolves an optional project before dispatch validation. Explicit pin
    // reads are deferred until the exact requested listfile has been prepared.
    let implicit = if inv.flag("--snapshot").is_none() {
        Some(selection::resolve_pinned(inv)?)
    } else {
        None
    };
    let kind = inv.flag("--listfile").unwrap_or("");
    let query = inv.flag("--query").filter(|value| !value.is_empty());
    let extension = inv.flag("--extension").filter(|value| !value.is_empty());
    let name = inv.flag("--name").filter(|value| !value.is_empty());
    let id = inv
        .flag("--file-id")
        .map(str::parse::<u32>)
        .transpose()
        .map_err(|_| ToolkitError::arguments("invalid --file-id"))?
        .filter(|id| *id != 0);
    if kind.is_empty()
        || [
            query.is_some(),
            extension.is_some(),
            name.is_some(),
            id.is_some(),
        ]
        .into_iter()
        .filter(|selected| *selected)
        .count()
            != 1
    {
        return Err(ToolkitError::arguments(
            "asset search requires --snapshot, --listfile and exactly one of --query, --extension, --name, --file-id",
        ));
    }
    let limit = inv.usize_flag("--limit", 50, 1, 200)?;
    let maximum = inv.usize_flag("--max-bytes", 256 << 20, 1, 512 << 20)?;
    let reading = source::prepare(
        &inv.home,
        kind,
        inv.flag_bool("--offline"),
        false,
        listfile::Limits {
            bytes: maximum,
            ..Default::default()
        },
        &inv.control,
        &source::PublicFetcher,
    )?;
    let pin = match implicit {
        Some(pin) => pin,
        None => {
            selection::load_pinned_controlled(&inv.home, inv.required("--snapshot")?, &inv.control)?
        }
    };
    let data = pin.data.as_ref().ok_or_else(|| {
        ToolkitError::new("records.data_pin_required", "records.data_pin_required", 2)
    })?;
    let context = json!({"snapshot":pin.id,"pin":data,"source":"listfile","listfile":serde_json::to_value(&reading.provenance)?});
    let mut truncated = false;
    let result = if let Some(query) = query {
        let (total, files) = reading.index.search(query, limit, false, &inv.control)?;
        truncated = total > files.len();
        json!({"context":context,"query":query,"total":total,"returned":files.len(),"truncated":truncated,"files":listings(&files)?})
    } else if let Some(extension) = extension {
        let (total, files) = reading.index.search(extension, limit, true, &inv.control)?;
        truncated = total > files.len();
        json!({"context":context,"extension":extension,"total":total,"returned":files.len(),"truncated":truncated,"files":listings(&files)?})
    } else if let Some(name) = name {
        let matches = reading.index.resolve(name, &inv.control)?;
        json!({"context":context,"fileName":name,"normalized":listfile::normalize(name),"resolved":!matches.is_empty(),"matches":listings(&matches)?})
    } else {
        let id = id.expect("validated mode");
        let names = reading.index.names(id, &inv.control)?;
        let mut entry = json!({"fileDataID":id,"fileName":names.first().map_or("unknown", |entry| entry.file_name.as_str())});
        if !names.is_empty() {
            entry["names"] = serde_json::to_value(&names)?;
        }
        json!({"context":context,"entries":[entry]})
    };
    inv.control.check()?;
    let mut outcome = Outcome::new(result);
    outcome.selection = json!({"snapshot":pin.id});
    if truncated {
        outcome
            .warnings
            .push(json!("File-name search was truncated by --limit."));
    }
    Ok(outcome)
}
