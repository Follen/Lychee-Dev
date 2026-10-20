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

/// Scalar evaluation commonly pushes one result and immediately pops it. Keep
/// that value inline without changing the original four-slot admission or its
/// lifetime. Continuation Task storage deliberately keeps the ordinary Stack.
pub(super) struct ScalarStack<'a> {
    storage: ScalarStorage,
    retained: Option<crate::resource::RetainedReservation<'a>>,
    shared: Option<&'a crate::resource::Budget>,
}
#[cfg(test)]
thread_local! {
    static SCALAR_SPILLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
enum ScalarStorage {
    Unadmitted,
    // None still owns the original four-slot reservation until stack Drop.
    Inline(Option<Scalar>),
    Heap(Vec<Scalar>),
}
impl<'a> ScalarStack<'a> {
    pub(super) fn new(shared: Option<&'a crate::resource::Budget>) -> Result<Self> {
        Ok(Self {
            storage: ScalarStorage::Unadmitted,
            retained: None,
            shared,
        })
    }
    pub(super) fn push(&mut self, value: Scalar) -> Result<()> {
        match &mut self.storage {
            ScalarStorage::Unadmitted => {
                let bytes = 4usize
                    .checked_mul(std::mem::size_of::<Scalar>())
                    .ok_or_else(budget)?;
                if let Some(shared) = self.shared {
                    self.retained = Some(shared.reserve_retained(bytes)?);
                }
                self.storage = ScalarStorage::Inline(Some(value));
            }
            ScalarStorage::Inline(slot) if slot.is_none() => {
                *slot = Some(value);
            }
            ScalarStorage::Inline(slot) => {
                // Spill before taking the old value. Allocation failure leaves
                // that owner and its reservation intact, dropping incoming first.
                let mut values = Vec::new();
                values.try_reserve_exact(4).map_err(|_| budget())?;
                values.push(slot.take().expect("occupied inline scalar"));
                values.push(value);
                self.storage = ScalarStorage::Heap(values);
                #[cfg(test)]
                SCALAR_SPILLS.with(|count| count.set(count.get() + 1));
            }
            ScalarStorage::Heap(values) => {
                if values.len() == values.capacity() {
                    let capacity = values.capacity().max(2).checked_mul(2).ok_or_else(budget)?;
                    let bytes = (capacity - values.capacity())
                        .checked_mul(std::mem::size_of::<Scalar>())
                        .ok_or_else(budget)?;
                    if let Some(retained) = &mut self.retained {
                        retained.grow(bytes)?;
                    } else if let Some(shared) = self.shared {
                        self.retained = Some(shared.reserve_retained(bytes)?);
                    }
                    values
                        .try_reserve_exact(capacity - values.len())
                        .map_err(|_| budget())?;
                }
                values.push(value);
            }
        }
        Ok(())
    }
    pub(super) fn pop(&mut self) -> Option<Scalar> {
        match &mut self.storage {
            ScalarStorage::Unadmitted => None,
            ScalarStorage::Inline(value) => value.take(),
            ScalarStorage::Heap(values) => values.pop(),
        }
    }
    pub(super) fn len(&self) -> usize {
        match &self.storage {
            ScalarStorage::Unadmitted => 0,
            ScalarStorage::Inline(value) => usize::from(value.is_some()),
            ScalarStorage::Heap(values) => values.len(),
        }
    }
}
impl Drop for ScalarStack<'_> {
    fn drop(&mut self) {
        // Keep identical LIFO destruction before the retained guard refunds.
        while let Some(value) = self.pop() {
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

#[cfg(test)]
mod scalar_tests {
    use super::*;
    use crate::resource::{Budget, Cost};
    use std::sync::Arc;

    fn budget_for(bytes: usize, cancel: Option<Arc<AtomicBool>>) -> Budget {
        Budget::new(
            Cost {
                retained_bytes: bytes as u64,
                ..Cost::defaults()
            },
            Instant::now() + Duration::from_secs(30),
            cancel,
        )
    }
    fn same_value(a: Option<Scalar>, b: Option<Scalar>) {
        match (a, b) {
            (Some(Scalar::Float(a)), Some(Scalar::Float(b))) => {
                assert_eq!(a.to_bits(), b.to_bits())
            }
            (a, b) => assert_eq!(a, b),
        }
    }
    fn same_ledgers(a: &Budget, b: &Budget) {
        assert_eq!(a.snapshot().unwrap(), b.snapshot().unwrap());
        assert_eq!(a.logical_snapshot().unwrap(), b.logical_snapshot().unwrap());
        assert_eq!(a.refundable_retained_bytes(), b.refundable_retained_bytes());
    }
    #[test]
    fn scalar_inline_and_original_stack_have_identical_growth_lifo_and_ledgers() {
        let a = budget_for(64 * size_of::<Scalar>(), None);
        let b = budget_for(64 * size_of::<Scalar>(), None);
        let baseline = a.snapshot().unwrap();
        let mut original = Stack::new(Some(&a)).unwrap();
        let mut inline = ScalarStack::new(Some(&b)).unwrap();
        let scalars = [
            Scalar::Null,
            Scalar::Bool(false),
            Scalar::Int(i64::MIN),
            Scalar::UInt(u64::MAX),
            Scalar::Float(-0.0),
            Scalar::Float(0.1),
            Scalar::String("中文🙂\0".into()),
            Scalar::Float(f64::NAN),
        ];
        for value in scalars.iter().cycle().take(24) {
            original.push(value.clone()).unwrap();
            inline.push(value.clone()).unwrap();
            assert_eq!(original.len(), inline.len());
            same_ledgers(&a, &b);
        }
        while original.len() != 0 {
            same_value(original.pop(), inline.pop());
            same_ledgers(&a, &b);
        }
        assert!(matches!(&inline.storage, ScalarStorage::Heap(v) if v.capacity() >= 24));
        original.push(Scalar::UInt(u64::MAX)).unwrap();
        inline.push(Scalar::UInt(u64::MAX)).unwrap();
        same_ledgers(&a, &b);
        same_value(original.pop(), inline.pop());
        drop(original);
        drop(inline);
        same_ledgers(&a, &b);
        assert_eq!(a.snapshot().unwrap(), baseline);
    }
    #[test]
    fn scalar_repeated_single_values_never_allocate_backing_vec_but_keep_admission() {
        let a = budget_for(4 * size_of::<Scalar>(), None);
        let b = budget_for(4 * size_of::<Scalar>(), None);
        let baseline = a.snapshot().unwrap();
        let mut original = Stack::new(Some(&a)).unwrap();
        let mut inline = ScalarStack::new(Some(&b)).unwrap();
        for n in 0..1024 {
            original.push(Scalar::UInt(n)).unwrap();
            inline.push(Scalar::UInt(n)).unwrap();
            assert!(matches!(inline.storage, ScalarStorage::Inline(Some(_))));
            same_ledgers(&a, &b);
            same_value(original.pop(), inline.pop());
            same_ledgers(&a, &b);
            assert!(matches!(inline.storage, ScalarStorage::Inline(None)));
            assert_eq!(
                b.refundable_retained_bytes(),
                (4 * size_of::<Scalar>()) as u64
            );
        }
        drop(original);
        drop(inline);
        same_ledgers(&a, &b);
        assert_eq!(a.snapshot().unwrap(), baseline);
        let mut unshared = ScalarStack::new(None).unwrap();
        for _ in 0..8 {
            unshared.push(Scalar::Null).unwrap();
        }
        for _ in 0..8 {
            assert_eq!(unshared.pop(), Some(Scalar::Null));
        }
        assert!(matches!(unshared.storage, ScalarStorage::Heap(_)));
    }
    #[test]
    fn scalar_initial_and_growth_resource_failures_preserve_original_owners() {
        for bytes in [0, 4 * size_of::<Scalar>() - 1, 4 * size_of::<Scalar>()] {
            let a = budget_for(bytes, None);
            let b = budget_for(bytes, None);
            let baseline = a.snapshot().unwrap();
            let mut original = Stack::new(Some(&a)).unwrap();
            let mut inline = ScalarStack::new(Some(&b)).unwrap();
            for n in 0..5 {
                let x = original.push(Scalar::UInt(n));
                let y = inline.push(Scalar::UInt(n));
                assert_eq!(
                    x.as_ref().err().map(|e| (&e.code, e.exit_code)),
                    y.as_ref().err().map(|e| (&e.code, e.exit_code))
                );
                same_ledgers(&a, &b);
                if x.is_err() {
                    break;
                }
            }
            while original.len() != 0 {
                same_value(original.pop(), inline.pop());
            }
            assert_eq!(inline.len(), 0);
            drop(original);
            drop(inline);
            same_ledgers(&a, &b);
            assert_eq!(a.snapshot().unwrap(), baseline);
        }
    }
    #[test]
    fn scalar_spill_and_empty_reuse_do_not_add_control_checkpoints() {
        let token = Arc::new(AtomicBool::new(false));
        let a = budget_for(8 * size_of::<Scalar>(), Some(token.clone()));
        let b = budget_for(8 * size_of::<Scalar>(), Some(token.clone()));
        let baseline = a.snapshot().unwrap();
        let mut original = Stack::new(Some(&a)).unwrap();
        let mut inline = ScalarStack::new(Some(&b)).unwrap();
        original.push(Scalar::Null).unwrap();
        inline.push(Scalar::Null).unwrap();
        same_value(original.pop(), inline.pop());
        token.store(true, AtomicOrdering::Release);
        // Original capacity permits these operations without a new checkpoint.
        for n in 0..4 {
            original.push(Scalar::UInt(n)).unwrap();
            inline.push(Scalar::UInt(n)).unwrap();
            same_ledgers(&a, &b);
        }
        assert_eq!(
            original.push(Scalar::Null).unwrap_err().code,
            "operation.cancelled"
        );
        assert_eq!(
            inline.push(Scalar::Null).unwrap_err().code,
            "operation.cancelled"
        );
        same_ledgers(&a, &b);
        drop(original);
        drop(inline);
        same_ledgers(&a, &b);
        assert_eq!(a.snapshot().unwrap(), baseline);
        let mut original = Stack::new(Some(&a)).unwrap();
        let mut inline = ScalarStack::new(Some(&b)).unwrap();
        assert_eq!(
            original.push(Scalar::Null).unwrap_err().code,
            "operation.cancelled"
        );
        assert_eq!(
            inline.push(Scalar::Null).unwrap_err().code,
            "operation.cancelled"
        );
        same_ledgers(&a, &b);
        assert!(matches!(inline.storage, ScalarStorage::Unadmitted));
    }
    #[test]
    fn scalar_leaf_query_keeps_exact_ordered_values_inline_and_expressions_spill() {
        let rows = vec![
            vec![
                Scalar::UInt(2),
                Scalar::UInt(u64::MAX),
                Scalar::Int(i64::MIN),
                Scalar::Float(-0.0),
                Scalar::String("中文🙂".into()),
            ],
            vec![
                Scalar::UInt(1),
                Scalar::UInt(9_007_199_254_740_993),
                Scalar::Int(-7),
                Scalar::Float(0.1),
                Scalar::String("a\0b".into()),
            ],
        ];
        let sources = BTreeMap::from([(
            "t".into(),
            Source {
                columns: ["ID", "Exact", "SignedValue", "FloatValue", "Label"]
                    .map(str::to_owned)
                    .to_vec(),
                rows: rows.clone(),
                complete: true,
            },
        )]);
        SCALAR_SPILLS.with(|count| count.set(0));
        let result = Program::compile("SELECT ID,Exact,SignedValue,FloatValue,Label FROM t ORDER BY ID")
            .unwrap()
            .execute(&sources, &BTreeMap::new(), Limits::default(), None)
            .unwrap();
        assert_eq!(
            SCALAR_SPILLS.with(|count| count.get()),
            0,
            "leaf scalar evaluation needs no backing Vec"
        );
        assert_eq!(result.rows, vec![rows[1].clone(), rows[0].clone()]);
        let Scalar::Float(zero) = result.rows[1][3] else {
            panic!("float type");
        };
        assert_eq!(zero.to_bits(), (-0.0f64).to_bits());
        let result = Program::compile("SELECT ID+1 FROM t ORDER BY ID")
            .unwrap()
            .execute(&sources, &BTreeMap::new(), Limits::default(), None)
            .unwrap();
        assert_eq!(
            result.rows,
            vec![vec![Scalar::Int(2)], vec![Scalar::Int(3)]]
        );
        assert!(
            SCALAR_SPILLS.with(|count| count.get()) > 0,
            "binary evaluation exercises spill"
        );
    }
}
