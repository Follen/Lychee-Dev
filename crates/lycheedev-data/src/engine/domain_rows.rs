//! Domain adapters borrow native values and authenticated physical locators.
//! The locator is never reconstructed from a visible field named ID.
use super::{ResultOwner, OwnedProvenance};
use crate::resource::OwnedRetainedReservation;
use arrow_array::{Array, RecordBatch, UInt32Array};
use arrow_schema::SchemaRef;
use lycheedev_core::{Result, control::OperationControl};
pub(crate) struct OwnedDomainRows {
    pub(super) result: ResultOwner,
    pub(super) schema: Option<SchemaRef>,
    pub(super) ids: Vec<UInt32Array>,
    pub(super) unresolved: Vec<u32>,
    pub(super) extra: bool,
    pub(super) locator_owner: Option<OwnedRetainedReservation>,
}
impl OwnedDomainRows {
    pub(crate) fn row_count(&self) -> usize { self.result.row_count() }
    pub(crate) fn complete(&self) -> bool { self.result.complete() }
    pub(crate) fn provenance(&self) -> &serde_json::Value { self.result.provenance() }
    pub(crate) fn owned_provenance(&self) -> Result<OwnedProvenance> { self.result.owned_provenance() }
    pub(crate) fn unresolved_ids(&self) -> &[u32] { &self.unresolved }
    pub(crate) fn extra(&self) -> bool { self.extra }
    pub(crate) fn visit_records<T>(&self, control: &OperationControl,
        visitor: impl FnOnce(&SchemaRef, &[RecordBatch], &[UInt32Array]) -> Result<T>) -> Result<T> {
        control.check()?;
        let schema=self.schema.as_ref().ok_or_else(||super::error(super::unsupported("Physical locator schema missing")))?;
        if self.result.batches.len() != self.ids.len() { return Err(super::error(super::unsupported("Physical locator batches differ"))); }
        for (batch, ids) in self.result.batches.iter().zip(&self.ids) {
            control.check()?;
            if batch.schema().as_ref() != schema.as_ref() || batch.num_rows() != ids.len() || ids.null_count() != 0 {
                return Err(super::error(super::unsupported("Physical locator rows differ")));
            }
        }
        let value = visitor(schema, &self.result.batches, &self.ids)?;
        control.check()?;
        Ok(value)
    }
}
impl Drop for OwnedDomainRows {
    fn drop(&mut self) {
        // Buffers before their refund, then ResultOwner reconciles shared lifetimes.
        drop(std::mem::take(&mut self.ids)); drop(std::mem::take(&mut self.unresolved));
        self.schema.take();
        self.locator_owner.take();
    }
}
