//! Go 3.2 topk.go/topk_test.go. Rust execution status: NOT_RUN.
use lycheedev_data::relational::{Limits, Program, Scalar as S, Source};
use std::collections::BTreeMap;
fn run(sql: &str, rows: Vec<Vec<S>>, memory: usize) -> lycheedev_core::Result<Vec<Vec<S>>> {
    Program::compile(sql)?
        .execute(
            &BTreeMap::from([(
                "a".into(),
                Source {
                    columns: vec!["ID".into(), "Value".into()],
                    rows,
                    complete: true,
                },
            )]),
            &BTreeMap::new(),
            Limits {
                work: 1_000_000,
                memory_bytes: memory,
                ..Limits::default()
            },
            None,
        )
        .map(|r| r.rows)
}
fn rows() -> Vec<Vec<S>> {
    (0..40)
        .map(|i| vec![S::Int(i), if i % 7 == 0 { S::Null } else { S::Int(i % 3) }])
        .collect()
}
#[test]
fn bounded_top_rows_preserve_stable_pages_against_distinct_full_sort() {
    for order in [
        "",
        "ORDER BY Value",
        "ORDER BY Value DESC",
        "ORDER BY Value DESC NULLS FIRST",
        "ORDER BY Value ASC NULLS LAST",
    ] {
        for limit in [
            "LIMIT 0",
            "LIMIT 5",
            "LIMIT 5 OFFSET 3",
            "LIMIT 40 OFFSET 50",
        ] {
            let suffix = format!("{order} {limit}");
            assert_eq!(
                run(&format!("SELECT ID,Value FROM a {suffix}"), rows(), 1 << 20).unwrap(),
                run(
                    &format!("SELECT DISTINCT ID,Value FROM a {suffix}"),
                    rows(),
                    1 << 20
                )
                .unwrap(),
                "{suffix}"
            );
        }
    }
}
#[test]
fn topk_twenty_thousand_rows_fit_thirty_two_kib_retained_budget() {
    let rows = (0..20_000)
        .map(|i| vec![S::Int(i), S::Int(20_000 - i)])
        .collect();
    let got = run("SELECT ID FROM a ORDER BY Value LIMIT 20", rows, 32 << 10).unwrap();
    assert_eq!(got.len(), 20);
    assert_eq!(got[0], vec![S::Int(19_999)]);
}
#[test]
fn topk_checks_late_bad_rows_and_types_even_with_zero_limit() {
    for limit in [0, 1, 20] {
        for bad in [
            vec![S::Int(999), S::String("wrong".into())],
            vec![S::Int(999), S::Float(f64::NAN)],
            vec![S::Int(999)],
        ] {
            let mut rows = rows();
            rows.push(bad);
            assert!(
                run(
                    &format!("SELECT ID FROM a ORDER BY Value LIMIT {limit}"),
                    rows,
                    1 << 20
                )
                .is_err()
            );
        }
    }
}
#[test]
fn topk_limit_threshold_overflow_falls_back_without_truncating() {
    for suffix in [
        "ORDER BY Value LIMIT 1000001",
        "ORDER BY Value LIMIT 18446744073709551615 OFFSET 1",
        "ORDER BY Value,ID LIMIT 5",
    ] {
        assert_eq!(
            run(&format!("SELECT ID,Value FROM a {suffix}"), rows(), 1 << 20).unwrap(),
            run(
                &format!("SELECT DISTINCT ID,Value FROM a {suffix}"),
                rows(),
                1 << 20
            )
            .unwrap()
        );
    }
}
#[test]
fn topk_typed_keys_keep_large_integers_exact() {
    let rows = vec![
        vec![S::Int(1), S::UInt(9_007_199_254_740_993)],
        vec![S::Int(2), S::Float(9_007_199_254_740_992.0)],
        vec![S::Int(3), S::UInt(u64::MAX)],
    ];
    assert_eq!(
        run("SELECT ID FROM a ORDER BY Value LIMIT 2", rows, 1 << 20).unwrap(),
        vec![vec![S::Int(2)], vec![S::Int(1)]]
    );
}
