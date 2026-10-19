//! Bound-plan identity and representation invariants. Public named rows stay
//! the independent collector oracle; the complete decoder remains shared.
use super::*;
#[allow(dead_code)]
#[path = "../../tests/support/catalog_fixture.rs"]
mod fixture;
use fixture::{Cell, table};

fn definition(spec: &fixture::Table) -> Definition {
    crate::schema::Document::parse(spec.dbd.as_bytes())
        .unwrap()
        .select(fixture::BUILD, &format!("{:08X}", spec.layout))
        .unwrap()
}
fn opened(spec: &fixture::Table) -> Table<'_> {
    super::super::open(&spec.raw, &definition(spec), TableLimits::default()).unwrap()
}
fn rows(spec: &fixture::Table) -> Vec<Value> {
    opened(spec)
        .leased_row_values(1, 1 << 20)
        .unwrap()
        .value()
        .clone()
}

#[test]
fn projected_row_plan_binds_once_to_its_original_immutable_table() {
    let left = table(
        "Left",
        901,
        &[("ID", "uint"), ("Name", "text"), ("Amount", "signed")],
        &[vec![Cell::U(1), Cell::Text("left猫".into()), Cell::I(-7)]],
    );
    let right = table(
        "Right",
        902,
        &[("ID", "uint"), ("Name", "text"), ("Amount", "signed")],
        &[vec![Cell::U(1), Cell::Text("right犬".into()), Cell::I(99)]],
    );
    let left_table = opened(&left);
    let right_table = opened(&right);
    let bindings = projected_row_bindings();
    let selected = vec![1, 2];
    let allocation = selected.as_ptr();
    let left_plan = left_table.projected_rows(selected).unwrap();
    assert_eq!(
        left_plan.selected.as_ptr(),
        allocation,
        "the admitted selector allocation moves unchanged"
    );
    assert!(std::ptr::eq(left_plan.table, &left_table));
    assert!(!std::ptr::eq(left_plan.table, &right_table));
    let right_plan = right_table.projected_rows(vec![1, 2]).unwrap();
    assert_eq!(projected_row_bindings(), bindings + 2);
    for _ in 0..8 {
        assert_eq!(
            left_plan.leased_row(1, 1 << 20).unwrap().value(),
            &vec![Value::String("left猫".into()), Value::Int(-7)]
        );
        assert_eq!(
            right_plan.leased_row(1, 1 << 20).unwrap().value(),
            &vec![Value::String("right犬".into()), Value::Int(99)]
        );
    }
    assert_eq!(
        projected_row_bindings(),
        bindings + 2,
        "rows never rebind a plan or another schema"
    );
}

#[test]
fn projected_row_plan_rejects_unordered_duplicate_and_foreign_ordinals_without_decode() {
    let spec = table(
        "Selection",
        903,
        &[("ID", "uint"), ("Name", "text")],
        &[vec![Cell::U(1), Cell::Text("value".into())]],
    );
    let table = opened(&spec);
    let bindings = projected_row_bindings();
    let work = table.work.load(Ordering::Acquire);
    for indices in [vec![0, 0], vec![1, 0], vec![2], vec![usize::MAX]] {
        let error = table
            .projected_rows(indices)
            .err()
            .expect("invalid plan must fail");
        assert_eq!(error.code, "table.invalid_format");
        assert_eq!(table.work.load(Ordering::Acquire), work);
        assert_eq!(projected_row_bindings(), bindings);
    }
    assert!(
        table
            .projected_rows(Vec::new())
            .unwrap()
            .leased_row(1, 1 << 20)
            .unwrap()
            .value()
            .is_empty()
    );
    assert_eq!(
        table.leased_row_values(1, 1 << 20).unwrap().value(),
        &rows(&spec)
    );
}

#[test]
fn ordered_row_collector_matches_named_rows_for_every_field_subset() {
    let spec = table(
        "Values",
        904,
        &[
            ("ID", "uint"),
            ("Signed", "signed64"),
            ("Text", "text"),
            ("Real", "float"),
            ("Array", "array2signed"),
        ],
        &[vec![
            Cell::U(1),
            Cell::I64(i64::MIN),
            Cell::Text("<>&界🙂\0tail".into()),
            Cell::F(-0.0),
            Cell::Array(vec![Cell::I(i32::MIN), Cell::I(i32::MAX)]),
        ]],
    );
    let table = opened(&spec);
    for mask in 0..(1 << table.columns().len()) {
        let indices: Vec<_> = (0..table.columns().len())
            .filter(|i| mask & (1 << i) != 0)
            .collect();
        let names: BTreeSet<_> = indices
            .iter()
            .map(|&i| table.columns()[i].name.clone())
            .collect();
        let mut named = table.row_projected(1, 1 << 20, Some(&names)).unwrap();
        let expected: Vec<_> = indices
            .iter()
            .map(|&i| named.remove(&table.columns()[i].name).unwrap())
            .collect();
        assert!(named.is_empty());
        let ordered = table
            .projected_rows(indices)
            .unwrap()
            .leased_row(1, 1 << 20)
            .unwrap();
        assert_eq!(
            serde_json::to_vec(ordered.value()).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
        assert_eq!(ordered.value(), &expected);
    }
}
