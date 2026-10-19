//! A field-order row producer bound to one immutable Table. The caller moves
//! in its already-admitted selection vector; no schema names are copied here.
use super::*;

/// Prepared indices cannot be applied to another table: only this borrowed
/// owner is accepted by leased_row, and neither field can be modified outside
/// this module. A row lease borrows the table, not the transient plan.
pub(crate) struct ProjectedRows<'table, 'body> {
    table: &'table Table<'body>,
    selected: Vec<usize>,
}
impl<'table, 'body> ProjectedRows<'table, 'body> {
    pub(super) fn new(table: &'table Table<'body>, selected: Vec<usize>) -> Result<Self> {
        if selected
            .last()
            .is_some_and(|&index| index >= table.definition.fields.len())
            || selected.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return invalid();
        }
        #[cfg(test)]
        BINDINGS.with(|count| count.set(count.get() + 1));
        Ok(Self { table, selected })
    }
    pub(crate) fn leased_row(
        &self,
        id: u32,
        text_bytes: usize,
    ) -> Result<LeasedRow<'table, Vec<Value>>> {
        let mut allocation = RowAllocation::new(self.table.source.budget.as_deref(), true);
        let value = self.table.decode_row(
            id,
            text_bytes,
            RowSelection::Ordinals(&self.selected),
            &mut allocation,
        )?;
        Ok(LeasedRow { value, allocation })
    }
}

pub(super) enum RowSelection<'a> {
    Names(Option<&'a BTreeSet<String>>),
    // Only ProjectedRows constructs this after validating once against its
    // borrowed immutable table. The decoder advances monotonically in schema order.
    Ordinals(&'a [usize]),
}
impl RowSelection<'_> {
    pub(super) fn validate(&self, fields: &[Field]) -> Result<()> {
        if let Self::Names(Some(names)) = self
            && names
                .iter()
                .any(|name| !fields.iter().any(|field| &field.name == name))
        {
            return invalid();
        }
        Ok(())
    }
    pub(super) fn count(&self, fields: usize) -> usize {
        match self {
            Self::Names(None) => fields,
            Self::Names(Some(names)) => names.len(),
            Self::Ordinals(indices) => indices.len(),
        }
    }
    pub(super) fn wanted(&self, index: usize, field: &Field, next: &mut usize) -> bool {
        match self {
            Self::Names(names) => names.is_none_or(|names| names.contains(&field.name)),
            Self::Ordinals(indices) => {
                if indices.get(*next) == Some(&index) {
                    *next += 1;
                    true
                } else {
                    false
                }
            }
        }
    }
}

/// Both representations share the complete decoder and its failure ordering.
/// Preparing a collector happens only after the original row admission, and
/// collecting never performs SQL conversion or introduces a consumer callback.
pub(super) trait RowCollector: Default {
    fn prepare(&mut self, fields: usize) -> Result<()>;
    fn field(&mut self, field: &Field, value: Value);
}
impl RowCollector for BTreeMap<String, Value> {
    fn prepare(&mut self, _: usize) -> Result<()> {
        Ok(())
    }
    fn field(&mut self, field: &Field, value: Value) {
        self.insert(field.name.clone(), value);
    }
}
impl RowCollector for Vec<Value> {
    fn prepare(&mut self, fields: usize) -> Result<()> {
        self.try_reserve_exact(fields)
            .map_err(|_| fault("table.limit_exceeded", 3, "Row allocation failed"))
    }
    fn field(&mut self, _: &Field, value: Value) {
        self.push(value);
    }
}

#[cfg(test)]
thread_local! {
    static BINDINGS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
pub(crate) fn projected_row_bindings() -> usize {
    BINDINGS.with(std::cell::Cell::get)
}

#[cfg(test)]
#[path = "row_projection_tests.rs"]
mod tests;
