//! Go 3.2.3 effective.go and hotfix_snapshots.go. One SQL invocation owns its
//! authenticated immutable snapshots; an overlay never changes a static table.
use super::*;
use crate::hotfix::{self, CacheEntry};
use crate::resource::RetainedReservation;
use relational::{EqualityCursor, RowCursor};
use serde::Serialize;
use std::collections::BTreeSet;

const POLICY: &str = "push-ascending/capture-order/physical-index; valid-payload replaces; invalid deletes; cached-key exception";
fn identity() -> lycheedev_core::ToolkitError {
    fault(
        "records.hotfix_identity",
        4,
        "Effective source requires distinct complete cache captures for the exact DataPin",
    )
}
fn limit() -> lycheedev_core::ToolkitError {
    fault(
        "records.hotfix_limit",
        3,
        "Effective cache input or decoded fields exceed the bound",
    )
}
fn fields() -> lycheedev_core::ToolkitError {
    fault(
        "records.hotfix_fields",
        4,
        "Invalid effective record identity or fields",
    )
}
fn definition_error() -> lycheedev_core::ToolkitError {
    fault(
        "records.definition_identity",
        4,
        "Effective source requires an identity column and no reserved hotfix columns",
    )
}
#[derive(Clone)]
pub(super) struct SharedCapture {
    value: Arc<Value>,
    value_bytes: usize,
}
impl Serialize for SharedCapture {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.value.as_ref().serialize(serializer)
    }
}
fn tree_bytes<K, V>(entries: usize) -> usize {
    if entries == 0 {
        0
    } else {
        (entries.div_ceil(5) + 2).saturating_mul(16 * size_of::<(K, V)>() + 128)
    }
}
fn value_bytes(value: &Value, budget: &Budget) -> Result<usize> {
    budget.charge(Cost::work(1))?;
    let contents = match value {
        Value::String(text) => text.capacity(),
        Value::Number(number) => number.to_string().len(),
        Value::Array(values) => {
            let mut bytes = values.len().saturating_mul(2 * size_of::<Value>());
            for value in values {
                bytes = bytes
                    .checked_add(value_bytes(value, budget)?)
                    .ok_or_else(limit)?;
            }
            bytes
        }
        Value::Object(values) => {
            let mut bytes = tree_bytes::<String, Value>(values.len());
            for (key, value) in values {
                let value_bytes = value_bytes(value, budget)?;
                bytes = bytes
                    .checked_add(key.capacity())
                    .and_then(|bytes| bytes.checked_add(value_bytes))
                    .ok_or_else(limit)?;
            }
            bytes
        }
        _ => 0,
    };
    contents.checked_add(size_of::<Value>()).ok_or_else(limit)
}
impl SharedCapture {
    fn admit(value: Value, budget: &Budget) -> Result<Self> {
        let value_bytes = value_bytes(&value, budget)?;
        budget.charge(Cost::retained(
            value_bytes.saturating_add(2 * size_of::<usize>()),
        ))?;
        Ok(Self {
            value: Arc::new(value),
            value_bytes,
        })
    }
}

/// SQL rechecks immutable references because it does not hold Go's metadata
/// transaction for the entire query. That extra verification has a separate
/// monotonic work allowance, bounded by 16 references * 1 MiB * the existing
/// eight units per input byte. Ordinary decoding cannot spend this allowance.
/// Temporary raw/typed/identity copies still share the query's memory ledger.
pub(super) struct ReferenceAccounting<'a> {
    budget: &'a Budget,
    verification: Budget,
    scratch: std::cell::RefCell<Option<crate::resource::RetainedReservation<'a>>>,
}
impl<'a> ReferenceAccounting<'a> {
    pub fn new(budget: &'a Budget, control: &lycheedev_core::control::OperationControl) -> Self {
        Self {
            budget,
            verification: Budget::controlled(
                Cost {
                    decode_work: 16 * (1 << 20) * 8,
                    ..Default::default()
                },
                budget.deadline,
                control.clone(),
            ),
            scratch: Default::default(),
        }
    }
}
impl evidence::CaptureAccounting for ReferenceAccounting<'_> {
    fn check(&self) -> Result<()> {
        self.budget.check()?;
        self.verification.check()
    }
    fn admit_allocation(&self, bytes: usize) -> Result<()> {
        self.budget.charge(Cost::metadata(bytes))
    }
    fn admit_work(&self, units: usize) -> Result<()> {
        self.budget.charge(Cost::work(units))
    }
    fn admit_dependency(&self, bytes: usize) -> Result<()> {
        self.check()?;
        if bytes > 1 << 20 {
            return Err(limit());
        }
        // The evidence trait guarantees the prior raw document and parsed
        // capture have been dropped before the next call. Never refund work
        // or cumulative metadata, and never inspect/reset another owner.
        let mut scratch = self.scratch.borrow_mut();
        drop(scratch.take());
        let allocation = self.budget.reserve_retained(bytes * 24 + 4096)?;
        self.budget.charge(Cost {
            metadata_bytes: bytes as u64,
            ..Default::default()
        })?;
        self.verification.charge(Cost::work(bytes * 8))?;
        *scratch = Some(allocation);
        Ok(())
    }
}

