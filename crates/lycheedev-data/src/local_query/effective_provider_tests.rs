//! Callback contracts from fixed Go 3.2.3 effective.go, exercised through the
//! actual prepared OverlayPlan and query-scoped Program registry.
use super::*;
use relational::{
    EqualityCursor, EqualityOutcome, RowCursor, RowProvider, RowYield, Scalar, Source,
};
use std::cell::RefCell;
fn budget() -> Budget {
    Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    )
}
fn row(id: u32, value: u32) -> Vec<Scalar> {
    vec![Scalar::UInt(id as u64), Scalar::UInt(value as u64)]
}
fn entry(id: u32) -> CacheEntry {
    CacheEntry {
        index: id,
        region: None,
        push: id as i32,
        unique_id: None,
        table_hash: 42,
        record_id: id,
        status: 1,
        payload_offset: 0,
        payload_bytes: 8,
        payload_hex: String::new(),
        fields: None,
        decode_state: String::new(),
    }
}
fn plan() -> OverlayPlan {
    OverlayPlan {
        schema: Source {
            columns: [
                "ID",
                "Value",
                "__hotfix_source",
                "__hotfix_push",
                "__hotfix_index",
            ]
            .map(str::to_owned)
            .to_vec(),
            rows: vec![],
            complete: true,
        },
        identity_index: 0,
        capture_ids: vec!["CAP-fixture".into()],
        patches: BTreeMap::from([
            (
                1,
                Patch {
                    values: Some(row(1, 99)),
                    ordinal: 0,
                    entry: entry(1),
                },
            ),
            (
                2,
                Patch {
                    values: None,
                    ordinal: 0,
                    entry: entry(2),
                },
            ),
            (
                3,
                Patch {
                    values: Some(row(3, 77)),
                    ordinal: 0,
                    entry: entry(3),
                },
            ),
            (
                8,
                Patch {
                    values: Some(row(8, 88)),
                    ordinal: 0,
                    entry: entry(8),
                },
            ),
        ]),
    }
}
struct Base {
    calls: RefCell<Vec<Scalar>>,
    capable: bool,
}
impl Base {
    fn new() -> Self {
        Self {
            calls: RefCell::new(vec![]),
            capable: true,
        }
    }
}
struct BaseCursor {
    rows: &'static [(u32, u32)],
    next: usize,
    current: Vec<Scalar>,
}
impl RowCursor for BaseCursor {
    fn advance(&mut self) -> Result<bool> {
        self.current.fill(Scalar::Null);
        let Some(&(id, value)) = self.rows.get(self.next) else {
            self.current.clear();
            return Ok(false);
        };
        self.next += 1;
        self.current = row(id, value);
        Ok(true)
    }
    fn row(&self) -> &[Scalar] {
        &self.current
    }
}
impl RowProvider for Base {
    fn cursor(&self, _: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
        Ok(Box::new(BaseCursor {
            rows: &[(1, 10), (2, 20), (4, 40)],
            next: 0,
            current: vec![],
        }))
    }
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        provider::emit_rows(self.cursor(None)?, emit)
    }
    fn has_equal(&self) -> bool {
        self.capable
    }
    fn equal_cursor(&self, column: &str, value: &Scalar) -> Result<EqualityCursor<'_>> {
        self.calls.borrow_mut().push(value.clone());
        if column != "ID" {
            return Ok(EqualityCursor::Declined);
        }
        match value {
            Scalar::UInt(4) | Scalar::Int(4) => Ok(EqualityCursor::Rows(Box::new(BaseCursor {
                rows: &[(4, 40)],
                next: 0,
                current: vec![],
            }))),
            Scalar::Null | Scalar::UInt(_) | Scalar::Int(_) | Scalar::Float(_) => {
                Ok(EqualityCursor::Empty)
            }
            _ => Err(fault("relational.type_mismatch", 4, "fixture type")),
        }
    }
    fn equal(
        &self,
        column: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        match self.equal_cursor(column, value)? {
            EqualityCursor::Declined => Ok(EqualityOutcome::Declined),
            EqualityCursor::Empty => Ok(EqualityOutcome::Handled),
            EqualityCursor::Rows(cursor) => {
                provider::emit_rows(cursor, emit)?;
                Ok(EqualityOutcome::Handled)
            }
        }
    }
}

