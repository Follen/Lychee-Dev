//! Callback contracts from fixed Go 3.2.3 effective.go, exercised through the
//! actual prepared OverlayPlan and query-scoped Program registry.
use super::*;
use relational::{EqualityOutcome, RowProvider, RowYield, Scalar, Source};
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
impl RowProvider for Base {
    fn scan(&self, emit: &mut RowYield<'_>) -> Result<()> {
        for r in [row(1, 10), row(2, 20), row(4, 40)] {
            emit(&r)?;
        }
        Ok(())
    }
    fn has_equal(&self) -> bool {
        self.capable
    }
    fn equal(
        &self,
        column: &str,
        value: &Scalar,
        emit: &mut RowYield<'_>,
    ) -> Result<EqualityOutcome> {
        self.calls.borrow_mut().push(value.clone());
        if column != "ID" {
            return Ok(EqualityOutcome::Declined);
        }
        match value {
            Scalar::Null => {}
            Scalar::UInt(4) | Scalar::Int(4) => emit(&row(4, 40))?,
            Scalar::UInt(_) | Scalar::Int(_) | Scalar::Float(_) => {}
            _ => return Err(fault("relational.type_mismatch", 4, "fixture type")),
        }
        Ok(EqualityOutcome::Handled)
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
