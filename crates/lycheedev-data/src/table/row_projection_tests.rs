//! Current ordered collector parity against the named-row collector. The
//! retired bound-plan projection API is covered by the DataFusion provider.
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

#[test]
fn ordered_row_collector_matches_every_named_column() {
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
    let names: BTreeSet<_> = table.columns().iter().map(|c| c.name.clone()).collect();
    let mut named = table.row_projected(1, 1 << 20, Some(&names)).unwrap();
    let expected: Vec<_> = table
        .columns()
        .iter()
        .map(|c| named.remove(&c.name).unwrap())
        .collect();
    assert!(named.is_empty());
    let ordered = table.leased_row_values(1, 1 << 20).unwrap();
    assert_eq!(ordered.value(), &expected);
}
