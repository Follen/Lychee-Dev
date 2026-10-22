//! Query-local physical owners. Tables borrow the callback's external FileBytes;
//! schemas, patch plans and definitions never borrow sibling owner fields.
use super::*;
use crate::resource::{OwnedRetainedReservation, RetainedReservation};
use query_value::{RowCursor, Scalar, Source};
use std::collections::BTreeSet;
#[cfg(test)]
thread_local! {
    pub(super) static WDC_ROWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn binding() -> lycheedev_core::ToolkitError {
    fault(
        "relational.invalid_binding",
        4,
        "Typed provider schema or row is invalid",
    )
}
fn limit() -> lycheedev_core::ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "Typed provider allocation exceeds its bound",
    )
}
fn scalar(value: table::Value) -> Result<Scalar> {
    match value {
        table::Value::Null => Ok(Scalar::Null),
        table::Value::Int(value) => Ok(Scalar::Int(value)),
        table::Value::UInt(value) => Ok(Scalar::UInt(value)),
        table::Value::Float(value) if value.is_finite() => Ok(Scalar::Float(value)),
        table::Value::Float32(value) if value.is_finite() => Ok(Scalar::Float(value as f64)),
        table::Value::String(value) => Ok(Scalar::String(value)),
        _ => Err(fault(
            "relational.type_mismatch",
            4,
            "Typed provider scalar type is invalid",
        )),
    }
}

struct FlatRow<'query> {
    cells: Vec<Scalar>,
    _vector: RetainedReservation<'query>,
    // Keep the consumed row allocation alive through the current-row lease.
    _raw: Vec<table::Value>,
}
/// A row keeps both original refund obligations until its values are dropped.
struct OwnedFlatRow {
    cells: Vec<Scalar>,
    _raw: Vec<table::Value>,
    _vector: OwnedRetainedReservation,
}
impl FlatRow<'_> {
    fn into_owned(self, budget: &Arc<Budget>) -> Result<OwnedFlatRow> {
        Ok(OwnedFlatRow {
            cells: self.cells,
            _raw: self._raw,
            _vector: self._vector.into_owned(budget)?,
        })
    }
}
enum OwnedPhysicalRow {
    Wdc(table::OwnedLeasedRow<OwnedFlatRow>),
    Generation(super::generation_provider::OwnedRow),
}
impl OwnedPhysicalRow {
    fn cells(&self) -> &[Scalar] {
        match self {
            Self::Wdc(row) => &row.value().cells,
            Self::Generation(row) => row.cells(),
        }
    }
}

