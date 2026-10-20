//! Query-local physical owners. Tables borrow the callback's external FileBytes;
//! schemas, patch plans and definitions never borrow sibling owner fields.
use super::*;
use crate::resource::RetainedReservation;
use relational::{
    EqualityCursor, EqualityOutcome, RowCursor, RowProvider, RowYield, Scalar, Source,
};
use std::collections::BTreeSet;
#[cfg(test)]
thread_local! {
    pub(super) static WDC_ROWS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn binding() -> lycheedev_core::ToolkitError {
    fault(
        "relational.invalid_binding",
        4,
        "SQL provider schema or row is invalid",
    )
}
fn limit() -> lycheedev_core::ToolkitError {
    fault(
        "relational.resource_limit",
        3,
        "SQL provider allocation exceeds its bound",
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
            "SQL provider scalar type is invalid",
        )),
    }
}

pub(super) fn emit_rows(
    mut cursor: Box<dyn RowCursor + '_>,
    emit: &mut RowYield<'_>,
) -> Result<()> {
    while cursor.advance()? {
        emit(cursor.row())?;
    }
    Ok(())
}

struct Projection<'query, 'body> {
    indices: BTreeSet<usize>,
    rows: table::ProjectedRows<'query, 'body>,
}
struct FlatRow<'query> {
    cells: Vec<Scalar>,
    _vector: RetainedReservation<'query>,
    // Keep the consumed row allocation alive through the current-row lease.
    _raw: Vec<table::Value>,
}
type PhysicalRow<'query> = table::LeasedRow<'query, FlatRow<'query>>;

struct ScanCursor<'query, 'body> {
    current: Option<PhysicalRow<'query>>,
    physical: &'query Physical<'body>,
    projection: Option<Projection<'query, 'body>>,
    next: usize,
    done: bool,
    _allocation: RetainedReservation<'query>,
}
impl ScanCursor<'_, '_> {
    fn advance_inner(&mut self) -> Result<bool> {
        let Some(id) = self.physical.table.id_at(self.next) else {
            self.done = true;
            // This used to run after the last consumer returned, not before it.
            self.physical.budget.check()?;
            return Ok(false);
        };
        self.physical.budget.check()?;
        self.next += 1;
        #[cfg(test)]
        WDC_ROWS.with(|count| count.set(count.get() + 1));
        let raw = if let Some(projection) = &self.projection {
            projection.rows.leased_row(id, 1 << 20)?
        } else {
            self.physical.table.leased_row_values(id, 1 << 20)?
        };
        self.current = Some(raw.try_map(|raw| {
            self.physical.flatten(
                raw,
                self.projection.as_ref().map(|selected| &selected.indices),
            )
        })?);
        Ok(true)
    }
}
impl RowCursor for ScanCursor<'_, '_> {
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
    fn row(&self) -> &[Scalar] {
        self.current.as_ref().map_or(&[], |row| &row.value().cells)
    }
}
struct EqualCursor<'query, 'body> {
    current: Option<PhysicalRow<'query>>,
    physical: &'query Physical<'body>,
    pending: Option<u32>,
    _allocation: RetainedReservation<'query>,
}
impl RowCursor for EqualCursor<'_, '_> {
    fn advance(&mut self) -> Result<bool> {
        self.current = None;
        let Some(id) = self.pending.take() else {
            // Physical equality has no trailing budget check.
            return Ok(false);
        };
        #[cfg(test)]
        WDC_ROWS.with(|count| count.set(count.get() + 1));
        let raw = match self.physical.table.leased_row_values(id, 1 << 20) {
            Ok(raw) => raw,
            Err(error) if error.code == "table.record_not_found" => return Ok(false),
            Err(error) => return Err(error),
        };
        // Only decoder misses are handled as empty. Flattening and later
        // consumer errors, even record_not_found, remain the original error.
        self.current = Some(raw.try_map(|raw| self.physical.flatten(raw, None))?);
        Ok(true)
    }
    fn row(&self) -> &[Scalar] {
        self.current.as_ref().map_or(&[], |row| &row.value().cells)
    }
}