struct Snapshot {
    source: SharedCapture,
    raw: Vec<u8>,
    layout: u32,
    tables: BTreeMap<u32, Vec<CacheEntry>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Report {
    pub table: String,
    pub policy: &'static str,
    pub captures: Vec<SharedCapture>,
    pub replaced_or_added: usize,
    pub deleted: usize,
}
struct Patch {
    values: Option<Vec<relational::Scalar>>,
    ordinal: usize,
    entry: CacheEntry,
}

/// Prepared independently of the base provider and the mutable capture cache.
/// All selected patches are validated here, including for schema-only queries.
pub(super) struct OverlayPlan {
    schema: relational::Source,
    patches: BTreeMap<u32, Patch>,
    capture_ids: Vec<String>,
    identity_index: usize,
}

pub(super) struct Snapshots<'a> {
    inv: &'a Invocation,
    pin: &'a selection::DataPin,
    budget: &'a Budget,
    captures: BTreeMap<String, Snapshot>,
    bytes: usize,
}
impl<'a> Snapshots<'a> {
    pub fn new(inv: &'a Invocation, pin: &'a selection::DataPin, budget: &'a Budget) -> Self {
        Self {
            inv,
            pin,
            budget,
            captures: BTreeMap::new(),
            bytes: 0,
        }
    }
    fn check(&self) -> Result<()> {
        self.inv.control.check()?;
        self.budget.check()
    }
    fn load(&mut self, id: &str) -> Result<()> {
        self.check()?;
        if self.captures.contains_key(id) {
            return Ok(());
        }
        if self.captures.len() >= 16 {
            return Err(limit());
        }
        let build = self
            .pin
            .full_build
            .rsplit('.')
            .next()
            .unwrap_or("")
            .parse::<u32>()
            .map_err(|_| fault("records.hotfix_build_mismatch", 4, "Invalid cache build"))?;
        // Metadata documents are capped at 1 MiB. Reserve parse/identity and
        // projection scratch before the existing reader allocates any of them;
        // only the actual retained Value is kept after that bounded seam.
        let source = {
            let _parse = self.budget.reserve_retained((1 << 20) * 64 + 4096)?;
            let value = evidence::read_capture_controlled_with_check(
                &self.inv.home,
                id,
                &self.inv.control,
                || self.budget.check(),
            )?;
            self.check()?;
            let bytes = capture::json_size(&value, self.budget)?;
            self.budget.charge(Cost {
                metadata_bytes: bytes as u64,
                ..Default::default()
            })?;
            SharedCapture::admit(value, self.budget)?
        };
        let source_value = source.value.as_ref();
        if source_value["provenance"]["kind"] != "hotfix-cache"
            || source_value["complete"] != true
            || source_value["truncated"] != false
        {
            return Err(identity());
        }
        let origin = selection::load_pinned_controlled_with_check(
            &self.inv.home,
            source_value["provenance"]["snapshot"]
                .as_str()
                .unwrap_or(""),
            &self.inv.control,
            || self.budget.check(),
        )?;
        self.check()?;
        if origin.data.as_ref() != Some(self.pin)
            || source_value["provenance"]["dataBuild"] != self.pin.full_build
        {
            return Err(identity());
        }
        let bytes = source_value["blob"]["bytes"]
            .as_u64()
            .filter(|bytes| *bytes <= ((128 << 20) - self.bytes) as u64)
            .ok_or_else(limit)? as usize;
        self.budget.charge(Cost::metadata(bytes))?;
        let raw = evidence::read_blob_bounded_with_check(
            &self.inv.home,
            &source_value["blob"],
            128 << 20,
            &self.inv.control,
            || self.budget.check(),
        )?;
        let (_, layout, start) =
            hotfix::cache_layout_with_check(&raw, build, Some(self.budget), || {
                self.inv.control.check()
            })?;
        let mut tables: BTreeMap<u32, Vec<CacheEntry>> = BTreeMap::new();
        hotfix::walk_with_check(
            &raw,
            start,
            layout,
            Some(self.budget),
            || self.inv.control.check(),
            |entry| {
                self.budget.charge(Cost {
                    retained_bytes: 384,
                    metadata_bytes: 384,
                    decode_work: 1,
                    ..Default::default()
                })?;
                tables.entry(entry.table_hash).or_default().push(entry);
                Ok(())
            },
        )?;
        self.budget.charge(Cost::retained(
            id.len() + tree_bytes::<String, Snapshot>(self.captures.len() + 1)
                - tree_bytes::<String, Snapshot>(self.captures.len()),
        ))?;
        self.bytes += raw.len();
        self.captures.insert(
            id.into(),
            Snapshot {
                source,
                raw,
                layout,
                tables,
            },
        );
        Ok(())
    }
    pub fn apply(
        &mut self,
        base: relational::Source,
        definition: &schema::Definition,
        bundle: &definitions::Bundle,
        ids: &[String],
    ) -> Result<(relational::Source, Report)> {
        self.apply_mode(base, definition, bundle, ids, false)
    }
    pub fn schema(
        &mut self,
        base: relational::Source,
        definition: &schema::Definition,
        bundle: &definitions::Bundle,
        ids: &[String],
    ) -> Result<(relational::Source, Report)> {
        self.apply_mode(base, definition, bundle, ids, true)
    }
    fn apply_mode(
        &mut self,
        base: relational::Source,
        definition: &schema::Definition,
        bundle: &definitions::Bundle,
        ids: &[String],
        schema_only: bool,
    ) -> Result<(relational::Source, Report)> {
        let (plan, report) = self.prepare(&base.columns, base.complete, definition, bundle, ids)?;
        if schema_only {
            return Ok((plan.schema, report));
        }
        // Compatibility for existing owned-source consumers. The actual SQL
        // path borrows the plan's provider and never materializes this table.
        let safety = self.budget.safety_only();
        let mut retained = safety.reserve_retained(0)?;
        let mut rows = Vec::new();
        plan.scan(&base, self.budget, &mut |row| {
            retained.grow(
                cloned_row_bytes(row, 0, 0)?
                    .checked_add(size_of::<Vec<relational::Scalar>>())
                    .ok_or_else(limit)?,
            )?;
            rows.try_reserve_exact(1).map_err(|_| limit())?;
            rows.push(clone_row(row, row.len())?);
            Ok(())
        })?;
        let mut source = plan.schema;
        source.rows = rows;
        // Source has no reservation owner; its owned rows follow the legacy
        // conservative query-lifetime admission rather than being refunded.
        retained.commit();
        Ok((source, report))
    }

