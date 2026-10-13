use lycheedev_data::{
    schema::Document,
    table::{self, TableLimits, Value},
};
use std::collections::BTreeSet;
#[test]
fn whole_query_wdc_admission_is_shared_across_tables_and_caller_control() {
    use lycheedev_core::control::OperationControl;
    use lycheedev_data::resource::{Budget, Cost};
    use std::{
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };
    let (raw, _, _) = fixture(3);
    let definition = definition("int ID", "$id$ID<32>");
    let unconstrained = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        None,
    ));
    let first = table::open_budgeted(
        &raw,
        &definition,
        limits(),
        &BTreeSet::new(),
        &[],
        unconstrained.clone(),
    )
    .unwrap();
    let admitted = first.admitted_metadata_bytes();
    assert!(admitted > 0 && admitted < 4096);
    assert_eq!(unconstrained.snapshot().unwrap().metadata_bytes, admitted);
    assert_eq!(first.row(1, 100).unwrap()["ID"], Value::UInt(1));
    let shared = Arc::new(Budget::new(
        Cost {
            metadata_bytes: admitted * 2 - 1,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(10),
        None,
    ));
    let one = table::open_budgeted(
        &raw,
        &definition,
        limits(),
        &BTreeSet::new(),
        &[],
        shared.clone(),
    )
    .unwrap();
    assert_eq!(one.admitted_metadata_bytes(), admitted);
    assert_eq!(
        table::open_budgeted(
            &raw,
            &definition,
            limits(),
            &BTreeSet::new(),
            &[],
            shared.clone()
        )
        .err()
        .unwrap()
        .code,
        "records.query_resource_budget"
    );
    assert!(shared.snapshot().unwrap().metadata_bytes < admitted * 2);
    let token = Arc::new(AtomicBool::new(false));
    let controlled = Arc::new(Budget::controlled(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(10),
        OperationControl::new(Instant::now() + Duration::from_secs(10), token.clone()),
    ));
    let table = table::open_budgeted(
        &raw,
        &definition,
        limits(),
        &BTreeSet::new(),
        &[],
        controlled.clone(),
    )
    .unwrap();
    token.store(true, Ordering::Release);
    assert_eq!(table.row(1, 100).unwrap_err().code, "command.cancelled");
    assert_eq!(table.raw_record(1).unwrap_err().exit_code, 7);
    let denial = Arc::new(Budget::new(
        Cost {
            metadata_bytes: 0,
            ..Cost::defaults()
        },
        Instant::now() + Duration::from_secs(10),
        None,
    ));
    assert_eq!(
        table::open_budgeted(
            &raw,
            &definition,
            limits(),
            &BTreeSet::new(),
            &[],
            denial.clone()
        )
        .err()
        .unwrap()
        .code,
        "records.query_resource_budget"
    );
    assert_eq!(denial.snapshot().unwrap().metadata_bytes, 0);
}
fn limits() -> TableLimits {
    TableLimits {
        file_bytes: 1 << 20,
        metadata_bytes: 1 << 16,
        rows: 1000,
        columns: 100,
        partitions: 100,
        decode_work: 1_000_000,
    }
}
fn put32(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put16(raw: &mut [u8], at: usize, value: u16) {
    raw[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn definition(columns: &str, fields: &str) -> lycheedev_data::schema::Definition {
    Document::parse(
        format!("COLUMNS\n{columns}\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n{fields}\n").as_bytes(),
    )
    .unwrap()
    .select("12.1.0.69875", "12345678")
    .unwrap()
}
fn fixture(version: u8) -> (Vec<u8>, usize, usize) {
    let header = 72 + if version == 5 { 132 } else { 0 };
    let section = if version == 2 { 36 } else { 40 };
    let start = header + section + 4 + 24;
    let mut raw = vec![0; start + 8];
    raw[..4].copy_from_slice(format!("WDC{version}").as_bytes());
    let base = 4 + if version == 5 { 132 } else { 0 };
    if version == 5 {
        put32(&mut raw, 4, 1);
        raw[8..20].copy_from_slice(b"12.1.0.69875");
    }
    for (at, value) in [
        (0, 2),
        (4, 1),
        (8, 4),
        (16, 0xaabbccdd),
        (20, 0x12345678),
        (24, 1),
        (28, 2),
        (40, 1),
        (52, 24),
        (64, 1),
    ] {
        put32(&mut raw, base + at, value);
    }
    put32(&mut raw, header + 8, start as u32);
    put32(&mut raw, header + 12, 2);
    put16(&mut raw, start - 24 + 2, 32);
    put32(&mut raw, start, 1);
    put32(&mut raw, start + 4, 2);
    (raw, base, start)
}
fn records(external: bool, copies: &[u32], relations: &[u32]) -> Vec<u8> {
    let (mut raw, _, _) = fixture(3);
    if external {
        put32(&mut raw, 96, 8);
        raw.extend(words(&[10, 20]));
    }
    put32(&mut raw, 108, (copies.len() / 2) as u32);
    put32(&mut raw, 100, (relations.len() * 4) as u32);
    raw.extend(words(copies));
    raw.extend(words(relations));
    raw
}

#[test]
fn all_supported_headers_bind_exact_hashes_without_materializing_rows() {
    let def = definition("int ID", "$id$ID<32>");
    for version in [2, 3, 4, 5] {
        let (raw, _, start) = fixture(version);
        let layout = table::inspect(&raw, limits()).unwrap();
        assert_eq!(layout.version, version);
        assert_eq!(layout.rows, 2);
        assert_eq!(layout.table_hash, 0xaabbccdd);
        assert_eq!(layout.layout_hash, 0x12345678);
        assert_eq!(layout.metadata_end, start);
        let table = table::open(&raw, &def, limits()).unwrap();
        assert_eq!(table.row(2, 100).unwrap()["ID"], Value::UInt(2));
        assert_eq!(table.raw_record(1).unwrap().as_ptr(), raw[start..].as_ptr());
    }
}
#[test]
fn header_corruption_and_allocation_budgets_fail_before_partial_view() {
    let (base, offset, start) = fixture(3);
    for (at, value) in [
        (offset, 3),
        (offset + 52, 25),
        (offset + 60, 4),
        (offset + 56, 8),
        (80, 1),
        (80, base.len() as u32),
        (96, 1),
        (start - 16, 6),
        (offset + 64, 10001),
        (offset + 40, 10001),
    ] {
        let mut raw = base.clone();
        put32(&mut raw, at, value);
        assert!(table::inspect(&raw, limits()).is_err(), "offset {at}");
    }
    assert!(table::inspect(&base[..start - 1], limits()).is_err());
    let mut budget = limits();
    budget.metadata_bytes = 10;
    assert_eq!(
        table::inspect(&base, budget).err().unwrap().code,
        "table.limit_exceeded"
    );
}
#[test]
fn copied_ids_keep_origin_typed_integer_and_null_relationship_distinct_from_zero() {
    let raw = records(true, &[101, 100, 100, 10], &[1, 0, 0, 0, 0]);
    let def = definition(
        "int ID\nint Value\nint Parent",
        "$noninline,id$ID\nValue<32>\n$noninline,relation$Parent",
    );
    let table = table::open(&raw, &def, limits()).unwrap();
    for id in [10, 100, 101] {
        let row = table.row(id, 100).unwrap();
        assert_eq!(row["ID"], Value::UInt(id as u64));
        assert_eq!(row["Value"], Value::Int(1));
        assert_eq!(row["Parent"], Value::UInt(0));
    }
    assert_eq!(table.row(20, 100).unwrap()["Parent"], Value::Null);
    assert_eq!(table.ids().collect::<Vec<_>>(), [10, 20, 100, 101]);
    assert_eq!(
        table.row(999, 100).err().unwrap().code,
        "table.record_not_found"
    );
}
#[test]
fn invalid_copy_chains_and_relation_metadata_never_become_zero_rows() {
    let def = definition(
        "int ID\nint Value\nint Parent",
        "$noninline,id$ID\nValue<32>\n$noninline,relation$Parent",
    );
    for raw in [
        records(true, &[100, 101, 101, 100], &[]),
        records(true, &[100, 999], &[]),
        records(true, &[100, 10, 100, 20], &[]),
        records(true, &[10, 20], &[]),
        records(true, &[], &[1, 0, 0, 5, 2]),
        records(true, &[], &[2, 0, 0, 5, 0, 6, 0]),
        records(true, &[], &[2, 0, 0, 5, 0]),
    ] {
        assert!(table::open(&raw, &def, limits()).is_err());
    }
}
#[test]
fn every_alignment_and_width_preserves_exact_64_bit_patterns() {
    let raw = [0x81, 0x42, 0x24, 0x18, 0xaa, 0x55, 0xf0, 0x0f, 0x93];
    for width in 1..=64u32 {
        for offset in 0..8u64 {
            let mut expected = 0u64;
            for bit in 0..width {
                if raw[((offset + bit as u64) / 8) as usize] & (1 << ((offset + bit as u64) % 8))
                    != 0
                {
                    expected |= 1u64 << bit;
                }
            }
            assert_eq!(table::extract_bits(&raw, offset, width).unwrap(), expected);
        }
    }
    assert!(table::extract_bits(&raw, raw.len() as u64 * 8, 1).is_err());
}
#[test]
fn common_pallet_signed_and_array_codecs_retain_exact_values() {
    for (codec, width, parameters, aux, record, field, want) in [
        (
            1,
            9,
            [0, 9, 0],
            vec![],
            vec![0xff, 0x01],
            "Value<u32>",
            Value::UInt(511),
        ),
        (
            5,
            9,
            [0, 9, 0],
            vec![],
            vec![0xff, 0x01],
            "Value<32>",
            Value::Int(-1),
        ),
        (
            2,
            0,
            [33, 0, 0],
            words(&[7, 99]),
            vec![0],
            "Value<32>",
            Value::Int(99),
        ),
        (
            3,
            2,
            [0, 2, 0],
            words(&[100, 200, 300]),
            vec![2],
            "Value<32>",
            Value::Int(300),
        ),
        (
            4,
            2,
            [0, 2, 2],
            words(&[1, 2, 3, 4]),
            vec![1],
            "Value<32>[2]",
            Value::Array(vec![Value::Int(3), Value::Int(4)]),
        ),
    ] {
        let (mut raw, base, start) = fixture(3);
        raw.truncate(start);
        put32(&mut raw, base, 1);
        put32(&mut raw, base + 8, record.len() as u32);
        put32(&mut raw, 84, 1);
        put32(&mut raw, 80, (start + aux.len()) as u32);
        put32(&mut raw, 96, 4);
        put16(&mut raw, start - 22, width);
        put32(&mut raw, start - 20, aux.len() as u32);
        put32(&mut raw, start - 16, codec);
        for (i, p) in parameters.iter().enumerate() {
            put32(&mut raw, start - 12 + i * 4, *p);
        }
        if codec == 2 {
            put32(&mut raw, base + 56, aux.len() as u32);
        } else if [3, 4].contains(&codec) {
            put32(&mut raw, base + 60, aux.len() as u32);
        }
        raw.extend(aux);
        raw.extend(record);
        raw.extend(words(&[7]));
        let def = definition("int ID\nint Value", &format!("$noninline,id$ID\n{field}"));
        let table = table::open(&raw, &def, limits()).unwrap();
        assert_eq!(table.row(7, 100).unwrap()["Value"], want, "codec {codec}");
    }
}
fn typed_sparse() -> Vec<u8> {
    let (base, _, _) = fixture(3);
    let mut raw = vec![0; 224];
    raw[..112].copy_from_slice(&base[..112]);
    for (at, value) in [
        (4, 1),
        (8, 4),
        (28, 10),
        (32, 10),
        (44, 4),
        (56, 96),
        (80, 224),
        (84, 1),
        (100, 20),
        (104, 1),
        (108, 1),
    ] {
        put32(&mut raw, at, value);
    }
    put16(&mut raw, 40, 1);
    for (i, width) in [32, 8, 32, 32].iter().enumerate() {
        put16(&mut raw, 128 + i * 24 + 2, *width);
    }
    let mut data = "猫\0".as_bytes().to_vec();
    data.push(249);
    data.extend(1.25f32.to_bits().to_le_bytes());
    data.extend(3u16.to_le_bytes());
    data.extend(u16::MAX.to_le_bytes());
    raw.extend(&data);
    let sparse_offset = raw.len();
    put32(&mut raw, 92, sparse_offset as u32);
    raw.extend(words(&[99, 10]));
    raw.extend(224u32.to_le_bytes());
    raw.extend((data.len() as u16).to_le_bytes());
    raw.extend(words(&[1, 0, 0, 0, 0]));
    raw.extend(words(&[10]));
    raw
}
#[test]
fn sparse_ordered_utf8_signed_float_arrays_and_copy_relations_match_go() {
    let raw = typed_sparse();
    let def = definition(
        "int ID\nstring Name\nint Score\nfloat Ratio\nint Flags\nint Parent",
        "$noninline,id$ID\nName\nScore<8>\nRatio\nFlags<u16>[2]\n$noninline,relation$Parent",
    );
    let table = table::open(&raw, &def, limits()).unwrap();
    for id in [10, 99] {
        let row = table.row(id, 3).unwrap();
        assert_eq!(row["Name"], Value::String("猫".into()));
        assert_eq!(row["Score"], Value::Int(-7));
        assert_eq!(row["Ratio"], Value::Float32(1.25));
        assert_eq!(
            row["Flags"],
            Value::Array(vec![Value::UInt(3), Value::UInt(65535)])
        );
        assert_eq!(row["Parent"], Value::UInt(0));
    }
    assert_eq!(table.row(10, 2).err().unwrap().code, "table.limit_exceeded");
}
#[test]
fn projection_still_rejects_corrupt_unused_fields() {
    let mut raw = typed_sparse();
    raw[229..233].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
    let def = definition(
        "int ID\nstring Name\nint Score\nfloat Ratio\nint Flags\nint Parent",
        "$noninline,id$ID\nName\nScore<8>\nRatio\nFlags<u16>[2]\n$noninline,relation$Parent",
    );
    let table = table::open(&raw, &def, limits()).unwrap();
    assert_eq!(
        table
            .row_projected(10, 100, Some(&BTreeSet::from(["ID".into()])))
            .err()
            .unwrap()
            .code,
        "table.invalid_format"
    );
}
#[test]
fn real_wdc5_fixture_identity_is_exact_and_empty_is_honest() {
    let raw = include_bytes!("../../../tests/fixtures/db2/JournalEncounterSection-classic-era.db2");
    assert_eq!(
        lycheedev_core::storage::sha256(raw),
        "992c9edb6bdaa11a0001e846d267b89a8477d3fed6fb7b4497c9dc642235c73a"
    );
    let layout = table::inspect(raw, limits()).unwrap();
    assert_eq!(
        (layout.version, layout.rows, layout.columns, layout.stride),
        (5, 0, 15, 8)
    );
    assert_eq!(layout.table_hash, 0x7956786c);
    assert_eq!(layout.layout_hash, 0xcb88312b);
    assert_eq!(layout.schema_build, "WOWSTATIC_1_15_8_63631");
}
#[test]
fn decode_work_and_pagination_are_bounded_and_views_are_concurrently_readable() {
    let raw = records(true, &[99, 10], &[]);
    let def = definition("int ID\nint Value", "$noninline,id$ID\nValue<32>");
    let table = table::open(&raw, &def, limits()).unwrap();
    let page = table.page(None, 1, 100).unwrap();
    assert!(page.truncated);
    assert_eq!(page.after_id, Some(10));
    assert_eq!(table.page(page.after_id, 100, 100).unwrap().rows.len(), 2);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let table = &table;
            scope.spawn(move || {
                for _ in 0..100 {
                    assert_eq!(table.row(99, 100).unwrap()["Value"], Value::Int(1));
                }
            });
        }
    });
    let mut budget = limits();
    budget.decode_work = 4;
    assert!(table::open(&raw, &def, budget).is_err());
}

