//! Raidbots is an explicitly supplied DBCache snapshot, never an implicit HTTP download.
use super::*;
use chrono::{DateTime, Utc};
use std::{
    fs,
    path::Path,
    time::{Duration, SystemTime},
};
pub(super) fn query(inv: &Invocation, q: Query, budget: &Budget) -> Result<RemoteResult> {
    if !q.search.is_empty() || q.from.is_some() || q.to.is_some() {
        return Err(error(
            "filter_unsupported",
            2,
            "cache rows carry no search text or timestamps",
        ));
    }
    if q.page != 0 {
        return Err(error(
            "filter_unsupported",
            2,
            "snapshot pagination uses the immutable after-index cursor",
        ));
    }
    if !q.table.is_empty() && q.table_hash.is_none() {
        return Err(error(
            "filter_unsupported",
            2,
            "cache rows carry table hashes; resolve the table name first",
        ));
    }
    let cursor = if q.cursor.is_empty() {
        None
    } else {
        Some(Cursor::decode(&q.cursor, &q, "raidbots")?)
    };
    let path = Path::new(inv.required("--raidbots")?);
    budget.check()?;
    let before = fs::metadata(path)?;
    if !before.is_file() {
        return Err(error("format", 4, "records.hotfix_format"));
    }
    let modified = before.modified()?;
    let captured_at = storage::timestamp(DateTime::<Utc>::from(modified));
    if SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO)
        > Duration::from_secs(30 * 24 * 60 * 60)
    {
        return Err(error(
            "snapshot_stale",
            3,
            format!(
                "snapshot captured at {} is older than 720h0m0s",
                DateTime::<Utc>::from(modified).format("%Y-%m-%dT%H:%M:%SZ")
            ),
        ));
    }
    // Go's remote snapshot CLI deliberately does not forward --max-bytes.
    let raw = super::super::local::read_source_file(path, 512 << 20, budget)?;
    budget.check()?;
    let after = fs::metadata(path)?;
    if before.len() != after.len() || modified != after.modified()? {
        return Err(error(
            "identity",
            4,
            "Cache changed while reading; retry a stable copy",
        ));
    }
    let digest = storage::sha256(&raw);
    budget.check()?;
    let mut filter = super::super::CacheFilter {
        build: q.build_number()?,
        table_hash: q.table_hash,
        record_id: q.record_id,
        push: q.push_id,
        region: q.region_id,
        status: q.status,
        latest: q.latest,
        limit: q.limit,
        after_index: None,
    };
    if let Some(cursor) = &cursor {
        if cursor.reference != digest {
            return Err(error(
                "cursor",
                2,
                "snapshot bytes changed since the cursor was cut",
            ));
        }
        filter.after_index = Some(
            cursor
                .after_index
                .ok_or_else(|| error("cursor", 2, "snapshot cursor has no after-index"))?,
        );
    }
    let page = super::super::inspect_budgeted(&raw, filter, Some(budget))?;
    let page = if let Some(definition) = &q.definition {
        super::super::decode_page(&raw, page, definition, budget)?
    } else {
        page
    };
    let records = page
        .entries
        .iter()
        .map(|e| Record {
            provider: "raidbots".into(),
            push: e.push,
            unique_id: e.unique_id,
            region_id: e.region,
            table_hash: Some(e.table_hash),
            record_id: e.record_id,
            status: e.status,
            build: page.filter.build.to_string(),
            index: Some(e.index),
            payload_length: e.payload_bytes,
            payload_hex: e.payload_hex.clone(),
            fields: e.fields.clone(),
            decode_state: e.decode_state.clone(),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let next = if page.truncated && page.next_index.is_some() {
        Cursor {
            source: "raidbots".into(),
            reference: digest.clone(),
            scope: q.scope_digest()?,
            after_index: page.next_index,
            ..Default::default()
        }
        .encode(budget)?
    } else {
        String::new()
    };
    let mut coverage = Coverage::new("raidbots", &q);
    coverage.truncated = page.truncated;
    coverage.scanned = page.scanned as usize;
    coverage.matched = page.matched as usize;
    coverage.returned = records.len();
    coverage.selected_push = page.selected_push;
    coverage.captured_at = Some(captured_at);
    coverage.reference = digest;
    if page.truncated {
        coverage.truncation_reason = "limit".into();
    }
    let mut result = RemoteResult::new("raidbots", q, records, coverage, next)?;
    // Record-window completion and Raidbots server coverage are different fields.
    result.complete = page.complete;
    Ok(result)
}
