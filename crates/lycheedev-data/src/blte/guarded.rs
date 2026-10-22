//! Private refundable decoded pages for a query-owned prepared reader.
use super::*;
use std::sync::Arc;

impl Ranges<'_> {
    /// Authenticate one decoded page and retain its actual allocation. This
    /// private entry requires the exact Arc used to construct budgeted Ranges;
    /// the caller separately admits Go's cumulative metadata/page cost. Public
    /// ByteSource callers keep their existing Cow and query-lifetime accounting.
    pub(crate) fn read_checked_owned_span(
        &self,
        offset: usize,
        bytes: usize,
        expected: &[u8],
        owner: &Arc<Budget>,
    ) -> Result<OwnedDecodedSpan> {
        self.check_owner(owner)?;
        if let Some(raw) = self.checked_direct_span(offset, bytes, expected, |raw| {
            raw.into_decoded_owned(owner, || self.check())
        })? {
            return Ok(raw);
        }
        let end = self.extent(offset, bytes)?;
        self.owned_result(offset, bytes, end, Some(expected), owner)
    }
    /// Owned range without an independent page checksum. The common decoder
    /// still checks every intersecting framed chunk, exactly like read_span;
    /// untouched bytes and the whole decoded CKey are not authenticated here.
    pub(crate) fn read_owned_span(
        &self,
        offset: usize,
        bytes: usize,
        owner: &Arc<Budget>,
    ) -> Result<OwnedDecodedSpan> {
        self.check_owner(owner)?;
        let end = self.extent(offset, bytes)?;
        self.owned_result(offset, bytes, end, None, owner)
    }
    fn check_owner(&self, owner: &Arc<Budget>) -> Result<()> {
        self.check()?;
        owner.check()?;
        if !self
            .budget
            .is_some_and(|budget| std::ptr::eq(budget, owner.as_ref()))
        {
            return Err(fault(
                "records.internal",
                8,
                "Prepared range budget differs",
            ));
        }
        Ok(())
    }
    fn owned_result(
        &self,
        offset: usize,
        bytes: usize,
        end: usize,
        expected: Option<&[u8]>,
        owner: &Arc<Budget>,
    ) -> Result<OwnedDecodedSpan> {
        let _scratch = if self.session {
            Some(owner.reserve_logical_scratch(bytes as u64)?)
        } else {
            None
        };
        let reservation = owner.reserve_retained(bytes)?;
        // The same chunk/digest/codec implementation as read_span. Its existing
        // conservative decoder costs are deliberately not refunded by this
        // owner. Only the final output reservation travels with the result.
        let raw = self.decode_span(offset, bytes, end)?;
        if expected.is_some_and(|expected| Md5::digest(&raw).as_slice() != expected) {
            return integrity();
        }
        self.check()?;
        owner.check()?;
        OwnedDecodedSpan::from_reserved(raw, reservation, owner)
    }
}

#[cfg(test)]
mod tests;