pub(crate) struct Physical<'body> {
    pub table: Arc<table::Table<'body>>,
    pub(crate) generation: Option<Arc<crate::generation::Generation>>,
    schema: Source,
    identity: usize,
    budget: Arc<Budget>,
}
impl<'body> Physical<'body> {
    pub fn new(table: table::Table<'body>, budget: Arc<Budget>) -> Result<Self> {
        let (schema, identity) = Self::prepare_schema(&table, &budget)?;
        // One stable owner can be shared by static/effective aliases of the
        // exact same prepared bundle without rebinding or rebuilding its index.
        budget.safety_only().charge(Cost::retained(
            size_of::<table::Table<'body>>() + 2 * size_of::<usize>(),
        ))?;
        Ok(Self {
            table: Arc::new(table),
            generation: None,
            schema,
            identity,
            budget,
        })
    }
    pub fn shared(&self) -> Result<Self> {
        let (schema, identity) = Self::prepare_schema(&self.table, &self.budget)?;
        Ok(Self {
            table: self.table.clone(),
            generation: self.generation.clone(),
            schema,
            identity,
            budget: self.budget.clone(),
        })
    }
    pub(super) fn close_generation(&self) -> Result<()> {
        self.generation
            .as_ref()
            .map_or(Ok(()), |generation| generation.close())
    }
    fn prepare_schema(table: &table::Table<'_>, budget: &Budget) -> Result<(Source, usize)> {
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
        let mut identity = None;
        for field in table.columns() {
            if field.identity {
                if identity.is_some() || field.array {
                    return Err(binding());
                }
                identity = Some(columns.len());
            }
            if field.array {
                for i in 0..field.elements {
                    columns.push(format!("{}[{i}]", field.name));
                }
            } else {
                columns.push(field.name.clone());
            }
        }
        Ok((
            Source {
                columns,
                rows: vec![],
                complete: table.is_complete(),
            },
            identity.ok_or_else(binding)?,
        ))
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
    fn projection<'query>(&'query self, columns: &[usize]) -> Result<Projection<'query, 'body>> {
        self.budget.check()?;
        if columns.iter().any(|i| *i >= self.schema.columns.len()) {
            return Err(binding());
        }
        let bytes = columns
            .len()
            .checked_mul(192)
            .and_then(|n| n.checked_add(self.schema.columns.iter().map(String::len).sum::<usize>()))
            .ok_or_else(limit)?;
        // Projection's logical charge counts requested scalar indices, even
        // when several array elements select the same named field.
        self.budget.charge_split(
            Cost::metadata(columns.len().checked_mul(128).ok_or_else(limit)?),
            Cost::metadata(bytes),
        )?;
        let indices: BTreeSet<_> = columns.iter().copied().collect();
        // One ordinal per selected field fits within the original conservative
        // 192 bytes per requested scalar (including duplicate requests). The
        // plan takes this already-admitted Vec without copying or re-admitting.
        let mut fields = Vec::new();
        fields
            .try_reserve_exact(indices.len().min(self.table.columns().len()))
            .map_err(|_| limit())?;
        let mut offset = 0usize;
        for (index, field) in self.table.columns().iter().enumerate() {
            let width = if field.array {
                field.elements as usize
            } else {
                1
            };
            if (offset..offset + width).any(|i| indices.contains(&i)) {
                fields.push(index);
            }
            offset += width;
        }
        Ok(Projection {
            indices,
            rows: self.table.projected_rows(fields)?,
        })
    }
}
impl RowProvider for Physical<'_> {
    fn cursor(&self, selected: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
        let projection = selected
            .map(|columns| self.projection(columns))
            .transpose()?;
        self.budget.check()?;
        // Go's default SQL TableSource uses a 4 MiB sorted-ID scan allowance.
        // Rust reuses its already-admitted sorted IDs but retains this limit.
        if self.table.logical_count() > (4 << 20) / 4 {
            return Err(fault(
                "table.limit_exceeded",
                3,
                "SQL table scan index exceeds its bound",
            ));
        }
        if let Some(generation) = &self.generation {
            return super::generation_provider::Cursor::scan(
                generation,
                self.table.columns(),
                &self.budget,
                projection.map(|projection| projection.indices),
            );
        }
        let allocation = self
            .budget
            .reserve_retained(size_of::<ScanCursor<'_, '_>>())?;
        Ok(Box::new(ScanCursor {
            current: None,
            physical: self,
            projection,
            next: 0,
            done: false,
            _allocation: allocation,
        }))
    }
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        emit_rows(self.cursor(None)?, emit)
    }
    fn scan_selected(&self, columns: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
        emit_rows(self.cursor(Some(columns))?, emit)
    }
    fn has_equal(&self) -> bool {
        true
    }
    fn equal_cursor(&self, column: &str, value: &Scalar) -> Result<EqualityCursor<'_>> {
        // Match Go's callback order. The evaluator checks original control
        // while evaluating the candidate; wrong-column/NULL/type branches do
        // not perform a speculative table read or replace their first error.
        if !crate::hotfix::remote::equal_fold(column, &self.schema.columns[self.identity]) {
            return Ok(EqualityCursor::Declined);
        }
        let id = match value {
            Scalar::Null => return Ok(EqualityCursor::Empty),
            Scalar::Int(n) => match u32::try_from(*n) {
                Ok(n) => n,
                Err(_) => return Ok(EqualityCursor::Empty),
            },
            Scalar::UInt(n) => match u32::try_from(*n) {
                Ok(n) => n,
                Err(_) => return Ok(EqualityCursor::Empty),
            },
            Scalar::Float(n) if !n.is_finite() => {
                return Err(fault(
                    "relational.numeric_range",
                    4,
                    "SQL number exceeds supported finite 64-bit range",
                ));
            }
            Scalar::Float(n) if *n < 0.0 || *n > u32::MAX as f64 || n.trunc() != *n => {
                return Ok(EqualityCursor::Empty);
            }
            Scalar::Float(n) => *n as u32,
            _ => {
                return Err(fault(
                    "relational.type_mismatch",
                    4,
                    "SQL scalar types are incompatible",
                ));
            }
        };
        if let Some(generation) = &self.generation {
            return Ok(EqualityCursor::Rows(
                super::generation_provider::Cursor::equal(
                    generation,
                    self.table.columns(),
                    &self.budget,
                    id,
                )?,
            ));
        }
        let allocation = self
            .budget
            .reserve_retained(size_of::<EqualCursor<'_, '_>>())?;
        Ok(EqualityCursor::Rows(Box::new(EqualCursor {
            current: None,
            physical: self,
            pending: Some(id),
            _allocation: allocation,
        })))
    }

    fn equal(
        &self,
        column: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        match self.equal_cursor(column, value)? {
            EqualityCursor::Declined => Ok(EqualityOutcome::Declined),
            EqualityCursor::Empty => Ok(EqualityOutcome::Handled),
            EqualityCursor::Rows(cursor) => {
                emit_rows(cursor, emit)?;
                Ok(EqualityOutcome::Handled)
            }
        }
    }
}