    pub(super) fn prepare(
        &mut self,
        columns: &[String],
        complete: bool,
        definition: &schema::Definition,
        bundle: &definitions::Bundle,
        ids: &[String],
    ) -> Result<(OverlayPlan, Report)> {
        self.check()?;
        if ids.is_empty() || ids.len() > 16 {
            return Err(identity());
        }
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        self.budget.charge(Cost::retained(
            bundle.identity.name.len() + ids.len().saturating_mul(2 * size_of::<SharedCapture>()),
        ))?;
        let mut report = Report {
            table: bundle.identity.name.clone(),
            policy: POLICY,
            captures: vec![],
            replaced_or_added: 0,
            deleted: 0,
        };
        for (ordinal, id) in ids.iter().enumerate() {
            self.check()?;
            if !seen.insert(id) {
                return Err(identity());
            }
            self.load(id)?;
            let snapshot = &self.captures[id];
            let selected = snapshot
                .tables
                .get(&bundle.identity.hash)
                .map(Vec::as_slice)
                .unwrap_or_default();
            if selected.len() > 100_000 - entries.len() {
                return Err(limit());
            }
            self.budget.charge(Cost {
                retained_bytes: selected.len() as u64 * 384,
                metadata_bytes: selected.len() as u64 * 384,
                decode_work: selected.len() as u64,
                ..Default::default()
            })?;
            for entry in selected {
                self.check()?;
                if snapshot.layout >= 7 && entry.status > 4 {
                    return Err(fault(
                        "records.hotfix_format",
                        4,
                        format!("unsupported effective status {}", entry.status),
                    ));
                }
                entries.push((ordinal, entry.clone()));
            }
            report.captures.push(snapshot.source.clone());
        }
        let valid = |ordinal: usize, entry: &CacheEntry| {
            let layout = self.captures[&ids[ordinal]].layout;
            entry.payload_bytes > 0
                && if layout >= 7 {
                    entry.status == 1
                } else {
                    entry.status != 0
                }
        };
        let should_delete = !matches!(bundle.identity.hash, 3744420815 | 35137211)
            || !entries
                .iter()
                .any(|(ordinal, entry)| entry.push == -1 && valid(*ordinal, entry));
        self.budget
            .charge(Cost::work(entries.len().saturating_mul(20)))?;
        // (push, capture ordinal, physical index) is unique, so unstable sort
        // gives the exact stable Go ordering without a second scratch array.
        entries.sort_unstable_by_key(|(ordinal, entry)| (entry.push, *ordinal, entry.index));
        self.check()?;
        let identity = definition
            .fields
            .iter()
            .rev()
            .find(|field| field.identity)
            .map(|field| field.name.as_str())
            .unwrap_or("");
        let identity_index = columns
            .iter()
            .rposition(|name| name == identity)
            .ok_or_else(definition_error)?;
        if columns.len() > 4093 || columns.iter().any(|name| name.starts_with("__hotfix_")) {
            return Err(definition_error());
        }
        let mut patches = BTreeMap::new();
        let mut field_bytes = 0usize;
        for (ordinal, entry) in entries {
            self.check()?;
            if !valid(ordinal, &entry) {
                if should_delete {
                    self.budget.charge(Cost::metadata(384))?;
                    patches.insert(
                        entry.record_id,
                        Patch {
                            values: None,
                            ordinal,
                            entry,
                        },
                    );
                }
                continue;
            }
            self.budget.charge(Cost {
                retained_bytes: (entry.payload_bytes * 4 + columns.len() * 64 + 384) as u64,
                metadata_bytes: (columns.len() * 64 + 384) as u64,
                decode_work: (columns.len() + 1) as u64,
                ..Default::default()
            })?;
            let raw = &self.captures[&ids[ordinal]].raw;
            let begin = entry.payload_offset as usize;
            let decoded = hotfix::decode_fields_scoped(
                &raw[begin..begin + entry.payload_bytes],
                entry.record_id,
                definition,
                Some(self.budget),
            )?;
            let size = hotfix::fields_json_size(decoded.fields(), self.budget)?;
            field_bytes = field_bytes
                .checked_add(size)
                .filter(|size| *size <= 32 << 20)
                .ok_or_else(limit)?;
            let values = flatten(columns, decoded.fields())?;
            drop(decoded);
            patches.insert(
                entry.record_id,
                Patch {
                    values: Some(values),
                    ordinal,
                    entry,
                },
            );
        }
        for patch in patches.values() {
            self.check()?;
            if patch.values.is_some() {
                report.replaced_or_added += 1;
            } else {
                report.deleted += 1;
            }
        }
        // The existing schema allocation charge stays in its original ledger.
        // Borrowed base columns and capture IDs need new owned strings, whose
        // additional Rust storage is admitted separately from Go's counters.
        self.budget.charge(Cost::retained(
            (columns.len() + 3) * size_of::<String>() + 128,
        ))?;
        let copied_strings = columns.iter().chain(ids).try_fold(
            ids.len()
                .checked_mul(size_of::<String>())
                .ok_or_else(limit)?,
            |bytes, text| bytes.checked_add(text.len()).ok_or_else(limit),
        )?;
        self.budget
            .safety_only()
            .charge(Cost::retained(copied_strings))?;
        let mut owned_columns = Vec::new();
        owned_columns
            .try_reserve_exact(columns.len() + 3)
            .map_err(|_| limit())?;
        for column in columns {
            self.check()?;
            owned_columns.push(clone_text(column)?);
        }
        owned_columns.extend([
            "__hotfix_source".into(),
            "__hotfix_push".into(),
            "__hotfix_index".into(),
        ]);
        let mut capture_ids = Vec::new();
        capture_ids
            .try_reserve_exact(ids.len())
            .map_err(|_| limit())?;
        for id in ids {
            self.check()?;
            capture_ids.push(clone_text(id)?);
        }
        self.check()?;
        Ok((
            OverlayPlan {
                schema: relational::Source {
                    columns: owned_columns,
                    rows: vec![],
                    complete,
                },
                patches,
                capture_ids,
                identity_index,
            },
            report,
        ))
    }
}