pub(crate) enum OwnedEquality<C> {
    Empty,
    Rows(C),
}
enum OwnedPhysicalMode {
    Scan { next: usize },
    Equal { pending: Option<u32> },
}
/// The production pull boundary owns its table, budget and at most one row.
/// Peeking uses the authenticated physical index, never decoding the next row.
/// Projection is deliberately left to DataFusion after full visited-row checks.
pub(crate) struct OwnedPhysicalCursor {
    current: Option<OwnedPhysicalRow>,
    current_id: Option<u32>,
    generation: Option<super::generation_provider::OwnedRows>,
    physical: Arc<Physical<'static>>,
    mode: OwnedPhysicalMode,
    done: bool,
    _allocation: OwnedRetainedReservation,
}
impl OwnedPhysicalCursor {
    /// The exact physical locator of the live row, independent of SQL columns.
    pub(crate) fn current_id(&self) -> Option<u32> {
        self.current_id
    }
    pub(crate) fn peek_id(&self) -> Option<u32> {
        if self.done {
            return None;
        }
        match self.mode {
            OwnedPhysicalMode::Scan { next } => self.physical.table.id_at(next),
            OwnedPhysicalMode::Equal { pending } => pending,
        }
    }
    pub(crate) fn clear_row(&mut self) {
        self.current = None;
        self.current_id = None;
    }
    fn advance_inner(&mut self) -> Result<bool> {
        let (id, equality) = match &mut self.mode {
            OwnedPhysicalMode::Scan { next } => {
                self.physical.budget.check()?;
                let Some(id) = self.physical.table.id_at(*next) else {
                    self.done = true;
                    self.generation = None;
                    return Ok(false);
                };
                *next += 1;
                (id, false)
            }
            OwnedPhysicalMode::Equal { pending } => {
                self.done = true;
                let Some(id) = pending.take() else {
                    return Ok(false);
                };
                (id, true)
            }
        };
        if let Some(generation) = &mut self.generation {
            let row = generation.read(id, self.physical.table.columns(), &self.physical.budget)?;
            let Some(row) = row else {
                if equality {
                    return Ok(false);
                }
                return Err(fault(
                    "records.generation_invalid",
                    4,
                    "Typed batch is missing an authenticated physical row",
                ));
            };
            self.current = Some(OwnedPhysicalRow::Generation(row));
        } else {
            #[cfg(test)]
            WDC_ROWS.with(|count| count.set(count.get() + 1));
            let raw = match self.physical.table.leased_row_values(id, 1 << 20) {
                Ok(raw) => raw,
                Err(error) if equality && error.code == "table.record_not_found" => {
                    return Ok(false);
                }
                Err(error) => return Err(error),
            };
            let row = raw
                .try_map(|raw| {
                    self.physical
                        .flatten(raw, None)?
                        .into_owned(&self.physical.budget)
                })?
                .into_owned(&self.physical.budget)?;
            self.current = Some(OwnedPhysicalRow::Wdc(row));
        }
        self.current_id = Some(id);
        Ok(true)
    }
}
impl RowCursor for OwnedPhysicalCursor {
    fn advance(&mut self) -> Result<bool> {
        self.clear_row();
        if self.done {
            self.generation = None;
            return Ok(false);
        }
        let result = self.advance_inner();
        if result.is_err() {
            self.done = true;
            self.generation = None;
        }
        result
    }
    fn row(&self) -> &[Scalar] {
        self.current.as_ref().map_or(&[], OwnedPhysicalRow::cells)
    }
}

pub(crate) struct Physical<'body> {
    pub table: Arc<table::Table<'body>>,
    pub(crate) generation: Option<Arc<crate::generation::Generation>>,
    schema: Source,
    budget: Arc<Budget>,
}
impl<'body> Physical<'body> {
    pub fn new(table: table::Table<'body>, budget: Arc<Budget>) -> Result<Self> {
        let schema = Self::prepare_schema(&table, &budget)?;
        // One stable owner can be shared by static/effective aliases of the
        // exact same prepared bundle without rebinding or rebuilding its index.
        budget.safety_only().charge(Cost::retained(
            size_of::<table::Table<'body>>() + 2 * size_of::<usize>(),
        ))?;
        Ok(Self {
            table: Arc::new(table),
            generation: None,
            schema,
            budget,
        })
    }
    fn prepare_schema(table: &table::Table<'_>, budget: &Budget) -> Result<Source> {
        budget.check()?;
        let mut count = 0usize;
        let mut strings = 0usize;
        for field in table.columns() {
            let width = if field.array {
                field.elements as usize
            } else {
                1
            };
            count = count
                .checked_add(width)
                .filter(|n| *n <= 4096)
                .ok_or_else(|| {
                    fault(
                        "table.limit_exceeded",
                        3,
                        "SQL table scalar column bound exceeded",
                    )
                })?;
            strings = strings
                .checked_add(width.checked_mul(field.name.len() + 12).ok_or_else(limit)?)
                .ok_or_else(limit)?;
        }
        // Fixed Go TableSource ReserveMetadata(count*128), independently from
        // the conservative Rust allocation estimate. No row/index is decoded.
        let logical = count.checked_mul(128).ok_or_else(limit)?;
        let allocation = count
            .checked_mul(size_of::<String>() + 64)
            .and_then(|n| n.checked_add(strings))
            .ok_or_else(limit)?;
        budget.charge_split(Cost::metadata(logical), Cost::metadata(allocation))?;
        let mut columns = Vec::new();
        columns.try_reserve_exact(count).map_err(|_| limit())?;
        let mut identity = false;
        for field in table.columns() {
            if field.identity {
                if identity || field.array {
                    return Err(binding());
                }
                identity = true;
            }
            if field.array {
                for i in 0..field.elements {
                    columns.push(format!("{}[{i}]", field.name));
                }
            } else {
                columns.push(field.name.clone());
            }
        }
        if !identity {
            return Err(binding());
        }
        Ok(Source {
            columns,
            rows: vec![],
            complete: table.is_complete(),
        })
    }
    pub fn schema(&self) -> &Source {
        &self.schema
    }

