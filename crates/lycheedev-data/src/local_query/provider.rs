//! Query-local physical owners. Tables borrow the callback's external FileBytes;
//! schemas, patch plans and definitions never borrow sibling owner fields.
use super::*;
use relational::{EqualityOutcome, RowProvider, RowYield, Scalar, Source};
use std::collections::BTreeSet;

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

pub(super) struct Physical<'body> {
    pub table: Arc<table::Table<'body>>,
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
            schema,
            identity,
            budget,
        })
    }
    pub fn shared(&self) -> Result<Self> {
        let (schema, identity) = Self::prepare_schema(&self.table, &self.budget)?;
        Ok(Self {
            table: self.table.clone(),
            schema,
            identity,
            budget: self.budget.clone(),
        })
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

    fn flatten(
        &self,
        mut raw: BTreeMap<String, table::Value>,
        selected: Option<&BTreeSet<usize>>,
        emit: &mut RowYield<'_>,
    ) -> Result<()> {
        let safety = self.budget.safety_only();
        let bytes = self
            .schema
            .columns
            .len()
            .checked_mul(size_of::<Scalar>())
            .and_then(|n| n.checked_add(64))
            .ok_or_else(limit)?;
        let _scratch = safety.reserve_retained(bytes)?;
        let mut row = Vec::new();
        row.try_reserve_exact(self.schema.columns.len())
            .map_err(|_| limit())?;
        let mut offset = 0usize;
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
            let value = raw.remove(&field.name).ok_or_else(binding)?;
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
        emit(&row)
    }
    fn scan_mode(
        &self,
        selected: Option<(&BTreeSet<usize>, &BTreeSet<String>)>,
        emit: &mut RowYield<'_>,
    ) -> Result<()> {
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
        for id in self.table.ids() {
            self.budget.check()?;
            self.table.with_row_projected(
                id,
                1 << 20,
                selected.map(|(_, names)| names),
                |raw| self.flatten(raw, selected.map(|(indices, _)| indices), emit),
            )?;
        }
        self.budget.check()
    }
}
impl RowProvider for Physical<'_> {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        self.scan_mode(None, emit)
    }
    fn scan_selected(&self, columns: &[usize], emit: &mut RowYield<'_>) -> Result<()> {
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
        let mut names = BTreeSet::new();
        let mut offset = 0usize;
        for field in self.table.columns() {
            let width = if field.array {
                field.elements as usize
            } else {
                1
            };
            if (offset..offset + width).any(|i| indices.contains(&i)) {
                names.insert(field.name.clone());
            }
            offset += width;
        }
        self.scan_mode(Some((&indices, &names)), emit)
    }
    fn has_equal(&self) -> bool {
        true
    }
    fn equal(
        &self,
        column: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        // Match Go's callback order. The evaluator checks original control
        // while evaluating the candidate; wrong-column/NULL/type branches do
        // not perform a speculative table read or replace their first error.
        if !crate::hotfix::remote::equal_fold(column, &self.schema.columns[self.identity]) {
            return Ok(EqualityOutcome::Declined);
        }
        let id = match value {
            Scalar::Null => return Ok(EqualityOutcome::Handled),
            Scalar::Int(n) => match u32::try_from(*n) {
                Ok(n) => n,
                Err(_) => return Ok(EqualityOutcome::Handled),
            },
            Scalar::UInt(n) => match u32::try_from(*n) {
                Ok(n) => n,
                Err(_) => return Ok(EqualityOutcome::Handled),
            },
            Scalar::Float(n) if !n.is_finite() => {
                return Err(fault(
                    "relational.numeric_range",
                    4,
                    "SQL number exceeds supported finite 64-bit range",
                ));
            }
            Scalar::Float(n) if *n < 0.0 || *n > u32::MAX as f64 || n.trunc() != *n => {
                return Ok(EqualityOutcome::Handled);
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
        let mut decoded = false;
        match self.table.with_row_projected(id, 1 << 20, None, |row| {
            decoded = true;
            self.flatten(row, None, emit)
        }) {
            Ok(()) => {}
            Err(error) if !decoded && error.code == "table.record_not_found" => {
                return Ok(EqualityOutcome::Handled);
            }
            Err(error) => return Err(error),
        }
        Ok(EqualityOutcome::Handled)
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
