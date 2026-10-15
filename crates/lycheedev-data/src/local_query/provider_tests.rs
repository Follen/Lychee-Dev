//! Real WDC provider boundary regressions for the fixed Go 3.2.3 TableSource.
//! Native execution status is recorded separately; these are executable tests.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

fn put32(raw: &mut [u8], at: usize, value: u32) {
    raw[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn definition() -> schema::Definition {
    schema::Document::parse(b"COLUMNS\nint ID\nint Value\n\nBUILD 12.1.0.69875\nLAYOUT 12345678\n$noninline,id$ID\nValue<32>\n")
        .unwrap().select("12.1.0.69875", "12345678").unwrap()
}
fn fixture() -> Vec<u8> {
    // Two physical records with external IDs 10,20 and copy chain 101->100->10.
    let mut raw = vec![0; 148];
    raw[..4].copy_from_slice(b"WDC3");
    for (at, value) in [
        (4, 2),
        (8, 1),
        (12, 4),
        (20, 0xaabbccdd),
        (24, 0x12345678),
        (28, 10),
        (32, 20),
        (44, 1),
        (56, 24),
        (68, 1),
        (80, 140),
        (84, 2),
        (96, 8),
        (108, 2),
    ] {
        put32(&mut raw, at, value);
    }
    raw[118..120].copy_from_slice(&32u16.to_le_bytes());
    put32(&mut raw, 140, 11);
    put32(&mut raw, 144, 22);
    for value in [10u32, 20, 101, 100, 100, 10] {
        raw.extend(value.to_le_bytes());
    }
    raw
}
fn budget() -> Arc<Budget> {
    Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        None,
    ))
}
fn physical<'a>(
    raw: &'a [u8],
    definition: &schema::Definition,
    budget: &Arc<Budget>,
) -> Physical<'a> {
    Physical::new(
        table::open_budgeted(
            raw,
            definition,
            Default::default(),
            &Default::default(),
            &[],
            budget.clone(),
        )
        .unwrap(),
        budget.clone(),
    )
    .unwrap()
}
fn scan(provider: &dyn RowProvider) -> Vec<Vec<Scalar>> {
    let mut rows = vec![];
    provider
        .scan(&mut |row| {
            rows.push(row.to_vec());
            Ok(())
        })
        .unwrap();
    rows
}
fn equal(
    provider: &dyn RowProvider,
    column: &str,
    value: Scalar,
) -> (EqualityOutcome, Vec<Vec<Scalar>>) {
    let mut rows = vec![];
    let outcome = provider
        .equal(column, &value, &mut |row| {
            rows.push(row.to_vec());
            Ok(())
        })
        .unwrap();
    (outcome, rows)
}
#[test]
fn physical_scan_sorted_copy_ids_and_equal_use_requested_identity() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(&raw, &d, &b);
    let rows = scan(&p);
    assert_eq!(
        rows.iter().map(|r| r[0].clone()).collect::<Vec<_>>(),
        [
            Scalar::UInt(10),
            Scalar::UInt(20),
            Scalar::UInt(100),
            Scalar::UInt(101)
        ]
    );
    assert_eq!(rows[0][1], Scalar::Int(11));
    for id in [10, 100, 101] {
        assert_eq!(
            equal(&p, "id", Scalar::UInt(id)),
            (
                EqualityOutcome::Handled,
                vec![vec![Scalar::UInt(id), Scalar::Int(11)]]
            )
        );
    }
    assert!(equal(&p, "ID", Scalar::UInt(999)).1.is_empty());
}
#[test]
fn physical_projection_keeps_full_width_and_unused_nulls() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(&raw, &d, &b);
    for indices in [vec![], vec![1], vec![1, 1]] {
        let mut rows = vec![];
        p.scan_selected(&indices, &mut |row| {
            rows.push(row.to_vec());
            Ok(())
        })
        .unwrap();
        assert_eq!(rows.len(), 4);
        for row in &rows {
            assert_eq!(row.len(), 2);
            assert_eq!(row[0], Scalar::Null);
        }
        assert_eq!(
            rows[0][1],
            if indices.is_empty() {
                Scalar::Null
            } else {
                Scalar::Int(11)
            }
        );
    }
    assert_eq!(
        p.scan_selected(&[2], &mut |_| Ok(())).unwrap_err().code,
        "relational.invalid_binding"
    );
}
#[test]
fn physical_equal_preserves_null_range_type_and_wrong_column_order() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(&raw, &d, &b);
    for value in [
        Scalar::Null,
        Scalar::Int(-1),
        Scalar::UInt(u32::MAX as u64 + 1),
        Scalar::Float(1.5),
        Scalar::Float(-1.0),
    ] {
        assert_eq!(equal(&p, "ID", value), (EqualityOutcome::Handled, vec![]));
    }
    for value in [Scalar::Bool(true), Scalar::String("10".into())] {
        assert_eq!(
            equal(&p, "Value", value.clone()),
            (EqualityOutcome::Declined, vec![])
        );
        assert_eq!(
            p.equal("ID", &value, &mut |_| Ok(())).unwrap_err().code,
            "relational.type_mismatch"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            p.equal("ID", &Scalar::Float(value), &mut |_| Ok(()))
                .unwrap_err()
                .code,
            "relational.numeric_range"
        );
    }
    assert_eq!(
        equal(&p, "ID", Scalar::Float(10.0)).1[0][0],
        Scalar::UInt(10)
    );
}
#[test]
fn physical_equal_uses_existing_go_unicode_fold() {
    let raw = fixture();
    for (stored, query, handled) in [
        ("K", "K", true),
        ("S", "ſ", true),
        ("Σ", "ς", true),
        ("Σ", "σ", true),
        ("I", "ı", false),
        ("I", "İ", false),
        ("ı", "ı", true),
        ("İ", "İ", true),
        ("字段", "字段", true),
        ("字段", "字断", false),
    ] {
        let mut d = definition();
        d.fields[0].name = stored.into();
        let b = budget();
        let p = physical(&raw, &d, &b);
        let (outcome, rows) = equal(&p, query, Scalar::UInt(10));
        assert_eq!(
            outcome,
            if handled {
                EqualityOutcome::Handled
            } else {
                EqualityOutcome::Declined
            },
            "{stored}/{query}"
        );
        assert_eq!(rows.len(), usize::from(handled));
    }
}
#[test]
fn physical_equal_original_budget_controls_rows_but_not_decline_null_type_branches() {
    let token = Arc::new(AtomicBool::new(false));
    let b = Arc::new(Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(30),
        Some(token.clone()),
    ));
    let raw = fixture();
    let d = definition();
    let p = physical(&raw, &d, &b);
    assert!(Arc::ptr_eq(&p.budget, &b));
    token.store(true, Ordering::Release);
    assert_eq!(
        equal(&p, "other", Scalar::UInt(10)).0,
        EqualityOutcome::Declined
    );
    assert!(equal(&p, "ID", Scalar::Null).1.is_empty());
    assert_eq!(
        p.equal("ID", &Scalar::Bool(true), &mut |_| Ok(()))
            .unwrap_err()
            .code,
        "relational.type_mismatch"
    );
    assert_eq!(
        p.equal("ID", &Scalar::UInt(10), &mut |_| Ok(()))
            .unwrap_err()
            .code,
        "operation.cancelled"
    );
    assert_eq!(
        p.scan(&mut |_| Ok(())).unwrap_err().code,
        "operation.cancelled"
    );
}
#[test]
fn physical_schema_is_lazy_but_projected_scan_validates_unused_malformed_string() {
    let raw = fixture();
    let mut d = definition();
    d.fields[1].kind = "string".into();
    let b = budget();
    // Offset 11 is outside this fixture's empty string table. Opening validates
    // structure, while every actual row read validates even unselected strings.
    let p = physical(&raw, &d, &b);
    assert!(p.schema().rows.is_empty());
    assert_eq!(
        p.scan_selected(&[0], &mut |_| Ok(())).unwrap_err().code,
        "table.invalid_format"
    );
    assert!(equal(&p, "ID", Scalar::UInt(999)).1.is_empty());
    assert_eq!(
        p.equal("ID", &Scalar::UInt(10), &mut |_| Ok(()))
            .unwrap_err()
            .code,
        "table.invalid_format"
    );
}
#[test]
fn physical_provider_is_consumed_by_actual_program_without_extra_index_build() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(&raw, &d, &b);
    let sources = BTreeMap::from([("T".into(), p.schema())]);
    let providers = [relational::PreparedSource {
        schema: p.schema(),
        provider: &p,
    }];
    let result =
        relational::Program::compile("SELECT ID, Value FROM T WHERE ID IN (101,10,101,20)")
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
        result.rows,
        vec![
            vec![Scalar::UInt(101), Scalar::Int(11)],
            vec![Scalar::UInt(10), Scalar::Int(11)],
            vec![Scalar::UInt(20), Scalar::Int(22)]
        ]
    );
}