#[test]
fn missing_partition_ranges_do_not_turn_encrypted_placeholders_into_rows() {
    let mut raw = vec![0; 196];
    let (base, _, _) = fixture(3);
    raw[..72].copy_from_slice(&base[..72]);
    put32(&mut raw, 68, 2);
    for (index, offset) in [180u32, 188].iter().enumerate() {
        let at = 72 + index * 40;
        put32(&mut raw, at + 8, *offset);
        put32(&mut raw, at + 12, 1);
        put32(&mut raw, at + 24, 4);
    }
    raw[112..120].copy_from_slice(&7u64.to_le_bytes());
    put16(&mut raw, 158, 32);
    put32(&mut raw, 180, 11);
    put32(&mut raw, 184, 10);
    put32(&mut raw, 188, 22);
    put32(&mut raw, 192, 20);
    let def = definition("int ID\nint Value", "$noninline,id$ID\nValue<32>");
    let unavailable = BTreeSet::from([1]);
    let missing = 188..196;
    let table = table::open_available(
        &raw,
        &def,
        limits(),
        &unavailable,
        std::slice::from_ref(&missing),
    )
    .unwrap();
    assert_eq!(table.ids().collect::<Vec<_>>(), [10]);
    assert_eq!(table.row(10, 100).unwrap()["Value"], Value::Int(11));
    assert_eq!(table.coverage()["complete"], false);
    assert_eq!(
        table.row(20, 100).err().unwrap().code,
        "table.record_not_found"
    );
    assert!(
        table::open_available(
            &raw,
            &def,
            limits(),
            &BTreeSet::from([0]),
            std::slice::from_ref(&missing)
        )
        .is_err()
    );
    assert_eq!(
        table::open_available(
            &raw,
            &def,
            limits(),
            &BTreeSet::new(),
            std::slice::from_ref(&missing)
        )
        .err()
        .unwrap()
        .code,
        "table.partition_unavailable"
    );
}
fn sparse_version(version: u8, secondary: bool) -> Vec<u8> {
    let (mut raw, base, start) = fixture(version);
    raw.truncate(start);
    put16(&mut raw, base + 36, if secondary { 3 } else { 1 });
    put32(&mut raw, base + 24, 10);
    put32(&mut raw, base + 28, if version == 2 { 13 } else { 20 });
    raw.extend(words(&[111, 222, 333]));
    let section = 72 + if version == 5 { 132 } else { 0 };
    let append_entry = |raw: &mut Vec<u8>, offset: u32, length: u16| {
        raw.extend(offset.to_le_bytes());
        raw.extend(length.to_le_bytes());
    };
    if version == 2 {
        put32(&mut raw, section + 24, (start + 12) as u32);
        put32(&mut raw, section + 20, 8);
        put32(&mut raw, section + 32, 20);
        append_entry(&mut raw, start as u32, 4);
        append_entry(&mut raw, 0, 0);
        append_entry(&mut raw, (start + 4) as u32, 8);
        append_entry(&mut raw, (start + 4) as u32, 8);
        raw.extend(words(&[99, 10]));
        raw.extend(words(&[1, 0, 0, 777, 1]));
    } else {
        put32(&mut raw, section + 20, (start + 12) as u32);
        put32(&mut raw, section + 28, 20);
        put32(&mut raw, section + 32, 2);
        put32(&mut raw, section + 36, 1);
        raw.extend(words(&[99, 10]));
        append_entry(&mut raw, start as u32, 4);
        append_entry(&mut raw, (start + 4) as u32, 8);
        let id = if secondary && version >= 4 {
            raw.extend(words(&[10, 20]));
            20
        } else {
            1
        };
        raw.extend(words(&[1, 0, 0, 777, id]));
        if !secondary || version < 4 {
            raw.extend(words(&[10, 20]));
        }
    }
    raw
}
#[test]
fn sparse_all_versions_and_secondary_relationship_order_preserve_physical_views() {
    let def = definition(
        "int ID\nint Value\nint Parent",
        "$noninline,id$ID\nValue<32>\n$noninline,relation$Parent",
    );
    for version in [2, 3, 4, 5] {
        for secondary in [false, true] {
            let raw = sparse_version(version, secondary);
            let table = table::open(&raw, &def, limits()).unwrap();
            let second = if version == 2 { 12 } else { 20 };
            assert_eq!(table.raw_record(10).unwrap().len(), 4);
            assert_eq!(table.raw_record(second).unwrap().len(), 8);
            assert_eq!(table.raw_record(99).unwrap().len(), 4);
            assert_eq!(
                table.raw_record(99).unwrap().as_ptr(),
                table.raw_record(10).unwrap().as_ptr()
            );
            if version == 2 {
                assert_eq!(
                    table.raw_record(13).unwrap().as_ptr(),
                    table.raw_record(12).unwrap().as_ptr()
                );
            }
        }
    }
}
#[test]
fn malformed_sparse_maps_fail_for_offsets_overlap_ids_counts_and_truncation() {
    let def = definition(
        "int ID\nint Value\nint Parent",
        "$noninline,id$ID\nValue<32>\n$noninline,relation$Parent",
    );
    let (base, _, start) = fixture(3);
    drop(base);
    let map = start + 12 + 8;
    for (name, at, value) in [
        ("before", map, 1),
        ("after", map, (start + 10) as u32),
        ("overlap", map + 6, (start + 2) as u32),
        ("count", 104, 1),
        ("relation", start + 12 + 8 + 12 + 16, 99),
    ] {
        let mut raw = sparse_version(3, false);
        put32(&mut raw, at, value);
        assert!(table::open(&raw, &def, limits()).is_err(), "{name}");
    }
    let mut raw = sparse_version(3, false);
    put16(&mut raw, map + 4, 0);
    assert!(table::open(&raw, &def, limits()).is_err());
    let mut raw = sparse_version(3, false);
    let len = raw.len();
    let id = raw[len - 8..len - 4].to_vec();
    raw[len - 4..].copy_from_slice(&id);
    assert!(table::open(&raw, &def, limits()).is_err());
    let raw = sparse_version(3, false);
    assert!(table::open(&raw[..raw.len() - 1], &def, limits()).is_err());
}
#[test]
fn packed_unsigned_64_never_rounds_through_float() {
    let (mut raw, base, start) = fixture(3);
    raw.truncate(start);
    put32(&mut raw, base, 1);
    put32(&mut raw, base + 8, 8);
    put32(&mut raw, 84, 1);
    put32(&mut raw, 96, 4);
    put16(&mut raw, start - 22, 64);
    raw.extend(u64::MAX.to_le_bytes());
    raw.extend(words(&[7]));
    let def = definition("int ID\nint Value", "$noninline,id$ID\nValue<u64>");
    let table = table::open(&raw, &def, limits()).unwrap();
    assert_eq!(table.row(7, 100).unwrap()["Value"], Value::UInt(u64::MAX));
    assert_eq!(
        serde_json::to_string(&table.row(7, 100).unwrap()["Value"]).unwrap(),
        "18446744073709551615"
    );
}

