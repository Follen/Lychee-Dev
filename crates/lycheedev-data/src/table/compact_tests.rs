// SPDX-License-Identifier: MIT
//! Retained wire compact-index regressions. The frozen reference fixture and test hashes are
//! recorded separately; declarations here are not evidence of execution.
use super::*;
use crate::schema::Document;
use std::time::{Duration, Instant};

fn put32(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put16(raw: &mut [u8], at: usize, value: u16) {
    raw[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn definition(columns: &str, fields: &str) -> Definition {
    Document::parse(
        format!("COLUMNS\n{columns}\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n{fields}\n").as_bytes(),
    )
    .unwrap()
    .select("12.1.0.69875", "12345678")
    .unwrap()
}
fn values_definition() -> Definition {
    definition(
        "int ID\nint Value\nint Parent",
        "$noninline,id$ID\nValue<32>\n$noninline,relation$Parent",
    )
}
fn fixture(version: u8) -> (Vec<u8>, usize, usize) {
    let header = 72 + if version == 5 { 132 } else { 0 };
    let section = if version == 2 { 36 } else { 40 };
    let start = header + section + 4 + 24;
    let base = 4 + if version == 5 { 132 } else { 0 };
    let mut raw = vec![0; start + 8];
    raw[..4].copy_from_slice(format!("WDC{version}").as_bytes());
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
    put16(&mut raw, start - 22, 32);
    put32(&mut raw, start, 1);
    put32(&mut raw, start + 4, 2);
    (raw, base, start)
}
fn records(copies: &[u32], relations: &[u32]) -> Vec<u8> {
    let (mut raw, _, _) = fixture(3);
    put32(&mut raw, 96, 8);
    put32(&mut raw, 108, (copies.len() / 2) as u32);
    put32(&mut raw, 100, (relations.len() * 4) as u32);
    raw.extend(words(&[10, 20]));
    raw.extend(words(copies));
    raw.extend(words(relations));
    raw
}
fn sparse(version: u8, secondary: bool) -> Vec<u8> {
    let (mut raw, base, start) = fixture(version);
    raw.truncate(start);
    put16(&mut raw, base + 36, if secondary { 3 } else { 1 });
    put32(&mut raw, base + 24, 10);
    put32(&mut raw, base + 28, if version == 2 { 13 } else { 20 });
    raw.extend(words(&[111, 222, 333]));
    let section = 72 + if version == 5 { 132 } else { 0 };
    let entry = |raw: &mut Vec<u8>, offset: u32, length: u16| {
        raw.extend(offset.to_le_bytes());
        raw.extend(length.to_le_bytes());
    };
    if version == 2 {
        put32(&mut raw, section + 24, (start + 12) as u32);
        put32(&mut raw, section + 20, 8);
        put32(&mut raw, section + 32, 20);
        entry(&mut raw, start as u32, 4);
        entry(&mut raw, 0, 0);
        entry(&mut raw, (start + 4) as u32, 8);
        entry(&mut raw, (start + 4) as u32, 8);
        raw.extend(words(&[99, 10]));
        raw.extend(words(&[1, 0, 0, 777, 1]));
    } else {
        put32(&mut raw, section + 20, (start + 12) as u32);
        put32(&mut raw, section + 28, 20);
        put32(&mut raw, section + 32, 2);
        put32(&mut raw, section + 36, 1);
        raw.extend(words(&[99, 10]));
        entry(&mut raw, start as u32, 4);
        entry(&mut raw, (start + 4) as u32, 8);
        let ordinal = if secondary && version >= 4 {
            raw.extend(words(&[10, 20]));
            20
        } else {
            1
        };
        raw.extend(words(&[1, 0, 0, 777, ordinal]));
        if !secondary || version < 4 {
            raw.extend(words(&[10, 20]));
        }
    }
    raw
}
fn aliases(count: u32) -> Vec<u8> {
    let mut raw = sparse(2, false);
    let at = u32at(&raw, 96).unwrap() as usize;
    let mut suffix = raw[at + 24..].to_vec();
    put32(&mut suffix, 0, 20000);
    // Copy through the last implicit alias, not directly through its origin.
    put32(&mut suffix, 4, 12 + count);
    let entry = raw[at + 18..at + 24].to_vec();
    raw.truncate(at + 18);
    for _ in 0..count {
        raw.extend_from_slice(&entry);
    }
    raw.extend(suffix);
    put32(&mut raw, 32, 12 + count);
    raw
}
fn budget(caps: Cost) -> Arc<Budget> {
    Arc::new(Budget::new(
        caps,
        Instant::now() + Duration::from_secs(120),
        None,
    ))
}

#[test]
fn compact_copy_chains_share_one_physical_slot_and_preserve_sorted_logical_rows() {
    let raw = records(&[101, 100, 100, 10], &[1, 0, 0, 0, 0]);
    let table = open(&raw, &values_definition(), TableLimits::default()).unwrap();
    assert_eq!(std::mem::size_of::<Location>(), 32);
    assert_eq!(table.physical.len(), 2);
    assert_eq!(table.locations.len(), 4);
    assert_eq!(table.ids().collect::<Vec<_>>(), [10, 20, 100, 101]);
    for id in [10, 100, 101] {
        let index: u32 = table.locations[&id];
        assert_eq!(index, table.locations[&10]);
        assert!(std::ptr::eq(
            table.location(id).unwrap(),
            table.location(10).unwrap()
        ));
        let row = table.leased_row_values(id, 100).unwrap();
        assert_eq!(row.value[0], Value::UInt(u64::from(id)));
        assert_eq!(row.value[1], Value::Int(1));
        assert_eq!(row.value[2], Value::UInt(0));
    }
    assert_eq!(table.leased_row_values(20, 100).unwrap().value[2], Value::Null);
    assert_eq!(table.logical_count(), 4);
}

#[test]
fn compact_record_index_fits_large_table_after_source_preparation() {
    // Exact retained wire shape: 189102 physical rows + 227231 explicit copies,
    // with 96 MiB already admitted and only 48 MiB of query metadata left.
    const PHYSICAL: u32 = 189102;
    const COPIES: u32 = 227231;
    let (mut raw, base, start) = fixture(5);
    raw.resize(start + (PHYSICAL as usize + COPIES as usize) * 8, 0);
    put32(&mut raw, base, PHYSICAL);
    put32(&mut raw, base + 28, PHYSICAL + COPIES);
    put32(&mut raw, 204 + 12, PHYSICAL);
    put32(&mut raw, 204 + 24, PHYSICAL * 4);
    put32(&mut raw, 204 + 36, COPIES);
    for i in 0..PHYSICAL {
        put32(&mut raw, start + i as usize * 4, i + 1);
        put32(
            &mut raw,
            start + PHYSICAL as usize * 4 + i as usize * 4,
            i + 1,
        );
    }
    for i in 0..COPIES {
        let at = start + PHYSICAL as usize * 8 + i as usize * 8;
        put32(&mut raw, at, PHYSICAL + i + 1);
        put32(&mut raw, at + 4, i % PHYSICAL + 1);
    }
    let query = budget(Cost {
        metadata_bytes: 144 << 20,
        ..Cost::defaults()
    });
    query.charge(Cost::metadata(96 << 20)).unwrap();
    let limits = TableLimits {
        file_bytes: 128 << 20,
        rows: 1_000_000,
        ..Default::default()
    };
    let def = definition("int ID\nint Value", "$noninline,id$ID\nValue<32>");
    let table = open_budgeted(&raw, &def, limits, &BTreeSet::new(), &[], query.clone()).unwrap();
    let id = PHYSICAL + COPIES;
    let origin = (COPIES - 1) % PHYSICAL + 1;
    assert_eq!(table.physical.len(), PHYSICAL as usize);
    assert_eq!(table.locations.len(), id as usize);
    assert_eq!(table.locations[&id], table.locations[&origin]);
    assert_eq!(table.location(id).unwrap().origin, origin);
    assert_eq!(table.raw_record(id).unwrap(), origin.to_le_bytes());
    let row = table.leased_row_values(id, 100).unwrap();
    assert_eq!(row.value[0], Value::UInt(u64::from(id)));
    assert_eq!(row.value[1], Value::Int(i64::from(origin)));
    let logical_index =
        u64::from(PHYSICAL + COPIES) * 32 + u64::from(PHYSICAL) * 40 + u64::from(COPIES) * 72 + 4;
    assert_eq!(
        index_allocation(&table.layout, u64::from(COPIES)),
        (logical_index, logical_index + 256)
    );
    assert!(table.admitted_metadata_bytes() < 48 << 20);
    assert!(query.logical_snapshot().unwrap().metadata_bytes <= 144 << 20);
    assert!(query.snapshot().unwrap().metadata_bytes <= 144 << 20);
}

#[test]
fn sparse_implicit_aliases_share_relationships_and_charge_exactly_32_bytes_each() {
    let def = values_definition();
    let one_raw = aliases(1);
    let one = open(&one_raw, &def, TableLimits::default()).unwrap();
    let raw = aliases(1000);
    let query = budget(Cost::defaults());
    let table = open_budgeted(
        &raw,
        &def,
        TableLimits::default(),
        &BTreeSet::new(),
        &[],
        query.clone(),
    )
    .unwrap();
    assert_eq!(table.physical.len(), 2);
    assert_eq!(table.locations.len(), 1003);
    assert_eq!(table.logical_count(), 1003);
    for id in [12, 13, 1012, 20000] {
        assert_eq!(table.locations[&id], table.locations[&12]);
        assert!(std::ptr::eq(
            table.location(id).unwrap(),
            table.location(12).unwrap()
        ));
        let location = table.location(id).unwrap();
        assert_eq!(location.origin, 12);
        assert_eq!(location.relation, Some(777));
        assert_eq!(table.raw_record(id).unwrap(), words(&[222, 333]));
    }
    assert_eq!(
        table.admitted_metadata_bytes() - one.admitted_metadata_bytes(),
        999 * 32
    );
    assert_eq!(
        query.logical_snapshot().unwrap().metadata_bytes + 256,
        table.admitted_metadata_bytes()
    );
    let small = budget(Cost {
        metadata_bytes: 16 << 10,
        ..Cost::defaults()
    });
    assert_eq!(
        open_budgeted(
            &raw,
            &def,
            TableLimits::default(),
            &BTreeSet::new(),
            &[],
            small.clone()
        )
        .err()
        .unwrap()
        .code,
        "records.query_resource_budget"
    );
    assert!(small.snapshot().unwrap().metadata_bytes <= 16 << 10);
}

#[test]
fn wdc3_plus_same_offset_rows_keep_distinct_origins_and_relationships() {
    for version in [3, 4, 5] {
        for secondary in [false, true] {
            let mut raw = sparse(version, secondary);
            let section = 72 + if version == 5 { 132 } else { 0 };
            let map = u32at(&raw, section + 20).unwrap() as usize + 8;
            let offset = u32at(&raw, map).unwrap();
            put32(&mut raw, map + 6, offset);
            put16(&mut raw, map + 10, 4);
            let table = open(&raw, &values_definition(), TableLimits::default()).unwrap();
            assert_eq!(table.physical.len(), 2);
            assert_ne!(table.locations[&10], table.locations[&20]);
            assert_eq!(table.locations[&99], table.locations[&10]);
            assert_eq!(table.location(20).unwrap().origin, 20);
            assert_eq!(table.location(20).unwrap().relation, Some(777));
            assert_eq!(table.location(10).unwrap().relation, None);
            assert_eq!(table.location(99).unwrap().relation, None);
            assert_eq!(table.leased_row_values(20, 100).unwrap().value[2], Value::UInt(777));
            assert_eq!(table.leased_row_values(99, 100).unwrap().value[2], Value::Null);
            assert_eq!(
                table.raw_record(10).unwrap().as_ptr(),
                table.raw_record(20).unwrap().as_ptr()
            );
        }
    }
}

#[test]
fn compact_copy_uses_origin_common_override_and_requested_inline_identity() {
    let raw = {
        let (mut raw, base, start) = fixture(3);
        raw.truncate(start);
        put32(&mut raw, base, 1);
        put32(&mut raw, base + 56, 8);
        put32(&mut raw, 80, (start + 8) as u32);
        put32(&mut raw, 84, 1);
        put32(&mut raw, 96, 4);
        put32(&mut raw, 108, 1);
        put16(&mut raw, start - 22, 0);
        put32(&mut raw, start - 20, 8);
        put32(&mut raw, start - 16, 2);
        put32(&mut raw, start - 12, 33);
        raw.extend(words(&[7, 99])); // common override belongs to origin 7
        raw.extend(words(&[0, 7, 100, 7])); // data, external ID, copy pair
        raw
    };
    let table = open(&raw, &values_definition(), TableLimits::default()).unwrap();
    assert_eq!(table.physical.len(), 1);
    assert_eq!(table.leased_row_values(100, 100).unwrap().value[1], Value::Int(99));
    let (mut raw, _, _) = fixture(3);
    put32(&mut raw, 108, 1);
    raw.extend(words(&[100, 1]));
    let table = open(
        &raw,
        &definition("int ID", "$id$ID<32>"),
        TableLimits::default(),
    )
    .unwrap();
    assert_eq!(table.leased_row_values(100, 100).unwrap().value[0], Value::UInt(100));
    assert_eq!(table.raw_record(100).unwrap(), 1u32.to_le_bytes());
}

#[test]
fn compact_index_rejects_duplicates_cycles_missing_origins_and_relation_conflicts() {
    let mut duplicate_physical = records(&[], &[]);
    let len = duplicate_physical.len();
    put32(&mut duplicate_physical, len - 4, 10);
    let mut alias_collision = aliases(1);
    let map = u32at(&alias_collision, 96).unwrap() as usize;
    put32(&mut alias_collision, map + 24, 13);
    let mut inconsistent_alias_length = aliases(1);
    put16(&mut inconsistent_alias_length, map + 22, 4);
    for raw in [
        duplicate_physical,
        alias_collision,
        inconsistent_alias_length,
        records(&[100, 101, 101, 100], &[]),
        records(&[100, 999], &[]),
        records(&[100, 10, 100, 20], &[]),
        records(&[10, 20], &[]),
        records(&[], &[1, 0, 0, 5, 2]),
        records(&[], &[2, 0, 0, 5, 0, 6, 0]),
    ] {
        assert_eq!(
            open(&raw, &values_definition(), TableLimits::default())
                .err()
                .unwrap()
                .code,
            "table.invalid_format"
        );
    }
}

#[test]
fn compact_index_excludes_missing_partitions_without_changing_partial_coverage() {
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
    for (at, value) in [(180, 11), (184, 10), (188, 22), (192, 20)] {
        put32(&mut raw, at, value);
    }
    let table = open_available(
        &raw,
        &values_definition(),
        TableLimits::default(),
        &BTreeSet::from([1]),
        &[188..196],
    )
    .unwrap();
    assert_eq!(table.physical.len(), 1);
    assert_eq!(table.ids().collect::<Vec<_>>(), [10]);
    assert_eq!(table.leased_row_values(10, 100).unwrap().value[1], Value::Int(11));
    assert_eq!(table.coverage()["complete"], false);
    assert_eq!(table.coverage()["physicalRows"], 2);
    assert_eq!(table.coverage()["availableRows"], 1);
    assert_eq!(
        table.raw_record(20).unwrap_err().code,
        "table.record_not_found"
    );
    assert_eq!(
        open_available(
            &raw,
            &values_definition(),
            TableLimits::default(),
            &BTreeSet::new(),
            &[188..196]
        )
        .err()
        .unwrap()
        .code,
        "table.partition_unavailable"
    );
}

#[test]
fn sparse_alias_row_limit_precedes_new_alias_query_charge() {
    let def = values_definition();
    let one_raw = aliases(1);
    let one = open(&one_raw, &def, TableLimits::default()).unwrap();
    let raw = aliases(1000);
    let query = budget(Cost {
        metadata_bytes: one.admitted_metadata_bytes(),
        ..Cost::defaults()
    });
    // Two physical rows plus one declared copy pass the header bound. The
    // second implicit alias must fail the logical-row cap before charging it.
    let limits = TableLimits {
        rows: 3,
        ..Default::default()
    };
    assert_eq!(
        open_budgeted(&raw, &def, limits, &BTreeSet::new(), &[], query.clone())
            .err()
            .unwrap()
            .code,
        "table.limit_exceeded"
    );
    assert_eq!(
        query.snapshot().unwrap().metadata_bytes,
        one.admitted_metadata_bytes()
    );
}

// Valid ranges, unique IDs, and distinct nonoverlapping bytes; only the
// declaration of one physical row is wrong. Count failure belongs at map end.
fn underdeclared_sparse(count: u32) -> Vec<u8> {
    let (mut raw, base, start) = fixture(2);
    raw.truncate(start);
    put16(&mut raw, base + 36, 1);
    put32(&mut raw, base, 1);
    put32(&mut raw, base + 28, count);
    put32(&mut raw, 84, 1);
    put32(&mut raw, 96, start as u32 + count);
    raw.resize(start + count as usize, 1);
    for i in 0..count {
        raw.extend((start as u32 + i).to_le_bytes());
        raw.extend(1u16.to_le_bytes());
    }
    raw
}

#[test]
fn malformed_sparse_capacity_admission_is_safety_only_and_keeps_late_format_error() {
    let raw = underdeclared_sparse(100000);
    let def = values_definition();
    let generous = budget(Cost::defaults());
    assert_eq!(
        open_budgeted(
            &raw,
            &def,
            TableLimits::default(),
            &BTreeSet::new(),
            &[],
            generous.clone()
        )
        .err()
        .unwrap()
        .code,
        "table.invalid_format"
    );
    let tight = budget(Cost {
        metadata_bytes: 16 << 10,
        ..Cost::defaults()
    });
    assert_eq!(
        open_budgeted(
            &raw,
            &def,
            TableLimits::default(),
            &BTreeSet::new(),
            &[],
            tight.clone()
        )
        .err()
        .unwrap()
        .code,
        "records.query_resource_budget"
    );
    // Failed safety admission is atomic, and successful admission never enters
    // the Go logical ledger. All original pre-index charges remain identical.
    assert_eq!(
        generous.logical_snapshot().unwrap(),
        tight.logical_snapshot().unwrap()
    );
    assert!(generous.snapshot().unwrap().metadata_bytes > tight.snapshot().unwrap().metadata_bytes);
    assert!(tight.snapshot().unwrap().metadata_bytes < 16 << 10);
}

#[test]
fn malformed_sparse_reservation_precedes_first_unreserved_physical_insert() {
    let raw = underdeclared_sparse(100000);
    let limits = TableLimits::default();
    let query = budget(Cost {
        metadata_bytes: 16 << 10,
        ..Cost::defaults()
    });
    let source = Source::new(&raw, &[], limits, Some(query.clone())).unwrap();
    let layout = inspect_source(&source, limits).unwrap();
    let p = layout.partitions[0].clone();
    let aux = &raw[p.sparse_offset as usize..];
    let mut table = Table {
        source,
        layout,
        programs: vec![],
        locations: HashMap::with_capacity(1),
        physical: Vec::with_capacity(1),
        ids: vec![],
        definition: values_definition(),
        column_mapping: vec![],
        unavailable: BTreeSet::new(),
        work: AtomicU64::new(0),
        limits,
    };
    let before = query.logical_snapshot().unwrap();
    let physical_capacity = table.physical.capacity();
    let map_capacity = table.locations.capacity();
    assert_eq!(
        table.index_sparse(0, &p, aux).err().unwrap().code,
        "records.query_resource_budget"
    );
    assert_eq!(table.physical.len(), 1);
    assert_eq!(table.locations.len(), 1);
    assert_eq!(table.physical.capacity(), physical_capacity);
    assert_eq!(table.locations.capacity(), map_capacity);
    assert_eq!(query.logical_snapshot().unwrap(), before);
}

#[test]
fn malformed_growth_bound_checks_overflow_and_large_capacity_cliffs() {
    assert!(compact_growth_peak::<Location>(0, usize::MAX, 1, 3, 0).is_none());
    assert!(compact_growth_peak::<Location>(usize::MAX, 0, 1, 3, 0).is_none());
    assert!(compact_growth_peak::<u32>(1, 0, 1, 4, usize::MAX).is_none());
    let capacity = 1usize << 20;
    let peak = compact_growth_peak::<Location>(capacity, capacity, 1, 3, 0).unwrap();
    assert!(peak >= capacity * 3 * std::mem::size_of::<Location>());
    // The charge is based on the whole future domain, not one extra row.
    assert!(peak > 1 << 20);
}

#[test]
fn owned_table_moves_original_body_and_retains_it_through_shared_consumers() {
    struct Body {
        bytes: Vec<u8>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }
    impl AsRef<[u8]> for Body {
        fn as_ref(&self) -> &[u8] {
            &self.bytes
        }
    }
    impl Drop for Body {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
    let raw = records(&[100, 10], &[1, 0, 0, 0, 0]);
    let pointer = raw.as_ptr();
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let table = Arc::new(
        open_owned_budgeted(
            Body {
                bytes: raw,
                dropped: dropped.clone(),
            },
            &values_definition(),
            TableLimits::default(),
            &BTreeSet::new(),
            &[],
            budget(Cost::defaults()),
        )
        .unwrap(),
    );
    assert_eq!(table.source.raw.as_ptr(), pointer);
    let borrowed = table.clone();
    drop(table);
    assert!(!dropped.load(Ordering::SeqCst));
    assert_eq!(borrowed.leased_row_values(100, 1 << 20).unwrap().value[1], Value::Int(1));
    drop(borrowed);
    assert!(dropped.load(Ordering::SeqCst));
}

#[test]
fn owned_table_does_not_rebuild_or_double_charge_compact_index() {
    let raw = records(&[101, 100, 100, 10], &[1, 0, 0, 0, 0]);
    let borrowed_budget = budget(Cost::defaults());
    let owned_budget = budget(Cost::defaults());
    let borrowed = open_budgeted(
        &raw,
        &values_definition(),
        TableLimits::default(),
        &BTreeSet::new(),
        &[],
        borrowed_budget.clone(),
    )
    .unwrap();
    let owned = open_owned_budgeted(
        raw.clone(),
        &values_definition(),
        TableLimits::default(),
        &BTreeSet::new(),
        &[],
        owned_budget.clone(),
    )
    .unwrap();
    assert_eq!(
        borrowed_budget.logical_snapshot().unwrap(),
        owned_budget.logical_snapshot().unwrap()
    );
    assert_eq!(
        borrowed.ids().collect::<Vec<_>>(),
        owned.ids().collect::<Vec<_>>()
    );
    assert_eq!(borrowed.physical.len(), owned.physical.len());
    assert_eq!(owned.locations[&101], owned.locations[&10]);
    for id in [10, 20, 100, 101] {
        assert_eq!(
            borrowed.leased_row_values(id, 1 << 20).unwrap().value,
            owned.leased_row_values(id, 1 << 20).unwrap().value
        );
    }
}

#[test]
fn owned_table_failure_drops_body_and_preserves_original_table_error() {
    struct Body {
        bytes: Vec<u8>,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    }
    impl AsRef<[u8]> for Body {
        fn as_ref(&self) -> &[u8] {
            &self.bytes
        }
    }
    impl Drop for Body {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let error = open_owned_budgeted(
        Body {
            bytes: b"WDC3".to_vec(),
            dropped: dropped.clone(),
        },
        &values_definition(),
        TableLimits::default(),
        &BTreeSet::new(),
        &[],
        budget(Cost::defaults()),
    )
    .err()
    .unwrap();
    assert_eq!(error.code, "table.invalid_format");
    assert!(dropped.load(Ordering::SeqCst));
}