#[test]
fn effective_lazy_scan_replaces_deletes_and_appends_unvisited_numeric_order() {
    let p = plan();
    let b = Base::new();
    let budget = budget();
    let mut rows = vec![];
    p.scan(&b, &budget, &mut |r| {
        rows.push(r.to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(
        rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        [
            Scalar::UInt(1),
            Scalar::UInt(4),
            Scalar::UInt(3),
            Scalar::UInt(8)
        ]
    );
    assert_eq!(
        rows[0],
        vec![
            Scalar::UInt(1),
            Scalar::UInt(99),
            Scalar::String("CAP-fixture".into()),
            Scalar::Int(1),
            Scalar::UInt(1)
        ]
    );
    assert_eq!(&rows[1][2..], &[Scalar::Null, Scalar::Null, Scalar::Null]);
    assert!(b.calls.borrow().is_empty());
}
#[test]
fn effective_lazy_equal_masks_base_replacement_deletion_and_missing_addition() {
    let p = plan();
    let b = Base::new();
    let budget = budget();
    for (id, expected) in [(1, Some(99)), (2, None), (3, Some(77)), (8, Some(88))] {
        let mut rows = vec![];
        assert_eq!(
            p.equal(&b, &budget, "ID", &Scalar::UInt(id), &mut |r| {
                rows.push(r.to_vec());
                Ok(())
            })
            .unwrap(),
            EqualityOutcome::Handled
        );
        assert_eq!(rows.len(), usize::from(expected.is_some()));
        if let Some(value) = expected {
            assert_eq!(rows[0][1], Scalar::UInt(value));
        }
    }
    assert!(
        b.calls.borrow().is_empty(),
        "patched equality never looks up its base"
    );
    let mut rows = vec![];
    p.equal(&b, &budget, "ID", &Scalar::UInt(4), &mut |r| {
        rows.push(r.to_vec());
        Ok(())
    })
    .unwrap();
    assert_eq!(*b.calls.borrow(), vec![Scalar::UInt(4)]);
    assert_eq!(&rows[0][2..], &[Scalar::Null, Scalar::Null, Scalar::Null]);
}
#[test]
fn effective_lazy_equal_declines_before_value_and_delegates_original_scalar() {
    let p = plan();
    let b = Base::new();
    let budget = budget();
    assert_eq!(
        p.equal(&b, &budget, "Value", &Scalar::Bool(true), &mut |_| Ok(()))
            .unwrap(),
        EqualityOutcome::Declined
    );
    assert!(b.calls.borrow().is_empty());
    for value in [
        Scalar::Null,
        Scalar::Int(-1),
        Scalar::Float(1.5),
        Scalar::UInt(u32::MAX as u64 + 1),
    ] {
        p.equal(&b, &budget, "ID", &value, &mut |_| Ok(())).unwrap();
        assert_eq!(b.calls.borrow().last(), Some(&value));
    }
    assert_eq!(
        p.equal(&b, &budget, "ID", &Scalar::Bool(true), &mut |_| Ok(()))
            .unwrap_err()
            .code,
        "relational.type_mismatch"
    );
    let incapable = Base {
        capable: false,
        calls: RefCell::new(vec![]),
    };
    assert_eq!(
        p.equal(&incapable, &budget, "ID", &Scalar::UInt(1), &mut |_| Ok(()))
            .unwrap(),
        EqualityOutcome::Declined
    );
    assert!(incapable.calls.borrow().is_empty());
}
#[test]
fn effective_lazy_consumer_failure_stops_scan_and_releases_only_scratch() {
    let p = plan();
    let b = Base::new();
    let budget = budget();
    let keep = budget.reserve_retained(13).unwrap();
    let before = budget.snapshot().unwrap().retained_bytes;
    let mut calls = 0;
    let error = p
        .scan(&b, &budget, &mut |_| {
            calls += 1;
            Err(fault("fixture.consumer", 4, "stop"))
        })
        .unwrap_err();
    assert_eq!(error.code, "fixture.consumer");
    assert_eq!(calls, 1);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, before);
    drop(keep);
    assert_eq!(budget.snapshot().unwrap().retained_bytes, 0);
}
#[test]
fn effective_lazy_registry_uses_candidate_order_and_preserves_original_predicate() {
    struct Provider<'a> {
        plan: &'a OverlayPlan,
        base: &'a Base,
        budget: &'a Budget,
    }
    impl RowProvider for Provider<'_> {
        fn cursor(&self, _: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
            self.plan.cursor(self.base, self.budget)
        }
        fn equal_cursor(&self, c: &str, v: &Scalar) -> Result<EqualityCursor<'_>> {
            self.plan.equal_cursor(self.base, self.budget, c, v)
        }
        fn scan(&self, e: &mut RowYield<'_>) -> Result<()> {
            self.plan.scan(self.base, self.budget, e)
        }
        fn has_equal(&self) -> bool {
            true
        }
        fn equal(&self, c: &str, v: &Scalar, e: &mut RowYield<'_>) -> Result<EqualityOutcome> {
            self.plan.equal(self.base, self.budget, c, v, e)
        }
    }
    let p = plan();
    let b = Base::new();
    let budget = budget();
    let provider = Provider {
        plan: &p,
        base: &b,
        budget: &budget,
    };
    let sources = BTreeMap::from([("T".into(), p.schema())]);
    let providers = [relational::PreparedSource {
        schema: p.schema(),
        provider: &provider,
    }];
    let out = relational::Program::compile("SELECT ID,Value FROM T WHERE ID IN (8,2,3,1,8,4)")
        .unwrap()
        .execute_providers_budgeted(
            &sources,
            &providers,
            &BTreeMap::new(),
            Default::default(),
            None,
            None,
        )
        .unwrap();
    assert_eq!(
        out.rows,
        vec![row(8, 88), row(3, 77), row(1, 99), row(4, 40)]
    );
    assert_eq!(*b.calls.borrow(), vec![Scalar::Int(4)]);
}

