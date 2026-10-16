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

impl<T> Drop for Stack<'_, T> {
    fn drop(&mut self) {
        // Continuations are owners: release youngest suspended state first.
        // In particular nested temporary marks cannot truncate a live parent.
        while let Some(value) = self.values.pop() {
            drop(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    struct Owner<'a>(usize, &'a RefCell<Vec<usize>>);
    impl Drop for Owner<'_> {
        fn drop(&mut self) {
            self.1.borrow_mut().push(self.0);
        }
    }
    #[test]
    fn failed_growth_releases_newest_then_suspended_owners_and_exact_reservation() {
        let released = RefCell::new(Vec::new());
        let budget = crate::resource::Budget::new(
            crate::resource::Cost {
                retained_bytes: (4 * std::mem::size_of::<Owner<'_>>()) as u64,
                ..crate::resource::Cost::defaults()
            },
            Instant::now() + Duration::from_secs(10),
            None,
        );
        let before = budget.snapshot().unwrap();
        let logical = budget.logical_snapshot().unwrap();
        let mut stack = Stack::new(Some(&budget)).unwrap();
        for index in 0..4 {
            stack.push(Owner(index, &released)).unwrap();
        }
        assert!(released.borrow().is_empty());
        assert_eq!(
            stack.push(Owner(4, &released)).unwrap_err().code,
            "records.query_resource_budget"
        );
        assert_eq!(*released.borrow(), [4]);
        drop(stack);
        assert_eq!(*released.borrow(), [4, 3, 2, 1, 0]);
        assert_eq!(budget.snapshot().unwrap(), before);
        assert_eq!(budget.logical_snapshot().unwrap(), logical);
    }
}