#[test]
fn source_float32_serialization_keeps_original_precision_for_arrays_and_negative_zero() {
    let (mut raw, base, start) = fixture(3);
    raw.truncate(start);
    put32(&mut raw, base + 8, 8);
    put32(&mut raw, 96, 8);
    put16(&mut raw, start - 22, 64);
    for value in [-230.989f32, 1571.57f32, f32::from_bits(0x80000000), 1.25] {
        raw.extend(value.to_bits().to_le_bytes());
    }
    raw.extend(words(&[1, 2]));
    let def = definition("int ID\nfloat Corpse", "$noninline,id$ID\nCorpse[2]");
    let table = table::open(&raw, &def, limits()).unwrap();
    let first = table.row(1, 100).unwrap();
    assert_eq!(
        serde_json::to_string(&first["Corpse"]).unwrap(),
        "[-230.989,1571.57]"
    );
    let envelope = serde_json::json!({"schema":"lycheedev.result.v1","result":serde_json::to_value(&first).unwrap()});
    assert_eq!(
        envelope["result"]["Corpse"].to_string(),
        "[-230.989,1571.57]"
    );
    let second = table.row(2, 100).unwrap();
    match &second["Corpse"] {
        Value::Array(values) => match values[0] {
            Value::Float32(value) => assert_eq!(value.to_bits(), 0x80000000),
            _ => panic!("source width lost"),
        },
        _ => panic!("array lost"),
    };
    let mut bad = raw.clone();
    bad[start + 4..start + 8].copy_from_slice(&f32::INFINITY.to_bits().to_le_bytes());
    let damaged = table::open(&bad, &def, limits()).unwrap();
    assert_eq!(
        damaged.row(1, 100).err().unwrap().code,
        "table.invalid_format"
    );
}