struct LeaseBase<'budget> {
    budget: &'budget Budget,
    id: u32,
    late_error: bool,
    opened: std::cell::Cell<usize>,
    active: std::cell::Cell<usize>,
    resumed: std::cell::Cell<usize>,
}
impl<'budget> LeaseBase<'budget> {
    fn new(budget: &'budget Budget, id: u32, late_error: bool) -> Self {
        Self {
            budget,
            id,
            late_error,
            opened: Default::default(),
            active: Default::default(),
            resumed: Default::default(),
        }
    }
}
struct LeasedFixtureRow<'query> {
    cells: Vec<Scalar>,
    _allocation: RetainedReservation<'query>,
    active: &'query std::cell::Cell<usize>,
}
impl Drop for LeasedFixtureRow<'_> {
    fn drop(&mut self) {
        self.active.set(self.active.get() - 1);
    }
}
struct LeasedBaseCursor<'query, 'budget> {
    current: Option<LeasedFixtureRow<'query>>,
    base: &'query LeaseBase<'budget>,
    next: usize,
}
impl RowCursor for LeasedBaseCursor<'_, '_> {
    fn advance(&mut self) -> Result<bool> {
        self.current = None;
        match self.next {
            0 => {
                self.next = 1;
                let allocation = self.base.budget.reserve_retained(4096)?;
                self.base.active.set(self.base.active.get() + 1);
                self.current = Some(LeasedFixtureRow {
                    cells: row(self.base.id, 40),
                    _allocation: allocation,
                    active: &self.base.active,
                });
                Ok(true)
            }
            1 => {
                self.next = 2;
                self.base.resumed.set(self.base.resumed.get() + 1);
                if self.base.late_error {
                    return Err(fault("fixture.base_late", 5, "late base failure"));
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
    fn row(&self) -> &[Scalar] {
        self.current.as_ref().map_or(&[], |row| &row.cells)
    }
}
impl RowProvider for LeaseBase<'_> {
    fn cursor(&self, _: Option<&[usize]>) -> Result<Box<dyn RowCursor + '_>> {
        self.opened.set(self.opened.get() + 1);
        Ok(Box::new(LeasedBaseCursor {
            current: None,
            base: self,
            next: 0,
        }))
    }
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        provider::emit_rows(self.cursor(None)?, emit)
    }
    fn has_equal(&self) -> bool {
        true
    }
    fn equal_cursor(&self, column: &str, value: &Scalar) -> Result<EqualityCursor<'_>> {
        if column != "ID" {
            return Ok(EqualityCursor::Declined);
        }
        if value != &Scalar::UInt(self.id as u64) {
            return Ok(EqualityCursor::Empty);
        }
        Ok(EqualityCursor::Rows(self.cursor(None)?))
    }
}