struct AugmentedRow<'query> {
    cells: Vec<relational::Scalar>,
    _allocation: RetainedReservation<'query>,
}

struct OverlayScanCursor<'query> {
    current: Option<AugmentedRow<'query>>,
    // The base remains on its current row while the augmented row is consumed.
    // Its decoded row/vector leases cannot be refunded merely after cloning.
    base: Option<Box<dyn RowCursor + 'query>>,
    visited: BTreeSet<u32>,
    _visited: RetainedReservation<'query>,
    visited_bytes: usize,
    patches: std::collections::btree_map::Iter<'query, u32, Patch>,
    plan: &'query OverlayPlan,
    budget: &'query Budget,
    done: bool,
    _allocation: RetainedReservation<'query>,
}
impl OverlayScanCursor<'_> {
    fn advance_inner(&mut self) -> Result<bool> {
        while let Some(base) = &mut self.base {
            if !base.advance()? {
                self.base = None;
                break;
            }
            self.budget.check()?;
            let row = base.row();
            let id = row_id(row.get(self.plan.identity_index).ok_or_else(fields)?)?;
            if let Some(patch) = self.plan.patches.get(&id) {
                if !self.visited.contains(&id) {
                    let count = self.visited.len().checked_add(1).ok_or_else(limit)?;
                    let next = tree_bytes::<u32, ()>(count);
                    self._visited
                        .grow(next.checked_sub(self.visited_bytes).ok_or_else(limit)?)?;
                    self.visited.insert(id);
                    self.visited_bytes = next;
                }
                if let Some(values) = &patch.values {
                    self.current = Some(self.plan.augment(values, Some(patch), self.budget)?);
                    return Ok(true);
                }
            } else {
                self.current = Some(self.plan.augment(row, None, self.budget)?);
                return Ok(true);
            }
        }
        // This phase starts only after successful base exhaustion, including
        // its terminal checks. A late base error cannot leak appended patches.
        for (id, patch) in self.patches.by_ref() {
            self.budget.check()?;
            if !self.visited.contains(id)
                && let Some(values) = &patch.values
            {
                self.current = Some(self.plan.augment(values, Some(patch), self.budget)?);
                return Ok(true);
            }
        }
        self.done = true;
        Ok(false)
    }
}
impl RowCursor for OverlayScanCursor<'_> {
    fn advance(&mut self) -> Result<bool> {
        self.current = None;
        if self.done {
            return Ok(false);
        }
        let result = self.advance_inner();
        if result.is_err() {
            self.done = true;
        }
        result
    }
    fn row(&self) -> &[relational::Scalar] {
        self.current.as_ref().map_or(&[], |row| &row.cells)
    }
}

enum EqualInput<'query> {
    Patch(Option<&'query Patch>),
    Base(Box<dyn RowCursor + 'query>),
}
struct OverlayEqualCursor<'query> {
    current: Option<AugmentedRow<'query>>,
    input: EqualInput<'query>,
    plan: &'query OverlayPlan,
    budget: &'query Budget,
    done: bool,
    _allocation: RetainedReservation<'query>,
}
impl OverlayEqualCursor<'_> {
    fn advance_inner(&mut self) -> Result<bool> {
        match &mut self.input {
            EqualInput::Patch(pending) => {
                let Some(patch) = pending.take() else {
                    self.done = true;
                    return Ok(false);
                };
                let values = patch.values.as_ref().ok_or_else(fields)?;
                self.current = Some(self.plan.augment(values, Some(patch), self.budget)?);
            }
            EqualInput::Base(base) => {
                if !base.advance()? {
                    self.done = true;
                    return Ok(false);
                }
                self.current = Some(self.plan.augment(base.row(), None, self.budget)?);
            }
        }
        Ok(true)
    }
}
impl RowCursor for OverlayEqualCursor<'_> {
    fn advance(&mut self) -> Result<bool> {
        self.current = None;
        if self.done {
            return Ok(false);
        }
        let result = self.advance_inner();
        if result.is_err() {
            self.done = true;
        }
        result
    }
    fn row(&self) -> &[relational::Scalar] {
        self.current.as_ref().map_or(&[], |row| &row.cells)
    }
}

impl OverlayPlan {
    pub(super) fn schema(&self) -> &relational::Source {
        &self.schema
    }