#[test]
#[ignore = "explicit local same-work performance experiment; not a functional acceptance skip"]
fn local_wdc_same_work_benchmark() {
    let input = std::env::var("LYCHEEDEV_WDC_BENCH_FILE").expect("explicit benchmark file");
    let output = std::env::var("LYCHEEDEV_WDC_BENCH_OUTPUT").expect("explicit benchmark output");
    let raw = std::fs::read(input).unwrap();
    let def = definition("int ID\nint Value", "$id$ID<u32>\nValue<u64>");
    let budget = TableLimits {
        file_bytes: 8 << 20,
        metadata_bytes: 64 << 20,
        rows: 50000,
        columns: 100,
        partitions: 100,
        decode_work: 2_000_000,
    };
    let mut samples = vec![];
    for _ in 0..20 {
        let start = std::time::Instant::now();
        let table = table::open(&raw, &def, budget).unwrap();
        let prepare = start.elapsed().as_nanos();
        let start = std::time::Instant::now();
        let mut checksum = 0u64;
        for index in 0..100000u64 {
            let id = ((index * 7919) % 50000 + 1) as u32;
            let row = table.row(id, 100).unwrap();
            match row["Value"] {
                Value::UInt(value) => checksum = checksum.wrapping_add(value),
                _ => panic!("integer type lost"),
            }
        }
        let decode = start.elapsed().as_nanos();
        samples.push(serde_json::json!({"prepareNs":prepare,"decodeNs":decode,"totalNs":prepare+decode,"checksum":checksum.to_string()}));
    }
    std::fs::write(output,serde_json::to_vec_pretty(&serde_json::json!({"schema":"lycheedev.wdc-local-performance.v1","backend":"rust","records":50000,"lookups":100000,"samples":samples,"scope":"prepare view plus typed integer row lookups; excludes CLI/CASC/DBD resolution and I/O"})).unwrap()).unwrap();
}
