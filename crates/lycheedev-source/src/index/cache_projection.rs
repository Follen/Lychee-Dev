//! Retaining and validation-only choices share the exact binary decoder.
//! A summary hit validates borrowed rows without per-row String/Value owners.
use super::{AssetRecord, Document, Index, Symbol, persistence_wire};
use lycheedev_core::{Result, control::OperationControl, selection::SourcePin};
use serde_json::Value;
use std::{marker::PhantomData, path::Path};

pub(super) trait CollectionLength {
    fn len(&self) -> usize;
}
impl<T> CollectionLength for Vec<T> {
    fn len(&self) -> usize {
        Vec::len(self)
    }
}
pub(super) struct Counted<T> {
    count: usize,
    element: PhantomData<T>,
}
impl<T> Default for Counted<T> {
    fn default() -> Self {
        Self {
            count: 0,
            element: PhantomData,
        }
    }
}
impl<T> Counted<T> {
    pub(super) fn from_count(count: usize) -> Self {
        Self {
            count,
            element: PhantomData,
        }
    }
    pub(super) fn increment(&mut self) {
        self.count += 1;
    }
}
impl<T> CollectionLength for Counted<T> {
    fn len(&self) -> usize {
        self.count
    }
}
impl<T> From<Vec<T>> for Counted<T> {
    fn from(rows: Vec<T>) -> Self {
        Self {
            count: rows.len(),
            element: PhantomData,
        }
    }
}
type SummaryIndex = Index<Counted<Document>, Counted<AssetRecord>, Counted<Symbol>>;

pub(crate) fn build_summary_with_control(
    home: &Path,
    pin: &SourcePin,
    control: &OperationControl,
) -> Result<(Value, Value)> {
    let (index, _): (SummaryIndex, _) =
        super::build_material_projected_with_control(home, pin, control)?;
    let coverage = index.summary.coverage(index.documents.len());
    Ok((serde_json::to_value(index.summary)?, coverage))
}
/// Read-only readiness consumes the same complete authenticated main index,
/// including all symbols, without retaining row owners or rebuilding anything.
pub(crate) fn decode_summary(
    raw: &[u8],
    identity: &str,
    control: &OperationControl,
) -> Result<(super::SourceSummary, usize)> {
    let index: SummaryIndex = persistence_wire::decode(raw, identity, control)?;
    Ok((index.summary, index.documents.len()))
}
#[cfg(test)]
mod tests;