    pub(super) fn cursor<'query>(
        &'query self,
        base: &'query dyn relational::RowProvider,
        budget: &'query Budget,
    ) -> Result<Box<dyn RowCursor + 'query>> {
        budget.check()?;
        let visited = budget.reserve_retained(0)?;
        let allocation = budget.reserve_retained(size_of::<OverlayScanCursor<'_>>())?;
        let base = base.cursor(None)?;
        Ok(Box::new(OverlayScanCursor {
            current: None,
            base: Some(base),
            visited: BTreeSet::new(),
            _visited: visited,
            visited_bytes: 0,
            patches: self.patches.iter(),
            plan: self,
            budget,
            done: false,
            _allocation: allocation,
        }))
    }

    pub(super) fn scan(
        &self,
        base: &dyn relational::RowProvider,
        budget: &Budget,
        emit: &mut relational::RowYield<'_>,
    ) -> Result<()> {
        provider::emit_rows(self.cursor(base, budget)?, emit)
    }

    pub(super) fn equal_cursor<'query>(
        &'query self,
        base: &'query dyn relational::RowProvider,
        budget: &'query Budget,
        column: &str,
        value: &relational::Scalar,
    ) -> Result<EqualityCursor<'query>> {
        // Go declines before looking at the value or touching a provider.
        // The executor owns the outer operation check before this boundary.
        if !crate::hotfix::remote::equal_fold(column, &self.schema.columns[self.identity_index])
            || !base.has_equal()
        {
            return Ok(EqualityCursor::Declined);
        }
        let input = if let Ok(id) = row_id(value)
            && let Some(patch) = self.patches.get(&id)
        {
            // A replacement/deletion masks the base, including missing IDs.
            // Deletion is handled without a new allocation or budget check.
            if patch.values.is_none() {
                return Ok(EqualityCursor::Empty);
            }
            EqualInput::Patch(Some(patch))
        } else {
            // Preserve the original scalar and the base's NULL/type/range
            // branches. Empty/declined outcomes need no overlay allocation.
            match base.equal_cursor(column, value)? {
                EqualityCursor::Declined => return Ok(EqualityCursor::Declined),
                EqualityCursor::Empty => return Ok(EqualityCursor::Empty),
                EqualityCursor::Rows(cursor) => EqualInput::Base(cursor),
            }
        };
        let allocation = budget.reserve_retained(size_of::<OverlayEqualCursor<'_>>())?;
        Ok(EqualityCursor::Rows(Box::new(OverlayEqualCursor {
            current: None,
            input,
            plan: self,
            budget,
            done: false,
            _allocation: allocation,
        })))
    }

    pub(super) fn equal(
        &self,
        base: &dyn relational::RowProvider,
        budget: &Budget,
        column: &str,
        value: &relational::Scalar,
        emit: &mut relational::RowYield<'_>,
    ) -> Result<relational::EqualityOutcome> {
        match self.equal_cursor(base, budget, column, value)? {
            EqualityCursor::Declined => Ok(relational::EqualityOutcome::Declined),
            EqualityCursor::Empty => Ok(relational::EqualityOutcome::Handled),
            EqualityCursor::Rows(cursor) => {
                provider::emit_rows(cursor, emit)?;
                Ok(relational::EqualityOutcome::Handled)
            }
        }
    }

    fn augment<'query>(
        &self,
        row: &[relational::Scalar],
        patch: Option<&Patch>,
        budget: &'query Budget,
    ) -> Result<AugmentedRow<'query>> {
        let source = patch.map(|patch| self.capture_ids[patch.ordinal].as_str());
        let length = row.len().checked_add(3).ok_or_else(limit)?;
        // reserve_retained always charges only safety bytes; borrow the stable
        // budget rather than a temporary safety_only view across suspension.
        let allocation =
            budget.reserve_retained(cloned_row_bytes(row, 3, source.map_or(0, str::len))?)?;
        let mut out = clone_row(row, length)?;
        if let Some(patch) = patch {
            out.extend([
                relational::Scalar::String(clone_text(source.ok_or_else(fields)?)?),
                relational::Scalar::Int(patch.entry.push as i64),
                relational::Scalar::UInt(patch.entry.index as u64),
            ]);
        } else {
            out.extend([
                relational::Scalar::Null,
                relational::Scalar::Null,
                relational::Scalar::Null,
            ]);
        }
        budget.check()?;
        Ok(AugmentedRow {
            cells: out,
            _allocation: allocation,
        })
    }
}

fn cloned_row_bytes(
    row: &[relational::Scalar],
    extra_cells: usize,
    extra_bytes: usize,
) -> Result<usize> {
    row.iter().try_fold(
        row.len()
            .checked_add(extra_cells)
            .and_then(|count| count.checked_mul(size_of::<relational::Scalar>()))
            .and_then(|bytes| bytes.checked_add(extra_bytes))
            .ok_or_else(limit)?,
        |bytes, value| {
            bytes
                .checked_add(match value {
                    relational::Scalar::String(text) => text.len(),
                    _ => 0,
                })
                .ok_or_else(limit)
        },
    )
}

fn clone_text(text: &str) -> Result<String> {
    let mut copy = String::new();
    copy.try_reserve_exact(text.len()).map_err(|_| limit())?;
    copy.push_str(text);
    Ok(copy)
}

/// Caller must admit both the vector and all copied strings before entering.
fn clone_row(row: &[relational::Scalar], capacity: usize) -> Result<Vec<relational::Scalar>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(capacity).map_err(|_| limit())?;
    for value in row {
        copy.push(match value {
            relational::Scalar::String(text) => relational::Scalar::String(clone_text(text)?),
            value => value.clone(),
        });
    }
    Ok(copy)
}

fn output_limit() -> lycheedev_core::ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "SQL result exceeds evidence budget",
    )
}
/// Stop before building any repeated capture Value tree. Reports share source
/// strings until this bounded final projection, whose allocations are admitted
/// from the measured source trees rather than a fixed per-capture estimate.
pub(super) fn reports_value(reports: &[Report], budget: &Budget) -> Result<Value> {
    struct Counter<'a> {
        budget: &'a Budget,
        bytes: usize,
        error: Option<lycheedev_core::ToolkitError>,
    }
    impl std::io::Write for Counter<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let checked = (|| {
                if bytes.len() > (16 << 20) - self.bytes {
                    return Err(output_limit());
                }
                self.budget
                    .charge(Cost::work(bytes.len().saturating_add(1)))?;
                self.bytes += bytes.len();
                Ok(bytes.len())
            })();
            checked.map_err(|error| {
                self.error = Some(error);
                std::io::Error::other("SQL report admission stopped")
            })
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter {
        budget,
        bytes: 0,
        error: None,
    };
    if let Err(error) = serde_json::to_writer(&mut counter, reports) {
        return Err(counter.error.unwrap_or_else(|| error.into()));
    }
    let mut bytes = reports.len().saturating_mul(2 * size_of::<Value>());
    for report in reports {
        budget.check()?;
        bytes = bytes
            .checked_add(
                tree_bytes::<String, Value>(5)
                    + report.table.len()
                    + POLICY.len()
                    + 256
                    + report.captures.len().saturating_mul(2 * size_of::<Value>()),
            )
            .ok_or_else(limit)?;
        for source in &report.captures {
            bytes = bytes.checked_add(source.value_bytes).ok_or_else(limit)?;
        }
    }
    budget.charge(Cost::retained(bytes))?;
    serde_json::to_value(reports).map_err(Into::into)
}

