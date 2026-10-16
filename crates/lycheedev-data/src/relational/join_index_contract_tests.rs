//! Exact optional index boundaries from fixed Go join_index.go. Program-level
//! stream tests live in provider_tests.rs; these exercise its real index builder.
use super::*;
fn work(shared: Option<&crate::resource::Budget>, bytes: usize) -> Work<'_> {
    Work {
        left: 100,
        bytes,
        deadline: Instant::now() + Duration::from_secs(30),
        cancel: None,
        shared,
        planning: false,
        temporaries: vec![],
        selection_sources: vec![],
        providers: &[],
        resolver: None,
        roots: BTreeMap::new(),
        measure: false,
        scans: vec![],
    }
}
fn condition() -> Term {
    Term::Binary(
        Box::new(Term::Column(ColumnRef::qualified("a", "ID"))),
        BinaryOperator::Eq,
        Box::new(Term::Column(ColumnRef::qualified("b", "ID"))),
    )
}
#[test]
fn join_index_counts_duplicate_key_entries_and_half_allowance_exactly() {
    let rows = vec![vec![Scalar::Int(1)], vec![Scalar::Float(1.0)]];
    let columns = vec!["ID".into(), "ID".into()];
    let owners = vec!["a".into(), "b".into()];
    let mut w = work(None, 324);
    let index = index(Some(&condition()), &columns, &owners, 1, &rows, &mut w)
        .unwrap()
        .unwrap();
    assert_eq!(index.buckets["n1"], [0, 1]);
    assert_eq!((w.left, w.bytes), (96, 162));
}
#[test]
fn join_index_local_fallback_restores_only_its_retention_and_keeps_work() {
    let rows = vec![vec![Scalar::Int(1)], vec![Scalar::Float(1.0)]];
    let columns = vec!["ID".into(), "ID".into()];
    let owners = vec!["a".into(), "b".into()];
    let mut w = work(None, 322);
    assert!(
        index(Some(&condition()), &columns, &owners, 1, &rows, &mut w)
            .unwrap()
            .is_none()
    );
    assert_eq!((w.left, w.bytes), (96, 322));
}
#[test]
fn heterogeneous_join_keys_decline_before_building_any_key_entries() {
    let rows = vec![vec![Scalar::Int(1)], vec![Scalar::String("1".into())]];
    let columns = vec!["ID".into(), "ID".into()];
    let owners = vec!["a".into(), "b".into()];
    let mut w = work(None, 1024);
    assert!(
        index(Some(&condition()), &columns, &owners, 1, &rows, &mut w)
            .unwrap()
            .is_none()
    );
    assert_eq!((w.left, w.bytes), (98, 1024));
}
#[test]
fn index_independent_safety_failure_is_fatal_not_a_local_budget_decline() {
    let shared = crate::resource::Budget::new(
        crate::resource::Cost {
            retained_bytes: 1,
            ..crate::resource::Cost::defaults()
        },
        Instant::now() + Duration::from_secs(30),
        None,
    );
    let rows = vec![vec![Scalar::Int(1)]];
    let columns = vec!["ID".into(), "ID".into()];
    let owners = vec!["a".into(), "b".into()];
    let mut w = work(Some(&shared), 1024);
    let error = match index(Some(&condition()), &columns, &owners, 1, &rows, &mut w) {
        Err(error) => error,
        Ok(_) => panic!("independent safety failure was swallowed"),
    };
    assert_eq!(error.code, "records.query_resource_budget");
    assert_eq!(w.bytes, 1024);
}
