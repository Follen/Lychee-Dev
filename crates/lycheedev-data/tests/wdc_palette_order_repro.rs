// Read-only audit witness for fixed Lychee commit 9693ba0.
// This is a proposed integration test, not a production change.
// It has NOT been executed against the production crate in this audit.
use lycheedev_data::{schema::Document, table::{self, TableLimits, Value}};

fn put32(raw: &mut [u8], offset: usize, value: u32) {
    raw[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn put16(raw: &mut [u8], offset: usize, value: u16) {
    raw[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn fixture(version: u8, array_first: bool) -> Vec<u8> {
    let header = 72 + if version == 5 { 132 } else { 0 };
    let base = 4 + if version == 5 { 132 } else { 0 };
    let metadata_end = header + 40 + 2 * 4 + 2 * 24;
    let start = metadata_end + 12;
    let mut raw = vec![0; start + 5];
    raw[..4].copy_from_slice(format!("WDC{version}").as_bytes());
    if version == 5 {
        put32(&mut raw, 4, 1);
        raw[8..20].copy_from_slice(b"12.1.5.69848");
    }
    for (offset, value) in [
        (0, 1), (4, 2), (8, 1), (16, 0xaabbccdd), (20, 0x12345678),
        (24, 7), (28, 7), (40, 2), (52, 48), (60, 12), (64, 1),
    ] { put32(&mut raw, base + offset, value); }
    put16(&mut raw, base + 36, 4);
    put32(&mut raw, header + 8, start as u32);
    put32(&mut raw, header + 12, 1);
    put32(&mut raw, header + 24, 4);
    let columns = if array_first { [(4, 8, 2), (3, 4, 0)] } else { [(3, 4, 0), (4, 8, 2)] };
    for (i, (codec, size, cardinality)) in columns.into_iter().enumerate() {
        let offset = header + 40 + 8 + i * 24;
        put16(&mut raw, offset, i as u16);
        put16(&mut raw, offset + 2, 1);
        put32(&mut raw, offset + 4, size);
        put32(&mut raw, offset + 8, codec);
        put32(&mut raw, offset + 16, 1);
        put32(&mut raw, offset + 20, cardinality);
    }
    let values = if version == 5 || !array_first { [900, 100, 200] } else { [100, 200, 900] };
    for (i, value) in values.into_iter().enumerate() { put32(&mut raw, metadata_end + 4 * i, value); }
    put32(&mut raw, start + 1, 7);
    raw
}
fn check(version: u8, array_first: bool) {
    let fields = if array_first { "Array<u32>[2]\nScalar<u32>" } else { "Scalar<u32>\nArray<u32>[2]" };
    let document = Document::parse(format!(
        "COLUMNS\nint ID\nint Array\nint Scalar\n\nBUILD 12.1.5.69848\nLAYOUT 12345678\n$noninline,id$ID\n{fields}\n"
    ).as_bytes()).unwrap();
    let definition = document.select("12.1.5.69848", "12345678").unwrap();
    let raw = fixture(version, array_first);
    let table = table::open(&raw, &definition, TableLimits::default()).unwrap();
    let row = table.row(7, 100).unwrap();
    assert_eq!(row["ID"], Value::UInt(7));
    assert_eq!(row["Array"], Value::Array(vec![Value::UInt(100), Value::UInt(200)]), "WDC{version} array_first={array_first}, row={row:?}");
    assert_eq!(row["Scalar"], Value::UInt(900), "WDC{version} array_first={array_first}, row={row:?}");
}

#[test]
fn audit_wdc5_array_before_scalar_palette_has_independent_wire_order() { check(5, true); }
#[test]
fn audit_wdc5_scalar_before_array_palette_positive_control() { check(5, false); }
#[test]
fn audit_wdc3_wdc4_keep_legacy_field_palette_order() {
    for version in [3, 4] { for array_first in [true, false] { check(version, array_first); } }
}