fn row_id(value: &relational::Scalar) -> Result<u32> {
    match value {
        relational::Scalar::UInt(value) => u32::try_from(*value).map_err(|_| fields()),
        relational::Scalar::Int(value) => u32::try_from(*value).map_err(|_| fields()),
        relational::Scalar::Float(value)
            if *value >= 0.0 && *value <= u32::MAX as f64 && value.trunc() == *value =>
        {
            Ok(*value as u32)
        }
        _ => Err(fields()),
    }
}
fn flatten(columns: &[String], row: &BTreeMap<String, Value>) -> Result<Vec<relational::Scalar>> {
    columns
        .iter()
        .map(|column| {
            let (name, index) = match column.split_once('[') {
                Some((name, index)) => (name, Some(index)),
                None => (column.as_str(), None),
            };
            let mut value = row.get(name).ok_or_else(fields)?;
            if let Some(index) = index {
                let index = index
                    .strip_suffix(']')
                    .unwrap_or(index)
                    .parse::<usize>()
                    .map_err(|_| fields())?;
                value = value
                    .as_array()
                    .and_then(|array| array.get(index))
                    .ok_or_else(fields)?;
            }
            super::scalar(value.clone()).map_err(|_| fields())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effective_ids_and_flatten_preserve_integer_array_boundaries() {
        for value in [
            relational::Scalar::UInt(u32::MAX as u64),
            relational::Scalar::Int(u32::MAX as i64),
            relational::Scalar::Float(u32::MAX as f64),
        ] {
            assert_eq!(row_id(&value).unwrap(), u32::MAX);
        }
        for value in [
            relational::Scalar::UInt(u32::MAX as u64 + 1),
            relational::Scalar::Int(-1),
            relational::Scalar::Float(1.5),
            relational::Scalar::Float(f64::NAN),
            relational::Scalar::Bool(true),
        ] {
            assert_eq!(row_id(&value).unwrap_err().code, "records.hotfix_fields");
        }
        let row = BTreeMap::from([("ID".into(), json!(u64::MAX)), ("A".into(), json!([1, 2]))]);
        assert_eq!(
            flatten(&["ID".into(), "A[1]".into()], &row).unwrap(),
            [
                relational::Scalar::UInt(u64::MAX),
                relational::Scalar::Int(2)
            ]
        );
        for name in ["Missing", "A[2]", "A[-1]", "A[wrong]", "ID[0]"] {
            assert!(flatten(&[name.into()], &row).is_err());
        }
    }
    fn invocation() -> Invocation {
        Invocation {
            command: "data sql".into(),
            positional: None,
            flags: BTreeMap::new(),
            home: "must-not-open-effective-unit".into(),
            cwd: ".".into(),
            control: Default::default(),
        }
    }
    fn schema_and_bundle(text: &str) -> (schema::Definition, definitions::Bundle) {
        let definition = schema::Document::parse(text.as_bytes())
            .unwrap()
            .select("12.1.0.69933", "")
            .unwrap();
        let bundle = definitions::Bundle {
            commit: "b".repeat(40),
            identity: definitions::TableIdentity {
                name: "Sample".into(),
                hash: 42,
                db2_file_data_id: 201,
                dbc_file_data_id: 0,
            },
            manifest: Value::Null,
            definition: Value::Null,
        };
        (definition, bundle)
    }
    fn entry(index: u32, bytes: usize) -> CacheEntry {
        CacheEntry {
            index,
            region: None,
            push: index as i32,
            unique_id: None,
            table_hash: 42,
            record_id: 1,
            status: 1,
            payload_offset: 0,
            payload_bytes: bytes,
            payload_hex: String::new(),
            fields: None,
            decode_state: String::new(),
        }
    }
    fn base() -> relational::Source {
        relational::Source {
            columns: vec!["ID".into(), "Value".into()],
            rows: vec![],
            complete: true,
        }
    }
    fn seed(snapshots: &mut Snapshots<'_>, raw: Vec<u8>, entries: Vec<CacheEntry>) {
        snapshots.captures.insert(
            "CAP-fixture".into(),
            Snapshot {
                source: SharedCapture::admit(json!({"id":"CAP-fixture"}), snapshots.budget)
                    .unwrap(),
                raw,
                layout: 9,
                tables: BTreeMap::from([(42, entries)]),
            },
        );
    }
    #[test]
    fn effective_explain_validates_patches_without_scanning_base_or_overlay_rows() {
        let inv = invocation();
        let pin = selection::DataPin::default();
        let (definition, bundle) = schema_and_bundle(
            "COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69933\n$id$ID<u32>\nValue<u32>\n",
        );
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        seed(
            &mut snapshots,
            [1u32.to_le_bytes(), 42u32.to_le_bytes()].concat(),
            vec![entry(0, 8)],
        );
        let mut invalid_base = base();
        invalid_base.rows = vec![vec![]];
        invalid_base.complete = false;
        let (schema, report) = snapshots
            .schema(invalid_base, &definition, &bundle, &["CAP-fixture".into()])
            .unwrap();
        assert!(schema.rows.is_empty());
        assert!(!schema.complete);
        assert_eq!(schema.columns.len(), 5);
        assert_eq!(report.replaced_or_added, 1);
        assert_eq!(report.deleted, 0);
        // Preparation still decodes selected patch payloads and refuses bad
        // records before a plan can be published.
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        seed(&mut snapshots, vec![0], vec![entry(0, 1)]);
        assert!(
            snapshots
                .schema(base(), &definition, &bundle, &["CAP-fixture".into()])
                .is_err()
        );
    }
    #[test]
    fn effective_admission_limits_and_original_stop_scope() {
        let inv = invocation();
        let pin = selection::DataPin::default();
        let (definition, bundle) = schema_and_bundle(
            "COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69933\n$id$ID<u32>\nValue<u32>\n",
        );
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        for ids in [vec![], vec!["CAP-fixture".into(); 17]] {
            assert_eq!(
                snapshots
                    .apply(base(), &definition, &bundle, &ids)
                    .err()
                    .unwrap()
                    .code,
                "records.hotfix_identity"
            );
        }
        seed(
            &mut snapshots,
            vec![],
            (0..100_001).map(|index| entry(index, 0)).collect(),
        );
        assert_eq!(
            snapshots
                .apply(base(), &definition, &bundle, &["CAP-fixture".into()])
                .err()
                .unwrap()
                .code,
            "records.hotfix_limit"
        );
        let limited = Budget::new(
            Cost {
                metadata_bytes: 1,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &limited);
        seed(&mut snapshots, vec![], vec![entry(0, 0)]);
        assert_eq!(
            snapshots
                .apply(base(), &definition, &bundle, &["CAP-fixture".into()])
                .err()
                .unwrap()
                .code,
            "records.query_resource_budget"
        );
        let cancelled = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            Some(Arc::new(std::sync::atomic::AtomicBool::new(true))),
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &cancelled);
        assert_eq!(
            snapshots
                .apply(base(), &definition, &bundle, &["CAP-fixture".into()])
                .err()
                .unwrap()
                .code,
            "operation.cancelled"
        );
        let expired = Budget::new(
            Cost::defaults(),
            Instant::now() - Duration::from_millis(1),
            None,
        );
        assert_eq!(
            Snapshots::new(&inv, &pin, &expired)
                .apply(base(), &definition, &bundle, &["CAP-fixture".into()])
                .err()
                .unwrap()
                .code,
            "records.query_resource_budget"
        );
    }
    #[test]
    fn effective_definition_reserves_metadata_columns_and_identity() {
        let inv = invocation();
        let pin = selection::DataPin::default();
        let (definition, bundle) = schema_and_bundle(
            "COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69933\n$id$ID<u32>\nValue<u32>\n",
        );
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        seed(&mut snapshots, vec![], vec![]);
        for columns in [
            vec!["Value".into()],
            vec!["ID".into(), "__hotfix_source".into()],
            std::iter::once("ID".into())
                .chain((0..4093).map(|i| format!("C{i}")))
                .collect(),
        ] {
            let base = relational::Source {
                columns,
                rows: vec![],
                complete: true,
            };
            assert_eq!(
                snapshots
                    .apply(base, &definition, &bundle, &["CAP-fixture".into()])
                    .err()
                    .unwrap()
                    .code,
                "records.definition_identity"
            );
        }
    }
    #[test]
    fn effective_fields_limit_counts_all_overwritten_records_with_go_escaping() {
        let inv = invocation();
        let pin = selection::DataPin::default();
        let (definition, bundle) = schema_and_bundle(
            "COLUMNS\nint ID\nstring Value\n\nBUILD 12.1.0.69933\n$id$ID<u32>\nValue\n",
        );
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        let mut raw = 1u32.to_le_bytes().to_vec();
        raw.extend(std::iter::repeat_n(b'<', 1 << 20));
        raw.push(0);
        let entries = (0..6).map(|index| entry(index, raw.len())).collect();
        seed(&mut snapshots, raw, entries);
        let error = snapshots
            .apply(base(), &definition, &bundle, &["CAP-fixture".into()])
            .err()
            .unwrap();
        assert_eq!(
            error.code, "records.hotfix_limit",
            "Repeated replacements still consume the cumulative 32 MiB decoded-field allowance: {error:?}"
        );
    }
    #[test]
    fn effective_large_capture_reports_share_until_bounded_projection() {
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let sources=(0..16).map(|index|SharedCapture::admit(json!({"id":format!("CAP-{index}"),"provenance":{"locator":"a".repeat(750_000)}}),&budget).unwrap()).collect::<Vec<_>>();
        let owned = budget.snapshot().unwrap().retained_bytes;
        assert!(
            owned > 12_000_000 && owned < 13_000_000,
            "Actual unique source storage is charged: {owned}"
        );
        let reports = (0..32)
            .map(|index| Report {
                table: format!("Table{index}"),
                policy: POLICY,
                captures: sources.clone(),
                replaced_or_added: 0,
                deleted: 0,
            })
            .collect::<Vec<_>>();
        for report in &reports {
            for (index, source) in report.captures.iter().enumerate() {
                assert!(Arc::ptr_eq(&source.value, &sources[index].value));
            }
        }
        let before = budget.snapshot().unwrap();
        assert_eq!(
            reports_value(&reports, &budget).unwrap_err().code,
            "relational.resource_limit"
        );
        assert_eq!(
            budget.snapshot().unwrap().retained_bytes,
            before.retained_bytes,
            "No repeated Value trees are allocated before the 16 MiB gate"
        );
        let short = Report {
            table: "OnlyOne".into(),
            policy: POLICY,
            captures: vec![sources[0].clone()],
            replaced_or_added: 0,
            deleted: 0,
        };
        let value = reports_value(&[short], &budget).unwrap();
        assert_eq!(
            value[0]["captures"][0]["provenance"]["locator"]
                .as_str()
                .unwrap()
                .len(),
            750_000
        );
        assert!(
            budget.snapshot().unwrap().retained_bytes >= before.retained_bytes + 750_000,
            "Successful repeated Value projection is admitted at its actual size"
        );
        let low = Budget::new(
            Cost {
                retained_bytes: 4096,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            None,
        );
        assert_eq!(
            SharedCapture::admit(json!({"locator":"a".repeat(750_000)}), &low)
                .err()
                .unwrap()
                .code,
            "records.query_resource_budget"
        );
    }
    #[test]
    fn effective_reference_accounting_reuses_scratch_with_separate_bounded_work() {
        use evidence::CaptureAccounting;
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(30),
            None,
        );
        let unrelated = budget.reserve_retained(13).unwrap();
        let meter = ReferenceAccounting::new(&budget, &Default::default());
        for _ in 0..16 {
            meter.admit_dependency(1 << 20).unwrap();
            assert_eq!(
                budget.snapshot().unwrap().retained_bytes,
                13 + (1 << 20) * 24 + 4096
            );
        }
        assert_eq!(budget.snapshot().unwrap().metadata_bytes, 16 << 20);
        assert_eq!(budget.snapshot().unwrap().decode_work, 0);
        assert_eq!(
            meter.verification.snapshot().unwrap().decode_work,
            128 << 20
        );
        assert!(
            meter.admit_dependency(1).is_err(),
            "Verification work remains cumulative"
        );
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 13);
        drop(meter);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 13);
        drop(unrelated);
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let control = lycheedev_core::control::OperationControl::new(
            Instant::now() + Duration::from_secs(30),
            cancelled,
        );
        let meter = ReferenceAccounting::new(&budget, &control);
        assert_eq!(
            meter.admit_dependency(1).unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
    }
    #[test]
    fn effective_wide_replacements_release_only_temporary_decoded_maps() {
        let inv = invocation();
        let pin = selection::DataPin::default();
        let mut text = String::from("COLUMNS\nint ID\n");
        for index in 1..3200 {
            text.push_str(&format!("int F{index}\n"));
        }
        text.push_str("\nBUILD 12.1.0.69933\n$id$ID<u32>\n");
        for index in 1..3200 {
            text.push_str(&format!("F{index}<u32>\n"));
        }
        let (definition, bundle) = schema_and_bundle(&text);
        let mut payload = 1u32.to_le_bytes().to_vec();
        payload.resize(3200 * 4, 0);
        let raw = payload.repeat(600);
        let entries = (0..600)
            .map(|index| {
                let mut entry = entry(index, payload.len());
                entry.payload_offset = index as u64 * payload.len() as u64;
                entry
            })
            .collect::<Vec<_>>();
        let budget = Budget::new(
            Cost::defaults(),
            Instant::now() + Duration::from_secs(120),
            None,
        );
        budget
            .charge(Cost::metadata(raw.len() + 600 * 384))
            .unwrap();
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        seed(&mut snapshots, raw, entries);
        let base = relational::Source {
            columns: definition
                .fields
                .iter()
                .map(|field| field.name.clone())
                .collect(),
            rows: vec![],
            complete: true,
        };
        let (result, report) = snapshots
            .apply(base, &definition, &bundle, &["CAP-fixture".into()])
            .unwrap();
        assert_eq!(report.replaced_or_added, 1);
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.columns.len(), 3203);
        let used = budget.snapshot().unwrap();
        assert!(
            used.retained_bytes < 256 << 20,
            "Per-record maps were freed, not accumulated: {}",
            used.retained_bytes
        );
        assert!(
            used.metadata_bytes > 120 << 20 && used.metadata_bytes < 128 << 20,
            "Cumulative metadata must not be reset: {}",
            used.metadata_bytes
        );
        assert!(
            used.decode_work > 1_920_000,
            "All record decoding remains charged"
        );
    }
    #[test]
    fn effective_cached_source_checks_original_invocation_before_independent_budget() {
        let mut inv = invocation();
        let pin = selection::DataPin::default();
        let control = lycheedev_core::control::OperationControl::uncancelable(
            Instant::now() + Duration::from_secs(30),
        );
        inv.control = control.clone();
        let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let budget = Budget::new(
            Cost::defaults(),
            control.deadline(),
            Some(cancelled.clone()),
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        seed(&mut snapshots, Vec::new(), Vec::new());
        let before = budget.snapshot().unwrap();
        control.tracked_cancel_handle().unwrap().cancel();
        cancelled.store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            snapshots.load("CAP-fixture").unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(budget.snapshot().unwrap(), before);
    }
    #[test]
    fn effective_uncached_source_checks_independent_budget_before_historical_io() {
        let inv = invocation();
        let pin = selection::DataPin::default();
        let cancelled = Budget::new(
            Cost::defaults(),
            inv.control.deadline(),
            Some(Arc::new(std::sync::atomic::AtomicBool::new(true))),
        );
        let expired = Budget::new(
            Cost::defaults(),
            Instant::now() - Duration::from_secs(1),
            None,
        );
        for (budget, code) in [
            (&cancelled, "operation.cancelled"),
            (&expired, "records.query_resource_budget"),
        ] {
            let mut snapshots = Snapshots::new(&inv, &pin, budget);
            assert_eq!(snapshots.load("not-a-capture").unwrap_err().code, code);
            assert_eq!(budget.snapshot().unwrap(), Cost::default());
        }
        assert_eq!(inv.control.stop_cause().unwrap(), None);
    }
}

#[cfg(test)]
#[path = "effective_provider_tests.rs"]
mod provider_tests;