    fn flatten<'query>(
        &'query self,
        mut raw: Vec<table::Value>,
        selected: Option<&BTreeSet<usize>>,
    ) -> Result<FlatRow<'query>> {
        let bytes = self
            .schema
            .columns
            .len()
            .checked_mul(size_of::<Scalar>())
            .and_then(|n| n.checked_add(64))
            .ok_or_else(limit)?;
        // reserve_retained always has zero logical cost. Borrow the stable
        // owner, not a temporary safety_only view, across cursor suspension.
        let vector = self.budget.reserve_retained(bytes)?;
        let mut row = Vec::new();
        row.try_reserve_exact(self.schema.columns.len())
            .map_err(|_| limit())?;
        let mut offset = 0usize;
        let mut next = 0usize;
        for field in self.table.columns() {
            self.budget.check()?;
            let width = if field.array {
                field.elements as usize
            } else {
                1
            };
            let wanted = |i| selected.is_none_or(|indices| indices.contains(&i));
            if !(offset..offset + width).any(wanted) {
                row.resize(row.len() + width, Scalar::Null);
                offset += width;
                continue;
            }
            let value =
                std::mem::replace(raw.get_mut(next).ok_or_else(binding)?, table::Value::Null);
            next += 1;
            if field.array {
                let table::Value::Array(values) = value else {
                    return Err(binding());
                };
                if values.len() != width {
                    return Err(binding());
                }
                for value in values {
                    row.push(if wanted(offset) {
                        scalar(value)?
                    } else {
                        Scalar::Null
                    });
                    offset += 1;
                }
            } else {
                row.push(scalar(value)?);
                offset += 1;
            }
        }
        if next != raw.len() {
            return Err(binding());
        }
        Ok(FlatRow {
            cells: row,
            _vector: vector,
            _raw: raw,
        })
    }
}

impl Physical<'static> {
    pub(super) fn uses_budget(&self, budget: &Arc<Budget>) -> bool {
        Arc::ptr_eq(&self.budget, budget)
    }

    fn owned_with_mode(self: &Arc<Self>, mode: OwnedPhysicalMode) -> Result<OwnedPhysicalCursor> {
        let allocation = self
            .budget
            .reserve_retained(size_of::<OwnedPhysicalCursor>())?
            .into_owned(&self.budget)?;
        let generation = self
            .generation
            .as_ref()
            .map(|generation| {
                super::generation_provider::OwnedRows::new(generation.clone(), &self.budget)
            })
            .transpose()?;
        Ok(OwnedPhysicalCursor {
            current: None,
            current_id: None,
            generation,
            physical: self.clone(),
            mode,
            done: false,
            _allocation: allocation,
        })
    }
    pub(crate) fn owned_cursor(self: &Arc<Self>) -> Result<OwnedPhysicalCursor> {
        self.budget.check()?;
        if self.table.logical_count() > (4 << 20) / 4 {
            return Err(fault(
                "table.limit_exceeded",
                3,
                "SQL table scan index exceeds its bound",
            ));
        }
        self.owned_with_mode(OwnedPhysicalMode::Scan { next: 0 })
    }
    /// Internal lookup addresses the authenticated physical index directly.
    /// It does not interpret a visible column name or convert a SQL scalar.
    pub(crate) fn owned_id_cursor(self: &Arc<Self>, id: u32) -> Result<OwnedPhysicalCursor> {
        self.owned_with_mode(OwnedPhysicalMode::Equal { pending: Some(id) })
    }
}

// Both borrowed and owned equality paths keep the same finite/type/range rules.
#[cfg(test)]
#[path = "provider_tests.rs"]
mod provider_tests;
