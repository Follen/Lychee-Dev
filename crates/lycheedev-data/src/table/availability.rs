//! Match authenticated missing spans to complete WDC section identities.
use super::*;
use crate::blte::Missing;

#[derive(Default)]
pub(crate) struct Availability {
    pub skipped: BTreeSet<usize>,
    pub ranges: Vec<Range<usize>>,
    pub partitions: Vec<UnavailablePartition>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UnavailablePartition {
    index: usize,
    #[serde(rename = "keyID")]
    key_id: String,
    rows: u32,
    copies: u32,
    #[serde(rename = "recordIDs", skip_serializing_if = "Vec::is_empty")]
    record_ids: Vec<u32>,
    #[serde(rename = "idsComplete")]
    ids_complete: bool,
}
fn missing_key() -> lycheedev_core::ToolkitError {
    fault(
        "container.key_unavailable",
        3,
        "WDC bytes require an unavailable encryption key",
    )
}

pub(crate) fn inspect_available_budgeted(
    raw: &[u8],
    missing: &[Missing],
    limits: TableLimits,
    budget: Arc<Budget>,
) -> Result<(Layout, Availability)> {
    budget.check()?;
    budget.charge(Cost::metadata(
        missing.len().checked_mul(64).ok_or_else(missing_key)?,
    ))?;
    let mut availability = Availability::default();
    for gap in missing {
        let end = gap
            .offset
            .checked_add(gap.bytes)
            .filter(|end| gap.bytes != 0 && *end <= raw.len())
            .ok_or_else(missing_key)?;
        if availability
            .ranges
            .last()
            .is_some_and(|prior| prior.end > gap.offset)
        {
            return Err(missing_key());
        }
        availability.ranges.push(gap.offset..end);
    }
    let layout = inspect_source(
        &Source::new(raw, &availability.ranges, limits, Some(budget.clone()))?,
        limits,
    )
    .map_err(|error| {
        if error.code == "table.partition_unavailable" {
            missing_key()
        } else {
            error
        }
    })?;
    for (gap, range) in missing.iter().zip(&availability.ranges) {
        budget.check()?;
        if gap.offset < layout.metadata_end {
            return Err(missing_key());
        }
        let mut covered = 0u64;
        for (index, partition) in layout.partitions.iter().enumerate() {
            budget.charge(Cost::work(1))?;
            let start = u64::from(partition.offset);
            let end = if layout.flags & 1 == 0 {
                start
                    + u64::from(partition.rows) * u64::from(layout.stride)
                    + u64::from(partition.string_bytes)
            } else {
                u64::from(partition.sparse_offset)
                    + if layout.version == 2 {
                        (u64::from(layout.last_id) - u64::from(layout.first_id) + 1) * 6
                    } else {
                        u64::from(partition.sparse_count) * 10
                    }
            } + u64::from(partition.identity_bytes)
                + u64::from(partition.copies) * 8
                + u64::from(partition.relation_bytes);
            if start >= range.end as u64 || end <= range.start as u64 {
                continue;
            }
            if partition.key_id == 0 || format!("{:016x}", partition.key_id) != gap.key_id {
                return Err(missing_key());
            }
            covered += end.min(range.end as u64) - start.max(range.start as u64);
            availability.skipped.insert(index);
        }
        if covered != gap.bytes as u64 {
            return Err(missing_key());
        }
    }
    for index in &availability.skipped {
        budget.check()?;
        let partition = &layout.partitions[*index];
        budget.charge(Cost::metadata(128 + partition.encrypted_ids.len() * 4))?;
        availability.partitions.push(UnavailablePartition {
            index: *index,
            key_id: format!("{:016x}", partition.key_id),
            rows: partition.rows,
            copies: partition.copies,
            record_ids: partition.encrypted_ids.clone(),
            ids_complete: partition.ids_complete,
        });
    }
    Ok((layout, availability))
}

impl Availability {
    /// Direct DB2 record selection may distinguish encrypted absence. Generic
    /// table lookups preserve ErrRecordMissing for SQL and navigation consumers.
    pub(crate) fn record_unavailable(&self, id: u32, budget: &Budget) -> Result<bool> {
        budget.check()?;
        for partition in &self.partitions {
            budget.charge(Cost::work(1))?;
            if !partition.ids_complete {
                return Ok(true);
            }
            for hidden in &partition.record_ids {
                budget.charge(Cost::work(1))?;
                if *hidden == id {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lycheedev_core::control::OperationControl;
    #[test]
    fn direct_record_availability_distinguishes_complete_and_unknown_id_lists() {
        // This leaf classification test has no workspace owner or adaptive probe.
        let control = OperationControl::default();
        let budget = Budget::controlled(Cost::defaults(), control.deadline(), control.clone());
        let mut availability = Availability {
            partitions: vec![UnavailablePartition {
                index: 1,
                key_id: "1234567890abcdef".into(),
                rows: 1,
                copies: 0,
                record_ids: vec![2],
                ids_complete: false,
            }],
            ..Default::default()
        };
        assert!(availability.record_unavailable(99, &budget).unwrap());
        availability.partitions[0].ids_complete = true;
        assert!(availability.record_unavailable(2, &budget).unwrap());
        assert!(!availability.record_unavailable(99, &budget).unwrap());
        control
            .cancel_handle()
            .store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            availability
                .record_unavailable(2, &budget)
                .unwrap_err()
                .exit_code,
            7
        );
    }
}
