//! Retained effective/hotfix-snapshot overlay semantics. One SQL invocation owns its
//! authenticated immutable snapshots; an overlay never changes a static table.
use super::*;
use crate::hotfix::{self, CacheEntry};
use crate::resource::{OwnedRetainedReservation, RetainedReservation};
use query_value::RowCursor;
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
pub(crate) struct SharedCapture {
    value: Arc<Value>,
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
        let bytes = value_bytes(&value, budget)?;
        budget.charge(Cost::retained(
            bytes.saturating_add(2 * size_of::<usize>()),
        ))?;
        Ok(Self {
            value: Arc::new(value),
        })
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
pub(crate) struct Report {
    pub table: String,
    pub policy: &'static str,
    pub captures: Vec<SharedCapture>,
    pub replaced_or_added: usize,
    pub deleted: usize,
}
struct Patch {
    values: Option<Vec<query_value::Scalar>>,
    ordinal: usize,
    entry: CacheEntry,
}

/// Prepared independently of the base provider and the mutable capture cache.
/// All selected patches are validated here, including for schema-only queries.
pub(crate) struct OverlayPlan {
    patches: BTreeMap<u32, Patch>,
    capture_ids: Vec<String>,
}

pub(crate) struct Snapshots<'a> {
    context: &'a crate::request::DataContext,
    pin: &'a selection::DataPin,
    budget: &'a Budget,
    captures: BTreeMap<String, Snapshot>,
    bytes: usize,
}
impl<'a> Snapshots<'a> {
    pub fn new(context: &'a crate::request::DataContext, pin: &'a selection::DataPin, budget: &'a Budget) -> Self {
        Self {
            context,
            pin,
            budget,
            captures: BTreeMap::new(),
            bytes: 0,
        }
    }
    fn check(&self) -> Result<()> {
        self.context.control.check()?;
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
                &self.context.home,
                id,
                &self.context.control,
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
            &self.context.home,
            source_value["provenance"]["snapshot"]
                .as_str()
                .unwrap_or(""),
            &self.context.control,
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
            &self.context.home,
            &source_value["blob"],
            128 << 20,
            &self.context.control,
            || self.budget.check(),
        )?;
        let (_, layout, start) =
            hotfix::cache_layout_with_check(&raw, build, Some(self.budget), || {
                self.context.control.check()
            })?;
        let mut tables: BTreeMap<u32, Vec<CacheEntry>> = BTreeMap::new();
        hotfix::walk_with_check(
            &raw,
            start,
            layout,
            Some(self.budget),
            || self.context.control.check(),
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
    pub(crate) fn prepare(
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
        // Capture IDs are retained as owned strings; base columns are addressed
        // through the authenticated table provider instead of a copied schema.
        self.budget
            .charge(Cost::retained(size_of::<String>() + 128))?;
        let copied_strings = ids.iter().try_fold(
            ids.len()
                .checked_mul(size_of::<String>())
                .ok_or_else(limit)?,
            |bytes, text| bytes.checked_add(text.len()).ok_or_else(limit),
        )?;
        self.budget
            .safety_only()
            .charge(Cost::retained(copied_strings))?;
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
                patches,
                capture_ids,
            },
            report,
        ))
    }
}

struct AugmentedRow<'query> {
    cells: Vec<query_value::Scalar>,
    _allocation: RetainedReservation<'query>,
}