#[test]
fn physical_scoped_rows_release_safety_but_keep_cumulative_work_and_metadata() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(&raw, &d, &b);
    let before = b.snapshot().unwrap();
    let logical = b.logical_snapshot().unwrap();
    for _ in 0..20 {
        p.equal("ID", &Scalar::UInt(10), &mut |row| {
            assert_eq!(row[1], Scalar::Int(11));
            assert!(b.snapshot().unwrap().retained_bytes > before.retained_bytes);
            Ok(())
        })
        .unwrap();
        assert_eq!(b.snapshot().unwrap().retained_bytes, before.retained_bytes);
        assert_eq!(
            b.logical_snapshot().unwrap().retained_bytes,
            logical.retained_bytes
        );
    }
    assert_eq!(
        b.logical_snapshot().unwrap().metadata_bytes,
        logical.metadata_bytes
    );
    assert!(b.logical_snapshot().unwrap().decode_work >= logical.decode_work + 20 * 3);
    let prior = b.logical_snapshot().unwrap();
    p.scan_selected(&[1, 1], &mut |_| Ok(())).unwrap();
    assert_eq!(
        b.logical_snapshot().unwrap().metadata_bytes,
        prior.metadata_bytes + 256
    );
    assert_eq!(
        b.logical_snapshot().unwrap().retained_bytes,
        prior.retained_bytes + 256
    );
}
#[test]
fn physical_scoped_error_releases_row_scratch_without_swallowing_consumer_error() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let p = physical(&raw, &d, &b);
    let before = b.snapshot().unwrap();
    let mut calls = 0;
    let error = p
        .equal("ID", &Scalar::UInt(10), &mut |_| {
            calls += 1;
            Err(fault("table.record_not_found", 4, "consumer-sentinel"))
        })
        .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(error.message, "consumer-sentinel");
    assert_eq!(b.snapshot().unwrap().retained_bytes, before.retained_bytes);
    assert!(b.snapshot().unwrap().decode_work > before.decode_work);
}

