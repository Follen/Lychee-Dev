//! Raidbots is an explicitly supplied DBCache snapshot, never an implicit HTTP download.
use super::*;
use chrono::{DateTime, Utc};
use std::{
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
    check_scope(inv, budget)?;
    let mut captured_at = String::new();
    // The same controlled directory/file owner supplies the mtime admission and
    // stable bounded body. Its checked close completes before publishing a pin.
    let raw = super::super::local::read_source_file_controlled(
        path,
        512 << 20,
        budget,
        &inv.control,
        |before| {
            let modified = before.modified()?;
            captured_at = storage::timestamp(DateTime::<Utc>::from(modified));
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
            Ok(())
        },
    )?;
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for block in raw.chunks(65536) {
        check_scope(inv, budget)?;
        hash.update(block);
    }
    let digest = crate::casc::hex(&hash.finalize());
    check_scope(inv, budget)?;
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
    let page = super::super::inspect_budgeted_with_check(&raw, filter, Some(budget), || {
        inv.control.check()
    })?;
    let page = if let Some(definition) = &q.definition {
        super::super::decode_page_with_check(&raw, page, definition, budget, || {
            inv.control.check()
        })?
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
    check_scope(inv, budget)?;
    Ok(result)
}
