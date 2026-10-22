//! A field-order row producer bound to one immutable Table. The caller moves
//! in its already-admitted selection vector; no schema names are copied here.
use super::*;

pub(super) enum RowSelection<'a> {
    Names(Option<&'a BTreeSet<String>>),
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
        }
    }
    pub(super) fn wanted(&self, _index: usize, field: &Field, _next: &mut usize) -> bool {
        match self {
            Self::Names(names) => names.is_none_or(|names| names.contains(&field.name)),
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
#[path = "row_projection_tests.rs"]
mod tests;