#[test]
fn physical_aliases_share_prepared_table_but_admit_each_table_source_schema() {
    let raw = fixture();
    let d = definition();
    let b = budget();
    let first = physical(&raw, &d, &b);
    let before = b.logical_snapshot().unwrap();
    let second = first.shared().unwrap();
    assert!(Arc::ptr_eq(&first.table, &second.table));
    assert!(!std::ptr::eq(first.schema(), second.schema()));
    let after = b.logical_snapshot().unwrap();
    assert_eq!(after.metadata_bytes - before.metadata_bytes, 2 * 128);
    assert_eq!(after.retained_bytes - before.retained_bytes, 2 * 128);
    assert_eq!(
        after.decode_work, before.decode_work,
        "no second index/bind"
    );
    assert_eq!(
        equal(&second, "ID", Scalar::UInt(101)).1[0][0],
        Scalar::UInt(101)
    );
}

#[test]
fn physical_array_projection_flattens_selected_element_without_reordering_columns() {
    let raw = fixture();
    let mut d = definition();
    d.fields[1].array = true;
    d.fields[1].elements = 2;
    d.fields[1].bits = 16;
    let b = budget();
    let p = physical(&raw, &d, &b);
    assert_eq!(p.schema().columns, ["ID", "Value[0]", "Value[1]"]);
    let mut seen = 0;
    p.scan_selected(&[2], &mut |row| {
        assert_eq!(row, [Scalar::Null, Scalar::Null, Scalar::Int(0)]);
        seen += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, 4);
    assert_eq!(
        equal(&p, "ID", Scalar::UInt(10)).1[0],
        [Scalar::UInt(10), Scalar::Int(11), Scalar::Int(0)]
    );
}
