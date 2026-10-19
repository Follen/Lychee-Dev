//! Complete pre-optimization producer, retained as a byte/value/error and ledger
//! oracle. These tests do not replace public SQL/export and cleanup regressions.
use super::*;
use crate::resource::{Budget, Cost};

fn legacy_finish(
    input: Vec<Output>,
    distinct: bool,
    order: &[(Term, bool, Option<bool>)],
    limit: usize,
    offset: usize,
    work: &mut Work<'_>,
) -> Result<Vec<Vec<Scalar>>> {
    work.retain(input.len().checked_mul(96).ok_or_else(budget)?)?;
    let mut rows = Vec::with_capacity(input.len());
    let mut seen = BTreeSet::new();
    for row in input {
        work.spend()?;
        if row.1.len() != order.len() {
            return Err(binding());
        }
        if distinct {
            let shared = work.shared;
            let mut allocation = shared.map(|b| b.reserve_retained(0)).transpose()?;
            let mut key = String::new();
            for value in &row.0 {
                work.spend()?;
                // A finite binary64 rational fits within this conservative
                // temporary buffer; text payloads retain their exact bound.
                let payload = if let Scalar::String(s) = value {
                    s.len()
                } else {
                    0
                };
                let _part = shared
                    .map(|b| b.reserve_retained(payload + 8192))
                    .transpose()?;
                let part = equal_key(value)?;
                work.consume(part.len())?;
                let grow = part.len().checked_add(32).ok_or_else(budget)?;
                if key.len().checked_add(grow).is_none_or(|n| n > work.bytes) {
                    return Err(budget());
                }
                if let Some(a) = &mut allocation {
                    a.grow(grow.checked_mul(2).ok_or_else(budget)?)?;
                }
                write!(&mut key, "{}:{part}", part.len()).map_err(|_| budget())?;
            }
            if seen.contains(&key) {
                continue;
            }
            work.retain(key.len().checked_add(64).ok_or_else(budget)?)?;
            seen.insert(key);
        }
        rows.push(row);
    }
    if !order.is_empty() {
        let mut buffer = Vec::with_capacity(rows.len());
        buffer.resize_with(rows.len(), || (Vec::new(), Vec::new()));
        let mut width = 1usize;
        while width < rows.len() {
            let mut start = 0;
            while start < rows.len() {
                let mid = (start + width).min(rows.len());
                let end = (mid + width).min(rows.len());
                let (mut left, mut right) = (start, mid);
                for slot in &mut buffer[start..end] {
                    work.spend()?;
                    let take_left = right == end
                        || left < mid
                            && compare_rows(order, &rows[left], &rows[right], work)?
                                != Ordering::Greater;
                    let at = if left < mid && take_left {
                        let at = left;
                        left += 1;
                        at
                    } else {
                        let at = right;
                        right += 1;
                        at
                    };
                    *slot = std::mem::take(&mut rows[at]);
                }
                start = end;
            }
            std::mem::swap(&mut rows, &mut buffer);
            width = width.checked_mul(2).ok_or_else(budget)?;
        }
    }
    Ok(rows
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|row| row.0)
        .collect())
}