pub(super) struct PreparedInput<'body> {
    pub name: String,
    pub base: Physical<'body>,
    pub overlay: Option<effective::OverlayPlan>,
    pub definition: Arc<schema::Definition>,
    pub bundle: definitions::Bundle,
    pub layout_hash: u32,
}
impl PreparedInput<'_> {
    pub fn schema(&self) -> &Source {
        self.overlay
            .as_ref()
            .map_or_else(|| self.base.schema(), |plan| plan.schema())
    }
}
impl RowProvider for PreparedInput<'_> {
    fn cursor(&self, selected: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
        if let Some(plan) = &self.overlay {
            // Effective Scan deliberately ignores projection, including its
            // validation/metadata charge, just like the original adapter.
            plan.cursor(&self.base, &self.base.budget)
        } else {
            self.base.cursor(selected)
        }
    }
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        if let Some(plan) = &self.overlay {
            plan.scan(&self.base, &self.base.budget, emit)
        } else {
            self.base.scan(emit)
        }
    }
    fn scan_selected(&self, columns: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
        if self.overlay.is_some() {
            self.scan(emit)
        } else {
            self.base.scan_selected(columns, emit)
        }
    }
    fn has_equal(&self) -> bool {
        self.base.has_equal()
    }
    fn equal_cursor(&self, column: &str, value: &Scalar) -> Result<EqualityCursor<'_>> {
        if let Some(plan) = &self.overlay {
            plan.equal_cursor(&self.base, &self.base.budget, column, value)
        } else {
            self.base.equal_cursor(column, value)
        }
    }
    fn equal(
        &self,
        column: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        if let Some(plan) = &self.overlay {
            plan.equal(&self.base, &self.base.budget, column, value, emit)
        } else {
            self.base.equal(column, value, emit)
        }
    }
}

#[cfg(test)]
#[path = "provider_tests.rs"]
mod provider_tests;

#[cfg(test)]
#[path = "provider_projection_tests.rs"]
mod projection_tests;