#[test]
fn effective_cursor_suspends_with_base_and_augmented_leases_without_resuming_producer() {
    let p = plan();
    let b = budget();
    let base = LeaseBase::new(&b, 1, true);
    let keep = b.reserve_retained(17).unwrap();
    let before = b.snapshot().unwrap();
    let logical = b.logical_snapshot().unwrap();
    let mut cursor = p.cursor(&base, &b).unwrap();
    assert_eq!(base.opened.get(), 1);
    assert_eq!(base.active.get(), 0);
    let opened = b.snapshot().unwrap();
    assert!(cursor.advance().unwrap());
    assert_eq!(cursor.row()[1], Scalar::UInt(99));
    assert_eq!(
        base.active.get(),
        1,
        "replacement retains the base row lease"
    );
    assert_eq!(base.resumed.get(), 0);
    assert!(b.snapshot().unwrap().retained_bytes > opened.retained_bytes + 4096);
    assert_eq!(b.logical_snapshot().unwrap(), logical);
    drop(cursor); // A consumer failure/drop must not execute the producer's tail.
    assert_eq!(base.resumed.get(), 0);
    assert_eq!(base.active.get(), 0);
    assert_eq!(b.snapshot().unwrap(), before);
    drop(keep);
    assert_eq!(b.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn effective_scan_cursor_reports_base_late_error_before_unmatched_patch_append() {
    let p = plan();
    let b = budget();
    let base = LeaseBase::new(&b, 1, true);
    let mut cursor = p.cursor(&base, &b).unwrap();
    assert!(cursor.advance().unwrap());
    assert_eq!(cursor.row()[0], Scalar::UInt(1));
    assert_eq!(base.resumed.get(), 0);
    assert_eq!(cursor.advance().unwrap_err().code, "fixture.base_late");
    assert!(cursor.row().is_empty());
    assert_eq!(base.resumed.get(), 1);
    assert_eq!(base.active.get(), 0);
    drop(cursor);
    assert_eq!(b.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn effective_equal_cursor_keeps_delegated_row_lease_and_delays_late_error() {
    let p = plan();
    let b = budget();
    let base = LeaseBase::new(&b, 4, true);
    let EqualityCursor::Rows(mut cursor) =
        p.equal_cursor(&base, &b, "ID", &Scalar::UInt(4)).unwrap()
    else {
        panic!("expected delegated equality cursor");
    };
    assert_eq!(base.active.get(), 0, "opening must not prefetch a row");
    assert!(cursor.advance().unwrap());
    assert_eq!(
        cursor.row(),
        [
            Scalar::UInt(4),
            Scalar::UInt(40),
            Scalar::Null,
            Scalar::Null,
            Scalar::Null
        ]
    );
    assert_eq!(base.active.get(), 1);
    assert_eq!(base.resumed.get(), 0);
    assert_eq!(cursor.advance().unwrap_err().code, "fixture.base_late");
    assert_eq!(base.resumed.get(), 1);
    drop(cursor);
    assert_eq!(base.active.get(), 0);
    assert_eq!(b.snapshot().unwrap().retained_bytes, 0);
}

#[test]
fn effective_equal_cursor_empty_and_declined_branches_do_not_touch_cancelled_budget() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let token = Arc::new(AtomicBool::new(false));
    let b = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(token.clone()),
    );
    let p = plan();
    let base = Base::new();
    token.store(true, Ordering::Release);
    assert!(matches!(
        p.equal_cursor(&base, &b, "Value", &Scalar::Bool(true))
            .unwrap(),
        EqualityCursor::Declined
    ));
    assert!(matches!(
        p.equal_cursor(&base, &b, "ID", &Scalar::UInt(2)).unwrap(),
        EqualityCursor::Empty
    ));
    assert!(
        base.calls.borrow().is_empty(),
        "patched deletion must not read base"
    );
    assert!(matches!(
        p.equal_cursor(&base, &b, "ID", &Scalar::Null).unwrap(),
        EqualityCursor::Empty
    ));
    assert_eq!(*base.calls.borrow(), [Scalar::Null]);
    assert_eq!(
        p.equal_cursor(&base, &b, "ID", &Scalar::Bool(true))
            .err()
            .unwrap()
            .code,
        "relational.type_mismatch"
    );
    assert_eq!(b.snapshot().unwrap(), Cost::default());
}
