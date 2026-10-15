//! Owned immutable source and parsed directory; temporary views never point
//! into sibling fields after their operation returns.
use super::*;
use std::sync::Arc;

struct Directory {
    parts: Vec<Segment>,
    size: usize,
    encoded_bytes: usize,
    limits: Limits,
}
/// A private prepared reader owns the exact source authenticated by its caller.
/// There is deliberately no constructor from detached geometry or rebind method.
/// The public Ranges constructors remain independent uncached parsers.
pub(crate) struct PreparedRanges<S: EncodedRead> {
    directory: Directory,
    source: S,
    _allocation: crate::resource::OwnedRetainedReservation,
    budget: Arc<Budget>,
}
impl<S: EncodedRead> PreparedRanges<S> {
    pub(crate) fn open(source: S, limits: Limits, budget: Arc<Budget>) -> Result<Self> {
        source.check_encoded()?;
        budget.check()?;
        let encoded_bytes = source.encoded_size();
        let keys = BTreeMap::new();
        let Ranges {
            parts,
            size,
            parts_allocation,
            ..
        } = Ranges::from_policy(
            RangeInput::Reader(&source),
            limits,
            &keys,
            Some(&budget),
            true,
        )?;
        let allocation = parts_allocation
            .expect("prepared layout allocation")
            .into_owned(&budget)?;
        let RangeParts::Owned(parts) = parts else {
            return Err(fault(
                "records.internal",
                8,
                "Fresh range directory is borrowed",
            ));
        };
        // Drop every ephemeral input/key reference before storing the source.
        source.check_encoded()?;
        budget.check()?;
        if source.encoded_size() != encoded_bytes {
            return malformed();
        }
        Ok(Self {
            directory: Directory {
                parts,
                size,
                encoded_bytes,
                limits,
            },
            source,
            _allocation: allocation,
            budget,
        })
    }
    pub(crate) fn check(&self) -> Result<()> {
        self.source.check_encoded()?;
        self.budget.check()?;
        if self.source.encoded_size() != self.directory.encoded_bytes {
            return malformed();
        }
        Ok(())
    }
    pub(crate) fn size(&self) -> usize {
        self.directory.size
    }
    /// The current immutable key snapshot is borrowed only for this view. A
    /// later successful public-key recovery can replace the snapshot after all
    /// views drop, without rereading or copying authenticated directory bytes.
    pub(crate) fn view<'s>(&'s self, keys: &'s BTreeMap<u64, Vec<u8>>) -> Result<Ranges<'s>> {
        self.check()?;
        Ok(Ranges {
            raw: RangeInput::Reader(&self.source),
            parts: RangeParts::Borrowed(&self.directory.parts),
            size: self.directory.size,
            limits: self.directory.limits,
            keys,
            budget: Some(&self.budget),
            session: true,
            parts_allocation: None,
        })
    }
}

#[cfg(test)]
mod tests;