struct OwnedAugmentedRow {
    cells: Vec<query_value::Scalar>,
    _allocation: OwnedRetainedReservation,
}
impl AugmentedRow<'_> {
    fn into_owned(self, budget: &Arc<Budget>) -> Result<OwnedAugmentedRow> {
        Ok(OwnedAugmentedRow {
            cells: self.cells,
            _allocation: self._allocation.into_owned(budget)?,
        })
    }
}
enum OwnedOverlayMode {
    Merge { last_patch_id: Option<u32> },
    EqualPatch { pending: Option<u32> },
    EqualBase,
}
/// Globally ascending physical IDs, merged without retaining visited-ID sets
/// or decoding a base lookahead. The plan was fully authenticated/prepared
/// before this cursor exists and never borrows Snapshots or DataContext.
pub(crate) struct OwnedOverlayCursor {
    current: Option<OwnedAugmentedRow>,
    current_id: Option<u32>,
    // Keep a visited base row's original lease through its augmented consumer.
    base: Option<provider::OwnedPhysicalCursor>,
    plan: Arc<OverlayPlan>,
    budget: Arc<Budget>,
    mode: OwnedOverlayMode,
    done: bool,
    _allocation: OwnedRetainedReservation,
}
impl OwnedOverlayCursor {
    /// Original base locator or selected patch key, never a public SQL scalar.
    pub(crate) fn current_id(&self) -> Option<u32> {
        self.current_id
    }
    fn next_patch_id(&self) -> Option<u32> {
        use std::ops::Bound::{Excluded, Unbounded};
        let OwnedOverlayMode::Merge { last_patch_id } = self.mode else {
            return None;
        };
        self.plan
            .patches
            .range((last_patch_id.map_or(Unbounded, Excluded), Unbounded))
            .next()
            .map(|(&id, _)| id)
    }
    fn patch_row(&mut self, id: u32) -> Result<bool> {
        let patch = self.plan.patches.get(&id).ok_or_else(fields)?;
        let Some(values) = &patch.values else {
            return Ok(false);
        };
        self.current = Some(
            self.plan
                .augment(values, Some(patch), &self.budget)?
                .into_owned(&self.budget)?,
        );
        self.current_id = Some(id);
        Ok(true)
    }
    fn advance_inner(&mut self) -> Result<bool> {
        match &mut self.mode {
            OwnedOverlayMode::EqualPatch { pending } => {
                let id = pending.take();
                self.done = true;
                return id.map_or(Ok(false), |id| self.patch_row(id));
            }
            OwnedOverlayMode::EqualBase => {
                self.done = true;
                let Some(base) = &mut self.base else {
                    return Ok(false);
                };
                if !base.advance()? {
                    return Ok(false);
                }
                let id = base.current_id().ok_or_else(fields)?;
                self.current = Some(
                    self.plan
                        .augment(base.row(), None, &self.budget)?
                        .into_owned(&self.budget)?,
                );
                self.current_id = Some(id);
                return Ok(true);
            }
            OwnedOverlayMode::Merge { .. } => {}
        }
        loop {
            self.budget.check()?;
            let base_id = self
                .base
                .as_ref()
                .and_then(provider::OwnedPhysicalCursor::peek_id);
            // Finish the base's terminal check without decoding any further row.
            if base_id.is_none()
                && let Some(base) = &mut self.base
            {
                if base.advance()? {
                    return Err(fields());
                }
                self.base = None;
            }
            let patch_id = self.next_patch_id();
            match (base_id, patch_id) {
                (None, None) => {
                    self.done = true;
                    return Ok(false);
                }
                (None, Some(id)) | (Some(_), Some(id))
                    if base_id.is_none_or(|base_id| id < base_id) =>
                {
                    self.mode = OwnedOverlayMode::Merge {
                        last_patch_id: Some(id),
                    };
                    if self.patch_row(id)? {
                        return Ok(true);
                    }
                }
                (Some(id), patch_id) => {
                    let base = self.base.as_mut().ok_or_else(fields)?;
                    if !base.advance()? {
                        return Err(fields());
                    }
                    // Replacement/deletion never suppresses malformed visited
                    // base fields. This is intentionally unlike point equality.
                    let row = base.row();
                    if base.current_id() != Some(id) {
                        return Err(fields());
                    }
                    if patch_id == Some(id) {
                        self.mode = OwnedOverlayMode::Merge {
                            last_patch_id: Some(id),
                        };
                        if self.patch_row(id)? {
                            return Ok(true);
                        }
                        self.base.as_mut().ok_or_else(fields)?.clear_row();
                    } else {
                        self.current = Some(
                            self.plan
                                .augment(row, None, &self.budget)?
                                .into_owned(&self.budget)?,
                        );
                        self.current_id = Some(id);
                        return Ok(true);
                    }
                }
                (None, Some(_)) => unreachable!("patch-only branch handled above"),
            }
        }
    }
}
impl RowCursor for OwnedOverlayCursor {
    fn advance(&mut self) -> Result<bool> {
        self.current = None;
        self.current_id = None;
        if let Some(base) = &mut self.base {
            base.clear_row();
        }
        if self.done {
            self.base = None;
            return Ok(false);
        }
        let result = self.advance_inner();
        if result.is_err() {
            self.done = true;
            self.base = None;
        }
        result
    }
    fn row(&self) -> &[query_value::Scalar] {
        self.current
            .as_ref()
            .map_or(&[], |row| row.cells.as_slice())
    }
}

