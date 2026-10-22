// SPDX-License-Identifier: MIT
// Independent WDC5 palette-order regressions, including the three original
// witnesses from the fixed 9693ba0 audit.
use lycheedev_data::{
    schema::Document,
    table::{self, TableLimits, Value},
};

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
        (0, 1),
        (4, 2),
        (8, 1),
        (16, 0xaabbccdd),
        (20, 0x12345678),
        (24, 7),
        (28, 7),
        (40, 2),
        (52, 48),
        (60, 12),
        (64, 1),
    ] {
        put32(&mut raw, base + offset, value);
    }
    put16(&mut raw, base + 36, 4);
    put32(&mut raw, header + 8, start as u32);
    put32(&mut raw, header + 12, 1);
    put32(&mut raw, header + 24, 4);
    let columns = if array_first {
        [(4, 8, 2), (3, 4, 0)]
    } else {
        [(3, 4, 0), (4, 8, 2)]
    };
    for (i, (codec, size, cardinality)) in columns.into_iter().enumerate() {
        let offset = header + 40 + 8 + i * 24;
        put16(&mut raw, offset, i as u16);
        put16(&mut raw, offset + 2, 1);
        put32(&mut raw, offset + 4, size);
        put32(&mut raw, offset + 8, codec);
        put32(&mut raw, offset + 16, 1);
        put32(&mut raw, offset + 20, cardinality);
    }
    let values = if version == 5 || !array_first {
        [900, 100, 200]
    } else {
        [100, 200, 900]
    };
    for (i, value) in values.into_iter().enumerate() {
        put32(&mut raw, metadata_end + 4 * i, value);
    }
    put32(&mut raw, start + 1, 7);
    raw
}
fn check(version: u8, array_first: bool) {
    let fields = if array_first {
        "Array<u32>[2]\nScalar<u32>"
    } else {
        "Scalar<u32>\nArray<u32>[2]"
    };
    let document = Document::parse(format!(
        "COLUMNS\nint ID\nint Array\nint Scalar\n\nBUILD 12.1.5.69848\nLAYOUT 12345678\n$noninline,id$ID\n{fields}\n"
    ).as_bytes()).unwrap();
    let definition = document.select("12.1.5.69848", "12345678").unwrap();
    let raw = fixture(version, array_first);
    let table = table::open(&raw, &definition, TableLimits::default()).unwrap();
    let row = table.row(7, 100).unwrap();
    assert_eq!(row["ID"], Value::UInt(7));
    assert_eq!(
        row["Array"],
        Value::Array(vec![Value::UInt(100), Value::UInt(200)]),
        "WDC{version} array_first={array_first}, row={row:?}"
    );
    assert_eq!(
        row["Scalar"],
        Value::UInt(900),
        "WDC{version} array_first={array_first}, row={row:?}"
    );
}

#[test]
fn audit_wdc5_array_before_scalar_palette_has_independent_wire_order() {
    check(5, true);
}
#[test]
fn audit_wdc5_scalar_before_array_palette_positive_control() {
    check(5, false);
}
#[test]
fn audit_wdc3_wdc4_keep_legacy_field_palette_order() {
    for version in [3, 4] {
        for array_first in [true, false] {
            check(version, array_first);
        }
    }
}

