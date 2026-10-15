//! Temporary encoded allocations travel with their refundable safety admission.
use super::*;
use crate::resource::{OwnedRetainedReservation, RetainedReservation};
use std::{ops::Deref, sync::Arc};

/// Immutable bytes returned by an encoded reader. Owned allocations cannot be
/// detached from their admission; dropping the span refunds only that temporary
/// safety charge. Observed I/O and work are never refunded.
pub struct EncodedSpan<'a> {
    // Declaration order is intentional: bytes are released before the guard.
    bytes: Cow<'a, [u8]>,
    _reservation: Option<RetainedReservation<'a>>,
    budget: Option<&'a Budget>,
}
impl<'a> EncodedSpan<'a> {
    pub fn borrowed(bytes: &'a [u8]) -> Self {
        Self {
            bytes: Cow::Borrowed(bytes),
            _reservation: None,
            budget: None,
        }
    }
    /// Reserve before allocating, then fill caller-owned bounded storage. A
    /// failed fill or post-read stop drops the allocation and its reservation.
    pub fn read_owned(
        bytes: usize,
        budget: Option<&'a Budget>,
        fill: impl FnOnce(&mut [u8]) -> Result<()>,
    ) -> Result<Self> {
        let reservation = budget.map(|b| b.reserve_retained(bytes)).transpose()?;
        let mut raw = vec![0; bytes];
        fill(&mut raw)?;
        if let Some(budget) = budget {
            budget.check()?;
        }
        Ok(Self {
            bytes: Cow::Owned(raw),
            _reservation: reservation,
            budget,
        })
    }
    fn check(&self, range_budget: Option<&Budget>) -> Result<()> {
        if let Some(budget) = range_budget {
            budget.check()?;
        }
        if let Some(budget) = self.budget {
            budget.check()?;
        }
        Ok(())
    }
    /// Private prepared metadata can keep the original allocation instead of
    /// crossing ByteSource's legacy Cow boundary. A charged input must retain
    /// its exact Arc-backed ledger; a view sharing state is not that owner.
    pub(super) fn into_decoded_owned(
        self,
        owner: &Arc<Budget>,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<OwnedDecodedSpan> {
        check()?;
        self.check(Some(owner))?;
        if self
            .budget
            .is_some_and(|origin| !std::ptr::eq(origin, owner.as_ref()))
        {
            return Err(fault(
                "records.internal",
                8,
                "Decoded span owner budget differs",
            ));
        }
        match self.bytes {
            Cow::Owned(bytes) => {
                // N's logical page is already exactly this allocation. Transfer
                // its refund obligation without charging or copying it twice.
                let reservation = match self._reservation {
                    Some(reservation) => reservation,
                    // A legitimate unbudgeted reader already allocated these
                    // bytes. Admit ownership now; this cannot retroactively
                    // account for its earlier allocation/read. Prepared Files
                    // supplies Some(exact Budget) and never takes this branch.
                    None => owner.reserve_retained(bytes.len())?,
                };
                let owned = OwnedDecodedSpan::from_reserved(bytes, reservation, owner)?;
                check()?;
                owner.check()?;
                Ok(owned)
            }
            Cow::Borrowed(raw) => {
                // Borrowed input cannot outlive its source. Admit the real copy
                // while the original source remains live and check every block.
                let reservation = owner.reserve_retained(raw.len())?;
                let mut bytes = Vec::with_capacity(raw.len());
                for block in raw.chunks(65536) {
                    check()?;
                    owner.check()?;
                    bytes.extend_from_slice(block);
                }
                check()?;
                owner.check()?;
                OwnedDecodedSpan::from_reserved(bytes, reservation, owner)
            }
        }
    }
    /// ByteSource still returns Cow. Preserve true borrowed zero-copy, but make
    /// an independently admitted copy of owned input. Never detach encoded Vec.
    /// The originating ledger owns the output when present; an unbudgeted input
    /// uses the Ranges ledger. Both independent controls remain effective.
    pub(super) fn decoded_copy(
        self,
        range_budget: Option<&Budget>,
        mut check: impl FnMut() -> Result<()>,
    ) -> Result<Cow<'a, [u8]>> {
        check()?;
        self.check(range_budget)?;
        if let Cow::Borrowed(bytes) = &self.bytes {
            return Ok(Cow::Borrowed(*bytes));
        }
        let budget = self.budget.or(range_budget);
        let output = budget.map(|b| b.reserve_retained(self.len())).transpose()?;
        let mut bytes = Vec::with_capacity(self.len());
        for chunk in self.chunks(65536) {
            check()?;
            self.check(range_budget)?;
            bytes.extend_from_slice(chunk);
        }
        check()?;
        self.check(range_budget)?;
        // Existing ByteSource outputs are conservatively retained until query
        // end. Only this newly copied output is committed; encoded storage and
        // its guard stay alive until this function returns and then drop.
        if let Some(output) = output {
            output.commit();
        }
        Ok(Cow::Owned(bytes))
    }
}
impl Deref for EncodedSpan<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}
impl AsRef<[u8]> for EncodedSpan<'_> {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

/// One private prepared page/directory allocation and its exact safety owner.
/// No Clone/into-Vec/mutable access: eviction and drop refund storage once, after
/// the bytes are released. Logical metadata and observed work remain charged.
pub(crate) struct OwnedDecodedSpan {
    bytes: Vec<u8>,
    _reservation: OwnedRetainedReservation,
}
impl OwnedDecodedSpan {
    pub(super) fn from_reserved(
        bytes: Vec<u8>,
        reservation: RetainedReservation<'_>,
        owner: &Arc<Budget>,
    ) -> Result<Self> {
        Ok(Self {
            bytes,
            _reservation: reservation.into_owned(owner)?,
        })
    }
}
impl Deref for OwnedDecodedSpan {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}
impl AsRef<[u8]> for OwnedDecodedSpan {
    fn as_ref(&self) -> &[u8] {
        &self.bytes
    }
}

#[cfg(test)]
mod tests;