impl OverlayPlan {
    fn owned_with_mode(
        self: &Arc<Self>,
        base: Option<provider::OwnedPhysicalCursor>,
        budget: Arc<Budget>,
        mode: OwnedOverlayMode,
    ) -> Result<OwnedOverlayCursor> {
        let allocation = budget
            .reserve_retained(size_of::<OwnedOverlayCursor>())?
            .into_owned(&budget)?;
        Ok(OwnedOverlayCursor {
            current: None,
            current_id: None,
            base,
            plan: self.clone(),
            budget,
            mode,
            done: false,
            _allocation: allocation,
        })
    }
    pub(crate) fn owned_cursor(
        self: &Arc<Self>,
        base: Arc<provider::Physical<'static>>,
        budget: Arc<Budget>,
    ) -> Result<OwnedOverlayCursor> {
        if !base.uses_budget(&budget) {
            return Err(fault(
                "records.internal",
                8,
                "Overlay budget differs from its source",
            ));
        }
        self.owned_with_mode(
            Some(base.owned_cursor()?),
            budget,
            OwnedOverlayMode::Merge {
                last_patch_id: None,
            },
        )
    }
    /// Physical-ID lookup preserves patch-first semantics without consulting
    /// the SQL-visible identity column or synthesizing a Scalar from the key.
    pub(crate) fn owned_id_cursor(
        self: &Arc<Self>,
        base: Arc<provider::Physical<'static>>,
        budget: Arc<Budget>,
        id: u32,
    ) -> Result<provider::OwnedEquality<OwnedOverlayCursor>> {
        use provider::OwnedEquality;
        if !base.uses_budget(&budget) {
            return Err(fault(
                "records.internal",
                8,
                "Overlay budget differs from its source",
            ));
        }
        if let Some(patch) = self.patches.get(&id) {
            if patch.values.is_none() {
                return Ok(OwnedEquality::Empty);
            }
            return self
                .owned_with_mode(
                    None,
                    budget,
                    OwnedOverlayMode::EqualPatch { pending: Some(id) },
                )
                .map(OwnedEquality::Rows);
        }
        self.owned_with_mode(
            Some(base.owned_id_cursor(id)?),
            budget,
            OwnedOverlayMode::EqualBase,
        )
        .map(OwnedEquality::Rows)
    }
    pub(crate) fn patch_count(&self) -> usize {
        self.patches.len()
    }
    pub(crate) fn deleted_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.patches
            .iter()
            .filter_map(|(id, patch)| patch.values.is_none().then_some(*id))
    }
    fn augment<'query>(
        &self,
        row: &[query_value::Scalar],
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
                query_value::Scalar::String(clone_text(source.ok_or_else(fields)?)?),
                query_value::Scalar::Int(patch.entry.push as i64),
                query_value::Scalar::UInt(patch.entry.index as u64),
            ]);
        } else {
            out.extend([
                query_value::Scalar::Null,
                query_value::Scalar::Null,
                query_value::Scalar::Null,
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
    row: &[query_value::Scalar],
    extra_cells: usize,
    extra_bytes: usize,
) -> Result<usize> {
    row.iter().try_fold(
        row.len()
            .checked_add(extra_cells)
            .and_then(|count| count.checked_mul(size_of::<query_value::Scalar>()))
            .and_then(|bytes| bytes.checked_add(extra_bytes))
            .ok_or_else(limit)?,
        |bytes, value| {
            bytes
                .checked_add(match value {
                    query_value::Scalar::String(text) => text.len(),
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
fn clone_row(row: &[query_value::Scalar], capacity: usize) -> Result<Vec<query_value::Scalar>> {
    let mut copy = Vec::new();
    copy.try_reserve_exact(capacity).map_err(|_| limit())?;
    for value in row {
        copy.push(match value {
            query_value::Scalar::String(text) => query_value::Scalar::String(clone_text(text)?),
            value => value.clone(),
        });
    }
    Ok(copy)
}

fn flatten(columns: &[String], row: &BTreeMap<String, Value>) -> Result<Vec<query_value::Scalar>> {
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
            query_value::from_json(value.clone()).map_err(|_| fields())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effective_flatten_preserves_integer_array_boundaries() {
        let row = BTreeMap::from([("ID".into(), json!(u64::MAX)), ("A".into(), json!([1, 2]))]);
        assert_eq!(
            flatten(&["ID".into(), "A[1]".into()], &row).unwrap(),
            [
                query_value::Scalar::UInt(u64::MAX),
                query_value::Scalar::Int(2)
            ]
        );
        for name in ["Missing", "A[2]", "A[-1]", "A[wrong]", "ID[0]"] {
            assert!(flatten(&[name.into()], &row).is_err());
        }
    }
    fn context() -> crate::request::DataContext {
        crate::request::DataContext {
            home: "must-not-open-effective-unit".into(),
            cwd: ".".into(),
            control: Default::default(),
            source: Default::default(),
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
    fn effective_cached_source_checks_original_context_before_independent_budget() {
        let mut inv = context();
        let pin = selection::DataPin::default();
        let control = lycheedev_core::control::OperationControl::uncancelable(
            Instant::now() + Duration::from_secs(30),
        );
        inv.control = control.clone();
        let cancelled = lycheedev_core::control::TrackedCancellation::new();
        let budget = Budget::new(
            Cost::defaults(),
            control.deadline(),
            Some(cancelled.clone()),
        );
        let mut snapshots = Snapshots::new(&inv, &pin, &budget);
        seed(&mut snapshots, Vec::new(), Vec::new());
        let before = budget.snapshot().unwrap();
        control.tracked_cancel_handle().unwrap().cancel();
        cancelled.cancel();
        assert_eq!(
            snapshots.load("CAP-fixture").unwrap_err().code,
            "command.cancelled"
        );
        assert_eq!(budget.snapshot().unwrap(), before);
    }
    #[test]
    fn effective_uncached_source_checks_independent_budget_before_historical_io() {
        let inv = context();
        let pin = selection::DataPin::default();
        let cancelled_token = lycheedev_core::control::TrackedCancellation::new();
        cancelled_token.cancel();
        let cancelled = Budget::new(
            Cost::defaults(),
            inv.control.deadline(),
            Some(cancelled_token),
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
#[path = "owned_overlay_tests.rs"]
mod owned_overlay_tests;

impl OverlayPlan {
    pub(crate) fn update_physical_ids(
        &self,
        ids: &mut BTreeSet<u32>,
        budget: &Budget,
    ) -> Result<()> {
        for (&id, patch) in &self.patches {
            budget.charge(crate::resource::Cost::work(1))?;
            if patch.values.is_some() {
                ids.insert(id);
            } else {
                ids.remove(&id);
            }
        }
        Ok(())
    }
}