fn work<'a>(shared: &'a Budget, left: u64, bytes: usize) -> Work<'a> {
    Work {
        left,
        bytes,
        deadline: shared.deadline,
        cancel: None,
        shared: Some(shared),
        planning: false,
        temporaries: Vec::new(),
        selection_sources: Vec::new(),
        providers: &[],
        resolver: None,
        roots: BTreeMap::new(),
        measure: false,
        scans: Vec::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn compare_producers(
    rows: &[Output],
    distinct: bool,
    order: &[(Term, bool, Option<bool>)],
    limit: usize,
    offset: usize,
    left: u64,
    bytes: usize,
    shared_limit: Cost,
    stopped: bool,
    expired: bool,
) {
    let deadline = if expired {
        Instant::now() - Duration::from_secs(1)
    } else {
        Instant::now() + Duration::from_secs(60)
    };
    let cancel = std::sync::Arc::new(AtomicBool::new(stopped));
    let old_budget = Budget::new(shared_limit, deadline, Some(cancel.clone()));
    let new_budget = Budget::new(shared_limit, deadline, Some(cancel));
    let mut old = work(&old_budget, left, bytes);
    let mut new = work(&new_budget, left, bytes);
    let expected = legacy_finish(rows.to_vec(), distinct, order, limit, offset, &mut old)
        .map(|rows| serde_json::to_vec(&rows).unwrap())
        .map_err(|error| (error.code, error.message, error.exit_code));
    let actual = finish(rows.to_vec(), distinct, order, limit, offset, &mut new)
        .map(|rows| serde_json::to_vec(&rows).unwrap())
        .map_err(|error| (error.code, error.message, error.exit_code));
    assert_eq!(
        actual, expected,
        "distinct={distinct}, limit={limit}, offset={offset}, left={left}, bytes={bytes}"
    );
    assert_eq!((new.left, new.bytes), (old.left, old.bytes));
    assert_eq!(
        new_budget.snapshot().unwrap(),
        old_budget.snapshot().unwrap()
    );
    assert_eq!(
        new_budget.logical_snapshot().unwrap(),
        old_budget.logical_snapshot().unwrap()
    );
}

fn output(value: Scalar, order: &[Scalar]) -> Output {
    (vec![value], order.to_vec())
}

fn ordering(
    width: usize,
    descending: bool,
    nulls: Option<bool>,
) -> Vec<(Term, bool, Option<bool>)> {
    (0..width)
        .map(|i| (Term::Ordinal(i), descending, nulls))
        .collect()
}

#[test]
fn reused_results_match_original_values_distinct_stability_and_pagination() {
    let inputs = [
        Vec::new(),
        vec![output(Scalar::String("猫\0:1".into()), &[])],
        vec![output(Scalar::Null, &[]); 5],
        [
            Scalar::Int(1),
            Scalar::String("猫\0:1".into()),
            Scalar::UInt(1),
            Scalar::Float(1.0),
            Scalar::Null,
            Scalar::Bool(false),
            Scalar::String("猫\0:1".into()),
            Scalar::Int(-1),
            Scalar::Null,
            Scalar::UInt(u64::MAX),
            Scalar::Float(-0.0),
            Scalar::Int(0),
        ]
        .into_iter()
        .map(|v| output(v, &[]))
        .collect(),
    ];
    for rows in inputs {
        for width in 0..=2 {
            let rows: Vec<_> = rows
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    let mut row = row.clone();
                    let keys = [
                        if i % 4 == 0 {
                            Scalar::Null
                        } else {
                            Scalar::Int((i % 3) as i64)
                        },
                        Scalar::String(format!("key{}猫", i % 2)),
                    ];
                    row.1.extend_from_slice(&keys[..width]);
                    row
                })
                .collect();
            for distinct in [false, true] {
                for descending in [false, true] {
                    for nulls in [None, Some(false), Some(true)] {
                        for limit in [0, 1, 3, usize::MAX] {
                            for offset in [0, 1, rows.len(), rows.len() + 1] {
                                compare_producers(
                                    &rows,
                                    distinct,
                                    &ordering(width, descending, nulls),
                                    limit,
                                    offset,
                                    1_000_000,
                                    8 << 20,
                                    Cost::defaults(),
                                    false,
                                    false,
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn reused_results_preserve_every_work_boundary_and_memory_ledger() {
    let rows = vec![
        output(Scalar::String("甲".into()), &[Scalar::Int(2)]),
        output(Scalar::String("甲".into()), &[Scalar::Int(1)]),
        output(Scalar::String("乙".into()), &[Scalar::Int(0)]),
    ];
    let order = ordering(1, false, None);
    for distinct in [false, true] {
        for left in 0..=128 {
            compare_producers(
                &rows,
                distinct,
                &order,
                0,
                0,
                left,
                1 << 20,
                Cost::defaults(),
                false,
                false,
            );
        }
        for bytes in [0, 1, 287, 288, 289, 330, 355, 356, 422, 423, 1024] {
            compare_producers(
                &rows,
                distinct,
                &order,
                usize::MAX,
                0,
                10000,
                bytes,
                Cost::defaults(),
                false,
                false,
            );
        }
        for retained_bytes in [0, 1, 287, 288, 289, 8191, 8192, 8482, 8483, 20000] {
            compare_producers(
                &rows,
                distinct,
                &order,
                usize::MAX,
                0,
                10000,
                1 << 20,
                Cost {
                    retained_bytes,
                    ..Cost::defaults()
                },
                false,
                false,
            );
        }
        for decode_work in 0..=128 {
            compare_producers(
                &rows,
                distinct,
                &order,
                1,
                1,
                10000,
                1 << 20,
                Cost {
                    decode_work,
                    ..Cost::defaults()
                },
                false,
                false,
            );
        }
    }
}

#[test]
fn reused_results_preserve_late_invalid_rows_cancel_deadline_and_zero_limit_errors() {
    let order = ordering(1, false, None);
    let inputs = [
        vec![],
        vec![output(Scalar::Int(1), &[Scalar::Int(0)])],
        vec![
            output(Scalar::Int(1), &[Scalar::Int(0)]),
            output(Scalar::Int(1), &[]),
        ],
        vec![
            output(Scalar::Int(1), &[Scalar::Int(0)]),
            output(Scalar::Int(2), &[Scalar::Bool(true)]),
        ],
        vec![
            output(Scalar::Int(1), &[Scalar::Int(0)]),
            output(Scalar::Int(2), &[Scalar::Float(f64::NAN)]),
        ],
        vec![output(
            Scalar::String("x".repeat((1 << 20) + 1)),
            &[Scalar::Int(0)],
        )],
    ];
    for rows in inputs {
        for distinct in [false, true] {
            for limit in [0, 1, usize::MAX] {
                for (stopped, expired) in [(false, false), (true, false), (false, true)] {
                    for left in [0, 1, 2, 10000] {
                        compare_producers(
                            &rows,
                            distinct,
                            &order,
                            limit,
                            0,
                            left,
                            8 << 20,
                            Cost::defaults(),
                            stopped,
                            expired,
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn reused_results_preserve_input_and_row_allocations_without_string_clones() {
    let mut rows = Vec::with_capacity(32);
    rows.extend([
        output(
            Scalar::String("first".repeat(512)),
            &[Scalar::String("z".repeat(512))],
        ),
        output(
            Scalar::String("first".repeat(512)),
            &[Scalar::String("a".repeat(512))],
        ),
        output(
            Scalar::String("second".repeat(512)),
            &[Scalar::String("y".repeat(512))],
        ),
        output(
            Scalar::String("first".repeat(512)),
            &[Scalar::String("b".repeat(512))],
        ),
    ]);
    let allocation = (rows.as_ptr(), rows.capacity());
    let survivors = [0, 1, 2, 3];
    let owned: Vec<_> = survivors
        .iter()
        .map(|&i| {
            let Scalar::String(value) = &rows[i].0[0] else {
                unreachable!()
            };
            let Scalar::String(key) = &rows[i].1[0] else {
                unreachable!()
            };
            (
                rows[i].0.as_ptr(),
                value.as_ptr(),
                rows[i].1.as_ptr(),
                key.as_ptr(),
            )
        })
        .collect();
    let shared = Budget::new(
        Cost::defaults(),
        Instant::now() + Duration::from_secs(60),
        None,
    );
    let mut w = work(&shared, 1_000_000, 8 << 20);
    let rows = validate_rows(rows, 1, &mut w).unwrap();
    assert_eq!((rows.as_ptr(), rows.capacity()), allocation);
    assert_eq!(rows.len(), owned.len());
    for (row, expected) in rows.iter().zip(owned) {
        let Scalar::String(value) = &row.0[0] else {
            unreachable!()
        };
        let Scalar::String(key) = &row.1[0] else {
            unreachable!()
        };
        assert_eq!(
            (row.0.as_ptr(), value.as_ptr(), row.1.as_ptr(), key.as_ptr()),
            expected
        );
    }
}
