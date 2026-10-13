//! DB2 typed JSONL provider. Rows are decoded/yielded individually, with one
//! immutable source identity and caller control; no row array is materialized.
use crate::{
    capture, db2, fault,
    resource::{Budget, Cost},
    table,
};
use lycheedev_core::{Invocation, Outcome, Result, evidence};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, Instant},
};

fn field_error(name: &str) -> lycheedev_core::ToolkitError {
    let mut error = fault(
        "records.field_unknown",
        2,
        format!("records.field_unknown: {name}"),
    );
    error.details = json!({"stage":"query"});
    error
}
fn parse_filter(text: &str) -> Result<Option<(&str, &str)>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let Some((field, value)) = text.split_once('=') else {
        let mut error = fault(
            "records.field_type_mismatch",
            2,
            format!("records.field_type_mismatch: invalid filter {text:?}, expected field=value"),
        );
        error.details = json!({"stage":"query"});
        return Err(error);
    };
    let field = field.trim();
    if field.is_empty() {
        let mut error = fault(
            "records.field_type_mismatch",
            2,
            format!("records.field_type_mismatch: invalid filter {text:?}, expected field=value"),
        );
        error.details = json!({"stage":"query"});
        return Err(error);
    }
    Ok(Some((field, value.trim())))
}
fn matches(value: &table::Value, want: &str) -> bool {
    if matches!(value, table::Value::Null) {
        return false;
    }
    if db2::search_text(value) == want {
        return true;
    }
    match value {
        table::Value::Int(n) => want.parse::<i64>().is_ok_and(|v| v == *n),
        table::Value::UInt(n) => want
            .strip_prefix('+')
            .unwrap_or(want)
            .parse::<u64>()
            .is_ok_and(|v| v == *n),
        _ => false,
    }
}
pub fn stream(inv: &Invocation, sink: &mut dyn FnMut(&Value) -> Result<()>) -> Result<Outcome> {
    inv.control.check()?;
    if inv.flag("--format") != Some("jsonl") {
        return Err(fault(
            "command.invalid_arguments",
            2,
            "data db2 stream requires --format jsonl",
        ));
    }
    let name = inv.argument()?;
    inv.required("--limit")?;
    let limit = inv.usize_flag("--limit", 0, 1, 200000)?;
    let filter_text = inv.flag("--filter").unwrap_or("");
    let filter = parse_filter(filter_text)?;
    let timeout = inv.usize_flag("--timeout-seconds", 300, 1, 3600)?;
    let budget = Arc::new(Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(timeout as u64),
        inv.control.clone(),
    ));
    let db2::Prepared {
        pin,
        bundle,
        file,
        layout,
        definition,
    } = db2::prepare(inv, name, &budget)?;
    let view = table::open_budgeted(
        &file.bytes,
        &definition,
        Default::default(),
        &Default::default(),
        &[],
        budget.clone(),
    )?;
    let fields: Vec<String> = match inv.flag("--fields").filter(|s| !s.is_empty()) {
        Some(text) => text.split(',').map(|v| v.trim().to_owned()).collect(),
        None => definition.fields.iter().map(|f| f.name.clone()).collect(),
    };
    if fields.len() > 1024 {
        return Err(fault(
            "records.query_resource_budget",
            3,
            "Stream projection field budget exceeded",
        ));
    }
    for name in fields
        .iter()
        .map(String::as_str)
        .chain(filter.iter().map(|(name, _)| *name))
    {
        if !definition.fields.iter().any(|f| f.name == name) {
            return Err(field_error(name));
        }
    }
    let proof = json!({"contentVerified":true,"keySource":file.reading["keySource"],"name":name,"identity":bundle.identity,"layoutHash":format!("{:08X}",layout.layout_hash),"definitionSHA256":definition.sha256,"fileDataID":bundle.identity.db2_file_data_id,"contentKey":file.reading["entry"]["contentKey"],"logicalRows":view.logical_count()});
    let begin = json!({"tables":[proof.clone()],"snapshot":pin.id,"source":"installation","table":bundle.identity.name,"identity":bundle.identity,"layoutHash":format!("{:08X}",layout.layout_hash),"fields":fields,"filter":filter_text,"limit":limit});
    budget.check()?;
    sink(&json!({"frame":"begin","begin":begin}))?;
    let mut count = 0;
    let mut extra = false;
    let walk = (|| -> Result<()> {
        for id in view.ids() {
            budget.check()?;
            let mut row = view.row(id, 1 << 20)?;
            if let Some((field, value)) = filter
                && !matches(row.get(field).ok_or_else(|| field_error(field))?, value)
            {
                continue;
            }
            if count == limit {
                extra = true;
                break;
            }
            budget.charge(Cost::metadata(fields.len().checked_mul(128).ok_or_else(
                || {
                    fault(
                        "records.query_resource_budget",
                        3,
                        "Stream projection size overflow",
                    )
                },
            )?))?;
            let mut projected = BTreeMap::new();
            for field in &fields {
                if let Some(value) = row.remove(field) {
                    projected.insert(field.clone(), value);
                }
            }
            let frame =
                json!({"frame":"record","record":{"index":count,"recordID":id,"row":projected}});
            budget.check()?;
            sink(&frame)?;
            count += 1;
        }
        budget.check()?;
        Ok(())
    })();
    let publication = walk.and_then(|()| {
        let end = json!({"partial":false,"partialReasons":null,"count":count,"complete":!extra,"truncated":extra,"truncationReason":if extra{"limit_reached"}else{"none"}});
        budget.check()?;
        let manifest = json!({"begin":begin,"end":end});
        let raw = capture::json_bytes(&manifest, &budget, evidence::DEFAULT_CAPTURE_BYTES,
            || fault("vault.blob_limit", 4, "blob exceeds budget"))?;
        let capture = evidence::commit_capture_budgeted(
            &inv.home,
            evidence::CaptureInput {
                bytes: &raw, maximum_bytes: evidence::DEFAULT_CAPTURE_BYTES, media_type: "application/json",
                provenance: json!({"kind":"data-stream-manifest","locator":format!("db2:{name}:stream"),"snapshot":pin.id,"dataBuild":pin.data.as_ref().unwrap().full_build}),
                complete: !extra, truncated: extra,
            },
            &inv.control,
            budget.as_ref(),
        )?;
        budget.check()?;
        Ok((end, capture))
    });
    let (end, capture) = match publication {
        Ok(value) => value,
        Err(error) => {
            let code = if error.exit_code == 7
                || error.code == "command.deadline"
                || error.code == "operation.deadline"
            {
                "records.cancelled"
            } else {
                &error.code
            };
            // Best effort error marker; a failed writer cannot accept even it. No
            // end frame or generic envelope may follow the failed owned stream.
            let _ = sink(&json!({"frame":"error","error":{"code":code,"message":error.message}}));
            return Err(error);
        }
    };
    let data = json!({"snapshot":pin.id,"pin":pin.data,"source":"installation","tables":[proof],"relations":[],"complete":!extra,"truncated":extra,"truncationReason":if extra{"limit_reached"}else{"none"},"partial":false,"partialReasons":null,"table":bundle.identity.name,"mode":"stream","limit":limit,"fields":fields,"filter":filter_text,"count":count,"resourceAccounting":{"wholeQueryAccountingComplete":false,"used":budget.snapshot()?}});
    let mut outcome = Outcome::new(data);
    outcome.complete = !extra;
    outcome.provenance = vec![capture];
    let terminal = json!({"frame":"end","end":end});
    // All result construction precedes the success terminal frame.
    sink(&terminal)?;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_filter_retains_exact_signed_unsigned_and_array_rendering() {
        assert!(matches(
            &table::Value::UInt(u64::MAX),
            "+18446744073709551615"
        ));
        assert!(!matches(
            &table::Value::UInt(u64::MAX),
            "18446744073709551616"
        ));
        assert!(matches(&table::Value::Int(-2), "-02"));
        assert!(matches(
            &table::Value::Array(vec![table::Value::Int(1), table::Value::Int(2)]),
            "[1 2]"
        ));
        assert!(!matches(&table::Value::Null, ""));
        assert_eq!(parse_filter(" ID = 01 ").unwrap(), Some(("ID", "01")));
        assert!(parse_filter("=1").is_err());
        assert!(parse_filter("x").is_err());
    }
}
