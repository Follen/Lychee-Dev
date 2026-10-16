//! The row loop has no Invocation, flags, provider, capture or JSON dependency.
use super::{Filter, Limit, Projection, StreamEvent, StreamRecord, WalkSummary, field_error};
use crate::{
    db2, fault,
    resource::{Budget, Cost},
    table,
};
use lycheedev_core::Result;
use std::collections::BTreeMap;

pub(super) fn matches(value: &table::Value, want: &str) -> bool {
    if matches!(value, table::Value::Null) {
        return false;
    }
    if db2::search_text(value) == want {
        return true;
    }
    match value {
        table::Value::Int(n) => want.parse::<i64>().is_ok_and(|value| value == *n),
        // Go ParseUint rejects a leading sign; Rust's integer parser accepts '+'.
        table::Value::UInt(n) => {
            !want.starts_with('+') && want.parse::<u64>().is_ok_and(|value| value == *n)
        }
        _ => false,
    }
}

pub(super) fn execute_rows(
    view: &table::Table<'_>,
    projection: &Projection,
    filter: Option<Filter<'_>>,
    limit: Limit,
    budget: &Budget,
    sink: &mut dyn FnMut(StreamEvent<'_>) -> Result<()>,
) -> Result<WalkSummary> {
    let mut summary = WalkSummary {
        count: 0,
        truncated: false,
    };
    for id in view.ids() {
        budget.check()?;
        let mut row = view.row(id, 1 << 20)?;
        if let Some(filter) = filter
            && !matches(
                row.get(filter.field)
                    .ok_or_else(|| field_error(filter.field))?,
                filter.value,
            )
        {
            continue;
        }
        // The first additional matching row proves truncation, but is never
        // projected or yielded. Filtering must precede this limit probe.
        if summary.count == limit.get() {
            summary.truncated = true;
            break;
        }
        budget.charge(Cost::metadata(
            projection.fields.len().checked_mul(128).ok_or_else(|| {
                fault(
                    "records.query_resource_budget",
                    3,
                    "Stream projection size overflow",
                )
            })?,
        ))?;
        let mut projected = BTreeMap::new();
        for field in &projection.fields {
            if let Some(value) = row.remove(field) {
                projected.insert(field.clone(), value);
            }
        }
        let record = StreamRecord {
            index: summary.count,
            record_id: id,
            row: &projected,
        };
        budget.check()?;
        summary.count += 1;
        sink(StreamEvent::Record(record))?;
    }
    budget.check()?;
    Ok(summary)
}
