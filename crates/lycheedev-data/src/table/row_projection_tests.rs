//! Independent ordered-row values, original text boundary and float width.
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
    super::open(&spec.raw, &definition(spec), TableLimits::default()).unwrap()
}

#[test]
fn ordered_row_keeps_independent_types_text_boundary_and_signed_zero() {
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
    let ordered = table.leased_row_values(1, 1 << 20).unwrap();
    assert_eq!(ordered.value, [
        Value::UInt(1), Value::Int(i64::MIN), Value::String("<>&界🙂".into()),
        Value::Float32(-0.0), Value::Array(vec![Value::Int(i32::MIN as i64), Value::Int(i32::MAX as i64)]),
    ]);
    match ordered.value[3] {
        Value::Float32(value) => assert_eq!(value.to_bits(), (-0.0f32).to_bits()),
        _ => panic!("source float width lost"),
    }
}
