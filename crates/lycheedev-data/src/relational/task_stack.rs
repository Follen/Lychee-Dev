//! Refundable heap storage for bounded interpreter continuations. This uses the
//! original physical-allocation ledger; it never charges extra Go logical work.
use super::*;

pub(super) struct Stack<'a, T> {
    values: Vec<T>,
    retained: Option<crate::resource::RetainedReservation<'a>>,
    shared: Option<&'a crate::resource::Budget>,
}
impl<'a, T> Stack<'a, T> {
    pub(super) fn new(shared: Option<&'a crate::resource::Budget>) -> Result<Self> {
        Ok(Self {
            values: Vec::new(),
            retained: None,
            shared,
        })
    }
    pub(super) fn push(&mut self, value: T) -> Result<()> {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .capacity()
                .max(2)
                .checked_mul(2)
                .ok_or_else(budget)?;
            let bytes = (capacity - self.values.capacity())
                .checked_mul(std::mem::size_of::<T>())
                .ok_or_else(budget)?;
            if let Some(retained) = &mut self.retained {
                retained.grow(bytes)?;
            } else if let Some(shared) = self.shared {
                self.retained = Some(shared.reserve_retained(bytes)?);
            }
            self.values
                .try_reserve_exact(capacity - self.values.len())
                .map_err(|_| budget())?;
        }
        self.values.push(value);
        Ok(())
    }
    pub(super) fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }
    pub(super) fn len(&self) -> usize {
        self.values.len()
    }
}