// Independent wire oracle from wowdev/DBCD e732093f: WDC5Reader reads
// all Pallet columns first, then all PalletArray columns. Older readers
// consume both classes in field order. The literal payloads below are
// intentionally not sorted or generated with the production offset logic.
fn interleaved_fixture(version: u8) -> (Vec<u8>, usize, usize, usize) {
    let header = 72 + if version == 5 { 132 } else { 0 };
    let base = 4 + if version == 5 { 132 } else { 0 };
    let section = if version == 2 { 36 } else { 40 };
    let storage_start = header + section + 5 * 4;
    let palette_start = storage_start + 5 * 24;
    let start = palette_start + 60 + 8;
    let mut raw = vec![0; start + 2 + 8];
    raw[..4].copy_from_slice(format!("WDC{version}").as_bytes());
    if version == 5 {
        put32(&mut raw, 4, 1);
        raw[8..20].copy_from_slice(b"12.1.5.69848");
    }
    for (offset, value) in [
        (0, 2),
        (4, 5),
        (8, 1),
        (16, 0xaabbccdd),
        (20, 0x12345678),
        (24, 7),
        (28, 8),
        (40, 5),
        (52, 120),
        (56, 8),
        (60, 60),
        (64, 1),
    ] {
        put32(&mut raw, base + offset, value);
    }
    put16(&mut raw, base + 36, 4);
    put32(&mut raw, header + 8, start as u32);
    put32(&mut raw, header + 12, 2);
    put32(&mut raw, header + if version == 2 { 28 } else { 24 }, 8);
    // ArrayA[2], ScalarA, Common, ArrayB[3], ScalarB, in schema order.
    for (i, (bit, codec, bytes, default, width, cardinality)) in [
        (0, 4, 16, 0, 2, 2),
        (2, 3, 12, 0, 2, 0),
        (4, 2, 8, 700, 0, 0),
        (4, 4, 24, 0, 2, 3),
        (6, 3, 8, 0, 2, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let offset = storage_start + i * 24;
        put16(&mut raw, offset, bit);
        put16(&mut raw, offset + 2, width as u16);
        put32(&mut raw, offset + 4, bytes);
        put32(&mut raw, offset + 8, codec);
        put32(&mut raw, offset + 12, default);
        put32(&mut raw, offset + 16, width);
        put32(&mut raw, offset + 20, cardinality);
    }
    let palette = if version == 5 {
        [
            900, 901, 902, 800, 801, 100, 101, 110, 111, 200, 201, 202, 210, 211, 212,
        ]
    } else {
        [
            100, 101, 110, 111, 900, 901, 902, 200, 201, 202, 210, 211, 212, 800, 801,
        ]
    };
    for (i, value) in palette.into_iter().enumerate() {
        put32(&mut raw, palette_start + i * 4, value);
    }
    // ID 7 overrides Common; ID 8 uses its default. Palette indexes are
    // [1, 2, 1, 1] for ID 7 and [0, 0, 0, 0] for ID 8.
    put32(&mut raw, palette_start + 60, 7);
    put32(&mut raw, palette_start + 64, 777);
    raw[start] = 0x59;
    put32(&mut raw, start + 2, 7);
    put32(&mut raw, start + 6, 8);
    (raw, base, storage_start, start)
}

fn interleaved_definition() -> lycheedev_data::schema::Definition {
    Document::parse(b"COLUMNS\nint ID\nint ArrayA\nint ScalarA\nint Common\nint ArrayB\nint ScalarB\n\nBUILD 12.1.5.69848\nLAYOUT 12345678\n$noninline,id$ID\nArrayA<u32>[2]\nScalarA<u32>\nCommon<u32>\nArrayB<u32>[3]\nScalarB<u32>\n")
        .unwrap()
        .select("12.1.5.69848", "12345678")
        .unwrap()
}

fn check_interleaved(raw: &[u8]) {
    let definition = interleaved_definition();
    let table = table::open(raw, &definition, TableLimits::default()).unwrap();
    for (id, scalar_a, scalar_b, common, array_a, array_b) in [
        (7, 902, 801, 777, [110, 111], [210, 211, 212]),
        (8, 900, 800, 700, [100, 101], [200, 201, 202]),
    ] {
        let expected = [
            ("ID".to_owned(), Value::UInt(id as u64)),
            (
                "ArrayA".to_owned(),
                Value::Array(array_a.map(Value::UInt).to_vec()),
            ),
            ("ScalarA".to_owned(), Value::UInt(scalar_a)),
            ("Common".to_owned(), Value::UInt(common)),
            (
                "ArrayB".to_owned(),
                Value::Array(array_b.map(Value::UInt).to_vec()),
            ),
            ("ScalarB".to_owned(), Value::UInt(scalar_b)),
        ]
        .into_iter()
        .collect::<std::collections::BTreeMap<_, _>>();
        assert_eq!(
            table.row(id, 100).unwrap(),
            expected,
            "WDC{} ID {id}",
            table.layout().version
        );
    }
}

#[test]
fn wdc5_multiple_interleaved_palettes_preserve_each_class_and_common_data() {
    let (raw, _, _, _) = interleaved_fixture(5);
    check_interleaved(&raw);
}

#[test]
fn older_formats_keep_interleaved_field_order_with_nonzero_indexes() {
    for version in [2, 3, 4] {
        let (mut raw, _, _, _) = interleaved_fixture(version);
        check_interleaved(&raw);
        if version == 2 {
            raw[..4].copy_from_slice(b"1SLC");
            check_interleaved(&raw);
        }
    }
}

#[test]
fn wdc5_palette_indexes_cannot_read_into_another_fields_palette() {
    let definition = interleaved_definition();
    let (original, _, _, start) = interleaved_fixture(5);
    for (shift, invalid_index) in [(0, 2), (2, 3), (4, 2), (6, 2)] {
        let mut raw = original.clone();
        raw[start] = (raw[start] & !(3 << shift)) | (invalid_index << shift);
        let table = table::open(&raw, &definition, TableLimits::default()).unwrap();
        assert_eq!(
            table.row(7, 100).unwrap_err().code,
            "table.invalid_format",
            "index at bit {shift}"
        );
    }
}

#[test]
fn wdc5_palette_metadata_keeps_size_and_cardinality_validation() {
    let definition = interleaved_definition();
    let (original, base, storage, _) = interleaved_fixture(5);
    for (offset, value) in [
        (base + 60, 56),             // Declared total differs from the field totals.
        (storage + 4, u32::MAX - 7), // Aligned oversized palette cannot wrap the total.
        (storage + 20, 0),           // An array must have a nonzero cardinality.
        (storage + 20, 3),           // Array byte count must be divisible by cardinality.
    ] {
        let mut raw = original.clone();
        put32(&mut raw, offset, value);
        assert_eq!(
            table::open(&raw, &definition, TableLimits::default())
                .err()
                .unwrap()
                .code,
            "table.invalid_format",
            "metadata at byte {offset}"
        );
    }
}
